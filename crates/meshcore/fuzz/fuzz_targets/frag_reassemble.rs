#![no_main]
//! Fuzzes fragment reassembly with an adversarial frame sequence.
//!
//! Reassembly holds attacker-controlled state across frames — out-of-order, duplicated, and
//! truncated fragments all land here — so bugs are stateful and won't show up from a single
//! input. The fuzzer feeds a whole sequence, splitting the input on a length byte, and advances
//! a manual clock so eviction paths get exercised too.

use libfuzzer_sys::fuzz_target;
use meshcore::{frag::Reassembler, Tunables};

fuzz_target!(|data: &[u8]| {
    let cfg = Tunables::default();
    let slots = cfg.reassembly_slots;
    let max_bytes = cfg.reassembly_max_bytes;
    let mut r = Reassembler::new(cfg);

    let mut now: u64 = 0;
    let mut rest = data;
    while !rest.is_empty() {
        // First byte is the frame length, so one input becomes a sequence of frames.
        let len = rest[0] as usize;
        rest = &rest[1..];
        let take = len.min(rest.len());
        let (frame, tail) = rest.split_at(take);
        rest = tail;

        if let Ok(Some(packet)) = r.ingest(frame, now) {
            assert!(
                packet.len() <= max_bytes,
                "reassembled {} bytes, above the {} cap",
                packet.len(),
                max_bytes,
            );
        }
        // In-flight slots are a memory bound: an attacker must not be able to grow them past
        // the configured cap by interleaving partial messages.
        assert!(r.in_flight() <= slots, "in-flight {} exceeds {} slots", r.in_flight(), slots);

        now = now.wrapping_add(u64::from(len as u8) * 97);
        r.evict_expired(now);
    }
});
