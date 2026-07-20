//! The one surface the platform implements. Everything above this line is pure Rust; below it
//! is CoreBluetooth (iOS) or the Android BLE stack (or [`SimTransport`] in the simulator).
//!
//! Outbound: the core hands the platform a frame + a link to send it on.
//! Inbound: the platform reports link up/down, received frames, and MTU changes.

/// An opaque handle to one active BLE connection. The platform assigns these; the core treats
/// them as opaque tokens.
pub type LinkId = u64;

/// Outbound side: the core calls this to push a frame onto a specific link.
pub trait Transport {
    /// Enqueue `frame` for delivery on `link`. Best-effort; the platform may drop it if the
    /// link has since died (the core will hear a `LinkDown` and recover).
    fn send(&self, link: LinkId, frame: &[u8]);
}

/// Inbound side: events the platform feeds into [`crate::node::MeshNode::on_transport_event`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportEvent {
    /// A new BLE connection is up. `mtu` is the usable ATT payload; `peer_hint` is any
    /// platform-visible address (often randomized and useless — identity comes from the
    /// signed Announce, not this).
    LinkUp {
        link: LinkId,
        mtu: usize,
        peer_hint: Option<[u8; 6]>,
    },
    /// A connection dropped.
    LinkDown { link: LinkId },
    /// A frame arrived on a link.
    FrameReceived { link: LinkId, frame: Vec<u8> },
    /// MTU renegotiated upward after connection (Android can reach 517).
    MtuChanged { link: LinkId, mtu: usize },
}
