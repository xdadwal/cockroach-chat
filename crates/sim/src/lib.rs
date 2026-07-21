//! Deterministic mesh simulator.
//!
//! Because `meshcore` is sans-IO — time comes from a [`Clock`], the network from a
//! [`Transport`] — we can run hundreds of real `MeshNode`s in one process against an in-memory
//! radio model and get bit-for-bit reproducible results. Every scenario seeds its RNG, so a
//! failure is always replayable.
//!
//! Radio model: each edge has a latency and a loss probability; a node's outbound frames are
//! delivered to the peer on the other end of the edge after the latency, dropped with the loss
//! probability. (A per-radio-cell airtime budget is a documented follow-up; see
//! `docs/PROGRESS.md`.)

use meshcore::store::{MemoryStore, Store};
use meshcore::{
    Clock, LinkId, LocalIdentity, MeshEvent, MeshNode, Transport, TransportEvent, Tunables,
};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

pub mod scenarios;
pub mod topology;

/// A virtual-time clock shared by every node in a world.
#[derive(Clone)]
pub struct SimClock(Rc<Cell<u64>>);

impl SimClock {
    fn new(start: u64) -> Self {
        Self(Rc::new(Cell::new(start)))
    }
    fn set(&self, t: u64) {
        self.0.set(t);
    }
}

impl Clock for SimClock {
    fn now_ms(&self) -> u64 {
        self.0.get()
    }
}

/// Shared buffer of a node's pending outbound `(link, frame)` sends.
type Outbox = Rc<RefCell<Vec<(LinkId, Vec<u8>)>>>;
/// Links the node asked to close (redundant-link dedup).
type Closed = Rc<RefCell<Vec<LinkId>>>;

/// A transport that buffers a node's outbound frames for the world to route.
#[derive(Clone)]
pub struct SimTransport {
    outbox: Outbox,
    closed: Closed,
}

impl SimTransport {
    fn new() -> Self {
        Self {
            outbox: Rc::new(RefCell::new(Vec::new())),
            closed: Rc::new(RefCell::new(Vec::new())),
        }
    }
}

impl Transport for SimTransport {
    fn send(&self, link: LinkId, frame: &[u8]) {
        self.outbox.borrow_mut().push((link, frame.to_vec()));
    }
    fn close(&self, link: LinkId) {
        self.closed.borrow_mut().push(link);
    }
}

type SimMeshNode = MeshNode<SimTransport, SimClock, MemoryStore>;

struct SimNode {
    node: SimMeshNode,
    outbox: Outbox,
    closed: Closed,
    events: Vec<MeshEvent>,
}

/// One undirected radio link. Its index in `World::edges` is used as the [`LinkId`] on both
/// endpoints (an edge joins exactly two nodes, so the id is unambiguous per node).
#[derive(Clone)]
pub struct Edge {
    pub a: usize,
    pub b: usize,
    pub latency_ms: u64,
    pub loss: f64,
    pub mtu: usize,
    pub up: bool,
}

struct InFlight {
    deliver_at: u64,
    dest: usize,
    link: LinkId,
    frame: Vec<u8>,
}

pub struct World {
    clock: SimClock,
    nodes: Vec<SimNode>,
    edges: Vec<Edge>,
    inflight: VecDeque<InFlight>,
    rng: StdRng,
    step_ms: u64,
    now: u64,
    tx_frames: u64,
}

