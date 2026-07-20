//! [`MeshNode`] — the sans-IO orchestrator. It owns the identity, the flood-control machinery,
//! the store, and the set of live links, and it turns transport events + user commands into
//! outbound frames + [`MeshEvent`]s. It never touches the clock or the network directly: time
//! comes from [`Clock`], I/O goes through [`Transport`]. That is what lets the simulator run it
//! deterministically.

use crate::channels::{self, SyncFilter};
use crate::clock::{Clock, Millis};
use crate::compress;
use crate::config::Tunables;
use crate::frag::{self, Reassembler};
use crate::identity::{Fingerprint, LocalIdentity};
use crate::relay::{self, RateLimiter, RelayScheduler, SeenCache};
use crate::store::{PeerRecord, Store, StoredMessage};
use crate::transport::{LinkId, Transport, TransportEvent};
use crate::wire::{EphId, MsgType, Packet};
use ed25519_dalek::VerifyingKey;
use rand::rngs::StdRng;
use rand::SeedableRng;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Something the UI should react to. The core accumulates these; the caller drains them with
/// [`MeshNode::take_events`] (the FFI layer forwards them to a platform `EventListener`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MeshEvent {
    /// A channel message was delivered to us.
    MessageReceived {
        channel: String,
        sender: EphId,
        body: String,
        timestamp_ms: u64,
        /// True iff we knew the sender's key and the signature checked out.
        verified: bool,
    },
    /// We learned (or refreshed) a peer's identity from a signed announce.
    PeerAppeared {
        fingerprint: Fingerprint,
        eph: EphId,
        petname: Option<String>,
    },
    /// A link dropped.
    PeerLost { link: LinkId },
    /// Periodic counters for the "Mesh Active" UI / debug screen.
    Stats {
        links: usize,
        seen: usize,
        pending_relays: usize,
    },
}

struct LinkState {
    mtu: usize,
    peer_eph: Option<EphId>,
}

pub struct MeshNode<T: Transport, C: Clock, S: Store> {
    identity: LocalIdentity,
    cfg: Tunables,
    transport: T,
    clock: C,
    store: S,

    seen: SeenCache,
    rate: RateLimiter,
    scheduler: RelayScheduler,
    reassembler: Reassembler,
    rng: StdRng,

    links: BTreeMap<LinkId, LinkState>,
    eph_keys: HashMap<EphId, [u8; 32]>,
    subscribed: BTreeSet<String>,
    events: Vec<MeshEvent>,
    nickname: String,
    relays_fired: u64,
}

impl<T: Transport, C: Clock, S: Store> MeshNode<T, C, S> {
    pub fn new(
        identity: LocalIdentity,
        cfg: Tunables,
        transport: T,
        clock: C,
        store: S,
        nickname: impl Into<String>,
        rng_seed: u64,
    ) -> Self {
        let mut subscribed = BTreeSet::new();
        subscribed.insert(channels::DEFAULT_CHANNEL.to_string());
        Self {
            identity,
            seen: SeenCache::new(cfg.clone()),
            rate: RateLimiter::new(cfg.clone()),
            scheduler: RelayScheduler::new(cfg.clone()),
            reassembler: Reassembler::new(cfg.clone()),
            rng: StdRng::seed_from_u64(rng_seed),
            cfg,
            transport,
            clock,
            store,
            links: BTreeMap::new(),
            eph_keys: HashMap::new(),
            subscribed,
            events: Vec::new(),
            nickname: nickname.into(),
            relays_fired: 0,
        }
    }

    // --- introspection ---

    pub fn eph_id(&self) -> EphId {
        self.identity.eph_id()
    }

    pub fn fingerprint(&self) -> Fingerprint {
        self.identity.fingerprint()
    }

    pub fn link_count(&self) -> usize {
        self.links.len()
    }

    /// How many times this node has rebroadcast a packet (fired a scheduled relay). The mesh-wide
    /// sum over one message is the flooding cost; suppression keeps it well below the node count.
    pub fn relays_fired(&self) -> u64 {
        self.relays_fired
    }

    /// Reset the rebroadcast counter (used by the simulator to measure a single message).
    pub fn reset_relays_fired(&mut self) {
        self.relays_fired = 0;
    }

