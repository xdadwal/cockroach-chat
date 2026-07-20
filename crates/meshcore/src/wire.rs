//! The binary packet format. Hand-rolled and fixed-offset — no varints, no schema evolution
//! magic — so it is trivial to fuzz and to reimplement from `docs/protocol.md`.
//!
//! Wire layout (big-endian), matching `docs/protocol.md` v0.1:
//! ```text
//! version(1) | type(1) | flags(1) | TTL(1) | timestamp_ms(8) | sender(8)
//!   | [recipient(8) if FLAG_HAS_RECIPIENT] | payload_len(2) | payload(len) | signature(64)
//! ```
//! The **TTL byte is excluded from the signature** (signed as zero), so a relay can decrement
//! it without re-signing. The dedup key is the first 8 bytes of SHA-256(signature).

use crate::error::WireError;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};

pub const PROTOCOL_VERSION: u8 = 1;

/// Rotating 8-byte ephemeral wire identifier (rotated with the BLE MAC, ~every 15 min).
pub type EphId = [u8; 8];

/// The broadcast recipient sentinel (absence of a recipient == broadcast).
pub const BROADCAST: Option<EphId> = None;

pub mod flags {
    pub const HAS_RECIPIENT: u8 = 0b0000_0001;
    pub const COMPRESSED: u8 = 0b0000_0010;
    /// Reserved for a future post-quantum handshake; must be zero for now.
    pub const RESERVED_PQ: u8 = 0b0000_0100;
}

const SIG_LEN: usize = 64;
const MIN_HEADER: usize = 1 + 1 + 1 + 1 + 8 + 8 + 2; // through payload_len, no recipient

/// Message types. Unknown types are preserved as [`MsgType::Unknown`] so a relay forwards a
/// newer node's traffic instead of silently dropping it (the bitchat interop-bug lesson).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsgType {
    Announce,
    ChannelMessage,
    DirectMessage,
    NoiseHandshake,
    Receipt,
    SyncRequest,
    SyncResponse,
    MediaOffer,
    MediaFetchRequest,
    MediaChunk,
    MediaFetchAck,
    Unknown(u8),
}

impl MsgType {
    pub fn from_u8(b: u8) -> Self {
        match b {
            0x01 => Self::Announce,
            0x02 => Self::ChannelMessage,
            0x03 => Self::DirectMessage,
            0x04 => Self::NoiseHandshake,
            0x05 => Self::Receipt,
            0x06 => Self::SyncRequest,
            0x07 => Self::SyncResponse,
            0x10 => Self::MediaOffer,
            0x11 => Self::MediaFetchRequest,
            0x12 => Self::MediaChunk,
            0x13 => Self::MediaFetchAck,
            other => Self::Unknown(other),
        }
    }

    pub fn to_u8(self) -> u8 {
        match self {
            Self::Announce => 0x01,
            Self::ChannelMessage => 0x02,
            Self::DirectMessage => 0x03,
            Self::NoiseHandshake => 0x04,
            Self::Receipt => 0x05,
            Self::SyncRequest => 0x06,
            Self::SyncResponse => 0x07,
            Self::MediaOffer => 0x10,
            Self::MediaFetchRequest => 0x11,
            Self::MediaChunk => 0x12,
            Self::MediaFetchAck => 0x13,
            Self::Unknown(b) => b,
        }
    }

    pub fn is_known(self) -> bool {
        !matches!(self, Self::Unknown(_))
    }
}

/// A fully-formed packet. Construct with [`Packet::new`], then [`Packet::sign`] to fill the
/// signature, then [`Packet::encode`]. Decode untrusted bytes with [`Packet::decode`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    pub version: u8,
    pub msg_type: MsgType,
    pub flags: u8,
    pub ttl: u8,
    pub timestamp_ms: u64,
    pub sender: EphId,
    pub recipient: Option<EphId>,
    pub payload: Vec<u8>,
    /// 64-byte Ed25519 signature. All-zero until [`Packet::sign`] is called.
    pub signature: [u8; SIG_LEN],
}

