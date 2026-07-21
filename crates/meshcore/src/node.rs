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
use crate::noise::NoiseSession;
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
    /// An end-to-end encrypted direct message was decrypted for us. `verified` is always true
    /// here — a DM only reaches this point after a mutually-authenticated Noise session whose
    /// remote static key matched the sender's announced identity.
    DmReceived {
        sender_fp: Fingerprint,
        from_eph: EphId,
        text: String,
    },
    /// A Noise session with a peer became ready (or was rejected). `verified` distinguishes an
    /// identity-bound session from a rejected/mismatched one.
    DmSession {
        peer_fp: Fingerprint,
        verified: bool,
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

    // --- direct messaging (M3) ---
    /// Per-peer Noise sessions, keyed by the peer's stable fingerprint.
    noise_sessions: HashMap<Fingerprint, NoiseSession>,
    /// DMs queued while a session is still being established.
    pending_dms: HashMap<Fingerprint, Vec<String>>,
    /// The peer's current ephemeral wire id (for addressing directed packets).
    fp_to_eph: HashMap<Fingerprint, EphId>,
    /// The peer's announced X25519 static key, used to bind the Noise session to its identity.
    peer_static_dh: HashMap<Fingerprint, [u8; 32]>,
    /// Ciphertexts that arrived before the session was ready (a DM can overtake the final
    /// handshake message over a relay); drained when the session completes.
    pending_inbound_dms: HashMap<Fingerprint, Vec<Vec<u8>>>,
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
            noise_sessions: HashMap::new(),
            pending_dms: HashMap::new(),
            fp_to_eph: HashMap::new(),
            peer_static_dh: HashMap::new(),
            pending_inbound_dms: HashMap::new(),
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
        let payload = encode_announce(
            self.identity.verifying_key().as_bytes(),
            self.identity.dh_public().as_bytes(),
            &self.nickname,
        );
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

    /// Send an end-to-end encrypted direct message to a peer (by fingerprint). On first use this
    /// establishes a Noise session, queuing the message until the handshake completes.
    pub fn send_dm(&mut self, peer_fp: Fingerprint, text: &str) {
        let ready = self
            .noise_sessions
            .get(&peer_fp)
            .map(|s| s.is_ready())
            .unwrap_or(false);
        if ready {
            self.encrypt_and_send_dm(peer_fp, text);
        } else {
            self.pending_dms
                .entry(peer_fp)
                .or_default()
                .push(text.to_string());
            if !self.noise_sessions.contains_key(&peer_fp) {
                self.begin_handshake(peer_fp);
            }
        }
    }

    pub fn panic_wipe(&mut self) {
        self.store.panic_wipe();
        self.eph_keys.clear();
        self.noise_sessions.clear();
        self.pending_dms.clear();
        self.fp_to_eph.clear();
        self.peer_static_dh.clear();
        self.pending_inbound_dms.clear();
        self.events.clear();
    }

    // --- direct-message internals ---

    fn eph_to_fp(&self, eph: EphId) -> Option<Fingerprint> {
        self.eph_keys.get(&eph).map(|pk| Sha256::digest(pk).into())
    }

    fn begin_handshake(&mut self, peer_fp: Fingerprint) {
        let priv_bytes = self.identity.dh_private_bytes();
        let mut session = match NoiseSession::new_initiator(&priv_bytes) {
            Ok(s) => s,
            Err(_) => return,
        };
        let msg = match session.write_handshake() {
            Ok(m) => m,
            Err(_) => return,
        };
        self.noise_sessions.insert(peer_fp, session);
        self.send_directed(MsgType::NoiseHandshake, peer_fp, msg);
    }

    fn handle_noise_handshake(&mut self, pkt: &Packet) {
        if pkt.recipient != Some(self.identity.eph_id()) {
            return; // not addressed to us; relaying is handled separately
        }
        let Some(fp) = self.eph_to_fp(pkt.sender) else {
            return; // we haven't learned this peer's identity yet
        };
        if self
            .noise_sessions
            .get(&fp)
            .map(|s| s.is_ready())
            .unwrap_or(false)
        {
            return; // already established; ignore a stray/late handshake
        }
        // Responder path: create the session on the first handshake packet.
        if !self.noise_sessions.contains_key(&fp) {
            let priv_bytes = self.identity.dh_private_bytes();
            match NoiseSession::new_responder(&priv_bytes) {
                Ok(s) => {
                    self.noise_sessions.insert(fp, s);
                }
                Err(_) => return,
            }
        }
        {
            let session = self.noise_sessions.get_mut(&fp).unwrap();
            if session.read_handshake(&pkt.payload).is_err() {
                self.noise_sessions.remove(&fp);
                return;
            }
        }
        self.advance_handshake(fp);
    }

    fn advance_handshake(&mut self, fp: Fingerprint) {
        let outbound = {
            let session = self.noise_sessions.get_mut(&fp).unwrap();
            if session.is_ready() {
                None
            } else {
                match session.write_handshake() {
                    Ok(m) => Some(m),
                    Err(_) => {
                        self.noise_sessions.remove(&fp);
                        return;
                    }
                }
            }
        };
        if let Some(msg) = outbound {
            self.send_directed(MsgType::NoiseHandshake, fp, msg);
        }
        if self
            .noise_sessions
            .get(&fp)
            .map(|s| s.is_ready())
            .unwrap_or(false)
        {
            self.finish_session(fp);
        }
    }

    fn finish_session(&mut self, fp: Fingerprint) {
        // Bind the encrypted channel to the peer's signed identity: the Noise remote static must
        // equal the X25519 key the peer announced. Otherwise it is a MITM — drop everything.
        let bound = match (
            self.noise_sessions.get(&fp).and_then(|s| s.remote_static()),
            self.peer_static_dh.get(&fp),
        ) {
            (Some(remote), Some(known)) => &remote == known,
            _ => false,
        };
        if !bound {
            self.noise_sessions.remove(&fp);
            self.pending_dms.remove(&fp);
            self.events.push(MeshEvent::DmSession {
                peer_fp: fp,
                verified: false,
            });
            return;
        }
        self.events.push(MeshEvent::DmSession {
            peer_fp: fp,
            verified: true,
        });
        // Drain outbound DMs queued during the handshake.
        let pending = self.pending_dms.remove(&fp).unwrap_or_default();
        for text in pending {
            self.encrypt_and_send_dm(fp, &text);
        }
        // Drain inbound DMs that overtook the final handshake message.
        let inbound = self.pending_inbound_dms.remove(&fp).unwrap_or_default();
        for ct in inbound {
            self.decrypt_and_emit(fp, &ct);
        }
    }

    fn encrypt_and_send_dm(&mut self, fp: Fingerprint, text: &str) {
        let ciphertext = {
            let Some(session) = self.noise_sessions.get_mut(&fp) else {
                return;
            };
            match session.encrypt(text.as_bytes()) {
                Ok(c) => c,
                Err(_) => return,
            }
        };
        self.send_directed(MsgType::DirectMessage, fp, ciphertext);
    }

    fn handle_direct_message(&mut self, pkt: &Packet) {
        if pkt.recipient != Some(self.identity.eph_id()) {
            return;
        }
        let Some(fp) = self.eph_to_fp(pkt.sender) else {
            return;
        };
        let ready = self
            .noise_sessions
            .get(&fp)
            .map(|s| s.is_ready())
            .unwrap_or(false);
        if !ready {
            // The DM overtook the final handshake message; hold it until the session is ready.
            let buf = self.pending_inbound_dms.entry(fp).or_default();
            if buf.len() < 32 {
                buf.push(pkt.payload.clone());
            }
            return;
        }
        self.decrypt_and_emit(fp, &pkt.payload);
    }

    fn decrypt_and_emit(&mut self, fp: Fingerprint, ciphertext: &[u8]) {
        let plaintext = {
            let Some(session) = self.noise_sessions.get_mut(&fp) else {
                return;
            };
            match session.decrypt(ciphertext) {
                Ok(p) => p,
                Err(_) => return,
            }
        };
        let from_eph = self.fp_to_eph.get(&fp).copied().unwrap_or([0u8; 8]);
        let text = String::from_utf8_lossy(&plaintext).to_string();
        self.events.push(MeshEvent::DmReceived {
            sender_fp: fp,
            from_eph,
            text,
        });
    }

    /// Build, sign, and flood a directed packet (DM or handshake) toward `peer_fp`'s current eph.
    fn send_directed(&mut self, msg_type: MsgType, peer_fp: Fingerprint, payload: Vec<u8>) {
        let Some(&recipient) = self.fp_to_eph.get(&peer_fp) else {
            return; // we don't know where this peer is right now
        };
        let now = self.clock.now_ms();
        let ttl = self.cfg.origin_ttl(self.links.len());
        let mut pkt = Packet::new(
            msg_type,
            ttl,
            now,
            self.identity.eph_id(),
            Some(recipient),
            payload,
        );
        pkt.sign(self.identity.signing_key());
        self.seen.observe(pkt.digest(), now);
        self.broadcast(&pkt, None);
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

        // Dedup FIRST: a message that arrives many times (relayed copies, or several redundant
        // links to the same peer) must be counted once. observe() still bumps the suppression
        // counter for every copy.
        let digest = pkt.digest();
        let obs = self.seen.observe(digest, now);
        if !obs.is_new {
            return; // duplicate — already processed
        }

        // Rate-limit *distinct* messages by originating identity. Doing this after dedup means
        // duplicates are free, so redundant links can't falsely greylist a well-behaved peer (the
        // bug that silently killed the DM handshake); a genuine flooder still sends distinct
        // messages and is throttled.
        if !self.rate.allow(pkt.sender, now) {
            return;
        }

        // Remember which eph is reachable on this link.
        if let Some(l) = self.links.get_mut(&link) {
            l.peer_eph = Some(pkt.sender);
        }

        match pkt.msg_type {
            MsgType::Announce => self.handle_announce(&pkt),
            MsgType::ChannelMessage => self.handle_channel(&pkt),
            MsgType::SyncRequest => self.handle_sync_request(link, &pkt),
            MsgType::NoiseHandshake => self.handle_noise_handshake(&pkt),
            MsgType::DirectMessage => self.handle_direct_message(&pkt),
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
        let Some((pubkey, dh_pub, nick)) = decode_announce(&pkt.payload) else {
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
        self.fp_to_eph.insert(fingerprint, pkt.sender);
        self.peer_static_dh.insert(fingerprint, dh_pub);
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

/// Announce payload: `ed25519_pub(32) ‖ x25519_pub(32) ‖ nick_len(1) ‖ nick`. The X25519 key lets
/// peers bind a future Noise session to this signed identity.
fn encode_announce(ed_pub: &[u8; 32], dh_pub: &[u8; 32], nick: &str) -> Vec<u8> {
    let nick_bytes = nick.as_bytes();
    let n = nick_bytes.len().min(24);
    let mut out = Vec::with_capacity(65 + n);
    out.extend_from_slice(ed_pub);
    out.extend_from_slice(dh_pub);
    out.push(n as u8);
    out.extend_from_slice(&nick_bytes[..n]);
    out
}

fn decode_announce(payload: &[u8]) -> Option<([u8; 32], [u8; 32], String)> {
    if payload.len() < 65 {
        return None;
    }
    let mut ed = [0u8; 32];
    ed.copy_from_slice(&payload[..32]);
    let mut dh = [0u8; 32];
    dh.copy_from_slice(&payload[32..64]);
    let n = payload[64] as usize;
    if 65 + n > payload.len() {
        return None;
    }
    let nick = String::from_utf8_lossy(&payload[65..65 + n]).to_string();
    Some((ed, dh, nick))
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
