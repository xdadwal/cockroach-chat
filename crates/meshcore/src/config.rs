//! Every tunable protocol constant lives here, in one struct, so the simulator can sweep them
//! and so there are no magic numbers scattered through the code. Defaults come from
//! `docs/protocol.md` v0.1, which in turn come from `docs/research-brief.md`.

use crate::clock::Millis;
use crate::wire::MsgType;

#[derive(Debug, Clone)]
pub struct Tunables {
    // --- link framing ---
    /// Usable ATT payload assumed before MTU negotiation completes (iOS 185 MTU − 3).
    pub default_mtu: usize,
    /// Max concurrent reassembly buffers.
    pub reassembly_slots: usize,
    /// Drop a partial reassembly after this long without progress.
    pub reassembly_timeout_ms: Millis,
    /// Hard cap on a single reassembled message.
    pub reassembly_max_bytes: usize,

    // --- relay / flood control ---
    /// Origination TTLs, per message type. Announce defines presence/discovery radius (largest);
    /// channels serve a local crowd; DM covers DirectMessage AND NoiseHandshake (a DM cannot
    /// exist without its handshake, so they must never diverge). The `_dense` variants apply at
    /// local degree >= `dense_degree`, where flooding is cheap and airtime is scarce.
    pub ttl_announce: u8,
    pub ttl_announce_dense: u8,
    pub ttl_channel: u8,
    pub ttl_channel_dense: u8,
    pub ttl_dm: u8,
    pub ttl_dm_dense: u8,
    /// Node degree at/above which the dense clamp applies.
    pub dense_degree: usize,
    /// Relay jitter window [min, max] ms before rebroadcast.
    pub jitter_min_ms: Millis,
    pub jitter_max_ms: Millis,
    /// Cancel a scheduled rebroadcast once this many duplicates have been heard.
    pub suppression_threshold: u32,
    /// Density-adaptive rebroadcast probability. `sparse` applies at degree ≤ `relay_sparse_max`,
    /// `mid` up to `relay_mid_max`, `dense` beyond. Thinning rebroadcasts as the crowd densifies
    /// is what keeps the broadcast storm from collapsing the channel.
    pub relay_prob_sparse: f64,
    pub relay_prob_mid: f64,
    pub relay_prob_dense: f64,
    pub relay_sparse_max: usize,
    pub relay_mid_max: usize,
    /// Seen-cache capacity (LRU) and entry lifetime.
    pub seen_cache_capacity: usize,
    pub seen_cache_ttl_ms: Millis,

    // --- rate limiting ---
    /// Token-bucket burst and sustained refill (packets) per remote sender.
    pub rate_burst: u32,
    pub rate_sustained_per_min: u32,
    /// How long a sender that exceeds its budget is greylisted.
    pub greylist_ms: Millis,

    // --- identity proof-of-work ---
    /// Leading zero bits required when minting an identity (anti-sybil friction).
    pub pow_bits: u32,

    // --- compression ---
    /// Only attempt LZ4 when payload exceeds this.
    pub compress_min_bytes: usize,
    /// Reject anything claiming to decompress beyond this (the zip-bomb defense).
    pub decompress_max_bytes: usize,

    // --- retention ---
    pub channel_history_ms: Millis,
    pub channel_history_max_msgs: usize,
    pub envelope_ttl_ms: Millis,
    pub envelope_max_per_peer: usize,
}

impl Default for Tunables {
    fn default() -> Self {
        Self {
            default_mtu: 182,
            reassembly_slots: 128,
            reassembly_timeout_ms: 30_000,
            reassembly_max_bytes: 1 << 20, // 1 MiB
            ttl_announce: 15,
            ttl_announce_dense: 12,
            ttl_channel: 10,
            ttl_channel_dense: 7,
            ttl_dm: 10,
            ttl_dm_dense: 7,
            dense_degree: 6,
            jitter_min_ms: 10,
            jitter_max_ms: 220,
            suppression_threshold: 3,
            relay_prob_sparse: 1.0,
            relay_prob_mid: 0.7,
            relay_prob_dense: 0.45,
            relay_sparse_max: 3,
            relay_mid_max: 6,
            seen_cache_capacity: 1000,
            seen_cache_ttl_ms: 5 * 60_000,
            rate_burst: 10,
            rate_sustained_per_min: 30,
            greylist_ms: 60_000,
            pow_bits: 22,
            compress_min_bytes: 128,
            decompress_max_bytes: 4096,
            channel_history_ms: 6 * 3_600_000,
            channel_history_max_msgs: 1000,
            envelope_ttl_ms: 24 * 3_600_000,
            envelope_max_per_peer: 100,
        }
    }
}

impl Tunables {
    /// The TTL to originate a packet of `msg_type` with, given current local node degree.
    /// Types not listed here are link-local (TTL 1) by design — list a type explicitly to
    /// let it flood.
    pub fn origin_ttl(&self, msg_type: MsgType, degree: usize) -> u8 {
        let dense = degree >= self.dense_degree;
        match msg_type {
            MsgType::Announce => {
                if dense {
                    self.ttl_announce_dense
                } else {
                    self.ttl_announce
                }
            }
            MsgType::ChannelMessage => {
                if dense {
                    self.ttl_channel_dense
                } else {
                    self.ttl_channel
                }
            }
            MsgType::DirectMessage | MsgType::NoiseHandshake => {
                if dense {
                    self.ttl_dm_dense
                } else {
                    self.ttl_dm
                }
            }
            _ => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::MsgType;

    #[test]
    fn origin_ttl_is_per_message_type() {
        let c = Tunables::default();
        // Sparse (degree < dense_degree).
        assert_eq!(c.origin_ttl(MsgType::Announce, 2), 15);
        assert_eq!(c.origin_ttl(MsgType::ChannelMessage, 2), 10);
        assert_eq!(c.origin_ttl(MsgType::DirectMessage, 2), 10);
        assert_eq!(c.origin_ttl(MsgType::NoiseHandshake, 2), 10);
    }

    #[test]
    fn origin_ttl_clamps_in_dense_crowds() {
        let c = Tunables::default();
        assert_eq!(c.origin_ttl(MsgType::Announce, 6), 12);
        assert_eq!(c.origin_ttl(MsgType::ChannelMessage, 6), 7);
        assert_eq!(c.origin_ttl(MsgType::DirectMessage, 6), 7);
        assert_eq!(c.origin_ttl(MsgType::NoiseHandshake, 6), 7);
    }

    #[test]
    fn handshake_ttl_always_equals_dm_ttl() {
        // DMs cannot exist without a completed handshake, so the two must never diverge.
        let c = Tunables::default();
        for degree in [0, 3, 6, 12] {
            assert_eq!(
                c.origin_ttl(MsgType::NoiseHandshake, degree),
                c.origin_ttl(MsgType::DirectMessage, degree),
            );
        }
    }

    #[test]
    fn unlisted_types_default_to_link_local() {
        // Safe-by-default: a future type must be listed explicitly to flood.
        let c = Tunables::default();
        assert_eq!(c.origin_ttl(MsgType::SyncRequest, 2), 1);
    }
}
