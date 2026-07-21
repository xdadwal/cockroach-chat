//! Crate-wide error type.

use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum Error {
    #[error("wire: {0}")]
    Wire(#[from] WireError),

    #[error("fragmentation: {0}")]
    Frag(#[from] FragError),

    #[error("identity: {0}")]
    Identity(String),

    #[error("compression: {0}")]
    Compress(String),

    #[error("rate limited")]
    RateLimited,

    #[error("store: {0}")]
    Store(String),

    #[error("noise: {0}")]
    Noise(String),
}

/// Errors from decoding untrusted bytes off the wire. These must never panic — they are the
/// most-attacked surface in the whole system (a malformed packet from a hostile relay).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum WireError {
    #[error("buffer too short: need {need} bytes, have {have}")]
    Short { need: usize, have: usize },

    #[error("unsupported protocol version {0} (expected {expected})", expected = crate::wire::PROTOCOL_VERSION)]
    BadVersion(u8),

    #[error("declared payload length {declared} exceeds remaining {remaining}")]
    BadLength { declared: usize, remaining: usize },

    #[error("trailing bytes after packet ({0} extra)")]
    Trailing(usize),

    #[error("bad signature")]
    BadSignature,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FragError {
    #[error("mtu {0} too small to carry any payload")]
    MtuTooSmall(usize),

    #[error("fragment index {index} >= total {total}")]
    BadIndex { index: u16, total: u16 },

    #[error("reassembly exceeds cap of {cap} bytes")]
    TooLarge { cap: usize },
}
