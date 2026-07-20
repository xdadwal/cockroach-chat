//! Link-layer framing: splitting a [`Packet`]'s encoded bytes into BLE-sized frames and
//! reassembling them. A frame must fit the usable ATT payload (182 B on iOS by default).
//!
//! Frame layout:
//! ```text
//! Complete (0x01): kind(1) | packet_bytes...
//! Fragment (0x02): kind(1) | digest(8) | index(2) | total(2) | chunk...
//! ```

use crate::clock::Millis;
use crate::config::Tunables;
use crate::error::FragError;
use std::collections::BTreeMap;

const KIND_COMPLETE: u8 = 0x01;
const KIND_FRAGMENT: u8 = 0x02;
const FRAG_HEADER: usize = 1 + 8 + 2 + 2; // kind + digest + index + total

/// Split encoded packet bytes into frames sized for `mtu`. Returns a single Complete frame when
/// the packet fits, otherwise N Fragment frames sharing `digest`.
pub fn split(packet_bytes: &[u8], digest: [u8; 8], mtu: usize) -> Result<Vec<Vec<u8>>, FragError> {
    if mtu <= FRAG_HEADER {
        return Err(FragError::MtuTooSmall(mtu));
    }
    // A Complete frame needs 1 header byte, so it fits when len < mtu.
    if packet_bytes.len() < mtu {
        let mut f = Vec::with_capacity(packet_bytes.len() + 1);
        f.push(KIND_COMPLETE);
        f.extend_from_slice(packet_bytes);
        return Ok(vec![f]);
    }

    let chunk_size = mtu - FRAG_HEADER;
    let total = packet_bytes.len().div_ceil(chunk_size);
    if total > u16::MAX as usize {
        return Err(FragError::TooLarge {
            cap: chunk_size * u16::MAX as usize,
        });
    }

    let mut frames = Vec::with_capacity(total);
    for (i, chunk) in packet_bytes.chunks(chunk_size).enumerate() {
        let mut f = Vec::with_capacity(FRAG_HEADER + chunk.len());
        f.push(KIND_FRAGMENT);
        f.extend_from_slice(&digest);
        f.extend_from_slice(&(i as u16).to_be_bytes());
        f.extend_from_slice(&(total as u16).to_be_bytes());
        f.extend_from_slice(chunk);
        frames.push(f);
    }
    Ok(frames)
}

struct Partial {
    total: u16,
    received: usize,
    chunks: Vec<Option<Vec<u8>>>,
    bytes: usize,
    first_seen_ms: Millis,
}

/// Reassembles fragments back into whole packets. Bounded: at most `reassembly_slots` in-flight
/// messages, each capped at `reassembly_max_bytes`, each expiring after `reassembly_timeout_ms`.
pub struct Reassembler {
    slots: BTreeMap<[u8; 8], Partial>,
    cfg: Tunables,
}

impl Reassembler {
    pub fn new(cfg: Tunables) -> Self {
        Self {
            slots: BTreeMap::new(),
            cfg,
        }
    }

    pub fn in_flight(&self) -> usize {
        self.slots.len()
    }

    /// Feed one frame. Returns `Ok(Some(packet_bytes))` when a message completes.
    pub fn ingest(&mut self, frame: &[u8], now_ms: Millis) -> Result<Option<Vec<u8>>, FragError> {
        if frame.is_empty() {
            return Ok(None);
        }
        match frame[0] {
            KIND_COMPLETE => Ok(Some(frame[1..].to_vec())),
            KIND_FRAGMENT => self.ingest_fragment(frame, now_ms),
            _ => Ok(None), // unknown frame kind: ignore, do not error the link
        }
    }

    fn ingest_fragment(
        &mut self,
        frame: &[u8],
        now_ms: Millis,
    ) -> Result<Option<Vec<u8>>, FragError> {
        if frame.len() < FRAG_HEADER {
            return Ok(None);
        }
        let mut digest = [0u8; 8];
        digest.copy_from_slice(&frame[1..9]);
        let index = u16::from_be_bytes([frame[9], frame[10]]);
        let total = u16::from_be_bytes([frame[11], frame[12]]);
        let chunk = &frame[FRAG_HEADER..];

        if total == 0 || index >= total {
            return Err(FragError::BadIndex { index, total });
        }

        // Evict the oldest slot if we are at capacity and this is a new message.
        if !self.slots.contains_key(&digest) && self.slots.len() >= self.cfg.reassembly_slots {
            self.evict_oldest();
        }

        let cap = self.cfg.reassembly_max_bytes;
        let entry = self.slots.entry(digest).or_insert_with(|| Partial {
            total,
            received: 0,
            chunks: vec![None; total as usize],
            bytes: 0,
            first_seen_ms: now_ms,
        });

        // A peer changing `total` mid-message is malformed; drop the slot.
        if entry.total != total {
            self.slots.remove(&digest);
            return Err(FragError::BadIndex { index, total });
        }

        let slot = &mut entry.chunks[index as usize];
        if slot.is_none() {
            if entry.bytes + chunk.len() > cap {
                self.slots.remove(&digest);
                return Err(FragError::TooLarge { cap });
            }
            *slot = Some(chunk.to_vec());
            entry.received += 1;
            entry.bytes += chunk.len();
        }

        if entry.received == entry.total as usize {
            let done = self.slots.remove(&digest).unwrap();
            let mut out = Vec::with_capacity(done.bytes);
            for c in done.chunks {
                out.extend_from_slice(&c.unwrap());
            }
            return Ok(Some(out));
        }
        Ok(None)
    }

