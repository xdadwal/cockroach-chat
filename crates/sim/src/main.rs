//! CLI front-end for the mesh simulator.
//!
//! Examples:
//!   cargo run -p sim -- --nodes 200 --scenario broadcast
//!   cargo run -p sim -- --scenario partition --seed 5

use sim::scenarios;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut nodes = 200usize;
    let mut scenario = "broadcast".to_string();
    let mut seed = 7u64;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--nodes" => {
                nodes = args
                    .get(i + 1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(nodes);
                i += 2;
            }
            "--scenario" => {
                scenario = args.get(i + 1).cloned().unwrap_or(scenario);
                i += 2;
            }
            "--seed" => {
                seed = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(seed);
                i += 2;
            }
            "-h" | "--help" => {
                print_help();
                return;
            }
            other => {
                eprintln!("unknown argument: {other}");
                print_help();
                std::process::exit(2);
            }
        }
    }

    match scenario.as_str() {
        "broadcast" => {
            let r = scenarios::broadcast(nodes, seed);
            println!("scenario: broadcast  nodes={}  seed={}", nodes, seed);
            println!(
                "  delivery ratio : {:.1}% ({} / {})",
                r.delivery_ratio * 100.0,
                (r.delivery_ratio * nodes as f64).round() as usize,
                nodes
            );
            println!(
                "  rebroadcasts   : {} ({:.2} per node)",
                r.relays_fired, r.relays_per_node
            );
            println!("  link frames    : {}", r.tx_frames);
        }
        "two" => println!(
            "scenario: two_nodes  ->  {}",
            ok(scenarios::two_nodes_hello(seed))
        ),
        "relay" => {
            let r = scenarios::line_relay(6, seed);
            println!(
                "scenario: line_relay  hops={}  reached_far_end={}",
                r.hops, r.reached_far_end
            );
        }
        "partition" => {
            let r = scenarios::partition_heal(seed);
            println!(
                "scenario: partition  converged={}  duplicate_ui_events={}",
                r.converged, r.duplicate_ui_events
            );
        }
        "storm" => {
            let r = scenarios::duplicate_storm(seed);
            println!(
                "scenario: duplicate_storm  delivered={}  rebroadcasts={}",
                r.delivered, r.relay_tx
            );
        }
        "flooder" => {
            let r = scenarios::malicious_flooder(seed);
            println!(
                "scenario: malicious_flooder  honest_delivered={}  attacker_contained={}",
                r.honest_delivered, r.attacker_greylisted_effect
            );
        }
        other => {
            eprintln!("unknown scenario: {other}");
            print_help();
            std::process::exit(2);
        }
    }
}

fn ok(b: bool) -> &'static str {
    if b {
        "OK"
    } else {
        "FAILED"
    }
}

fn print_help() {
    eprintln!(
        "usage: sim [--nodes N] [--seed S] --scenario <broadcast|two|relay|partition|storm|flooder>"
    );
}
