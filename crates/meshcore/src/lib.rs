//! `meshcore` — the sans-IO protocol core for Cockroach Chat.
//!
//! This crate is pure: no threads, no async runtime, no clock syscalls. Time is injected
//! via [`clock::Clock`], and the network is injected via [`transport::Transport`]. That is
//! what lets the simulator drive hundreds of virtual nodes deterministically.
//!
//! Layering (bottom-up): [`wire`] (packet codec) → [`frag`] (link framing) →
//! [`identity`] (keys + proof-of-work) → [`relay`] (flood control) →
//! [`store`] / [`channels`] → [`node`] (the orchestrator).

pub mod channels;
pub mod clock;
pub mod compress;
pub mod config;
pub mod error;
pub mod frag;
pub mod identity;
pub mod node;
pub mod noise;
pub mod relay;
pub mod store;
pub mod transport;
pub mod wire;

pub use clock::{Clock, ManualClock};
pub use config::Tunables;
pub use error::{Error, Result};
pub use identity::LocalIdentity;
pub use node::{MeshEvent, MeshNode};
pub use transport::{LinkId, Transport, TransportEvent};
pub use wire::{EphId, MsgType, Packet, PROTOCOL_VERSION};
