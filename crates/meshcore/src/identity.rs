//! Cryptographic identity: a long-term Ed25519 signing key + X25519 agreement key, a stable
//! fingerprint, a rotating ephemeral wire ID, and a hashcash proof-of-work used as anti-sybil
//! friction when minting an identity.
//!
//! The OS-level BLE MAC is useless as an identity (randomized ~every 15 min), so identity is
//! entirely app-layer: peers learn the `eph_id → static key` binding from a *signed* Announce.
//! The ephemeral ID is random per rotation, so it cannot be linked back to the fingerprint by
//! an observer.

use crate::wire::EphId;
use ed25519_dalek::{SigningKey, VerifyingKey};
use rand::RngCore;
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey as XPublic, StaticSecret as XSecret};
use zeroize::Zeroize;

/// SHA-256 of the Ed25519 public key. The stable, verifiable identity of a node.
pub type Fingerprint = [u8; 32];

pub struct LocalIdentity {
    signing: SigningKey,
    dh_secret: XSecret,
    eph_id: EphId,
}

impl LocalIdentity {
    /// Fresh random identity from the OS CSPRNG. Use on real devices.
    pub fn generate() -> Self {
        let mut seed = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut seed);
        let id = Self::from_seed(&seed);
        seed.zeroize();
        id
    }

    /// Deterministic identity from a 32-byte seed. Use in tests and the simulator so runs are
    /// reproducible.
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        let signing = SigningKey::from_bytes(seed);

        let mut dh_bytes = derive(seed, b"cockroach/dh/v1");
        let dh_secret = XSecret::from(dh_bytes);
        dh_bytes.zeroize();

        let mut eph = [0u8; 8];
        eph.copy_from_slice(&derive(seed, b"cockroach/eph/v1")[..8]);

        Self {
            signing,
            dh_secret,
            eph_id: eph,
        }
    }

    pub fn fingerprint(&self) -> Fingerprint {
        Sha256::digest(self.signing.verifying_key().as_bytes()).into()
    }

    pub fn verifying_key(&self) -> VerifyingKey {
        self.signing.verifying_key()
    }

    pub fn signing_key(&self) -> &SigningKey {
        &self.signing
    }

    pub fn dh_public(&self) -> XPublic {
        XPublic::from(&self.dh_secret)
    }

    pub fn eph_id(&self) -> EphId {
        self.eph_id
    }

    /// Rotate the ephemeral wire ID (call in lock-step with BLE MAC rotation).
    pub fn rotate_eph(&mut self, rng: &mut impl RngCore) {
        rng.fill_bytes(&mut self.eph_id);
    }
}

fn derive(seed: &[u8; 32], label: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(seed);
    h.update(label);
    h.finalize().into()
}

/// Hashcash-style proof of work over the static public key. Raises the cost of minting fresh
/// identities (sybil friction); it does not make sybil impossible without an out-of-band CA.
pub mod pow {
    use super::*;

    fn leading_zero_bits(bytes: &[u8]) -> u32 {
        let mut count = 0;
        for &b in bytes {
            if b == 0 {
                count += 8;
            } else {
                count += b.leading_zeros();
                break;
            }
        }
        count
    }

    fn hash(pubkey: &[u8], nonce: u64) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(pubkey);
        h.update(nonce.to_be_bytes());
        h.finalize().into()
    }

    /// Find a nonce whose `SHA-256(pubkey || nonce)` has at least `bits` leading zero bits.
    pub fn mint(pubkey: &[u8], bits: u32) -> u64 {
        let mut nonce = 0u64;
        loop {
            if leading_zero_bits(&hash(pubkey, nonce)) >= bits {
                return nonce;
            }
            nonce = nonce.wrapping_add(1);
        }
    }

    /// Check a claimed proof of work.
    pub fn verify(pubkey: &[u8], nonce: u64, bits: u32) -> bool {
        leading_zero_bits(&hash(pubkey, nonce)) >= bits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_from_seed() {
        let a = LocalIdentity::from_seed(&[7u8; 32]);
        let b = LocalIdentity::from_seed(&[7u8; 32]);
        assert_eq!(a.fingerprint(), b.fingerprint());
        assert_eq!(a.eph_id(), b.eph_id());
        assert_eq!(a.dh_public().as_bytes(), b.dh_public().as_bytes());
    }

    #[test]
    fn distinct_seeds_differ() {
        let a = LocalIdentity::from_seed(&[1u8; 32]);
        let b = LocalIdentity::from_seed(&[2u8; 32]);
        assert_ne!(a.fingerprint(), b.fingerprint());
    }

    #[test]
    fn sign_and_verify_roundtrip() {
        use crate::wire::{MsgType, Packet};
        let id = LocalIdentity::from_seed(&[5u8; 32]);
        let mut p = Packet::new(MsgType::Announce, 7, 1, id.eph_id(), None, vec![1, 2, 3]);
        p.sign(id.signing_key());
        p.verify(&id.verifying_key()).unwrap();
    }

    #[test]
    fn eph_rotation_changes_id() {
        use rand::rngs::StdRng;
        use rand::SeedableRng;
        let mut id = LocalIdentity::from_seed(&[9u8; 32]);
        let before = id.eph_id();
        let mut rng = StdRng::seed_from_u64(1);
        id.rotate_eph(&mut rng);
        assert_ne!(id.eph_id(), before);
        // Fingerprint (long-term identity) is unaffected by rotation.
    }

    #[test]
    fn pow_mint_then_verify() {
        let pk = [0xABu8; 32];
        let bits = 8; // small for a fast test
        let nonce = pow::mint(&pk, bits);
        assert!(pow::verify(&pk, nonce, bits));
        assert!(!pow::verify(&pk, nonce.wrapping_add(1), 24));
    }
}
