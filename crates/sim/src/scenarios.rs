//! Reusable scenario runners, shared by the integration tests and the `sim` CLI. Each returns a
//! metrics struct so callers can assert (tests) or print (CLI).

use crate::{topology, World};
use meshcore::store::Store;
use meshcore::{MeshEvent, Tunables};

const MTU: usize = 182;
const LINK_LATENCY_MS: u64 = 15;

/// A crowd dense enough that the whole cluster is within a handful of hops. We lift the TTL so a
/// single broadcast can traverse the entire connected graph — the default TTL (7, clamped 5)
/// models a physically tiny knot of people, whereas the simulator stretches a synthetic graph
/// across a unit square with a larger diameter.
fn crowd_cfg() -> Tunables {
    // Counter-based suppression already thins redundant rebroadcasts; combining it with aggressive
    // probabilistic thinning (0.45) under-covers at the ~8-link connection cap, because a
    // low-degree cut vertex may roll a miss on the one path to a whole sub-crowd. We keep
    // rebroadcast probability high and let suppression do the thinning. We also lift the TTL so a
    // single broadcast can cross the synthetic graph's larger diameter. (Simulator finding; see
    // docs/PROGRESS.md.)
    Tunables {
        ttl_default: 24,
        ttl_dense_clamp: 24,
        relay_prob_sparse: 1.0,
        relay_prob_mid: 1.0,
        relay_prob_dense: 0.85,
        ..Tunables::default()
    }
}

/// Average radio neighbours in the modelled crowd. A dense knot of people in physical proximity
/// has redundant links (not thin bridges), so we model ~12.
const CROWD_DEGREE: f64 = 12.0;

pub struct BroadcastResult {
    pub nodes: usize,
    pub delivery_ratio: f64,
    pub tx_frames: u64,
    /// Logical rebroadcasts across the whole mesh for this one message (suppression keeps this
    /// well below `nodes`).
    pub relays_fired: u64,
    /// Rebroadcasts per node — the flooding-efficiency figure.
    pub relays_per_node: f64,
}

/// Origin node 0 broadcasts one message across a geometric graph; measure how many nodes receive
/// it and how many transmissions it cost.
pub fn broadcast(nodes: usize, seed: u64) -> BroadcastResult {
    let g = topology::geometric(nodes, seed, CROWD_DEGREE);
    let mut world = World::new(nodes, crowd_cfg(), seed);
    for (a, b) in g.edges {
        world.link(a, b, LINK_LATENCY_MS, 0.0, MTU);
    }
    world.bootstrap();
    world.run_for(2000); // let announces settle and dedup
    world.reset_tx();
    let digest = world
        .node_mut(0)
        .send_channel_message("#general", "hello mesh");
    world.run_for(6000);

    let delivery_ratio = world.delivery_ratio(&digest);
    let tx_frames = world.tx_frames();
    let relays_fired = world.total_relays_fired();
    BroadcastResult {
        nodes,
        delivery_ratio,
        tx_frames,
        relays_fired,
        relays_per_node: relays_fired as f64 / nodes as f64,
    }
}

/// Two nodes, one link: a message from A must reach B.
pub fn two_nodes_hello(seed: u64) -> bool {
    let mut world = World::new(2, crowd_cfg(), seed);
    world.link(0, 1, LINK_LATENCY_MS, 0.0, MTU);
    world.bootstrap();
    world.run_for(1000);
    let digest = world.node_mut(0).send_channel_message("#general", "hi B");
    world.run_for(1000);
    world.count_with_message(&digest) == 2
}

pub struct RelayResult {
    pub reached_far_end: bool,
    pub hops: usize,
}

/// Line A—B—…—Z where the ends are out of radio range: the message must relay end to end.
pub fn line_relay(hops: usize, seed: u64) -> RelayResult {
    let n = hops + 1;
    let g = topology::line(n);
    let mut world = World::new(n, crowd_cfg(), seed);
    for (a, b) in g.edges {
        world.link(a, b, LINK_LATENCY_MS, 0.0, MTU);
    }
    world.bootstrap();
    world.run_for(1000);
    let digest = world
        .node_mut(0)
        .send_channel_message("#general", "relay me");
    world.run_for(3000);
    RelayResult {
        reached_far_end: world.node_mut(n - 1).store().has_message(&digest),
        hops,
    }
}

