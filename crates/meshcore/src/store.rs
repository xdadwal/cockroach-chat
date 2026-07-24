//! Persistence behind a trait. The core only ever touches [`Store`]; the SQLCipher-backed
//! implementation (ciphertext-at-rest, hardware-wrapped key) lands as its own task and slots in
//! behind the same interface. [`MemoryStore`] is the in-process implementation used by tests
//! and the simulator.
//!
//! `panic_wipe` is a first-class operation: it must leave nothing recoverable.

use crate::clock::Millis;
use crate::identity::Fingerprint;
use crate::wire::EphId;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredMessage {
    pub digest: [u8; 8],
    pub channel: String,
    pub sender: EphId,
    pub timestamp_ms: u64,
    pub body: Vec<u8>,
    /// The original signed packet bytes, kept so set-reconciliation can resend a message
    /// verbatim (preserving its signature and therefore its digest) instead of re-signing it.
    pub raw: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerRecord {
    pub fingerprint: Fingerprint,
    pub petname: Option<String>,
    pub verified: bool,
    pub last_eph: EphId,
    pub last_seen_ms: Millis,
}

/// One decrypted DM, persisted so a thread survives a restart (unlike the ephemeral Noise session).
/// Keyed by the peer's fingerprint; `mine` distinguishes sent from received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredDm {
    pub peer: Fingerprint,
    pub mine: bool,
    pub timestamp_ms: u64,
    pub body: Vec<u8>,
}

/// A packet queued for a peer that was offline when it was sent (store-and-forward).
#[derive(Debug, Clone)]
struct Envelope {
    packet_bytes: Vec<u8>,
    queued_ms: Millis,
}

pub trait Store {
    fn put_channel_message(&mut self, msg: StoredMessage);
    /// Whether a message with this digest is already stored (dedup at the storage layer).
    fn has_message(&self, digest: &[u8; 8]) -> bool;
    /// Most-recent-first channel history, capped at `limit`.
    fn channel_history(&self, channel: &str, limit: usize) -> Vec<StoredMessage>;
    /// All digests currently held for a channel (for set-reconciliation on partition heal).
    fn channel_digests(&self, channel: &str) -> Vec<[u8; 8]>;
    /// Look up a stored message by digest (to resend during sync).
    fn message_by_digest(&self, digest: &[u8; 8]) -> Option<StoredMessage>;

    fn upsert_peer(&mut self, peer: PeerRecord);
    fn get_peer(&self, fp: &Fingerprint) -> Option<PeerRecord>;
    /// Every known peer, so the UI can reload its contact list on restart instead of waiting for a
    /// live announce from each one.
    fn list_peers(&self) -> Vec<PeerRecord>;

    /// Persist a DM (sent or received) so the thread survives a restart.
    fn put_dm(&mut self, dm: StoredDm);
    /// A peer's DM thread, oldest-first, capped at `limit`.
    fn dm_history(&self, peer: &Fingerprint, limit: usize) -> Vec<StoredDm>;

    fn queue_envelope(&mut self, recipient: Fingerprint, packet_bytes: Vec<u8>, now_ms: Millis);
    fn take_envelopes(&mut self, recipient: &Fingerprint) -> Vec<Vec<u8>>;

    /// Remove a peer's record entirely (petname, verified flag, everything). DMs are separate —
    /// pair with [`Store::delete_dms`] for a full "forget this contact".
    fn delete_peer(&mut self, fp: &Fingerprint);
    /// Delete a peer's DM rows. With `only_control` true, only verification control rows (a bare
    /// kind byte 0x01/0x02 as the body) are removed — used by "unverify" so the derived
    /// confirmed-both-ends state resets while chat history survives.
    fn delete_dms(&mut self, fp: &Fingerprint, only_control: bool);

    /// Destroy everything. After this, no plaintext or key material remains.
    fn panic_wipe(&mut self);
}

/// Whether a stored DM row is a trust control row: verify-notice (1), verify-ack (2), or the
/// local key-changed marker (3) — all stored as a bare kind byte.
pub fn is_control_dm(body: &[u8]) -> bool {
    body.len() == 1 && (1..=3).contains(&body[0])
}

#[derive(Default)]
pub struct MemoryStore {
    channels: HashMap<String, Vec<StoredMessage>>,
    digests: std::collections::HashSet<[u8; 8]>,
    peers: HashMap<Fingerprint, PeerRecord>,
    dms: HashMap<Fingerprint, Vec<StoredDm>>,
    envelopes: HashMap<Fingerprint, Vec<Envelope>>,
    history_max: usize,
    history_ms: Millis,
    envelope_ttl_ms: Millis,
    envelope_max_per_peer: usize,
}

