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

    fn queue_envelope(&mut self, recipient: Fingerprint, packet_bytes: Vec<u8>, now_ms: Millis);
    fn take_envelopes(&mut self, recipient: &Fingerprint) -> Vec<Vec<u8>>;

    /// Destroy everything. After this, no plaintext or key material remains.
    fn panic_wipe(&mut self);
}

#[derive(Default)]
pub struct MemoryStore {
    channels: HashMap<String, Vec<StoredMessage>>,
    digests: std::collections::HashSet<[u8; 8]>,
    peers: HashMap<Fingerprint, PeerRecord>,
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

    fn panic_wipe(&mut self) {
        self.channels.clear();
        self.digests.clear();
        self.peers.clear();
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
        s.panic_wipe();
        assert!(s.channel_history("#general", 10).is_empty());
        assert!(!s.has_message(&[1; 8]));
        assert!(s.take_envelopes(&[9; 32]).is_empty());
    }
}
