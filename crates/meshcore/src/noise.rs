//! End-to-end encryption for direct messages, using the Noise Protocol Framework (pattern
//! `Noise_XX_25519_ChaChaPoly_SHA256`) via the vetted `snow` crate.
//!
//! XX is mutually authenticated and hides both parties' static keys until the peer is
//! authenticated. Crucially, both sides transmit their static public key *inside* the handshake,
//! so we can bind that key to the peer's advertised identity (its announced X25519 key) and
//! reject a man-in-the-middle — the exact hole bitchat shipped with at launch.
//!
//! Handshake message flow (each `write_handshake` produces a packet to send; each
//! `read_handshake` consumes one):
//! ```text
//! initiator ->  msg1 (e)
//! responder <-  msg2 (e, ee, s, es)
//! initiator ->  msg3 (s, se)          both sides now in transport mode
//! ```

use crate::error::{Error, Result};
use snow::params::NoiseParams;
use snow::{Builder, HandshakeState, TransportState};

const PATTERN: &str = "Noise_XX_25519_ChaChaPoly_SHA256";

fn params() -> Result<NoiseParams> {
    PATTERN
        .parse()
        .map_err(|_| Error::Noise("bad noise params".into()))
}

fn map<E: std::fmt::Display>(e: E) -> Error {
    Error::Noise(e.to_string())
}

enum Inner {
    Handshake(Box<HandshakeState>),
    Transport(Box<TransportState>),
    /// Transient state only observed mid-transition; any operation on it errors.
    Poisoned,
}

/// One Noise session with a single peer. Drive the handshake with [`NoiseSession::write_handshake`]
/// / [`NoiseSession::read_handshake`], then [`NoiseSession::encrypt`] / [`NoiseSession::decrypt`].
pub struct NoiseSession {
    inner: Inner,
    pub initiator: bool,
}

impl NoiseSession {
    /// Start a session as the initiator, using our 32-byte X25519 static private key.
    pub fn new_initiator(local_private: &[u8; 32]) -> Result<Self> {
        let hs = Builder::new(params()?)
            .local_private_key(local_private)
            .build_initiator()
            .map_err(map)?;
        Ok(Self {
            inner: Inner::Handshake(Box::new(hs)),
            initiator: true,
        })
    }

    /// Start a session as the responder.
    pub fn new_responder(local_private: &[u8; 32]) -> Result<Self> {
        let hs = Builder::new(params()?)
            .local_private_key(local_private)
            .build_responder()
            .map_err(map)?;
        Ok(Self {
            inner: Inner::Handshake(Box::new(hs)),
            initiator: false,
        })
    }

    pub fn is_ready(&self) -> bool {
        matches!(self.inner, Inner::Transport(_))
    }

    pub fn is_handshaking(&self) -> bool {
        matches!(self.inner, Inner::Handshake(_))
    }

    /// The peer's static public key, once known (available from msg2/msg3 onward). Compare this to
    /// the peer's announced X25519 key to bind the encrypted channel to its signed identity.
    pub fn remote_static(&self) -> Option<[u8; 32]> {
        let rs = match &self.inner {
            Inner::Handshake(hs) => hs.get_remote_static(),
            Inner::Transport(ts) => ts.get_remote_static(),
            Inner::Poisoned => None,
        }?;
        if rs.len() == 32 {
            let mut out = [0u8; 32];
            out.copy_from_slice(rs);
            Some(out)
        } else {
            None
        }
    }

