//! Deterministic graph topologies for scenarios.
//!
//! The workhorse is a *random geometric graph*: nodes are dropped uniformly in a unit square and
//! two nodes are linked if they fall within a connection radius. That models a physical crowd
//! (you connect to nearby phones). We then force connectivity so a scenario can meaningfully
//! measure whole-network delivery.

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

pub struct Graph {
    pub n: usize,
    pub edges: Vec<(usize, usize)>,
    pub positions: Vec<(f64, f64)>,
}

/// A random geometric graph of `n` nodes with a connection radius chosen to hit roughly
/// `target_degree` average neighbours, then augmented with the minimum edges needed to make it
/// connected.
pub fn geometric(n: usize, seed: u64, target_degree: f64) -> Graph {
    let mut rng = StdRng::seed_from_u64(seed);
    let positions: Vec<(f64, f64)> = (0..n)
        .map(|_| (rng.gen::<f64>(), rng.gen::<f64>()))
        .collect();

    // radius r such that expected neighbours ≈ n · π · r²  →  r = sqrt(deg / (n·π)).
    let r = (target_degree / (n as f64 * std::f64::consts::PI)).sqrt();
    let r2 = r * r;

    let mut edges = Vec::new();
    for i in 0..n {
        for j in (i + 1)..n {
            if dist2(&positions, i, j) <= r2 {
                edges.push((i, j));
            }
        }
    }

    connect_components(n, &mut edges, &positions);
    Graph {
        n,
        edges,
        positions,
    }
}

/// A simple line A—B—C—…; the canonical multi-hop relay topology.
pub fn line(n: usize) -> Graph {
    let edges = (0..n.saturating_sub(1)).map(|i| (i, i + 1)).collect();
    Graph {
        n,
        edges,
        positions: (0..n).map(|i| (i as f64, 0.0)).collect(),
    }
}

/// A fully-connected clique of `n` nodes (a single dense room).
pub fn clique(n: usize) -> Graph {
    let mut edges = Vec::new();
    for i in 0..n {
        for j in (i + 1)..n {
            edges.push((i, j));
        }
    }
    Graph {
        n,
        edges,
        positions: (0..n).map(|i| (i as f64, 0.0)).collect(),
    }
}

fn dist2(pos: &[(f64, f64)], i: usize, j: usize) -> f64 {
    let dx = pos[i].0 - pos[j].0;
    let dy = pos[i].1 - pos[j].1;
    dx * dx + dy * dy
}

/// Union-find connectivity: link the nearest pair of nodes from distinct components until the
/// graph is a single component.
fn connect_components(n: usize, edges: &mut Vec<(usize, usize)>, pos: &[(f64, f64)]) {
    let mut uf = UnionFind::new(n);
    for &(a, b) in edges.iter() {
        uf.union(a, b);
    }
    loop {
        // Find the closest pair (i, j) whose roots differ.
        let mut best: Option<(f64, usize, usize)> = None;
        for i in 0..n {
            for j in (i + 1)..n {
                if uf.find(i) != uf.find(j) {
                    let d = dist2(pos, i, j);
                    if best.map(|(bd, _, _)| d < bd).unwrap_or(true) {
                        best = Some((d, i, j));
                    }
                }
            }
        }
        match best {
            Some((_, i, j)) => {
                edges.push((i, j));
                uf.union(i, j);
            }
            None => break, // already one component
        }
    }
}

struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
        }
    }
    fn find(&mut self, x: usize) -> usize {
        let mut root = x;
        while self.parent[root] != root {
            root = self.parent[root];
        }
        // path compression
        let mut cur = x;
        while self.parent[cur] != root {
            let next = self.parent[cur];
            self.parent[cur] = root;
            cur = next;
        }
        root
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.parent[ra] = rb;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_connected(g: &Graph) -> bool {
        let mut uf = UnionFind::new(g.n);
        for &(a, b) in &g.edges {
            uf.union(a, b);
        }
        (0..g.n).all(|i| uf.find(i) == uf.find(0))
    }

    #[test]
    fn geometric_is_connected() {
        let g = geometric(200, 7, 6.0);
        assert!(is_connected(&g));
        assert!(g.edges.len() >= 199); // at least a spanning tree
    }

    #[test]
    fn line_has_expected_edges() {
        let g = line(5);
        assert_eq!(g.edges, vec![(0, 1), (1, 2), (2, 3), (3, 4)]);
    }

    #[test]
    fn geometric_is_deterministic() {
        let a = geometric(50, 42, 6.0);
        let b = geometric(50, 42, 6.0);
        assert_eq!(a.edges, b.edges);
    }
}
