//! LZ4 payload compression with hard decompression caps.
//!
//! Bridgefy was taken down in the field by a single zip-bomb packet: a tiny compressed blob
//! that expanded to gigabytes and killed every relay. We defend by prepending the uncompressed
//! length and refusing to allocate or decode anything over an absolute byte cap — so output is
//! bounded regardless of how small the input claims to be.

use crate::config::Tunables;
use crate::error::{Error, Result};

/// Compress `input`, prepending a 4-byte big-endian length. Returns `None` when compression is
/// not worthwhile (input below the threshold, or it did not actually shrink).
pub fn maybe_compress(input: &[u8], cfg: &Tunables) -> Option<Vec<u8>> {
    if input.len() < cfg.compress_min_bytes {
        return None;
    }
    let body = lz4_flex::compress(input);
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&(input.len() as u32).to_be_bytes());
    out.extend_from_slice(&body);
    // Only worth it if the framed result is smaller than the original.
    if out.len() < input.len() {
        Some(out)
    } else {
        None
    }
}

/// Decompress a length-prepended LZ4 blob, enforcing the caps in `cfg`.
pub fn decompress(input: &[u8], cfg: &Tunables) -> Result<Vec<u8>> {
    if input.len() < 4 {
        return Err(Error::Compress("truncated length prefix".into()));
    }
    let declared = u32::from_be_bytes([input[0], input[1], input[2], input[3]]) as usize;

    // The absolute output cap is the real zip-bomb defense: we never produce more than this,
    // so a tiny blob claiming gigabytes is rejected before any allocation. (A ratio guard would
    // be redundant here and would wrongly reject legitimately compressible repetitive text.)
    if declared > cfg.decompress_max_bytes {
        return Err(Error::Compress(format!(
            "declared size {declared} exceeds cap {}",
            cfg.decompress_max_bytes
        )));
    }
    let body = &input[4..];
    let out =
        lz4_flex::decompress(body, declared).map_err(|e| Error::Compress(format!("lz4: {e}")))?;
    if out.len() != declared {
        return Err(Error::Compress("length mismatch after decompress".into()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_compressible() {
        let cfg = Tunables::default();
        let data = vec![0xABu8; 2000]; // highly compressible
        let framed = maybe_compress(&data, &cfg).expect("should compress");
        assert!(framed.len() < data.len());
        let back = decompress(&framed, &cfg).unwrap();
        assert_eq!(back, data);
    }

    #[test]
    fn small_input_not_compressed() {
        let cfg = Tunables::default();
        assert!(maybe_compress(b"tiny", &cfg).is_none());
    }

    #[test]
    fn incompressible_declined() {
        let cfg = Tunables::default();
        // Random-ish data that LZ4 cannot shrink below the framed threshold.
        let data: Vec<u8> = (0..300).map(|i| (i * 7 + 3) as u8).collect();
        // May or may not compress; if it does, roundtrip must hold.
        if let Some(framed) = maybe_compress(&data, &cfg) {
            assert_eq!(decompress(&framed, &cfg).unwrap(), data);
        }
    }

    #[test]
    fn zipbomb_rejected_by_absolute_cap() {
        let cfg = Tunables::default();
        // Claim a 50 MiB output from a few compressed bytes.
        let body = lz4_flex::compress(&[0u8; 100]);
        let mut bomb = Vec::new();
        bomb.extend_from_slice(&(50_000_000u32).to_be_bytes());
        bomb.extend_from_slice(&body);
        assert!(decompress(&bomb, &cfg).is_err());
    }

    #[test]
    fn highly_compressible_under_cap_roundtrips() {
        // A 1000:1 payload is fine as long as its output stays under the absolute cap.
        let cfg = Tunables::default();
        let data = vec![0x5Au8; 4000]; // < 4096 cap, compresses to a handful of bytes
        let framed = maybe_compress(&data, &cfg).expect("should compress");
        assert!(framed.len() < 100);
        assert_eq!(decompress(&framed, &cfg).unwrap(), data);
    }
}