    pub fn take_events(&mut self) -> Vec<MeshEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn store(&self) -> &S {
        &self.store
    }

    // --- commands ---

    pub fn join_channel(&mut self, name: &str) {
        self.subscribed.insert(channels::normalize(name));
    }

    /// Originate a channel message: sign it, store it, and broadcast on every link. Returns the
    /// 8-byte digest so callers (and the simulator) can track its propagation.
    pub fn send_channel_message(&mut self, channel: &str, text: &str) -> [u8; 8] {
        let channel = channels::normalize(channel);
        self.subscribed.insert(channel.clone());
        let now = self.clock.now_ms();
        let ttl = self.cfg.origin_ttl(self.links.len());

        let payload = encode_channel(&channel, text, &self.cfg);
        let mut pkt = Packet::new(
            MsgType::ChannelMessage,
            ttl,
            now,
            self.identity.eph_id(),
            None,
            payload,
        );
        pkt.sign(self.identity.signing_key());
        let digest = pkt.digest();

        // Store locally and mark seen so an echo of our own packet is not re-relayed.
        self.store.put_channel_message(StoredMessage {
            digest,
            channel,
            sender: self.identity.eph_id(),
            timestamp_ms: now,
            body: text.as_bytes().to_vec(),
            raw: pkt.encode(),
        });
        self.seen.observe(digest, now);

        self.broadcast(&pkt, None);
        digest
    }

    /// Send our signed announce on every link (peers learn our eph → key binding from it).
    pub fn announce(&mut self) {
        let now = self.clock.now_ms();
        let payload = encode_announce(self.identity.verifying_key().as_bytes(), &self.nickname);
        let mut pkt = Packet::new(
            MsgType::Announce,
            self.cfg.origin_ttl(self.links.len()),
            now,
            self.identity.eph_id(),
            None,
            payload,
        );
        pkt.sign(self.identity.signing_key());
        self.seen.observe(pkt.digest(), now);
        self.broadcast(&pkt, None);
    }

    pub fn panic_wipe(&mut self) {
        self.store.panic_wipe();
        self.eph_keys.clear();
        self.events.clear();
    }

    // --- transport plumbing ---

    pub fn on_transport_event(&mut self, ev: TransportEvent) {
        match ev {
            TransportEvent::LinkUp { link, mtu, .. } => {
                self.links.insert(
                    link,
                    LinkState {
                        mtu,
                        peer_eph: None,
                    },
                );
                // Greet the new neighbour so they can verify our future traffic.
                self.announce();
                self.send_sync_requests(link);
            }
            TransportEvent::MtuChanged { link, mtu } => {
                if let Some(l) = self.links.get_mut(&link) {
                    l.mtu = mtu;
                }
            }
            TransportEvent::LinkDown { link } => {
                self.links.remove(&link);
                self.events.push(MeshEvent::PeerLost { link });
            }
            TransportEvent::FrameReceived { link, frame } => {
                self.on_frame(link, &frame);
            }
        }
    }

    /// Advance timers: release due rebroadcasts, evict stale state. Returns the delay (ms) until
    /// the next scheduled wake, which the platform uses to set its timer.
    pub fn tick(&mut self) -> Millis {
        let now = self.clock.now_ms();
        let due = self.scheduler.due(now, &self.seen);
        for d in due {
            self.relays_fired += 1;
            self.broadcast(&d.packet, Some(d.exclude_link));
        }
        self.seen.evict_expired(now);
        self.reassembler.evict_expired(now);
        // Simple fixed cadence; a later refinement can compute the true next deadline.
        250
    }

    // --- internals ---

    fn on_frame(&mut self, link: LinkId, frame: &[u8]) {
        let now = self.clock.now_ms();
        let packet_bytes = match self.reassembler.ingest(frame, now) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return,
            Err(_) => return, // malformed framing: drop, never panic
        };
        let pkt = match Packet::decode(&packet_bytes) {
            Ok(p) => p,
            Err(_) => return,
        };

        // Rate-limit by originating identity (relayed copies keep the origin's eph).
        if !self.rate.allow(pkt.sender, now) {
            return;
        }

