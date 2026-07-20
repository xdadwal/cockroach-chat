//! Every tunable protocol constant lives here, in one struct, so the simulator can sweep them
//! and so there are no magic numbers scattered through the code. Defaults come from
//! `docs/protocol.md` v0.1, which in turn come from `docs/research-brief.md`.

use crate::clock::Millis;

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
    /// Initial time-to-live (hops) for originated packets.
    pub ttl_default: u8,
    /// Clamp TTL to this when local node degree is high (dense crowd).
    pub ttl_dense_clamp: u8,
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
            ttl_default: 7,
            ttl_dense_clamp: 5,
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
    /// The effective TTL to originate with, given the current local node degree.
    pub fn origin_ttl(&self, degree: usize) -> u8 {
        if degree >= self.dense_degree {
            self.ttl_default.min(self.ttl_dense_clamp)
        } else {
            self.ttl_default
        }
    }
}