impl MemoryStore {
    pub fn new(
        history_max: usize,
        history_ms: Millis,
        envelope_ttl_ms: Millis,
        envelope_max_per_peer: usize,
    ) -> Self {
        Self {
            history_max,
            history_ms,
            envelope_ttl_ms,
            envelope_max_per_peer,
            ..Default::default()
        }
    }

    fn prune_channel(&mut self, channel: &str, now_ms: Millis) {
        if let Some(msgs) = self.channels.get_mut(channel) {
            if self.history_ms > 0 {
                msgs.retain(|m| now_ms.saturating_sub(m.timestamp_ms) < self.history_ms);
            }
            while msgs.len() > self.history_max {
                let removed = msgs.remove(0);
                self.digests.remove(&removed.digest);
            }
        }
    }
}

impl Store for MemoryStore {
    fn put_channel_message(&mut self, msg: StoredMessage) {
        if self.digests.contains(&msg.digest) {
            return;
        }
        let now = msg.timestamp_ms;
        let channel = msg.channel.clone();
        self.digests.insert(msg.digest);
        self.channels.entry(channel.clone()).or_default().push(msg);
        // Keep history sorted by timestamp for stable "most recent" queries.
        if let Some(v) = self.channels.get_mut(&channel) {
            v.sort_by_key(|m| m.timestamp_ms);
        }
        self.prune_channel(&channel, now);
    }

    fn has_message(&self, digest: &[u8; 8]) -> bool {
        self.digests.contains(digest)
    }

    fn channel_history(&self, channel: &str, limit: usize) -> Vec<StoredMessage> {
        let mut v = self.channels.get(channel).cloned().unwrap_or_default();
        v.sort_by_key(|m| m.timestamp_ms);
        if v.len() > limit {
            v = v.split_off(v.len() - limit);
        }
        v
    }

    fn channel_digests(&self, channel: &str) -> Vec<[u8; 8]> {
        self.channels
            .get(channel)
            .map(|v| v.iter().map(|m| m.digest).collect())
            .unwrap_or_default()
    }

    fn message_by_digest(&self, digest: &[u8; 8]) -> Option<StoredMessage> {
        self.channels
            .values()
            .flat_map(|v| v.iter())
            .find(|m| &m.digest == digest)
            .cloned()
    }

    fn upsert_peer(&mut self, peer: PeerRecord) {
        self.peers.insert(peer.fingerprint, peer);
    }

    fn get_peer(&self, fp: &Fingerprint) -> Option<PeerRecord> {
        self.peers.get(fp).cloned()
    }

    fn list_peers(&self) -> Vec<PeerRecord> {
        self.peers.values().cloned().collect()
    }

    fn put_dm(&mut self, dm: StoredDm) {
        let thread = self.dms.entry(dm.peer).or_default();
        thread.push(dm);
        thread.sort_by_key(|d| d.timestamp_ms);
        while thread.len() > self.history_max {
            thread.remove(0);
        }
    }

    fn dm_history(&self, peer: &Fingerprint, limit: usize) -> Vec<StoredDm> {
        let mut v = self.dms.get(peer).cloned().unwrap_or_default();
        v.sort_by_key(|d| d.timestamp_ms);
        if v.len() > limit {
            v = v.split_off(v.len() - limit);
        }
        v
    }

    fn queue_envelope(&mut self, recipient: Fingerprint, packet_bytes: Vec<u8>, now_ms: Millis) {
        let q = self.envelopes.entry(recipient).or_default();
        // Expire old envelopes first.
        q.retain(|e| now_ms.saturating_sub(e.queued_ms) < self.envelope_ttl_ms);
        q.push(Envelope {
            packet_bytes,
            queued_ms: now_ms,
        });
        // Bound per-peer queue (drop oldest).
        while q.len() > self.envelope_max_per_peer {
            q.remove(0);
        }
    }

    fn take_envelopes(&mut self, recipient: &Fingerprint) -> Vec<Vec<u8>> {
        self.envelopes
            .remove(recipient)
            .map(|q| q.into_iter().map(|e| e.packet_bytes).collect())
            .unwrap_or_default()
    }

    fn delete_peer(&mut self, fp: &Fingerprint) {
        self.peers.remove(fp);
    }

    fn delete_dms(&mut self, fp: &Fingerprint, only_control: bool) {
        if only_control {
            if let Some(thread) = self.dms.get_mut(fp) {
                thread.retain(|d| !is_control_dm(&d.body));
            }
        } else {
            self.dms.remove(fp);
        }
    }