impl Packet {
    /// Build an unsigned packet. `flags` need not set `HAS_RECIPIENT`; it is derived from
    /// `recipient` at encode time.
    pub fn new(
        msg_type: MsgType,
        ttl: u8,
        timestamp_ms: u64,
        sender: EphId,
        recipient: Option<EphId>,
        payload: Vec<u8>,
    ) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            msg_type,
            flags: 0,
            ttl,
            timestamp_ms,
            sender,
            recipient,
            payload,
            signature: [0u8; SIG_LEN],
        }
    }

    fn effective_flags(&self) -> u8 {
        let mut f = self.flags & !flags::HAS_RECIPIENT;
        if self.recipient.is_some() {
            f |= flags::HAS_RECIPIENT;
        }
        f
    }

    /// Serialize header + payload. `ttl` is written as `ttl_override` when signing (0), or the
    /// real value when encoding for the wire.
    fn write_body(&self, buf: &mut Vec<u8>, ttl_override: Option<u8>) {
        buf.push(self.version);
        buf.push(self.msg_type.to_u8());
        buf.push(self.effective_flags());
        buf.push(ttl_override.unwrap_or(self.ttl));
        buf.extend_from_slice(&self.timestamp_ms.to_be_bytes());
        buf.extend_from_slice(&self.sender);
        if let Some(r) = self.recipient {
            buf.extend_from_slice(&r);
        }
        buf.extend_from_slice(&(self.payload.len() as u16).to_be_bytes());
        buf.extend_from_slice(&self.payload);
    }

    /// The exact bytes covered by the signature (TTL forced to zero, no signature trailer).
    fn signed_region(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(MIN_HEADER + 8 + self.payload.len());
        self.write_body(&mut buf, Some(0));
        buf
    }

    /// Sign in place with the node's static Ed25519 key.
    pub fn sign(&mut self, key: &SigningKey) {
        let sig = key.sign(&self.signed_region());
        self.signature = sig.to_bytes();
    }

    /// Verify the signature against a claimed static public key.
    pub fn verify(&self, key: &VerifyingKey) -> Result<(), WireError> {
        let sig = Signature::from_bytes(&self.signature);
        key.verify(&self.signed_region(), &sig)
            .map_err(|_| WireError::BadSignature)
    }

    /// Full wire encoding including the real TTL and the signature trailer.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(MIN_HEADER + 8 + self.payload.len() + SIG_LEN);
        self.write_body(&mut buf, None);
        buf.extend_from_slice(&self.signature);
        buf
    }

    /// Parse untrusted bytes. Rejects unknown *versions*; preserves unknown *types*.
    pub fn decode(bytes: &[u8]) -> Result<Packet, WireError> {
        let mut off = 0usize;
        let need = |off: usize, n: usize, have: usize| -> Result<(), WireError> {
            if off + n > have {
                Err(WireError::Short {
                    need: off + n,
                    have,
                })
            } else {
                Ok(())
            }
        };
        let have = bytes.len();

        need(off, MIN_HEADER, have)?;
        let version = bytes[off];
        off += 1;
        if version != PROTOCOL_VERSION {
            return Err(WireError::BadVersion(version));
        }
        let msg_type = MsgType::from_u8(bytes[off]);
        off += 1;
        let flags = bytes[off];
        off += 1;
        let ttl = bytes[off];
        off += 1;

        let mut ts = [0u8; 8];
        ts.copy_from_slice(&bytes[off..off + 8]);
        let timestamp_ms = u64::from_be_bytes(ts);
        off += 8;

        let mut sender = [0u8; 8];
        sender.copy_from_slice(&bytes[off..off + 8]);
        off += 8;

        let recipient = if flags & flags::HAS_RECIPIENT != 0 {
            need(off, 8, have)?;
            let mut r = [0u8; 8];
            r.copy_from_slice(&bytes[off..off + 8]);
            off += 8;
            Some(r)
        } else {
            None
        };

        need(off, 2, have)?;
        let payload_len = u16::from_be_bytes([bytes[off], bytes[off + 1]]) as usize;
        off += 2;

        let remaining = have.saturating_sub(off);
        if payload_len + SIG_LEN > remaining {
            return Err(WireError::BadLength {
                declared: payload_len,
                remaining,
            });
        }
        let payload = bytes[off..off + payload_len].to_vec();
        off += payload_len;

        let mut signature = [0u8; SIG_LEN];
        signature.copy_from_slice(&bytes[off..off + SIG_LEN]);
        off += SIG_LEN;

        if off != have {
            return Err(WireError::Trailing(have - off));
        }

        Ok(Packet {
            version,
            msg_type,
            flags: flags & !flags::HAS_RECIPIENT,
            ttl,
            timestamp_ms,
            sender,
            recipient,
            payload,
            signature,
        })
    }

    /// The 8-byte dedup digest: first 8 bytes of SHA-256 over the signature. Stable across
    /// relays because the signature is TTL-independent.
    pub fn digest(&self) -> [u8; 8] {
        let hash = Sha256::digest(self.signature);
        let mut out = [0u8; 8];
        out.copy_from_slice(&hash[..8]);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    fn key() -> SigningKey {
        SigningKey::generate(&mut OsRng)
    }

    fn sample(recipient: Option<EphId>) -> Packet {
        Packet::new(
            MsgType::ChannelMessage,
            7,
            1_700_000_000_000,
            [1, 2, 3, 4, 5, 6, 7, 8],
            recipient,
            b"hello mesh".to_vec(),
        )
    }

    #[test]
    fn roundtrip_broadcast() {
        let sk = key();
        let mut p = sample(None);
        p.sign(&sk);
        let bytes = p.encode();
        let d = Packet::decode(&bytes).unwrap();
        assert_eq!(d, p);
        d.verify(&sk.verifying_key()).unwrap();
    }

    #[test]
    fn roundtrip_directed() {
        let sk = key();
        let mut p = sample(Some([9, 9, 9, 9, 9, 9, 9, 9]));
        p.sign(&sk);
        let d = Packet::decode(&p.encode()).unwrap();
        assert_eq!(d.recipient, Some([9; 8]));
        d.verify(&sk.verifying_key()).unwrap();
    }

    #[test]
    fn ttl_excluded_from_signature() {
        // A relay decrements TTL; the signature must still verify.
        let sk = key();
        let mut p = sample(None);
        p.sign(&sk);
        let mut relayed = Packet::decode(&p.encode()).unwrap();
        relayed.ttl -= 1;
        relayed.verify(&sk.verifying_key()).unwrap();
        // Digest is stable across the TTL change.
        assert_eq!(relayed.digest(), p.digest());
    }

    #[test]
    fn tampered_payload_fails_verify() {
        let sk = key();
        let mut p = sample(None);
        p.sign(&sk);
        let mut bytes = p.encode();
        // Flip a payload byte (payload starts after the fixed header).
        let idx = MIN_HEADER + 2;
        bytes[idx] ^= 0xff;
        let d = Packet::decode(&bytes).unwrap();
        assert!(d.verify(&sk.verifying_key()).is_err());
    }

    #[test]
    fn unknown_type_preserved() {
        let sk = key();
        let mut p = Packet::new(MsgType::Unknown(0x7f), 3, 1, [0; 8], None, vec![1, 2, 3]);
        p.sign(&sk);
        let d = Packet::decode(&p.encode()).unwrap();
        assert_eq!(d.msg_type, MsgType::Unknown(0x7f));
        assert!(!d.msg_type.is_known());
    }

    #[test]
    fn bad_version_rejected() {
        let sk = key();
        let mut p = sample(None);
        p.sign(&sk);
        let mut bytes = p.encode();
        bytes[0] = 0xEE;
        assert_eq!(Packet::decode(&bytes), Err(WireError::BadVersion(0xEE)));
    }

    #[test]
    fn short_buffer_rejected_not_panic() {
        for n in 0..40 {
            let sk = key();
            let mut p = sample(None);
            p.sign(&sk);
            let bytes = p.encode();
            let truncated = &bytes[..n.min(bytes.len())];
            // Must be an error, never a panic.
            let _ = Packet::decode(truncated);
        }
    }

    #[test]
    fn trailing_bytes_rejected() {
        let sk = key();
        let mut p = sample(None);
        p.sign(&sk);
        let mut bytes = p.encode();
        bytes.push(0x00);
        assert_eq!(Packet::decode(&bytes), Err(WireError::Trailing(1)));
    }

    #[test]
    fn oversized_length_rejected() {
        let sk = key();
        let mut p = sample(None);
        p.sign(&sk);
        let mut bytes = p.encode();
        // Corrupt payload_len to a huge value. Locate the payload_len field.
        let len_off = MIN_HEADER - 2; // no recipient
        bytes[len_off] = 0xff;
        bytes[len_off + 1] = 0xff;
        assert!(matches!(
            Packet::decode(&bytes),
            Err(WireError::BadLength { .. })
        ));
    }
}