        let digest = pkt.digest();
        let obs = self.seen.observe(digest, now);
        if !obs.is_new {
            return; // duplicate: the observe() bumped the suppression counter, nothing else to do
        }

        // Remember which eph is reachable on this link.
        if let Some(l) = self.links.get_mut(&link) {
            l.peer_eph = Some(pkt.sender);
        }

        match pkt.msg_type {
            MsgType::Announce => self.handle_announce(&pkt),
            MsgType::ChannelMessage => self.handle_channel(&pkt),
            MsgType::SyncRequest => self.handle_sync_request(link, &pkt),
            _ => {} // unknown/other: relayed below, not parsed
        }

        self.maybe_relay(link, pkt);
    }

    fn maybe_relay(&mut self, arrived_link: LinkId, mut pkt: Packet) {
        if pkt.ttl <= 1 {
            return; // no hops left
        }
        let degree = self.links.len();
        if !relay::should_relay(degree, &self.cfg, &mut self.rng) {
            return;
        }
        pkt.ttl -= 1;
        let delay = relay::relay_delay(&self.cfg, &mut self.rng);
        self.scheduler
            .schedule(pkt.digest(), pkt, arrived_link, delay, self.clock.now_ms());
    }

    fn handle_announce(&mut self, pkt: &Packet) {
        let Some((pubkey, nick)) = decode_announce(&pkt.payload) else {
            return;
        };
        let Ok(vk) = VerifyingKey::from_bytes(&pubkey) else {
            return;
        };
        if pkt.verify(&vk).is_err() {
            return; // announce not signed by the key it carries: ignore
        }
        self.eph_keys.insert(pkt.sender, pubkey);
        let fingerprint: Fingerprint = Sha256::digest(pubkey).into();
        let petname = self.store.get_peer(&fingerprint).and_then(|p| p.petname);
        self.store.upsert_peer(PeerRecord {
            fingerprint,
            petname: petname.clone(),
            verified: false,
            last_eph: pkt.sender,
            last_seen_ms: pkt.timestamp_ms,
        });
        self.events.push(MeshEvent::PeerAppeared {
            fingerprint,
            eph: pkt.sender,
            petname: if nick.is_empty() { None } else { Some(nick) },
        });
    }

    fn handle_channel(&mut self, pkt: &Packet) {
        let Some((channel, text)) = decode_channel(&pkt.payload, &self.cfg) else {
            return;
        };
        let verified = self
            .eph_keys
            .get(&pkt.sender)
            .and_then(|pk| VerifyingKey::from_bytes(pk).ok())
            .map(|vk| pkt.verify(&vk).is_ok())
            .unwrap_or(false);

        let digest = pkt.digest();
        self.store.put_channel_message(StoredMessage {
            digest,
            channel: channel.clone(),
            sender: pkt.sender,
            timestamp_ms: pkt.timestamp_ms,
            body: text.clone().into_bytes(),
            raw: pkt.encode(),
        });

        if self.subscribed.contains(&channel) {
            self.events.push(MeshEvent::MessageReceived {
                channel,
                sender: pkt.sender,
                body: text,
                timestamp_ms: pkt.timestamp_ms,
                verified,
            });
        }
    }

    /// A peer asked what we have for a channel; reply with whatever they're missing.
    fn handle_sync_request(&mut self, link: LinkId, pkt: &Packet) {
        let Some(remote) = decode_sync(&pkt.payload) else {
            return;
        };
        let mine = SyncFilter::new(
            remote.channel.clone(),
            self.store.channel_digests(&remote.channel),
        );
        let missing = mine.missing_from(&remote);
        for digest in missing {
            if let Some(m) = self.store.message_by_digest(&digest) {
                // Resend the ORIGINAL signed bytes so the digest (and signature) are preserved —
                // the neighbour dedups against the same identity every other node uses.
                if !m.raw.is_empty() {
                    let mtu = self
                        .links
                        .get(&link)
                        .map(|l| l.mtu)
                        .unwrap_or(self.cfg.default_mtu);
                    self.send_bytes(link, &m.raw, digest, mtu);
                }
            }
        }
    }

    fn send_sync_requests(&mut self, link: LinkId) {
        let now = self.clock.now_ms();
        let channels: Vec<String> = self.subscribed.iter().cloned().collect();
        for channel in channels {
            let filter = SyncFilter::new(channel.clone(), self.store.channel_digests(&channel));
            let payload = encode_sync(&filter);
            let mut pkt = Packet::new(
                MsgType::SyncRequest,
                1, // one hop: only my direct neighbour answers
                now,
                self.identity.eph_id(),
                None,
                payload,
            );
            pkt.sign(self.identity.signing_key());
            self.send_on_link(link, &pkt);
        }
    }

    // --- outbound fanout ---

    fn broadcast(&mut self, pkt: &Packet, exclude: Option<LinkId>) {
        let bytes = pkt.encode();
        let digest = pkt.digest();
        let targets: Vec<(LinkId, usize)> = self
            .links
            .iter()
            .filter(|(id, _)| Some(**id) != exclude)
            .map(|(id, l)| (*id, l.mtu))
            .collect();
        for (link, mtu) in targets {
            self.send_bytes(link, &bytes, digest, mtu);
        }
    }

    fn send_on_link(&mut self, link: LinkId, pkt: &Packet) {
        let mtu = self
            .links
            .get(&link)
            .map(|l| l.mtu)
            .unwrap_or(self.cfg.default_mtu);
        let bytes = pkt.encode();
        self.send_bytes(link, &bytes, pkt.digest(), mtu);
    }

    fn send_bytes(&self, link: LinkId, bytes: &[u8], digest: [u8; 8], mtu: usize) {
        match frag::split(bytes, digest, mtu) {
            Ok(frames) => {
                for f in frames {
                    self.transport.send(link, &f);
                }
            }
            Err(_) => { /* packet too large to frame: drop */ }
        }
    }
}

