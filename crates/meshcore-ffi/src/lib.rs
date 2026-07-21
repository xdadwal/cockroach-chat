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

use meshcore::clock::Millis;
use meshcore::identity::{Fingerprint, LocalIdentity};
use meshcore::store::{MemoryStore, PeerRecord, Store, StoredMessage};
use meshcore::{Clock, MeshEvent, MeshNode, Transport, TransportEvent, Tunables};
use meshcore_store::SqliteStore;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

uniffi::setup_scaffolding!();

/// Implemented by the platform. The core hands it a link id + frame to put on the air.
#[uniffi::export(callback_interface)]
pub trait BleTransport: Send + Sync {
    fn send(&self, link: u64, frame: Vec<u8>);
    /// Tear down a redundant link (the core deduped it to another link with the same peer).
    fn close(&self, link: u64);
}

/// Adapter from the core's [`Transport`] to the foreign [`BleTransport`].
struct FfiTransport {
    inner: Box<dyn BleTransport>,
}

impl Transport for FfiTransport {
    fn send(&self, link: meshcore::LinkId, frame: &[u8]) {
        self.inner.send(link, frame.to_vec());
    }
    fn close(&self, link: meshcore::LinkId) {
        self.inner.close(link);
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

/// One concrete store type so [`FfiMeshNode`] stays non-generic: the encrypted SQLite store on a
/// device, or an in-memory store for the loopback demo / tests.
enum FfiStore {
    Memory(Box<MemoryStore>),
    Sqlite(Box<SqliteStore>),
}

macro_rules! delegate {
    ($self:ident, $m:ident ( $($a:expr),* )) => {
        match $self {
            FfiStore::Memory(s) => s.$m($($a),*),
            FfiStore::Sqlite(s) => s.$m($($a),*),
        }
    };
}

impl Store for FfiStore {
    fn put_channel_message(&mut self, msg: StoredMessage) {
        delegate!(self, put_channel_message(msg))
    }
    fn has_message(&self, digest: &[u8; 8]) -> bool {
        delegate!(self, has_message(digest))
    }
    fn channel_history(&self, channel: &str, limit: usize) -> Vec<StoredMessage> {
        delegate!(self, channel_history(channel, limit))
    }
    fn channel_digests(&self, channel: &str) -> Vec<[u8; 8]> {
        delegate!(self, channel_digests(channel))
    }
    fn message_by_digest(&self, digest: &[u8; 8]) -> Option<StoredMessage> {
        delegate!(self, message_by_digest(digest))
    }
    fn upsert_peer(&mut self, peer: PeerRecord) {
        delegate!(self, upsert_peer(peer))
    }
    fn get_peer(&self, fp: &Fingerprint) -> Option<PeerRecord> {
        delegate!(self, get_peer(fp))
    }
    fn queue_envelope(&mut self, recipient: Fingerprint, packet_bytes: Vec<u8>, now_ms: Millis) {
        delegate!(self, queue_envelope(recipient, packet_bytes, now_ms))
    }
    fn take_envelopes(&mut self, recipient: &Fingerprint) -> Vec<Vec<u8>> {
        delegate!(self, take_envelopes(recipient))
    }
    fn panic_wipe(&mut self) {
        delegate!(self, panic_wipe())
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
    /// A decrypted end-to-end encrypted direct message (`sender` is the peer's fingerprint hex).
    DirectMessage {
        sender: String,
        text: String,
    },
    /// A Noise session with a peer completed (`verified` false means identity binding failed).
    DmSession {
        peer: String,
        verified: bool,
    },
}

/// A stored channel message, for loading persisted history into the UI.
#[derive(uniffi::Record)]
pub struct FfiMessage {
    pub sender: String,
    pub body: String,
    pub timestamp_ms: u64,
    /// True if this node originated the message.
    pub mine: bool,
}

/// The FFI handle to a running mesh node.
#[derive(uniffi::Object)]
pub struct FfiMeshNode {
    inner: Mutex<MeshNode<FfiTransport, SystemClock, FfiStore>>,
}

fn mem_store(cfg: &Tunables) -> MemoryStore {
    MemoryStore::new(
        cfg.channel_history_max_msgs,
        cfg.channel_history_ms,
        cfg.envelope_ttl_ms,
        cfg.envelope_max_per_peer,
    )
}

fn make_node(
    seed: u64,
    nickname: String,
    transport: Box<dyn BleTransport>,
    store: FfiStore,
    cfg: Tunables,
) -> Arc<FfiMeshNode> {
    let mut id_seed = [0u8; 32];
    id_seed[..8].copy_from_slice(&seed.to_le_bytes());
    let node = MeshNode::new(
        LocalIdentity::from_seed(&id_seed),
        cfg,
        FfiTransport { inner: transport },
        SystemClock,
        store,
        nickname,
        seed,
    );
    Arc::new(FfiMeshNode {
        inner: Mutex::new(node),
    })
}

#[uniffi::export]
impl FfiMeshNode {
    /// Create a node with an in-memory store (loopback demo / tests).
    #[uniffi::constructor]
    pub fn new(seed: u64, nickname: String, transport: Box<dyn BleTransport>) -> Arc<Self> {
        let cfg = Tunables::default();
        let store = FfiStore::Memory(Box::new(mem_store(&cfg)));
        make_node(seed, nickname, transport, store, cfg)
    }

    /// Create a node whose state persists to an encrypted (SQLCipher) database at `db_path`, keyed
    /// with the 32-byte `db_key` (from the platform keystore). Falls back to an in-memory store if
    /// the key is malformed or the database cannot be opened, so the node always runs.
    #[uniffi::constructor]
    pub fn new_persistent(
        seed: u64,
        nickname: String,
        transport: Box<dyn BleTransport>,
        db_path: String,
        db_key: Vec<u8>,
    ) -> Arc<Self> {
        let cfg = Tunables::default();
        let store = open_sqlite(&db_path, &db_key, &cfg)
            .map(|s| FfiStore::Sqlite(Box::new(s)))
            .unwrap_or_else(|| FfiStore::Memory(Box::new(mem_store(&cfg))));
        make_node(seed, nickname, transport, store, cfg)
    }

    /// Our current ephemeral wire id (hex).
    pub fn eph_id(&self) -> String {
        hex(&self.inner.lock().unwrap().eph_id())
    }

    pub fn join_channel(&self, name: String) {
        self.inner.lock().unwrap().join_channel(&name);
    }

    /// Load persisted history for a channel (oldest-first), so the UI can show it after a restart.
    pub fn channel_history(&self, channel: String, limit: u32) -> Vec<FfiMessage> {
        let node = self.inner.lock().unwrap();
        let me = node.eph_id();
        node.store()
            .channel_history(&channel, limit as usize)
            .into_iter()
            .map(|m| FfiMessage {
                sender: hex(&m.sender),
                body: String::from_utf8_lossy(&m.body).to_string(),
                timestamp_ms: m.timestamp_ms,
                mine: m.sender == me,
            })
            .collect()
    }

    /// Re-broadcast our signed identity announce. Called periodically by the platform so peers
    /// learn our key even if the announce sent at link-up was dropped during GATT setup.
    pub fn announce(&self) {
        self.inner.lock().unwrap().announce();
    }

    /// Originate a channel message; returns the message digest (hex).
    pub fn send_channel_message(&self, channel: String, text: String) -> String {
        hex(&self
            .inner
            .lock()
            .unwrap()
            .send_channel_message(&channel, &text))
    }

    /// Send an end-to-end encrypted direct message to a peer, addressed by fingerprint hex (as
    /// delivered in `FfiEvent::PeerAppeared`). A no-op if the fingerprint is malformed.
    pub fn send_dm(&self, peer_fingerprint: String, text: String) {
        if let Some(fp) = decode_hex32(&peer_fingerprint) {
            self.inner.lock().unwrap().send_dm(fp, &text);
        }
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
        MeshEvent::DmReceived {
            sender_fp, text, ..
        } => Some(FfiEvent::DirectMessage {
            sender: hex(&sender_fp),
            text,
        }),
        MeshEvent::DmSession { peer_fp, verified } => Some(FfiEvent::DmSession {
            peer: hex(&peer_fp),
            verified,
        }),
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

fn open_sqlite(db_path: &str, db_key: &[u8], cfg: &Tunables) -> Option<SqliteStore> {
    if db_key.len() != 32 {
        return None;
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(db_key);
    SqliteStore::open(
        db_path,
        &key,
        cfg.channel_history_max_msgs,
        cfg.channel_history_ms,
        cfg.envelope_ttl_ms,
        cfg.envelope_max_per_peer,
    )
    .ok()
}

fn decode_hex32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}