    /// Drop reassembly buffers that have been idle past the timeout. Call periodically.
    pub fn evict_expired(&mut self, now_ms: Millis) -> usize {
        let timeout = self.cfg.reassembly_timeout_ms;
        let before = self.slots.len();
        self.slots
            .retain(|_, p| now_ms.saturating_sub(p.first_seen_ms) < timeout);
        before - self.slots.len()
    }

    fn evict_oldest(&mut self) {
        if let Some((&k, _)) = self.slots.iter().min_by_key(|(_, p)| p.first_seen_ms) {
            self.slots.remove(&k);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Tunables {
        Tunables::default()
    }

    #[test]
    fn small_packet_is_single_complete_frame() {
        let bytes = vec![7u8; 100];
        let frames = split(&bytes, [0; 8], 182).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0][0], KIND_COMPLETE);
        let mut r = Reassembler::new(cfg());
        assert_eq!(r.ingest(&frames[0], 0).unwrap(), Some(bytes));
    }

    #[test]
    fn large_packet_fragments_and_reassembles() {
        let bytes: Vec<u8> = (0..2000).map(|i| (i % 251) as u8).collect();
        let digest = [1, 2, 3, 4, 5, 6, 7, 8];
        let frames = split(&bytes, digest, 182).unwrap();
        assert!(frames.len() > 1);
        let mut r = Reassembler::new(cfg());
        let mut result = None;
        for f in &frames {
            if let Some(done) = r.ingest(f, 0).unwrap() {
                result = Some(done);
            }
        }
        assert_eq!(result, Some(bytes));
        assert_eq!(r.in_flight(), 0);
    }

    #[test]
    fn reassembles_out_of_order() {
        let bytes: Vec<u8> = (0..1500).map(|i| (i % 251) as u8).collect();
        let mut frames = split(&bytes, [9; 8], 182).unwrap();
        frames.reverse();
        let mut r = Reassembler::new(cfg());
        let mut result = None;
        for f in &frames {
            if let Some(done) = r.ingest(f, 0).unwrap() {
                result = Some(done);
            }
        }
        assert_eq!(result, Some(bytes));
    }

    #[test]
    fn duplicate_fragments_ignored() {
        let bytes: Vec<u8> = (0..1500).map(|i| (i % 251) as u8).collect();
        let frames = split(&bytes, [3; 8], 182).unwrap();
        let mut r = Reassembler::new(cfg());
        // Feed every fragment twice except the last, then the last once.
        for f in &frames[..frames.len() - 1] {
            assert_eq!(r.ingest(f, 0).unwrap(), None);
            assert_eq!(r.ingest(f, 0).unwrap(), None); // dup, still incomplete
        }
        let done = r.ingest(&frames[frames.len() - 1], 0).unwrap();
        assert_eq!(done, Some(bytes));
    }

    #[test]
    fn expired_partial_is_evicted() {
        let bytes = vec![5u8; 1500];
        let frames = split(&bytes, [4; 8], 182).unwrap();
        let mut r = Reassembler::new(cfg());
        r.ingest(&frames[0], 0).unwrap();
        assert_eq!(r.in_flight(), 1);
        let evicted = r.evict_expired(31_000);
        assert_eq!(evicted, 1);
        assert_eq!(r.in_flight(), 0);
    }

    #[test]
    fn mtu_too_small_errors() {
        assert!(matches!(
            split(&[0; 100], [0; 8], 5),
            Err(FragError::MtuTooSmall(5))
        ));
    }

    #[test]
    fn bad_index_rejected() {
        let mut r = Reassembler::new(cfg());
        // total=2 but index=5
        let mut frame = vec![KIND_FRAGMENT];
        frame.extend_from_slice(&[0u8; 8]); // digest
        frame.extend_from_slice(&5u16.to_be_bytes());
        frame.extend_from_slice(&2u16.to_be_bytes());
        frame.push(0xAA);
        assert!(r.ingest(&frame, 0).is_err());
    }
}