// --- payload (de)serialization helpers -------------------------------------------------

fn encode_announce(pubkey: &[u8; 32], nick: &str) -> Vec<u8> {
    let nick_bytes = nick.as_bytes();
    let n = nick_bytes.len().min(24);
    let mut out = Vec::with_capacity(33 + n);
    out.extend_from_slice(pubkey);
    out.push(n as u8);
    out.extend_from_slice(&nick_bytes[..n]);
    out
}

fn decode_announce(payload: &[u8]) -> Option<([u8; 32], String)> {
    if payload.len() < 33 {
        return None;
    }
    let mut pk = [0u8; 32];
    pk.copy_from_slice(&payload[..32]);
    let n = payload[32] as usize;
    if 33 + n > payload.len() {
        return None;
    }
    let nick = String::from_utf8_lossy(&payload[33..33 + n]).to_string();
    Some((pk, nick))
}

/// Channel payload: `[flag_compressed(1)][chan_len(1)][chan][text...]`, text optionally LZ4'd.
fn encode_channel(channel: &str, text: &str, cfg: &Tunables) -> Vec<u8> {
    let chan = channel.as_bytes();
    let clen = chan.len().min(255);
    let text_bytes = text.as_bytes();
    let (compressed, body) = match compress::maybe_compress(text_bytes, cfg) {
        Some(c) => (1u8, c),
        None => (0u8, text_bytes.to_vec()),
    };
    let mut out = Vec::with_capacity(2 + clen + body.len());
    out.push(compressed);
    out.push(clen as u8);
    out.extend_from_slice(&chan[..clen]);
    out.extend_from_slice(&body);
    out
}

fn decode_channel(payload: &[u8], cfg: &Tunables) -> Option<(String, String)> {
    if payload.len() < 2 {
        return None;
    }
    let compressed = payload[0];
    let clen = payload[1] as usize;
    if 2 + clen > payload.len() {
        return None;
    }
    let channel = String::from_utf8_lossy(&payload[2..2 + clen]).to_string();
    let body = &payload[2 + clen..];
    let text_bytes = if compressed == 1 {
        compress::decompress(body, cfg).ok()?
    } else {
        body.to_vec()
    };
    let text = String::from_utf8_lossy(&text_bytes).to_string();
    Some((channel, text))
}