    fn panic_wipe(&mut self) {
        self.channels.clear();
        self.digests.clear();
        self.peers.clear();
        self.dms.clear();
        self.envelopes.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> MemoryStore {
        MemoryStore::new(1000, 6 * 3_600_000, 24 * 3_600_000, 100)
    }

    fn msg(digest: u8, channel: &str, ts: u64) -> StoredMessage {
        StoredMessage {
            digest: [digest; 8],
            channel: channel.to_string(),
            sender: [1; 8],
            timestamp_ms: ts,
            body: b"hi".to_vec(),
            raw: vec![],
        }
    }

    #[test]
    fn stores_and_dedups() {
        let mut s = store();
        s.put_channel_message(msg(1, "#general", 100));
        s.put_channel_message(msg(1, "#general", 100)); // dup
        assert_eq!(s.channel_history("#general", 10).len(), 1);
        assert!(s.has_message(&[1; 8]));
    }

    #[test]
    fn history_capped_by_count() {
        let mut s = MemoryStore::new(2, 0, 0, 100);
        for i in 1..=5u8 {
            s.put_channel_message(msg(i, "#c", i as u64));
        }
        let h = s.channel_history("#c", 10);
        assert_eq!(h.len(), 2);
        // Newest two survive.
        assert_eq!(h[1].digest, [5; 8]);
    }

    #[test]
    fn envelopes_queue_and_drain() {
        let mut s = store();
        let fp = [7u8; 32];
        s.queue_envelope(fp, vec![1, 2, 3], 0);
        s.queue_envelope(fp, vec![4, 5, 6], 0);
        let drained = s.take_envelopes(&fp);
        assert_eq!(drained.len(), 2);
        assert!(s.take_envelopes(&fp).is_empty());
    }

    #[test]
    fn envelopes_expire() {
        let mut s = MemoryStore::new(1000, 0, 1000, 100);
        let fp = [8u8; 32];
        s.queue_envelope(fp, vec![1], 0);
        s.queue_envelope(fp, vec![2], 2000); // triggers expiry of the first
        let drained = s.take_envelopes(&fp);
        assert_eq!(drained, vec![vec![2u8]]);
    }

    #[test]
    fn panic_wipe_leaves_nothing() {
        let mut s = store();
        s.put_channel_message(msg(1, "#general", 100));
        s.queue_envelope([9; 32], vec![1], 0);
        s.put_dm(dm([3; 32], true, 1, b"hi"));
        s.panic_wipe();
        assert!(s.channel_history("#general", 10).is_empty());
        assert!(!s.has_message(&[1; 8]));
        assert!(s.take_envelopes(&[9; 32]).is_empty());
        assert!(s.list_peers().is_empty());
        assert!(s.dm_history(&[3; 32], 10).is_empty());
    }

    fn dm(peer: Fingerprint, mine: bool, ts: u64, body: &[u8]) -> StoredDm {
        StoredDm {
            peer,
            mine,
            timestamp_ms: ts,
            body: body.to_vec(),
        }
    }

    #[test]
    fn list_peers_returns_all_upserted() {
        let mut s = store();
        assert!(s.list_peers().is_empty());
        s.upsert_peer(PeerRecord {
            fingerprint: [1; 32],
            petname: Some("ava".into()),
            verified: true,
            last_eph: [0; 8],
            last_seen_ms: 5,
        });
        s.upsert_peer(PeerRecord {
            fingerprint: [2; 32],
            petname: None,
            verified: false,
            last_eph: [0; 8],
            last_seen_ms: 6,
        });
        let all = s.list_peers();
        assert_eq!(all.len(), 2, "both peers should be listable for UI reload");
        assert!(all.iter().any(|p| p.fingerprint == [1; 32] && p.verified));
    }

    #[test]
    fn dm_history_persists_and_orders_oldest_first() {
        let mut s = store();
        let peer = [7u8; 32];
        s.put_dm(dm(peer, true, 100, b"first"));
        s.put_dm(dm(peer, false, 200, b"second"));
        s.put_dm(dm([9u8; 32], true, 150, b"other peer")); // different thread
        let h = s.dm_history(&peer, 10);
        assert_eq!(h.len(), 2, "only this peer's DMs");
        assert_eq!(h[0].body, b"first", "oldest first");
        assert!(h[0].mine, "first was sent by us");
        assert_eq!(h[1].body, b"second");
        assert!(!h[1].mine, "second was received");
    }
}
