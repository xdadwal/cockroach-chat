//! Flood control: the machinery that lets a packet reach a whole crowd without melting the
//! 2.4 GHz band. Four cooperating pieces, all pure and clock-injected:
//!
//! * [`SeenCache`] — dedup + per-message hear counter (drives suppression).
//! * [`RateLimiter`] — per-sender token bucket + greylist (defeats a flooding node).
//! * [`RelayScheduler`] — holds jittered rebroadcasts and cancels them once enough copies
//!   have been overheard (counter-based suppression).
//! * free functions [`rebroadcast_probability`] / [`relay_delay`] — density-adaptive fanout.

use crate::clock::Millis;
use crate::config::Tunables;
use crate::transport::LinkId;
use crate::wire::Packet;
use rand::Rng;
use std::collections::{BTreeMap, HashMap};

/// What the [`SeenCache`] knows about a digest right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Observation {
    /// True only the first time this digest is seen (i.e. deliver/relay this copy).
    pub is_new: bool,
    /// How many times this digest has been heard, including this observation.
    pub hear_count: u32,
}

struct SeenEntry {
    first_seen_ms: Millis,
    hear_count: u32,
}

/// LRU-with-TTL dedup cache keyed on the 8-byte packet digest. Uses a `BTreeMap` so eviction
/// tie-breaks (and therefore the whole simulation) are deterministic.
pub struct SeenCache {
    map: BTreeMap<[u8; 8], SeenEntry>,
    cfg: Tunables,
}

impl SeenCache {
    pub fn new(cfg: Tunables) -> Self {
        Self {
            map: BTreeMap::new(),
            cfg,
        }
    }

    pub fn observe(&mut self, digest: [u8; 8], now_ms: Millis) -> Observation {
        if let Some(e) = self.map.get_mut(&digest) {
            e.hear_count = e.hear_count.saturating_add(1);
            return Observation {
                is_new: false,
                hear_count: e.hear_count,
            };
        }
        if self.map.len() >= self.cfg.seen_cache_capacity {
            self.evict_oldest();
        }
        self.map.insert(
            digest,
            SeenEntry {
                first_seen_ms: now_ms,
                hear_count: 1,
            },
        );
        Observation {
            is_new: true,
            hear_count: 1,
        }
    }

    pub fn hear_count(&self, digest: &[u8; 8]) -> u32 {
        self.map.get(digest).map(|e| e.hear_count).unwrap_or(0)
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn evict_expired(&mut self, now_ms: Millis) -> usize {
        let ttl = self.cfg.seen_cache_ttl_ms;
        let before = self.map.len();
        self.map
            .retain(|_, e| now_ms.saturating_sub(e.first_seen_ms) < ttl);
        before - self.map.len()
    }

    fn evict_oldest(&mut self) {
        if let Some(k) = self
            .map
            .iter()
            .min_by_key(|(_, e)| e.first_seen_ms)
            .map(|(k, _)| *k)
        {
            self.map.remove(&k);
        }
    }
}

struct Bucket {
    tokens: f64,
    last_ms: Millis,
    greylist_until: Millis,
}

/// Per-sender token bucket. A node that exceeds its budget is denied and greylisted, which is
/// how a single malicious flooder is contained without central moderation.
pub struct RateLimiter {
    buckets: HashMap<[u8; 8], Bucket>,
    cfg: Tunables,
}

impl RateLimiter {
    pub fn new(cfg: Tunables) -> Self {
        Self {
            buckets: HashMap::new(),
            cfg,
        }
    }

    fn refill_rate_per_ms(&self) -> f64 {
        self.cfg.rate_sustained_per_min as f64 / 60_000.0
    }

    /// Returns true if a packet from `sender` is admitted now.
    pub fn allow(&mut self, sender: [u8; 8], now_ms: Millis) -> bool {
        let burst = self.cfg.rate_burst as f64;
        let rate = self.refill_rate_per_ms();
        let greylist = self.cfg.greylist_ms;
        let bucket = self.buckets.entry(sender).or_insert(Bucket {
            tokens: burst,
            last_ms: now_ms,
            greylist_until: 0,
        });

        if now_ms < bucket.greylist_until {
            return false;
        }
        let elapsed = now_ms.saturating_sub(bucket.last_ms) as f64;
        bucket.tokens = (bucket.tokens + elapsed * rate).min(burst);
        bucket.last_ms = now_ms;

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            true
        } else {
            bucket.greylist_until = now_ms + greylist;
            false
        }
    }

    pub fn is_greylisted(&self, sender: &[u8; 8], now_ms: Millis) -> bool {
        self.buckets
            .get(sender)
            .map(|b| now_ms < b.greylist_until)
            .unwrap_or(false)
    }
}

/// A rebroadcast waiting out its jitter window.
struct Pending {
    packet: Packet,
    send_at_ms: Millis,
    exclude_link: LinkId,
}

/// A rebroadcast that is due to go out now (on every link except `exclude_link`).
#[derive(Debug, Clone)]
pub struct DueRelay {
    pub packet: Packet,
    pub exclude_link: LinkId,
}

/// Holds jittered rebroadcasts and releases or suppresses them on [`RelayScheduler::due`]. Uses a
/// `BTreeMap` so the order relays fire within a tick is deterministic.
pub struct RelayScheduler {
    pending: BTreeMap<[u8; 8], Pending>,
    cfg: Tunables,
}