impl World {
    /// Build a world of `n` nodes (not yet linked). `cfg` is cloned into every node.
    pub fn new(n: usize, cfg: Tunables, seed: u64) -> Self {
        let clock = SimClock::new(0);
        let mut nodes = Vec::with_capacity(n);
        for i in 0..n {
            let transport = SimTransport::new();
            let outbox = transport.outbox.clone();
            let closed = transport.closed.clone();
            let mut id_seed = [0u8; 32];
            id_seed[..8].copy_from_slice(&(i as u64).to_le_bytes());
            id_seed[8] = 0xC0;
            let node = MeshNode::new(
                LocalIdentity::from_seed(&id_seed),
                cfg.clone(),
                transport,
                clock.clone(),
                MemoryStore::new(
                    cfg.channel_history_max_msgs,
                    cfg.channel_history_ms,
                    cfg.envelope_ttl_ms,
                    cfg.envelope_max_per_peer,
                ),
                format!("node{i}"),
                seed ^ (i as u64).wrapping_mul(0x9E3779B97F4A7C15),
            );
            nodes.push(SimNode {
                node,
                outbox,
                closed,
                events: Vec::new(),
            });
        }
        Self {
            clock,
            nodes,
            edges: Vec::new(),
            inflight: VecDeque::new(),
            rng: StdRng::seed_from_u64(seed ^ 0xA5A5),
            step_ms: 10,
            now: 0,
            tx_frames: 0,
        }
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn now(&self) -> u64 {
        self.now
    }

    pub fn set_step_ms(&mut self, step: u64) {
        self.step_ms = step.max(1);
    }

    /// Add a bidirectional link between `a` and `b`. Returns its [`LinkId`].
    pub fn link(&mut self, a: usize, b: usize, latency_ms: u64, loss: f64, mtu: usize) -> LinkId {
        let id = self.edges.len() as LinkId;
        self.edges.push(Edge {
            a,
            b,
            latency_ms,
            loss,
            mtu,
            up: true,
        });
        id
    }

    /// Bring every current edge up at both endpoints (drives the initial announces/syncs).
    pub fn bootstrap(&mut self) {
        let edges = self.edges.clone();
        for (id, e) in edges.iter().enumerate() {
            let id = id as LinkId;
            self.nodes[e.a]
                .node
                .on_transport_event(TransportEvent::LinkUp {
                    link: id,
                    mtu: e.mtu,
                    peer_hint: None,
                });
            self.nodes[e.b]
                .node
                .on_transport_event(TransportEvent::LinkUp {
                    link: id,
                    mtu: e.mtu,
                    peer_hint: None,
                });
        }
        self.collect_and_route();
    }

    /// Set an edge up or down (used by partition scenarios).
    pub fn set_edge_up(&mut self, edge: LinkId, up: bool) {
        if let Some(e) = self.edges.get_mut(edge as usize) {
            if e.up != up {
                e.up = up;
                let (a, b, mtu) = (e.a, e.b, e.mtu);
                if up {
                    self.nodes[a]
                        .node
                        .on_transport_event(TransportEvent::LinkUp {
                            link: edge,
                            mtu,
                            peer_hint: None,
                        });
                    self.nodes[b]
                        .node
                        .on_transport_event(TransportEvent::LinkUp {
                            link: edge,
                            mtu,
                            peer_hint: None,
                        });
                } else {
                    self.nodes[a]
                        .node
                        .on_transport_event(TransportEvent::LinkDown { link: edge });
                    self.nodes[b]
                        .node
                        .on_transport_event(TransportEvent::LinkDown { link: edge });
                }
            }
        }
    }

    /// Reset the transmission and rebroadcast counters (call right before the message you want to
    /// measure, so bootstrap announce traffic is excluded).
    pub fn reset_tx(&mut self) {
        self.tx_frames = 0;
        for n in &mut self.nodes {
            n.node.reset_relays_fired();
        }
    }

    /// Total frames transmitted since the last [`World::reset_tx`]. For small (single-frame)
    /// messages this equals the number of packet transmissions.
    pub fn tx_frames(&self) -> u64 {
        self.tx_frames
    }

    pub fn node_mut(&mut self, i: usize) -> &mut SimMeshNode {
        &mut self.nodes[i].node
    }

    /// A node's stable identity fingerprint (used to address DMs).
    pub fn node_fingerprint(&self, i: usize) -> [u8; 32] {
        self.nodes[i].node.fingerprint()
    }

    /// How many links a node currently holds (after redundant-link dedup).
    pub fn link_count(&self, i: usize) -> usize {
        self.nodes[i].node.link_count()
    }

    /// Fraction of nodes whose store holds `digest`.
    pub fn delivery_ratio(&self, digest: &[u8; 8]) -> f64 {
        let have = self
            .nodes
            .iter()
            .filter(|n| n.node.store().has_message(digest))
            .count();
        have as f64 / self.nodes.len() as f64
    }

    /// Total logical rebroadcasts across all nodes (the mesh-wide flooding cost of a message).
    pub fn total_relays_fired(&self) -> u64 {
        self.nodes.iter().map(|n| n.node.relays_fired()).sum()
    }

    pub fn count_with_message(&self, digest: &[u8; 8]) -> usize {
        self.nodes
            .iter()
            .filter(|n| n.node.store().has_message(digest))
            .count()
    }

    /// Drain accumulated UI events for a node.
    pub fn events(&self, i: usize) -> &[MeshEvent] {
        &self.nodes[i].events
    }

    /// Advance virtual time by `duration_ms`, stepping the world.
    pub fn run_for(&mut self, duration_ms: u64) {
        let target = self.now + duration_ms;
        while self.now < target {
            self.step();
        }
    }

    fn step(&mut self) {
        self.now += self.step_ms;
        self.clock.set(self.now);

        // 1. Deliver frames whose latency has elapsed.
        let mut ready = Vec::new();
        let mut remaining = VecDeque::new();
        while let Some(f) = self.inflight.pop_front() {
            if f.deliver_at <= self.now {
                ready.push(f);
            } else {
                remaining.push_back(f);
            }
        }
        self.inflight = remaining;
        for f in ready {
            if self.edges[f.link as usize].up {
                self.nodes[f.dest]
                    .node
                    .on_transport_event(TransportEvent::FrameReceived {
                        link: f.link,
                        frame: f.frame,
                    });
            }
        }

        // 2. Tick all nodes (releases due rebroadcasts).
        for n in &mut self.nodes {
            n.node.tick();
        }

        // 3. Collect events and route outbound frames.
        self.collect_and_route();

        // 4. Honor link closes requested by dedup (tear the edge down on both ends).
        for i in 0..self.nodes.len() {
            let closed: Vec<LinkId> = self.nodes[i].closed.borrow_mut().drain(..).collect();
            for link in closed {
                self.set_edge_up(link, false);
            }
        }
    }

    fn collect_and_route(&mut self) {
        let now = self.now;
        for i in 0..self.nodes.len() {
            let evs = self.nodes[i].node.take_events();
            self.nodes[i].events.extend(evs);

            let out: Vec<(LinkId, Vec<u8>)> = self.nodes[i].outbox.borrow_mut().drain(..).collect();
            for (link, frame) in out {
                let edge = &self.edges[link as usize];
                if !edge.up {
                    continue;
                }
                self.tx_frames += 1;
                if edge.loss > 0.0 && self.rng.gen::<f64>() < edge.loss {
                    continue; // dropped on the air
                }
                let dest = if edge.a == i { edge.b } else { edge.a };
                self.inflight.push_back(InFlight {
                    deliver_at: now + edge.latency_ms,
                    dest,
                    link,
                    frame,
                });
            }
        }
    }
}
