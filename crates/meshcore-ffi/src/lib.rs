//! UniFFI bindings exposing `meshcore` to Kotlin (Android) and Swift (iOS).
//!
//! The core is generic (`MeshNode<T, C, S>`) and sans-IO; UniFFI cannot export generics, so this
//! crate pins concrete types and presents a flat, foreign-friendly surface:
//!
//! * [`FfiMeshNode`] — an `Arc`-shared object; every method locks an internal mutex, so the
//!   platform may call from any thread.
//! * [`BleTransport`] — a callback interface the platform implements (CoreBluetooth / Android
//!   BLE, or a loopback/TCP stand-in for the emulator, where BLE is unavailable).
//! * [`FfiEvent`] — drained by the UI via [`FfiMeshNode::poll_events`].

use meshcore::identity::LocalIdentity;
use meshcore::store::MemoryStore;
use meshcore::{Clock, MeshEvent, MeshNode, Transport, TransportEvent, Tunables};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

uniffi::setup_scaffolding!();

/// Implemented by the platform. The core hands it a link id + frame to put on the air.
#[uniffi::export(callback_interface)]
pub trait BleTransport: Send + Sync {
    fn send(&self, link: u64, frame: Vec<u8>);
}

/// Adapter from the core's [`Transport`] to the foreign [`BleTransport`].
struct FfiTransport {
    inner: Box<dyn BleTransport>,
}

impl Transport for FfiTransport {
    fn send(&self, link: meshcore::LinkId, frame: &[u8]) {
        self.inner.send(link, frame.to_vec());
    }
}

/// Wall-clock, read at the platform boundary. The core itself never calls this — it receives the
/// value through the injected [`Clock`] — so the sans-IO property is preserved.
struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}

/// UI-facing event. Byte identifiers are hex-encoded for easy display.
#[derive(uniffi::Enum)]
pub enum FfiEvent {
    Message {
        channel: String,
        sender: String,
        body: String,
        verified: bool,
        timestamp_ms: u64,
    },
    PeerAppeared {
        fingerprint: String,
        eph: String,
        petname: Option<String>,
    },
    PeerLost {
        link: u64,
    },
}

/// The FFI handle to a running mesh node.
#[derive(uniffi::Object)]
pub struct FfiMeshNode {
    inner: Mutex<MeshNode<FfiTransport, SystemClock, MemoryStore>>,
}

#[uniffi::export]
impl FfiMeshNode {
    /// Create a node with a deterministic identity derived from `seed`, a display `nickname`, and
    /// the platform `transport`.
    #[uniffi::constructor]
    pub fn new(seed: u64, nickname: String, transport: Box<dyn BleTransport>) -> Arc<Self> {
        let cfg = Tunables::default();
        let mut id_seed = [0u8; 32];
        id_seed[..8].copy_from_slice(&seed.to_le_bytes());
        let store = MemoryStore::new(
            cfg.channel_history_max_msgs,
            cfg.channel_history_ms,
            cfg.envelope_ttl_ms,
            cfg.envelope_max_per_peer,
        );
        let node = MeshNode::new(
            LocalIdentity::from_seed(&id_seed),
            cfg,
            FfiTransport { inner: transport },
            SystemClock,
            store,
            nickname,
            seed,
        );
        Arc::new(Self {
            inner: Mutex::new(node),
        })
    }

    /// Our current ephemeral wire id (hex).
    pub fn eph_id(&self) -> String {
        hex(&self.inner.lock().unwrap().eph_id())
    }

    pub fn join_channel(&self, name: String) {
        self.inner.lock().unwrap().join_channel(&name);
    }

    /// Originate a channel message; returns the message digest (hex).
    pub fn send_channel_message(&self, channel: String, text: String) -> String {
        hex(&self
            .inner
            .lock()
            .unwrap()
            .send_channel_message(&channel, &text))
    }

    /// Report a new link (BLE connection or stand-in) with its usable MTU.
    pub fn link_up(&self, link: u64, mtu: u32) {
        self.inner
            .lock()
            .unwrap()
            .on_transport_event(TransportEvent::LinkUp {
                link,
                mtu: mtu as usize,
                peer_hint: None,
            });
    }

    pub fn link_down(&self, link: u64) {
        self.inner
            .lock()
            .unwrap()
            .on_transport_event(TransportEvent::LinkDown { link });
    }

    /// Feed a frame received on a link into the core.
    pub fn receive_frame(&self, link: u64, frame: Vec<u8>) {
        self.inner
            .lock()
            .unwrap()
            .on_transport_event(TransportEvent::FrameReceived { link, frame });
    }

    /// Advance timers (releases due rebroadcasts); returns the suggested delay (ms) until the next
    /// tick.
    pub fn tick(&self) -> u64 {
        self.inner.lock().unwrap().tick()
    }

    /// Drain UI events accumulated since the last call.
    pub fn poll_events(&self) -> Vec<FfiEvent> {
        self.inner
            .lock()
            .unwrap()
            .take_events()
            .into_iter()
            .filter_map(to_ffi)
            .collect()
    }

    pub fn panic_wipe(&self) {
        self.inner.lock().unwrap().panic_wipe();
    }
}

fn to_ffi(e: MeshEvent) -> Option<FfiEvent> {
    match e {
        MeshEvent::MessageReceived {
            channel,
            sender,
            body,
            timestamp_ms,
            verified,
        } => Some(FfiEvent::Message {
            channel,
            sender: hex(&sender),
            body,
            verified,
            timestamp_ms,
        }),
        MeshEvent::PeerAppeared {
            fingerprint,
            eph,
            petname,
        } => Some(FfiEvent::PeerAppeared {
            fingerprint: hex(&fingerprint),
            eph: hex(&eph),
            petname,
        }),
        MeshEvent::PeerLost { link } => Some(FfiEvent::PeerLost { link }),
        MeshEvent::Stats { .. } => None,
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}
