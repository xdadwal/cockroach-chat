#![no_main]
//! Fuzzes LZ4 decompression and its caps.
//!
//! A zip bomb took down Bridgefy's entire mesh. `decompress` refuses any blob whose *declared*
//! output exceeds the configured cap, so output is bounded regardless of input — this target
//! asserts that invariant holds for arbitrary bytes, including a hostile length prefix.

use libfuzzer_sys::fuzz_target;
use meshcore::{compress, Tunables};

fuzz_target!(|data: &[u8]| {
    let cfg = Tunables::default();
    if let Ok(out) = compress::decompress(data, &cfg) {
        // The absolute cap is the zip-bomb defense. If a crafted input can ever exceed it,
        // the whole mesh is one message away from an OOM.
        assert!(
            out.len() <= cfg.decompress_max_bytes,
            "decompressed {} bytes, above the {} cap",
            out.len(),
            cfg.decompress_max_bytes,
        );
    }
});