impl RelayScheduler {
    pub fn new(cfg: Tunables) -> Self {
        Self {
            pending: BTreeMap::new(),
            cfg,
        }
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Schedule `packet` (already TTL-decremented) to rebroadcast after `delay_ms`, skipping the
    /// link it arrived on (split-horizon).
    pub fn schedule(
        &mut self,
        digest: [u8; 8],
        packet: Packet,
        exclude_link: LinkId,
        delay_ms: Millis,
        now_ms: Millis,
    ) {
        self.pending.entry(digest).or_insert(Pending {
            packet,
            send_at_ms: now_ms + delay_ms,
            exclude_link,
        });
    }

    /// Release all rebroadcasts whose jitter has elapsed, dropping any that have since been
    /// overheard `suppression_threshold` times.
    pub fn due(&mut self, now_ms: Millis, seen: &SeenCache) -> Vec<DueRelay> {
        let threshold = self.cfg.suppression_threshold;
        let ready: Vec<[u8; 8]> = self
            .pending
            .iter()
            .filter(|(_, p)| p.send_at_ms <= now_ms)
            .map(|(k, _)| *k)
            .collect();

        let mut out = Vec::new();
        for digest in ready {
            let p = self.pending.remove(&digest).unwrap();
            if seen.hear_count(&digest) >= threshold {
                continue; // suppressed: enough neighbors already carried it
            }
            out.push(DueRelay {
                packet: p.packet,
                exclude_link: p.exclude_link,
            });
        }
        out
    }
}

/// Density-adaptive rebroadcast probability, read from [`Tunables`]. In a dense crowd most
/// rebroadcasts are pure collision, so we thin them as node degree rises.
pub fn rebroadcast_probability(degree: usize, cfg: &Tunables) -> f64 {
    if degree <= cfg.relay_sparse_max {
        cfg.relay_prob_sparse
    } else if degree <= cfg.relay_mid_max {
        cfg.relay_prob_mid
    } else {
        cfg.relay_prob_dense
    }
}

/// Coin-flip whether to relay at all, given local degree.
pub fn should_relay(degree: usize, cfg: &Tunables, rng: &mut impl Rng) -> bool {
    rng.gen::<f64>() < rebroadcast_probability(degree, cfg)
}

/// A random delay inside the configured jitter window.
pub fn relay_delay(cfg: &Tunables, rng: &mut impl Rng) -> Millis {
    if cfg.jitter_max_ms <= cfg.jitter_min_ms {
        return cfg.jitter_min_ms;
    }
    rng.gen_range(cfg.jitter_min_ms..=cfg.jitter_max_ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::MsgType;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn cfg() -> Tunables {
        Tunables::default()
    }

    fn pkt(ttl: u8) -> Packet {
        Packet::new(MsgType::ChannelMessage, ttl, 1, [1; 8], None, vec![1, 2, 3])
    }

    #[test]
    fn dedup_first_is_new_rest_are_not() {
        let mut c = SeenCache::new(cfg());
        let d = [1u8; 8];
        assert!(c.observe(d, 0).is_new);
        let o = c.observe(d, 1);
        assert!(!o.is_new);
        assert_eq!(o.hear_count, 2);
        assert_eq!(c.hear_count(&d), 2);
    }

    #[test]
    fn seen_cache_evicts_expired() {
        let mut c = SeenCache::new(cfg());
        c.observe([1; 8], 0);
        assert_eq!(c.len(), 1);
        c.evict_expired(5 * 60_000 + 1);
        assert_eq!(c.len(), 0);
    }

    #[test]
    fn rate_limiter_allows_burst_then_greylists() {
        let mut rl = RateLimiter::new(cfg());
        let s = [9u8; 8];
        // Burst of 10 should pass at t=0.
        for _ in 0..10 {
            assert!(rl.allow(s, 0));
        }
        // 11th at the same instant is denied and greylists.
        assert!(!rl.allow(s, 0));
        assert!(rl.is_greylisted(&s, 0));
        // Still greylisted a moment later.
        assert!(!rl.allow(s, 1000));
    }

    #[test]
    fn rate_limiter_refills_over_time() {
        let mut rl = RateLimiter::new(cfg());
        let s = [4u8; 8];
        for _ in 0..10 {
            assert!(rl.allow(s, 0));
        }
        assert!(!rl.allow(s, 0));
        // After the greylist expires and enough time to refill a token (~2s at 30/min).
        assert!(rl.allow(s, 63_000));
    }

    #[test]
    fn scheduler_releases_when_due() {
        let mut sched = RelayScheduler::new(cfg());
        let seen = SeenCache::new(cfg());
        let p = pkt(5);
        sched.schedule([2; 8], p, 1, 100, 0);
        assert!(sched.due(50, &seen).is_empty()); // not yet
        let due = sched.due(150, &seen);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].exclude_link, 1);
    }

    #[test]
    fn scheduler_suppresses_when_overheard() {
        let mut sched = RelayScheduler::new(cfg());
        let mut seen = SeenCache::new(cfg());
        let d = [3; 8];
        seen.observe(d, 0);
        sched.schedule(d, pkt(5), 1, 100, 0);
        // Overhear it 3 times (threshold) before it fires.
        seen.observe(d, 10);
        seen.observe(d, 20);
        assert_eq!(seen.hear_count(&d), 3);
        assert!(sched.due(150, &seen).is_empty()); // suppressed
    }

    #[test]
    fn probability_thins_with_density() {
        let c = cfg();
        assert_eq!(rebroadcast_probability(2, &c), 1.0);
        assert_eq!(rebroadcast_probability(5, &c), 0.7);
        assert_eq!(rebroadcast_probability(9, &c), 0.45);
    }

    #[test]
    fn relay_delay_within_window() {
        let c = cfg();
        let mut rng = StdRng::seed_from_u64(42);
        for _ in 0..100 {
            let d = relay_delay(&c, &mut rng);
            assert!(d >= c.jitter_min_ms && d <= c.jitter_max_ms);
        }
    }
}