pub struct PartitionResult {
    pub converged: bool,
    pub duplicate_ui_events: usize,
}

/// Two rooms joined by a single door link. The door closes, each room chats, then the door
/// reopens; set-reconciliation must converge both rooms with no duplicate UI deliveries.
pub fn partition_heal(seed: u64) -> PartitionResult {
    // 6 nodes: {0,1,2} room A (clique), {3,4,5} room B (clique), door = edge 2—3.
    let mut world = World::new(6, crowd_cfg(), seed);
    let door_a = 2;
    let door_b = 3;
    // room A clique
    for (a, b) in [(0, 1), (1, 2), (0, 2)] {
        world.link(a, b, LINK_LATENCY_MS, 0.0, MTU);
    }
    // room B clique
    for (a, b) in [(3, 4), (4, 5), (3, 5)] {
        world.link(a, b, LINK_LATENCY_MS, 0.0, MTU);
    }
    let door = world.link(door_a, door_b, LINK_LATENCY_MS, 0.0, MTU);
    world.bootstrap();
    world.run_for(1000);

    // Close the door; each room speaks.
    world.set_edge_up(door, false);
    let d_a = world
        .node_mut(0)
        .send_channel_message("#general", "from room A");
    let d_b = world
        .node_mut(5)
        .send_channel_message("#general", "from room B");
    world.run_for(2000);

    // Reopen; reconciliation should carry each message across.
    world.set_edge_up(door, true);
    world.run_for(4000);

    let converged = (0..6).all(|i| {
        let s = world.node_mut(i).store();
        s.has_message(&d_a) && s.has_message(&d_b)
    });

    // No node should have delivered the same digest to the UI twice.
    let mut duplicate_ui_events = 0;
    for i in 0..6 {
        let mut seen = std::collections::HashSet::new();
        for e in world.events(i) {
            if let MeshEvent::MessageReceived {
                timestamp_ms,
                sender,
                ..
            } = e
            {
                if !seen.insert((*sender, *timestamp_ms)) {
                    duplicate_ui_events += 1;
                }
            }
        }
    }
    PartitionResult {
        converged,
        duplicate_ui_events,
    }
}

pub struct StormResult {
    pub delivered: bool,
    pub relay_tx: u64,
}

/// A duplicate storm: the same message arrives many times; dedup + suppression must keep total
/// rebroadcasts near zero while still delivering once.
pub fn duplicate_storm(seed: u64) -> StormResult {
    // Star: node 0 in the middle, many leaves that will all echo the same packet.
    let n = 8;
    let mut world = World::new(n, crowd_cfg(), seed);
    for i in 1..n {
        world.link(0, i, LINK_LATENCY_MS, 0.0, MTU);
    }
    world.bootstrap();
    world.run_for(1000);
    world.reset_tx();
    let digest = world.node_mut(1).send_channel_message("#general", "storm");
    world.run_for(3000);
    StormResult {
        delivered: world.count_with_message(&digest) == n,
        relay_tx: world.tx_frames(),
    }
}

pub struct DmResult {
    /// The recipient decrypted the DM.
    pub delivered: bool,
    /// The plaintext matched.
    pub text_ok: bool,
    /// The relaying middle node never decrypted it.
    pub eavesdropper_blind: bool,
}