/// Sync payload: `[chan_len(1)][chan][digest*8...]`.
fn encode_sync(filter: &SyncFilter) -> Vec<u8> {
    let chan = filter.channel.as_bytes();
    let clen = chan.len().min(255);
    let mut out = Vec::with_capacity(1 + clen + filter.digests.len() * 8);
    out.push(clen as u8);
    out.extend_from_slice(&chan[..clen]);
    for d in &filter.digests {
        out.extend_from_slice(d);
    }
    out
}

fn decode_sync(payload: &[u8]) -> Option<SyncFilter> {
    if payload.is_empty() {
        return None;
    }
    let clen = payload[0] as usize;
    if 1 + clen > payload.len() {
        return None;
    }
    let channel = String::from_utf8_lossy(&payload[1..1 + clen]).to_string();
    let rest = &payload[1 + clen..];
    let mut digests = Vec::with_capacity(rest.len() / 8);
    for chunk in rest.chunks_exact(8) {
        let mut d = [0u8; 8];
        d.copy_from_slice(chunk);
        digests.push(d);
    }
    Some(SyncFilter::new(channel, digests))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::ManualClock;
    use crate::store::MemoryStore;
    use std::cell::RefCell;

    /// A transport that just records outbound frames, for single-node unit tests.
    #[derive(Default)]
    struct RecordingTransport {
        sent: RefCell<Vec<(LinkId, Vec<u8>)>>,
    }
    impl Transport for RecordingTransport {
        fn send(&self, link: LinkId, frame: &[u8]) {
            self.sent.borrow_mut().push((link, frame.to_vec()));
        }
    }

    fn store() -> MemoryStore {
        MemoryStore::new(1000, 6 * 3_600_000, 24 * 3_600_000, 100)
    }

    fn node(seed: u8) -> MeshNode<RecordingTransport, ManualClock, MemoryStore> {
        MeshNode::new(
            LocalIdentity::from_seed(&[seed; 32]),
            Tunables::default(),
            RecordingTransport::default(),
            ManualClock::new(1000),
            store(),
            "tester",
            seed as u64,
        )
    }

    #[test]
    fn announce_roundtrip_between_two_nodes() {
        let mut a = node(1);
        let mut b = node(2);

        // a gets a link; it announces. Capture a's announce frames and feed them to b.
        a.on_transport_event(TransportEvent::LinkUp {
            link: 1,
            mtu: 182,
            peer_hint: None,
        });
        let a_frames: Vec<Vec<u8>> = a
            .transport
            .sent
            .borrow()
            .iter()
            .map(|(_, f)| f.clone())
            .collect();

        b.on_transport_event(TransportEvent::LinkUp {
            link: 1,
            mtu: 182,
            peer_hint: None,
        });
        for f in a_frames {
            b.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
        }
        let evs = b.take_events();
        assert!(
            evs.iter()
                .any(|e| matches!(e, MeshEvent::PeerAppeared { .. })),
            "b should have learned a's identity, got {evs:?}"
        );
    }

    #[test]
    fn channel_message_encode_decode() {
        let cfg = Tunables::default();
        let enc = encode_channel("#general", "hello world", &cfg);
        let (chan, text) = decode_channel(&enc, &cfg).unwrap();
        assert_eq!(chan, "#general");
        assert_eq!(text, "hello world");
    }

    #[test]
    fn compressed_channel_message_roundtrips() {
        let cfg = Tunables::default();
        let long = "safe route north gate ".repeat(20);
        let enc = encode_channel("#north", &long, &cfg);
        assert_eq!(enc[0], 1, "long body should be compressed");
        let (_, text) = decode_channel(&enc, &cfg).unwrap();
        assert_eq!(text, long);
    }

    #[test]
    fn own_message_marked_seen_and_stored() {
        let mut a = node(3);
        a.send_channel_message("#general", "hi");
        assert_eq!(a.store().channel_history("#general", 10).len(), 1);
    }

    #[test]
    fn panic_wipe_clears_store() {
        let mut a = node(4);
        a.send_channel_message("#general", "secret");
        a.panic_wipe();
        assert!(a.store().channel_history("#general", 10).is_empty());
    }
}
