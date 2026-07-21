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
    // Suppression must keep the mesh-wide rebroadcast count below a naive flood, where every
    // reached node rebroadcasts once (~1.0/node at this delivery ratio). We currently land
    // ~0.9/node; driving this toward the aspirational 0.5N while holding ≥95% delivery is tracked
    // tuning (see docs/PROGRESS.md). (This is an RNG-sensitive soft metric — the robust invariant
    // is simply "below naive flood".)
    assert!(
        r.relays_per_node < 1.0,
        "relays/node {:.2} not below a naive flood — suppression ineffective",
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
fn encrypted_dm_relays_and_eavesdropper_is_blind() {
    let r = scenarios::direct_message(7);
    assert!(r.delivered, "recipient did not decrypt the DM");
    assert!(r.text_ok, "DM plaintext did not match");
    assert!(
        r.eavesdropper_blind,
        "the relaying middle node decrypted the DM — E2E encryption broken"
    );
}

#[test]
fn dm_survives_redundant_links() {
    // The regression that broke DMs on real phones: ~5 links to one peer falsely greylisted it.
    for seed in [1, 2, 3, 7] {
        assert!(
            scenarios::dm_over_redundant_links(seed),
            "DM failed over redundant links (seed {seed}) — duplicate copies greylisted the peer"
        );
    }
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