/// Line A — Eve — B (A and B out of direct range). A sends an encrypted DM to B; the Noise
/// handshake and ciphertext must relay through Eve, who forwards but cannot read them.
pub fn direct_message(seed: u64) -> DmResult {
    let mut world = World::new(3, crowd_cfg(), seed);
    world.link(0, 1, LINK_LATENCY_MS, 0.0, MTU);
    world.link(1, 2, LINK_LATENCY_MS, 0.0, MTU);
    world.bootstrap();
    world.run_for(2500); // let announces (with X25519 keys) propagate end-to-end

    let b_fp = world.node_fingerprint(2);
    world.node_mut(0).send_dm(b_fp, "meet at the safehouse");
    world.run_for(5000); // XX handshake (3 messages) relays A<->B, then the DM

    let received = |i: usize| -> Vec<String> {
        world
            .events(i)
            .iter()
            .filter_map(|e| match e {
                MeshEvent::DmReceived { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    };
    let b_msgs = received(2);
    DmResult {
        delivered: !b_msgs.is_empty(),
        text_ok: b_msgs.iter().any(|t| t == "meet at the safehouse"),
        eavesdropper_blind: received(1).is_empty(),
    }
}

/// A DM between two phones joined by SEVERAL redundant BLE links — the real two-phone setup, where
/// both sides advertise + scan + connect and end up with ~5 links to one peer. Regression guard:
/// duplicate copies of a message across those links must not falsely greylist the peer and kill the
/// Noise handshake (which delivers no message until it completes).
pub fn dm_over_redundant_links(seed: u64) -> bool {
    let mut world = World::new(2, crowd_cfg(), seed);
    for _ in 0..5 {
        world.link(0, 1, LINK_LATENCY_MS, 0.0, MTU);
    }
    world.bootstrap();
    world.run_for(3000);
    let b_fp = world.node_fingerprint(1);
    world.node_mut(0).send_dm(b_fp, "north gate clear");
    world.run_for(5000);
    world
        .events(1)
        .iter()
        .any(|e| matches!(e, MeshEvent::DmReceived { text, .. } if text == "north gate clear"))
}

pub struct DedupResult {
    pub links_after: usize,
    pub delivered: bool,
}

/// Two phones joined by 5 redundant BLE links. Once each learns the other's identity from its
/// announce, the mesh should collapse to a single link per peer — and messaging must still work.
pub fn link_dedup(seed: u64) -> DedupResult {
    let mut world = World::new(2, crowd_cfg(), seed);
    for _ in 0..5 {
        world.link(0, 1, LINK_LATENCY_MS, 0.0, MTU);
    }
    world.bootstrap();
    world.run_for(3000); // announces flow; redundant links get closed

    let links_after = world.link_count(0);
    let digest = world.node_mut(0).send_channel_message("#general", "still works");
    world.run_for(1500);
    DedupResult {
        links_after,
        delivered: world.count_with_message(&digest) == 2,
    }
}

pub struct FloodResult {
    pub honest_delivered: bool,
    pub attacker_greylisted_effect: bool,
}

/// A malicious node floods; the honest message must still get through, and the flooder's packets
/// must be rate-limited (only a bounded number of its packets propagate).
pub fn malicious_flooder(seed: u64) -> FloodResult {
    // line: honest(0) — relay(1) — victim(2), attacker(3) also on relay(1).
    let mut world = World::new(4, crowd_cfg(), seed);
    world.link(0, 1, LINK_LATENCY_MS, 0.0, MTU);
    world.link(1, 2, LINK_LATENCY_MS, 0.0, MTU);
    world.link(1, 3, LINK_LATENCY_MS, 0.0, MTU);
    world.bootstrap();
    world.run_for(1000);

    // Attacker floods 100 distinct messages rapidly.
    for k in 0..100 {
        world
            .node_mut(3)
            .send_channel_message("#spam", &format!("flood {k}"));
        world.run_for(20);
    }
    // Honest message during the flood.
    let honest = world.node_mut(0).send_channel_message("#general", "help");
    world.run_for(3000);

    // Victim received honest message.
    let honest_delivered = world.node_mut(2).store().has_message(&honest);
    // The relay's rate limiter should have dropped a large share of the flood: victim holds far
    // fewer than 100 spam messages.
    let spam_at_victim = world
        .node_mut(2)
        .store()
        .channel_history("#spam", 1000)
        .len();
    FloodResult {
        honest_delivered,
        attacker_greylisted_effect: spam_at_victim < 50,
    }
}