    /// Produce the next outbound handshake message.
    pub fn write_handshake(&mut self) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; 1024];
        let n = match &mut self.inner {
            Inner::Handshake(hs) => hs.write_message(&[], &mut buf).map_err(map)?,
            _ => return Err(Error::Noise("write_handshake after handshake".into())),
        };
        buf.truncate(n);
        self.maybe_transition()?;
        Ok(buf)
    }

    /// Consume an inbound handshake message.
    pub fn read_handshake(&mut self, msg: &[u8]) -> Result<()> {
        let mut buf = vec![0u8; 1024];
        match &mut self.inner {
            Inner::Handshake(hs) => {
                hs.read_message(msg, &mut buf).map_err(map)?;
            }
            _ => return Err(Error::Noise("read_handshake after handshake".into())),
        }
        self.maybe_transition()
    }

    /// Encrypt a plaintext DM (transport mode only).
    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; plaintext.len() + 64];
        match &mut self.inner {
            Inner::Transport(ts) => {
                let n = ts.write_message(plaintext, &mut buf).map_err(map)?;
                buf.truncate(n);
                Ok(buf)
            }
            _ => Err(Error::Noise("encrypt before handshake complete".into())),
        }
    }

    /// Decrypt a DM ciphertext (transport mode only).
    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; ciphertext.len() + 64];
        match &mut self.inner {
            Inner::Transport(ts) => {
                let n = ts.read_message(ciphertext, &mut buf).map_err(map)?;
                buf.truncate(n);
                Ok(buf)
            }
            _ => Err(Error::Noise("decrypt before handshake complete".into())),
        }
    }

    fn maybe_transition(&mut self) -> Result<()> {
        let finished = matches!(&self.inner, Inner::Handshake(hs) if hs.is_handshake_finished());
        if finished {
            let hs = match std::mem::replace(&mut self.inner, Inner::Poisoned) {
                Inner::Handshake(hs) => hs,
                other => {
                    self.inner = other;
                    return Ok(());
                }
            };
            let ts = hs.into_transport_mode().map_err(map)?;
            self.inner = Inner::Transport(Box::new(ts));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::LocalIdentity;

    /// Run the full XX handshake between two fresh sessions, returning them in transport mode.
    fn establish() -> (NoiseSession, NoiseSession, LocalIdentity, LocalIdentity) {
        let alice_id = LocalIdentity::from_seed(&[1u8; 32]);
        let bob_id = LocalIdentity::from_seed(&[2u8; 32]);
        let mut a = NoiseSession::new_initiator(&alice_id.dh_private_bytes()).unwrap();
        let mut b = NoiseSession::new_responder(&bob_id.dh_private_bytes()).unwrap();

        let m1 = a.write_handshake().unwrap(); // -> e
        b.read_handshake(&m1).unwrap();
        let m2 = b.write_handshake().unwrap(); // <- e, ee, s, es
        a.read_handshake(&m2).unwrap();
        let m3 = a.write_handshake().unwrap(); // -> s, se
        b.read_handshake(&m3).unwrap();

        assert!(a.is_ready() && b.is_ready());
        (a, b, alice_id, bob_id)
    }

    #[test]
    fn handshake_then_bidirectional_messages() {
        let (mut a, mut b, _, _) = establish();

        let ct = a.encrypt(b"meet at the north gate").unwrap();
        assert_ne!(&ct[..], b"meet at the north gate"); // actually encrypted
        assert_eq!(b.decrypt(&ct).unwrap(), b"meet at the north gate");

        let ct2 = b.encrypt(b"on my way").unwrap();
        assert_eq!(a.decrypt(&ct2).unwrap(), b"on my way");
    }

    #[test]
    fn remote_static_binds_to_identity() {
        // Each side's Noise remote static must equal the peer's announced X25519 key — this is the
        // hook the node uses to reject a MITM.
        let (a, b, alice_id, bob_id) = establish();
        assert_eq!(a.remote_static().unwrap(), *bob_id.dh_public().as_bytes());
        assert_eq!(b.remote_static().unwrap(), *alice_id.dh_public().as_bytes());
    }

    #[test]
    fn third_party_cannot_decrypt() {
        let (mut a, _b, _, _) = establish();
        let ct = a.encrypt(b"secret").unwrap();

        // Eve completes her own handshake with a fresh peer and tries to read Alice's ciphertext.
        let (_, mut eve, _, _) = establish();
        assert!(eve.decrypt(&ct).is_err());
    }

    #[test]
    fn tampered_ciphertext_rejected() {
        let (mut a, mut b, _, _) = establish();
        let mut ct = a.encrypt(b"authentic").unwrap();
        ct[0] ^= 0xff;
        assert!(b.decrypt(&ct).is_err());
    }

    #[test]
    fn encrypt_before_ready_errors() {
        let id = LocalIdentity::from_seed(&[9u8; 32]);
        let mut s = NoiseSession::new_initiator(&id.dh_private_bytes()).unwrap();
        assert!(s.encrypt(b"nope").is_err());
        assert!(s.is_handshaking());
    }
}
