//! Mesh scenario tests. Every one is deterministic (seeded); a failure is replayable by rerunning
//! with the same seed. These are the M0 "does the mesh actually work" acceptance gates.

use sim::scenarios;

#[test]
fn two_nodes_hello() {
    for seed in [1, 2, 3] {
        assert!(
            scenarios::two_nodes_hello(seed),
            "two-node delivery failed for seed {seed}"
        );
    }
}

#[test]
fn broadcast_200_nodes_reaches_the_crowd() {
    let r = scenarios::broadcast(200, 7);
    assert!(
        r.delivery_ratio >= 0.95,
        "delivery {:.3} < 0.95 ({} of {} nodes)",
        r.delivery_ratio,
        (r.delivery_ratio * r.nodes as f64).round(),
        r.nodes
    );
    // Suppression must keep the mesh-wide rebroadcast count below a naive flood (where every
    // node would rebroadcast once). We currently land ~0.7 rebroadcasts/node; driving this to the
    // aspirational 0.5N while holding ≥95% delivery is tracked tuning (see docs/PROGRESS.md).
    assert!(
        r.relays_per_node < 0.9,
        "relays/node {:.2} not below naive flood — suppression ineffective",
        r.relays_per_node
    );
}

#[test]
fn multi_hop_line_relay() {
    // 6 hops: the two ends are far out of radio range and rely entirely on relays.
    let r = scenarios::line_relay(6, 3);
    assert!(r.reached_far_end, "message did not survive {} hops", r.hops);
}

#[test]
fn partition_heals_without_duplicates() {
    for seed in [5, 6] {
        let r = scenarios::partition_heal(seed);
        assert!(
            r.converged,
            "rooms did not converge after heal (seed {seed})"
        );
        assert_eq!(
            r.duplicate_ui_events, 0,
            "reconciliation delivered a duplicate to the UI (seed {seed})"
        );
    }
}

#[test]
fn duplicate_storm_is_suppressed() {
    let r = scenarios::duplicate_storm(9);
    assert!(r.delivered, "storm message not delivered to all nodes");
    // Eight nodes echoing the same packet must collapse to a handful of rebroadcasts, not a storm.
    assert!(
        r.relay_tx < 20,
        "duplicate storm produced {} transmissions — suppression failed",
        r.relay_tx
    );
}

#[test]
fn malicious_flooder_is_contained() {
    let r = scenarios::malicious_flooder(11);
    assert!(r.honest_delivered, "honest message lost during flood");
    assert!(
        r.attacker_greylisted_effect,
        "flooder was not rate-limited — too much spam propagated"
    );
}
