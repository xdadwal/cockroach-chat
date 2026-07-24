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
use crate::store::{PeerRecord, Store, StoredDm, StoredMessage};
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
    /// remote static key matched the sender's announced identity. `kind` is one of the
    /// `DM_KIND_*` constants: ordinary text, or an in-person-verification notice/ack control
    /// message (`text` is empty for control kinds).
    DmReceived {
        sender_fp: Fingerprint,
        from_eph: EphId,
        text: String,
        kind: u8,
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
    /// The peer's stable identity on this link, once learned from its announce. Used to detect
    /// and drop redundant links to the same peer (both phones advertise+scan+connect).
    peer_fp: Option<Fingerprint>,
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
    /// Initiator handshake retry bookkeeping: last (re)send time and attempt count per peer. The
    /// recipient may not have had our announce when our first handshake landed (they'd drop it with
    /// no way to ask again), so we re-drive stalled handshakes from `tick` until they complete.
    handshake_last_ms: HashMap<Fingerprint, Millis>,
    handshake_attempts: HashMap<Fingerprint, u32>,
    /// Counts fresh handshakes a peer sends us *while we already hold a ready session*. A restarted
    /// peer lost its half and keeps re-initiating; past a small threshold we treat our session as a
    /// zombie and let the new handshake supersede it, instead of ignoring it forever.
    resync_pending: HashMap<Fingerprint, u8>,
    /// Peers we verified in person but haven't yet told: a verify-notice goes out as soon as an
    /// identity-bound session is (or becomes) ready. In-memory only — if the process restarts
    /// before it sends, the UI's "awaiting confirmation" state prompts a re-scan.
    notice_pending: BTreeSet<Fingerprint>,
}

/// How long to wait before re-sending a stalled initiator handshake, and how many times to try.
const DM_HANDSHAKE_RETRY_MS: Millis = 2000;
const DM_HANDSHAKE_MAX_ATTEMPTS: u32 = 12;

/// DM plaintext framing: `kind(1) || body`. Ordinary chat text.
pub const DM_KIND_TEXT: u8 = 0;
/// Control message: "I verified you in person" (sent once after a QR scan, empty body).
pub const DM_KIND_VERIFY_NOTICE: u8 = 1;
/// Control message: automatic reply to a verify-notice — its arrival is the sender's proof
/// that the pair's encrypted channel works in both directions (empty body).
pub const DM_KIND_VERIFY_ACK: u8 = 2;
/// How many unexpected handshakes a peer must send us against a live session before we conclude the
/// peer restarted and supersede it. >1 so a single stray/late retransmit never resets a good session.
const RESYNC_HANDSHAKE_THRESHOLD: u8 = 2;

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
            handshake_last_ms: HashMap::new(),
            handshake_attempts: HashMap::new(),
            resync_pending: HashMap::new(),
            notice_pending: BTreeSet::new(),
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
        let ttl = self
            .cfg
            .origin_ttl(MsgType::ChannelMessage, self.links.len());

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
            self.cfg.origin_ttl(MsgType::Announce, self.links.len()),
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
        // Persist the outbound DM up front so the thread survives a restart — whether it goes out
        // now, waits for a handshake, or is store-and-forwarded.
        self.store.put_dm(StoredDm {
            peer: peer_fp,
            mine: true,
            timestamp_ms: self.clock.now_ms(),
            body: text.as_bytes().to_vec(),
        });
        let ready = self
            .noise_sessions
            .get(&peer_fp)
            .map(|s| s.is_ready())
            .unwrap_or(false);
        if ready {
            self.encrypt_and_send_dm(peer_fp, DM_KIND_TEXT, text.as_bytes());
            return;
        }
        // If we don't currently know where this peer is (no announce seen / they've gone), hold the
        // message in the encrypted store and deliver it when they reappear (store-and-forward).
        if !self.fp_to_eph.contains_key(&peer_fp) {
            let now = self.clock.now_ms();
            self.store
                .queue_envelope(peer_fp, text.as_bytes().to_vec(), now);
            return;
        }
        // Peer is reachable: queue the message during the handshake and kick it off.
        self.pending_dms
            .entry(peer_fp)
            .or_default()
            .push(text.to_string());
        if !self.noise_sessions.contains_key(&peer_fp) {
            self.begin_handshake(peer_fp);
        } else {
            // A handshake is already in flight but not yet complete: a fresh user send resets the
            // retry budget so `tick` re-drives it promptly instead of waiting out the old backoff.
            self.handshake_attempts.remove(&peer_fp);
            self.handshake_last_ms.remove(&peer_fp);
        }
    }

    /// Mark a peer verified after an in-person QR fingerprint match. Persisted, and preserved
    /// across future announces (never silently downgraded).
    pub fn verify_peer(&mut self, fp: Fingerprint) {
        let mut p = self.peer_or_new(fp);
        p.verified = true;
        self.store.upsert_peer(p);
        // Tell the peer, visibly, that we verified them — immediately if a session is ready,
        // otherwise as soon as one completes (finish_session flushes this flag).
        self.notice_pending.insert(fp);
        self.try_send_verify_notice(fp);
    }

    /// Undo an in-person verification: the peer drops back to unverified. Clears the live
    /// session and the verification control rows (so a derived "confirmed both ends" state
    /// resets) but keeps chat history and petname. Re-scan to verify again.
    pub fn unverify_peer(&mut self, fp: Fingerprint) {
        if let Some(mut p) = self.store.get_peer(&fp) {
            p.verified = false;
            self.store.upsert_peer(p);
        }
        self.store.delete_dms(&fp, true);
        self.drop_peer_session_state(fp);
    }

    /// Forget a contact entirely: peer record, whole DM thread, live session state. They
    /// reappear as a stranger on their next announce.
    pub fn forget_peer(&mut self, fp: Fingerprint) {
        self.store.delete_dms(&fp, false);
        self.store.delete_peer(&fp);
        self.drop_peer_session_state(fp);
        self.fp_to_eph.remove(&fp);
    }

    fn drop_peer_session_state(&mut self, fp: Fingerprint) {
        self.noise_sessions.remove(&fp);
        self.pending_dms.remove(&fp);
        self.pending_inbound_dms.remove(&fp);
        self.handshake_last_ms.remove(&fp);
        self.handshake_attempts.remove(&fp);
        self.resync_pending.remove(&fp);
        self.notice_pending.remove(&fp);
    }

    /// Establish an encrypted session with a peer WITHOUT sending a message. Called right after
    /// in-person verification so the other device flips to "verified" immediately (the completed
    /// Noise session emits a `DmSession` on both ends), before any chat is exchanged.
    pub fn start_dm_session(&mut self, peer_fp: Fingerprint) {
        let ready = self
            .noise_sessions
            .get(&peer_fp)
            .map(|s| s.is_ready())
            .unwrap_or(false);
        if ready {
            // Already established — re-surface it so the UI reflects the verified state.
            self.events.push(MeshEvent::DmSession {
                peer_fp,
                verified: true,
            });
            return;
        }
        if !self.fp_to_eph.contains_key(&peer_fp) {
            return; // not reachable yet; verification is still recorded locally
        }
        if !self.noise_sessions.contains_key(&peer_fp) {
            self.begin_handshake(peer_fp);
        }
    }

    /// Set (or clear, if empty) a local petname for a peer. Persisted.
    pub fn set_petname(&mut self, fp: Fingerprint, name: &str) {
        let mut p = self.peer_or_new(fp);
        p.petname = if name.is_empty() {
            None
        } else {
            Some(name.to_string())
        };
        self.store.upsert_peer(p);
    }

    /// Whether a peer has been verified in person.
    pub fn peer_verified(&self, fp: &Fingerprint) -> bool {
        self.store.get_peer(fp).map(|p| p.verified).unwrap_or(false)
    }

    /// A peer's local petname, if set.
    pub fn peer_petname(&self, fp: &Fingerprint) -> Option<String> {
        self.store.get_peer(fp).and_then(|p| p.petname)
    }

    fn peer_or_new(&self, fp: Fingerprint) -> PeerRecord {
        self.store.get_peer(&fp).unwrap_or(PeerRecord {
            fingerprint: fp,
            petname: None,
            verified: false,
            last_eph: [0u8; 8],
            last_seen_ms: self.clock.now_ms(),
        })
    }

    pub fn panic_wipe(&mut self) {
        self.store.panic_wipe();
        self.eph_keys.clear();
        self.noise_sessions.clear();
        self.pending_dms.clear();
        self.fp_to_eph.clear();
        self.peer_static_dh.clear();
        self.pending_inbound_dms.clear();
        self.handshake_last_ms.clear();
        self.handshake_attempts.clear();
        self.resync_pending.clear();
        self.notice_pending.clear();
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
        self.handshake_last_ms.insert(peer_fp, self.clock.now_ms());
        *self.handshake_attempts.entry(peer_fp).or_insert(0) += 1;
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
            // We hold a live session, yet the peer is initiating a fresh handshake. A single
            // stray/late retransmit is ignored; but a peer that *keeps* re-initiating has lost its
            // half (it restarted) and our session is a zombie it can't use. Past the threshold,
            // drop the zombie and respond fresh so the pair re-syncs instead of deadlocking.
            let tries = self.resync_pending.entry(fp).or_insert(0);
            *tries += 1;
            if *tries < RESYNC_HANDSHAKE_THRESHOLD {
                return;
            }
            self.resync_pending.remove(&fp);
            self.noise_sessions.remove(&fp);
        }
        // Glare tie-breaker: both sides initiated simultaneously and both hold in-flight
        // *initiator* sessions, so each is about to feed the other's msg1 into a state machine
        // expecting msg2 — mutual destruction, retried in lockstep every 2 s. Deterministic rule:
        // the lower fingerprint stays initiator (ignores the intruding msg1); the higher one
        // yields, drops its attempt, and answers as responder. msg1 is recognized by size — in
        // XX with empty handshake payloads, msg1 (e) is exactly 32 bytes, msg2 (e, ee, s, es) is
        // 96 and msg3 (s, se) is 64 — so a legitimate msg2 never enters this branch. Feeding
        // first and using the read error as the glare signal would not work: a failed read
        // poisons the snow handshake state, destroying the very session the winner must keep.
        // Attempts/last_ms are kept so the retry budget still bounds the exchange.
        const XX_MSG1_LEN: usize = 32;
        if pkt.payload.len() == XX_MSG1_LEN {
            if let Some(session) = self.noise_sessions.get(&fp) {
                if session.is_handshaking() && session.initiator {
                    if self.fingerprint() < fp {
                        return; // we win: keep initiating, peer will yield to our msg1
                    }
                    self.noise_sessions.remove(&fp); // we yield: respond to theirs instead
                }
            }
        }
        // Responder path: create the session on the first handshake packet. Responders enter the
        // same retry bookkeeping as initiators (attempts + last-progress timestamp) so a stalled
        // half-open session — e.g. the final msg3 lost in the mesh — is eventually dropped and
        // re-driven by `retry_stalled_handshakes` instead of wedging forever while the initiator
        // believes the session is ready.
        if !self.noise_sessions.contains_key(&fp) {
            let priv_bytes = self.identity.dh_private_bytes();
            match NoiseSession::new_responder(&priv_bytes) {
                Ok(s) => {
                    self.noise_sessions.insert(fp, s);
                    self.handshake_last_ms.insert(fp, self.clock.now_ms());
                    self.handshake_attempts.entry(fp).or_insert(0);
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
        // Any successful read is progress — push the stall deadline out.
        self.handshake_last_ms.insert(fp, self.clock.now_ms());
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
            self.handshake_last_ms.remove(&fp);
            self.handshake_attempts.remove(&fp);
            self.events.push(MeshEvent::DmSession {
                peer_fp: fp,
                verified: false,
            });
            return;
        }
        self.handshake_last_ms.remove(&fp);
        self.handshake_attempts.remove(&fp);
        self.resync_pending.remove(&fp);
        self.events.push(MeshEvent::DmSession {
            peer_fp: fp,
            verified: true,
        });
        // The freshly-ready session may owe the peer an in-person verification notice.
        self.try_send_verify_notice(fp);
        // Drain outbound DMs queued during the handshake.
        let pending = self.pending_dms.remove(&fp).unwrap_or_default();
        for text in pending {
            self.encrypt_and_send_dm(fp, DM_KIND_TEXT, text.as_bytes());
        }
        // Drain inbound DMs that overtook the final handshake message.
        let inbound = self.pending_inbound_dms.remove(&fp).unwrap_or_default();
        for ct in inbound {
            self.decrypt_and_emit(fp, &ct);
        }
    }

    /// Encrypt and send one framed DM plaintext (`kind || body`) over the peer's ready session.
    fn encrypt_and_send_dm(&mut self, fp: Fingerprint, kind: u8, body: &[u8]) {
        let ciphertext = {
            let Some(session) = self.noise_sessions.get_mut(&fp) else {
                return;
            };
            let mut plaintext = Vec::with_capacity(1 + body.len());
            plaintext.push(kind);
            plaintext.extend_from_slice(body);
            match session.encrypt(&plaintext) {
                Ok(c) => c,
                Err(_) => return,
            }
        };
        self.send_directed(MsgType::DirectMessage, fp, ciphertext);
    }

    /// Persist and send a control DM (verify-notice / verify-ack). Only called with a ready
    /// session. The stored body is the bare kind byte, which is how "confirmed both ends" is
    /// later derived from history without a store schema change.
    fn send_control_dm(&mut self, fp: Fingerprint, kind: u8) {
        self.store.put_dm(StoredDm {
            peer: fp,
            mine: true,
            timestamp_ms: self.clock.now_ms(),
            body: vec![kind],
        });
        self.encrypt_and_send_dm(fp, kind, &[]);
    }

    /// If we've verified this peer and owe them the notice, send it over the (ready) session.
    fn try_send_verify_notice(&mut self, fp: Fingerprint) {
        let ready = self
            .noise_sessions
            .get(&fp)
            .map(|s| s.is_ready())
            .unwrap_or(false);
        if ready && self.notice_pending.remove(&fp) {
            self.send_control_dm(fp, DM_KIND_VERIFY_NOTICE);
        }
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
            // If we hold no session at all, the peer is sending on a session we lost (we restarted).
            // Kick a fresh handshake so we re-sync; the peer supersedes its stale one on our repeat
            // attempts (see handle_noise_handshake). The retry loop drives it to completion.
            if !self.noise_sessions.contains_key(&fp) {
                self.begin_handshake(fp);
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
        // A successful decrypt proves this session is live, not a zombie — forget any resync tally.
        self.resync_pending.remove(&fp);
        // Split the kind prefix. Anything that doesn't look framed (empty, or a first byte that
        // isn't a known kind) is treated as legacy plain text rather than dropped.
        let (kind, body) = match plaintext.split_first() {
            Some((&k, rest)) if k <= DM_KIND_VERIFY_ACK => (k, rest),
            _ => (DM_KIND_TEXT, &plaintext[..]),
        };
        // Persist so the thread survives a restart: text is stored bare (as it always was);
        // control messages store just their kind byte.
        self.store.put_dm(StoredDm {
            peer: fp,
            mine: false,
            timestamp_ms: self.clock.now_ms(),
            body: if kind == DM_KIND_TEXT {
                body.to_vec()
            } else {
                vec![kind]
            },
        });
        let from_eph = self.fp_to_eph.get(&fp).copied().unwrap_or([0u8; 8]);
        let text = if kind == DM_KIND_TEXT {
            String::from_utf8_lossy(body).to_string()
        } else {
            String::new()
        };
        self.events.push(MeshEvent::DmReceived {
            sender_fp: fp,
            from_eph,
            text,
            kind,
        });
        // A verify-notice gets an automatic ack — the sender's proof that this direction works.
        // Acks never trigger replies, so the exchange can't loop.
        if kind == DM_KIND_VERIFY_NOTICE {
            self.send_control_dm(fp, DM_KIND_VERIFY_ACK);
        }
    }

    /// Build, sign, and flood a directed packet (DM or handshake) toward `peer_fp`'s current eph.
    fn send_directed(&mut self, msg_type: MsgType, peer_fp: Fingerprint, payload: Vec<u8>) {
        let Some(&recipient) = self.fp_to_eph.get(&peer_fp) else {
            return; // we don't know where this peer is right now
        };
        let now = self.clock.now_ms();
        let ttl = self.cfg.origin_ttl(msg_type, self.links.len());
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
                        peer_fp: None,
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
        self.retry_stalled_handshakes(now);
        // Simple fixed cadence; a later refinement can compute the true next deadline.
        250
    }

    /// Re-drive Noise handshakes that have outbound DMs queued but haven't completed — the recipient
    /// may not have held our announce when our first handshake arrived (and would silently drop it).
    fn retry_stalled_handshakes(&mut self, now: Millis) {
        // Iterate every handshake we initiated (has an attempt count) — this covers both queued-DM
        // handshakes and message-less sessions started right after verification.
        let stalled: Vec<Fingerprint> = self
            .handshake_attempts
            .keys()
            .copied()
            .filter(|fp| {
                let ready = self
                    .noise_sessions
                    .get(fp)
                    .map(|s| s.is_ready())
                    .unwrap_or(false);
                if ready || !self.fp_to_eph.contains_key(fp) {
                    return false;
                }
                if self.handshake_attempts.get(fp).copied().unwrap_or(0)
                    >= DM_HANDSHAKE_MAX_ATTEMPTS
                {
                    return false;
                }
                let last = self.handshake_last_ms.get(fp).copied().unwrap_or(0);
                now.saturating_sub(last) >= DM_HANDSHAKE_RETRY_MS
            })
            .collect();
        for fp in stalled {
            // Restart the XX exchange cleanly (a fresh initiator message); the responder recreates
            // its half on the next first-message it can identify.
            self.noise_sessions.remove(&fp);
            self.begin_handshake(fp);
        }
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

        // Link-local identity tag: a SyncRequest is sent TTL=1 directly on the link and is never
        // relayed, so its sender is our *direct* neighbour. Tag BEFORE dedup, because the identical
        // per-link SyncRequests share a digest — dedup would otherwise drop all but one and we'd
        // only ever tag a single link.
        if pkt.msg_type == MsgType::SyncRequest {
            self.note_link_identity(link, pkt.sender);
        }

        // Dedup: a message that arrives many times (relayed copies, or redundant links) is counted
        // once. observe() still bumps the suppression counter for every copy.
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

    /// Record which peer a link reaches, then drop redundant links to the same identity.
    fn note_link_identity(&mut self, link: LinkId, eph: EphId) {
        if let Some(l) = self.links.get_mut(&link) {
            if l.peer_eph != Some(eph) {
                l.peer_eph = Some(eph);
                l.peer_fp = None; // eph changed → re-resolve its fingerprint
            }
        }
        self.dedup_links();
    }

    /// Resolve link fingerprints from learned announces, then keep exactly one link per peer
    /// identity (the lowest LinkId), closing the rest so we don't waste battery, airtime, and
    /// connection slots on ~5 links to the same phone.
    fn dedup_links(&mut self) {
        let pending: Vec<(LinkId, EphId)> = self
            .links
            .iter()
            .filter(|(_, l)| l.peer_fp.is_none() && l.peer_eph.is_some())
            .map(|(id, l)| (*id, l.peer_eph.unwrap()))
            .collect();
        for (id, eph) in pending {
            if let Some(fp) = self.eph_to_fp(eph) {
                if let Some(l) = self.links.get_mut(&id) {
                    l.peer_fp = Some(fp);
                }
            }
        }

        // self.links is a BTreeMap (ascending), so the first id seen per fingerprint is the primary.
        let mut by_fp: BTreeMap<Fingerprint, Vec<LinkId>> = BTreeMap::new();
        for (id, l) in &self.links {
            if let Some(fp) = l.peer_fp {
                by_fp.entry(fp).or_default().push(*id);
            }
        }
        let redundant: Vec<LinkId> = by_fp
            .values()
            .flat_map(|ids| ids.iter().skip(1).copied())
            .collect();
        for id in redundant {
            self.links.remove(&id);
            self.transport.close(id);
        }
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
        // Preserve any in-person verification and petname the user has set for this identity;
        // a fresh announce must never silently downgrade a peer we've verified face-to-face.
        let existing = self.store.get_peer(&fingerprint);
        self.store.upsert_peer(PeerRecord {
            fingerprint,
            petname: existing.as_ref().and_then(|p| p.petname.clone()),
            verified: existing.as_ref().map(|p| p.verified).unwrap_or(false),
            last_eph: pkt.sender,
            last_seen_ms: pkt.timestamp_ms,
        });
        self.events.push(MeshEvent::PeerAppeared {
            fingerprint,
            eph: pkt.sender,
            petname: if nick.is_empty() { None } else { Some(nick) },
        });
        // A newly-learned key may let us resolve (and dedup) links we couldn't identify yet.
        self.dedup_links();

        // Deliver any store-and-forward messages that were held while this peer was unreachable.
        let held = self.store.take_envelopes(&fingerprint);
        for bytes in held {
            let text = String::from_utf8_lossy(&bytes).to_string();
            self.send_dm(fingerprint, &text);
        }
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

    /// Drain (and clear) a node's captured outbound frames.
    fn drain(n: &MeshNode<RecordingTransport, ManualClock, MemoryStore>) -> Vec<Vec<u8>> {
        let mut s = n.transport.sent.borrow_mut();
        let out: Vec<Vec<u8>> = s.iter().map(|(_, f)| f.clone()).collect();
        s.clear();
        out
    }

    /// Regression: a DM must still deliver when the recipient hadn't yet learned the sender's
    /// announce at the moment the first Noise handshake arrived (they'd drop it, with no way to ask
    /// again). The initiator re-drives the handshake from `tick` until it completes.
    #[test]
    fn dm_retries_until_recipient_learns_sender() {
        let mut a = node(1);
        let mut b = node(2);
        a.on_transport_event(TransportEvent::LinkUp {
            link: 1,
            mtu: 182,
            peer_hint: None,
        });
        b.on_transport_event(TransportEvent::LinkUp {
            link: 1,
            mtu: 182,
            peer_hint: None,
        });

        // A learns B (deliver B's announce to A) — but B is NOT told about A yet.
        for f in drain(&b) {
            a.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
        }
        let b_fp = a
            .take_events()
            .iter()
            .find_map(|e| match e {
                MeshEvent::PeerAppeared { fingerprint, .. } => Some(*fingerprint),
                _ => None,
            })
            .expect("A should have learned B's identity");
        let _ = drain(&a); // discard A's own announce so B stays ignorant of A

        // A sends a DM: it begins the handshake. B receives it but can't identify A → drops it.
        a.send_dm(b_fp, "north gate clear");
        for f in drain(&a) {
            b.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
        }
        assert!(
            !b.take_events()
                .iter()
                .any(|e| matches!(e, MeshEvent::DmReceived { .. })),
            "B can't decrypt a DM from a sender it hasn't learned yet"
        );

        // Now B learns A (A's announce finally propagates).
        a.announce();
        for f in drain(&a) {
            b.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
        }
        let _ = b.take_events();

        // Advance past the retry interval and tick A: it re-drives the handshake. Shuttle frames
        // both ways to completion.
        a.clock.advance(DM_HANDSHAKE_RETRY_MS + 1);
        a.tick();
        for _ in 0..8 {
            for f in drain(&a) {
                b.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
            }
            for f in drain(&b) {
                a.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
            }
        }

        assert!(
            b.take_events().iter().any(
                |e| matches!(e, MeshEvent::DmReceived { text, .. } if text == "north gate clear")
            ),
            "the retried handshake should deliver the DM once B has learned A"
        );
        let a_fp = a.fingerprint();
        assert!(
            b.store()
                .dm_history(&a_fp, 10)
                .iter()
                .any(|d| !d.mine && d.body == b"north gate clear"),
            "the received DM should be persisted so the thread survives a restart"
        );
    }

    /// Verifying a peer starts a message-less session so both ends flip to a bound `DmSession`
    /// immediately — no chat message required, and none is delivered.
    #[test]
    fn start_dm_session_binds_both_ends_without_a_message() {
        let mut a = node(1);
        let mut b = node(2);
        a.on_transport_event(TransportEvent::LinkUp {
            link: 1,
            mtu: 182,
            peer_hint: None,
        });
        b.on_transport_event(TransportEvent::LinkUp {
            link: 1,
            mtu: 182,
            peer_hint: None,
        });
        // Exchange announces both ways.
        for f in drain(&a) {
            b.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
        }
        for f in drain(&b) {
            a.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
        }
        let b_fp = a
            .take_events()
            .iter()
            .find_map(|e| match e {
                MeshEvent::PeerAppeared { fingerprint, .. } => Some(*fingerprint),
                _ => None,
            })
            .expect("A should have learned B");
        let _ = b.take_events();

        a.start_dm_session(b_fp);
        for _ in 0..8 {
            for f in drain(&a) {
                b.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
            }
            for f in drain(&b) {
                a.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
            }
        }

        let b_evs = b.take_events();
        assert!(
            b_evs
                .iter()
                .any(|e| matches!(e, MeshEvent::DmSession { verified: true, .. })),
            "B should see a bound session (→ verified) purely from A verifying B"
        );
        assert!(
            !b_evs
                .iter()
                .any(|e| matches!(e, MeshEvent::DmReceived { .. })),
            "no chat message should be delivered by starting a session"
        );
    }

    /// A one-sided restart must self-heal. If B's process restarts mid-session, its in-memory Noise
    /// session is gone while A still holds its now-useless half. Without recovery, A's DMs are
    /// buffered forever by B, and B's re-handshake is ignored by A's "ready" zombie — a permanent
    /// deadlock until A *also* restarts (the field bug: DMs work one way, or not at all, until a
    /// reboot). After recovery, DMs must flow again.
    #[test]
    fn dm_recovers_after_one_side_restarts_midsession() {
        let mut a = node(1);
        let mut b = node(2);
        a.on_transport_event(TransportEvent::LinkUp {
            link: 1,
            mtu: 182,
            peer_hint: None,
        });
        b.on_transport_event(TransportEvent::LinkUp {
            link: 1,
            mtu: 182,
            peer_hint: None,
        });

        // Mutual discovery: each learns the other's signed announce.
        for f in drain(&b) {
            a.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
        }
        for f in drain(&a) {
            b.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
        }
        let b_fp = a
            .take_events()
            .iter()
            .find_map(|e| match e {
                MeshEvent::PeerAppeared { fingerprint, .. } => Some(*fingerprint),
                _ => None,
            })
            .expect("A should learn B");
        let _ = b.take_events();

        // Baseline: establish a session and confirm a DM flows A -> B.
        a.send_dm(b_fp, "before restart");
        for _ in 0..8 {
            for f in drain(&a) {
                b.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
            }
            for f in drain(&b) {
                a.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
            }
        }
        assert!(
            b.take_events().iter().any(
                |e| matches!(e, MeshEvent::DmReceived { text, .. } if text == "before restart")
            ),
            "baseline: the DM should deliver while both sessions are live"
        );

        // --- B restarts: a fresh in-memory node with the SAME identity seed. Its Noise session is
        //     gone; A still holds its (now-stale) half.
        let mut b = node(2);
        b.on_transport_event(TransportEvent::LinkUp {
            link: 1,
            mtu: 182,
            peer_hint: None,
        });
        a.announce(); // A re-announces (as it does periodically) so the fresh B relearns it.
        for f in drain(&b) {
            a.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
        }
        for f in drain(&a) {
            b.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
        }
        let _ = a.take_events();
        let _ = b.take_events();

        // A (unaware B restarted) sends a DM with its stale session — B can't decrypt it. This must
        // trigger recovery rather than deadlock. Model real timing: frames propagate in ms, but a
        // stalled handshake only retries every DM_HANDSHAKE_RETRY_MS — so advance the clock once per
        // retry cycle, then let frames fully settle before the next retry.
        a.send_dm(b_fp, "triggers recovery");
        for _ in 0..4 {
            for f in drain(&a) {
                b.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
            }
            for f in drain(&b) {
                a.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
            }
        }
        for _ in 0..6 {
            a.clock.advance(DM_HANDSHAKE_RETRY_MS + 1);
            b.clock.advance(DM_HANDSHAKE_RETRY_MS + 1);
            a.tick();
            b.tick();
            for _ in 0..6 {
                for f in drain(&a) {
                    b.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
                }
                for f in drain(&b) {
                    a.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
                }
            }
        }
        let _ = b.take_events();

        // The session is re-synced: a fresh DM from A must now reach the restarted B.
        a.send_dm(b_fp, "after recovery");
        for _ in 0..8 {
            for f in drain(&a) {
                b.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
            }
            for f in drain(&b) {
                a.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
            }
        }
        assert!(
            b.take_events().iter().any(
                |e| matches!(e, MeshEvent::DmReceived { text, .. } if text == "after recovery")
            ),
            "after a one-sided restart, the pair must re-sync and DMs must flow again"
        );
    }

    /// A DM ciphertext lost in transit must not permanently kill that direction. Noise transport
    /// nonces are implicit: if one A->B ciphertext is dropped by the mesh (radio loss, relay TTL,
    /// congestion), A's send counter is ahead of B's receive counter and every later A->B DM fails
    /// decryption — silently, forever — while B->A stays healthy, because the counters are
    /// per-direction. This is the field bug: after a lossy multi-hop exchange, one phone can
    /// receive DMs but its own DMs never arrive, and moving the phones closer doesn't heal it.
    #[test]
    fn dm_survives_a_lost_ciphertext_without_killing_the_direction() {
        let mut a = node(1);
        let mut b = node(2);
        a.on_transport_event(TransportEvent::LinkUp {
            link: 1,
            mtu: 182,
            peer_hint: None,
        });
        b.on_transport_event(TransportEvent::LinkUp {
            link: 1,
            mtu: 182,
            peer_hint: None,
        });

        // Mutual discovery.
        for f in drain(&b) {
            a.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
        }
        for f in drain(&a) {
            b.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
        }
        let b_fp = a
            .take_events()
            .iter()
            .find_map(|e| match e {
                MeshEvent::PeerAppeared { fingerprint, .. } => Some(*fingerprint),
                _ => None,
            })
            .expect("A should learn B");
        let _ = b.take_events();

        // Baseline: session establishes and a DM flows A -> B.
        a.send_dm(b_fp, "first: arrives");
        for _ in 0..8 {
            for f in drain(&a) {
                b.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
            }
            for f in drain(&b) {
                a.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
            }
        }
        assert!(
            b.take_events().iter().any(
                |e| matches!(e, MeshEvent::DmReceived { text, .. } if text == "first: arrives")
            ),
            "baseline: the first DM should deliver"
        );

        // A sends a DM that the mesh loses: A has encrypted it (its send counter advanced) but
        // B never sees the frames.
        a.send_dm(b_fp, "second: lost by the mesh");
        let _ = drain(&a);

        // The next DM must still deliver despite the gap in the counter sequence.
        a.send_dm(b_fp, "third: after the gap");
        for _ in 0..8 {
            for f in drain(&a) {
                b.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
            }
            for f in drain(&b) {
                a.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
            }
        }
        assert!(
            b.take_events().iter().any(
                |e| matches!(e, MeshEvent::DmReceived { text, .. } if text == "third: after the gap")
            ),
            "a single lost ciphertext must not break every later DM in that direction"
        );

        // And the reverse direction was never affected.
        let a_fp = a.fingerprint();
        b.send_dm(a_fp, "reply still works");
        for _ in 0..8 {
            for f in drain(&b) {
                a.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
            }
            for f in drain(&a) {
                b.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
            }
        }
        assert!(
            a.take_events().iter().any(
                |e| matches!(e, MeshEvent::DmReceived { text, .. } if text == "reply still works")
            ),
            "the healthy direction should keep working"
        );
    }

    /// Decode captured outbound frames back into packets (single-fragment frames only, which
    /// is all these small test packets produce).
    fn decode_frames(frames: &[Vec<u8>]) -> Vec<Packet> {
        let mut r = Reassembler::new(Tunables::default());
        frames
            .iter()
            .filter_map(|f| r.ingest(f, 0).ok().flatten())
            .filter_map(|bytes| Packet::decode(&bytes).ok())
            .collect()
    }

    /// Every floodable type must originate with its own configured TTL (announce needs the
    /// largest discovery radius; channels and DMs a smaller one).
    #[test]
    fn origination_ttl_is_per_message_type() {
        let mut a = node(1);
        let mut b = node(2);
        a.on_transport_event(TransportEvent::LinkUp {
            link: 1,
            mtu: 182,
            peer_hint: None,
        });
        b.on_transport_event(TransportEvent::LinkUp {
            link: 1,
            mtu: 182,
            peer_hint: None,
        });
        for f in drain(&b) {
            a.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
        }
        let b_fp = a
            .take_events()
            .iter()
            .find_map(|e| match e {
                MeshEvent::PeerAppeared { fingerprint, .. } => Some(*fingerprint),
                _ => None,
            })
            .expect("A should learn B");
        let _ = drain(&a);

        a.announce();
        let pkts = decode_frames(&drain(&a));
        assert!(
            pkts.iter()
                .any(|p| p.msg_type == MsgType::Announce && p.ttl == 15),
            "announce should originate with TTL 15, got {:?}",
            pkts.iter().map(|p| (p.msg_type, p.ttl)).collect::<Vec<_>>()
        );

        a.send_channel_message("#general", "hello");
        let pkts = decode_frames(&drain(&a));
        assert!(
            pkts.iter()
                .any(|p| p.msg_type == MsgType::ChannelMessage && p.ttl == 10),
            "channel messages should originate with TTL 10"
        );

        // send_dm to a fresh peer emits the first Noise handshake packet — same TTL rule as DMs.
        a.send_dm(b_fp, "hi");
        let pkts = decode_frames(&drain(&a));
        assert!(
            pkts.iter()
                .any(|p| p.msg_type == MsgType::NoiseHandshake && p.ttl == 10),
            "noise handshakes should originate with TTL 10"
        );
    }

    /// In a dense crowd (degree >= 6) origination TTLs clamp to the dense values.
    #[test]
    fn origination_ttl_clamps_when_dense() {
        let mut a = node(1);
        for link in 1..=6 {
            a.on_transport_event(TransportEvent::LinkUp {
                link,
                mtu: 182,
                peer_hint: None,
            });
        }
        let _ = drain(&a);

        a.announce();
        let pkts = decode_frames(&drain(&a));
        assert!(
            pkts.iter()
                .any(|p| p.msg_type == MsgType::Announce && p.ttl == 12),
            "dense announce should clamp to TTL 12"
        );

        a.send_channel_message("#general", "hello");
        let pkts = decode_frames(&drain(&a));
        assert!(
            pkts.iter()
                .any(|p| p.msg_type == MsgType::ChannelMessage && p.ttl == 7),
            "dense channel messages should clamp to TTL 7"
        );
    }

    /// A relayed packet is decremented by exactly 1 — no clamp anywhere rejects the new
    /// higher-than-7 TTLs (wire.rs carries TTL as a raw byte; relay only decrements).
    #[test]
    fn relay_decrements_high_ttl_by_one() {
        let mut b = node(2);
        b.on_transport_event(TransportEvent::LinkUp {
            link: 1,
            mtu: 182,
            peer_hint: None,
        });
        b.on_transport_event(TransportEvent::LinkUp {
            link: 2,
            mtu: 182,
            peer_hint: None,
        });
        let _ = drain(&b);

        // Hand-craft a TTL-15 announce from a third identity and feed it to B on link 1.
        let id = LocalIdentity::from_seed(&[7u8; 32]);
        let payload = encode_announce(
            id.verifying_key().as_bytes(),
            id.dh_public().as_bytes(),
            "stranger",
        );
        let mut pkt = Packet::new(MsgType::Announce, 15, 1000, id.eph_id(), None, payload);
        pkt.sign(id.signing_key());
        for f in frag::split(&pkt.encode(), pkt.digest(), 182).unwrap() {
            b.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
        }

        // Degree 2 => sparse => relay probability 1.0, so the rebroadcast is always scheduled.
        // Fire it by advancing past the jitter window.
        b.clock.advance(1000);
        b.tick();
        let relayed = decode_frames(&drain(&b));
        assert!(
            relayed
                .iter()
                .any(|p| p.msg_type == MsgType::Announce && p.ttl == 14),
            "a TTL-15 packet must relay with TTL 14, got {:?}",
            relayed
                .iter()
                .map(|p| (p.msg_type, p.ttl))
                .collect::<Vec<_>>()
        );
    }

    /// Set up two nodes with an established link and mutual announces; returns (a, b, b_fp, a_fp).
    fn linked_pair() -> (
        MeshNode<RecordingTransport, ManualClock, MemoryStore>,
        MeshNode<RecordingTransport, ManualClock, MemoryStore>,
        Fingerprint,
        Fingerprint,
    ) {
        let mut a = node(1);
        let mut b = node(2);
        a.on_transport_event(TransportEvent::LinkUp {
            link: 1,
            mtu: 182,
            peer_hint: None,
        });
        b.on_transport_event(TransportEvent::LinkUp {
            link: 1,
            mtu: 182,
            peer_hint: None,
        });
        for f in drain(&b) {
            a.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
        }
        for f in drain(&a) {
            b.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
        }
        let b_fp = a
            .take_events()
            .iter()
            .find_map(|e| match e {
                MeshEvent::PeerAppeared { fingerprint, .. } => Some(*fingerprint),
                _ => None,
            })
            .expect("A should learn B");
        let a_fp = a.fingerprint();
        let _ = b.take_events();
        (a, b, b_fp, a_fp)
    }

    /// REPRO 1: a scan-initiated (message-less) session must recover when the final handshake
    /// message (msg3) is lost. The initiator believes the session is ready; the responder is
    /// stuck half-open awaiting msg3 — and responders have no handshake_attempts entry, so the
    /// retry loop never drives them.
    #[test]
    fn scan_session_recovers_when_final_handshake_message_is_lost() {
        let (mut a, mut b, b_fp, _a_fp) = linked_pair();

        a.start_dm_session(b_fp);
        // msg1 a -> b, msg2 b -> a delivered normally.
        for f in drain(&a) {
            b.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
        }
        for f in drain(&b) {
            a.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
        }
        // msg3 a -> b: LOST in the mesh.
        let _ = drain(&a);
        assert!(
            a.take_events()
                .iter()
                .any(|e| matches!(e, MeshEvent::DmSession { verified: true, .. })),
            "initiator believes the session completed"
        );

        // Drive both nodes well past every retry budget (~50 s simulated).
        let mut b_verified = false;
        for _ in 0..200 {
            a.clock.advance(250);
            b.clock.advance(250);
            a.tick();
            b.tick();
            for f in drain(&a) {
                b.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
            }
            for f in drain(&b) {
                a.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
            }
            if b.take_events()
                .iter()
                .any(|e| matches!(e, MeshEvent::DmSession { verified: true, .. }))
            {
                b_verified = true;
                break;
            }
        }
        assert!(
            b_verified,
            "responder must eventually complete or re-sync after losing msg3"
        );
    }

    /// REPRO 2: both sides initiating at the same instant (mutual QR scan within the mesh
    /// latency window) — the crossed msg1s must not permanently wedge the pair. There is no
    /// initiator tie-breaker, and both sides retry on the same fixed 2 s cadence.
    #[test]
    fn simultaneous_mutual_scan_eventually_converges() {
        let (mut a, mut b, b_fp, a_fp) = linked_pair();

        a.start_dm_session(b_fp);
        b.start_dm_session(a_fp);

        let (mut a_verified, mut b_verified) = (false, false);
        for _ in 0..200 {
            // Simultaneous exchange: collect both sides' frames BEFORE delivering either.
            let af = drain(&a);
            let bf = drain(&b);
            for f in af {
                b.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
            }
            for f in bf {
                a.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
            }
            a_verified |= a
                .take_events()
                .iter()
                .any(|e| matches!(e, MeshEvent::DmSession { verified: true, .. }));
            b_verified |= b
                .take_events()
                .iter()
                .any(|e| matches!(e, MeshEvent::DmSession { verified: true, .. }));
            if a_verified && b_verified {
                break;
            }
            a.clock.advance(250);
            b.clock.advance(250);
            a.tick();
            b.tick();
        }
        assert!(
            a_verified && b_verified,
            "mutual simultaneous scan must converge (a: {a_verified}, b: {b_verified})"
        );
    }

    /// In-person verification produces a visible two-sided exchange: the scanner's phone sends a
    /// verify-notice once the session is ready, the peer auto-replies with a verify-ack, and both
    /// legs are persisted — the returning ack is the scanner's proof the channel works both ways.
    #[test]
    fn verify_notice_and_ack_roundtrip() {
        let (mut a, mut b, b_fp, a_fp) = linked_pair();

        // The scan flow: verify + message-less session start (mirrors BleController.verify()).
        a.verify_peer(b_fp);
        a.start_dm_session(b_fp);
        for _ in 0..8 {
            for f in drain(&a) {
                b.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
            }
            for f in drain(&b) {
                a.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
            }
        }

        let b_evs = b.take_events();
        assert!(
            b_evs.iter().any(|e| matches!(
                e,
                MeshEvent::DmReceived {
                    kind: DM_KIND_VERIFY_NOTICE,
                    ..
                }
            )),
            "B should receive A's verify-notice, got {b_evs:?}"
        );
        let a_evs = a.take_events();
        assert!(
            a_evs.iter().any(|e| matches!(
                e,
                MeshEvent::DmReceived {
                    kind: DM_KIND_VERIFY_ACK,
                    ..
                }
            )),
            "A should receive B's automatic verify-ack, got {a_evs:?}"
        );

        // Exactly one ack flows (acks never trigger replies), and all four legs are persisted
        // with the bare-kind-byte convention.
        let a_hist = a.store().dm_history(&b_fp, 10);
        let b_hist = b.store().dm_history(&a_fp, 10);
        assert_eq!(
            a_hist
                .iter()
                .filter(|d| d.body == vec![DM_KIND_VERIFY_ACK] && !d.mine)
                .count(),
            1,
            "A's thread should hold exactly one inbound ack: {a_hist:?}"
        );
        assert!(
            a_hist
                .iter()
                .any(|d| d.body == vec![DM_KIND_VERIFY_NOTICE] && d.mine),
            "A's thread should hold its own outbound notice"
        );
        assert!(
            b_hist
                .iter()
                .any(|d| d.body == vec![DM_KIND_VERIFY_NOTICE] && !d.mine),
            "B's thread should hold the inbound notice"
        );
        assert_eq!(
            b_hist
                .iter()
                .filter(|d| d.body == vec![DM_KIND_VERIFY_ACK] && d.mine)
                .count(),
            1,
            "B's thread should hold exactly one outbound ack (no loops)"
        );

        // Ordinary text still round-trips as kind 0 after the control exchange.
        a.send_dm(b_fp, "hello after verify");
        for f in drain(&a) {
            b.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
        }
        assert!(
            b.take_events().iter().any(|e| matches!(
                e,
                MeshEvent::DmReceived { kind: DM_KIND_TEXT, text, .. } if text == "hello after verify"
            )),
            "text DMs must still deliver as kind 0"
        );
    }

    /// Unverify keeps the conversation but resets trust: verified flag off, control rows gone
    /// (so "confirmed" cannot be re-derived), texts and petname intact. Forget removes the peer
    /// without a trace.
    #[test]
    fn unverify_resets_trust_and_forget_removes_everything() {
        let (mut a, mut b, b_fp, _a_fp) = linked_pair();
        a.set_petname(b_fp, "bee");
        a.verify_peer(b_fp);
        a.start_dm_session(b_fp);
        for _ in 0..8 {
            for f in drain(&a) {
                b.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
            }
            for f in drain(&b) {
                a.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
            }
        }
        a.send_dm(b_fp, "keep me");
        assert!(a.peer_verified(&b_fp));
        assert!(
            a.store()
                .dm_history(&b_fp, 10)
                .iter()
                .any(|d| crate::store::is_control_dm(&d.body)),
            "precondition: control rows exist after the verify exchange"
        );

        a.unverify_peer(b_fp);
        assert!(!a.peer_verified(&b_fp), "verified flag must reset");
        let hist = a.store().dm_history(&b_fp, 10);
        assert!(
            !hist.iter().any(|d| crate::store::is_control_dm(&d.body)),
            "control rows must be gone: {hist:?}"
        );
        assert!(
            hist.iter().any(|d| d.body == b"keep me"),
            "text history must survive unverify"
        );
        assert_eq!(
            a.peer_petname(&b_fp).as_deref(),
            Some("bee"),
            "petname must survive unverify"
        );

        a.forget_peer(b_fp);
        assert!(a.store().dm_history(&b_fp, 10).is_empty());
        assert!(a.store().get_peer(&b_fp).is_none());
    }

    /// DMs (unlike the ephemeral Noise session) must be persisted so a thread survives a restart.
    #[test]
    fn sent_dm_is_persisted() {
        let mut a = node(1);
        let fp = [0x42u8; 32];
        a.send_dm(fp, "cached for restart");
        let h = a.store().dm_history(&fp, 10);
        assert_eq!(h.len(), 1, "the outbound DM should be stored");
        assert!(h[0].mine);
        assert_eq!(h[0].body, b"cached for restart");
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

    #[test]
    fn verify_and_petname_persist_and_survive_announce() {
        let mut a = node(5);
        let fp = [0x42u8; 32];
        assert!(!a.peer_verified(&fp));

        a.verify_peer(fp);
        a.set_petname(fp, "ava");
        assert!(a.peer_verified(&fp));
        assert_eq!(a.peer_petname(&fp), Some("ava".to_string()));

        // A fresh announce for this identity must not downgrade verification or drop the petname.
        use crate::store::PeerRecord;
        let existing = a.store().get_peer(&fp).unwrap();
        assert!(existing.verified);
        a.store.upsert_peer(PeerRecord {
            fingerprint: fp,
            petname: existing.petname.clone(),
            verified: existing.verified,
            last_eph: [9; 8],
            last_seen_ms: 1,
        });
        assert!(a.peer_verified(&fp));
        assert_eq!(a.peer_petname(&fp), Some("ava".to_string()));

        // Clearing the petname works.
        a.set_petname(fp, "");
        assert_eq!(a.peer_petname(&fp), None);
    }
}
