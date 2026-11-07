//! The weighted set tree (`sets.yaml`). Groups nest; leaves point at a set directory.

use std::path::PathBuf;

use rand::{Rng, RngExt};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryFile {
    pub sets: Vec<NodeSpec>,
}

/// A group has `name` and `sets`; a leaf has `set` (a directory containing set.yaml).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeSpec {
    #[serde(default)]
    pub name: Option<String>,
    pub weight: u32,
    #[serde(default)]
    pub sets: Option<Vec<NodeSpec>>,
    #[serde(default)]
    pub set: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub enum Node {
    Group { weight: u32, children: Vec<Node> },
    Leaf { weight: u32, set: usize },
}

#[derive(Clone, Debug, Default)]
pub struct Library {
    pub roots: Vec<Node>,
}

impl Node {
    fn weight(&self) -> u32 {
        match self {
            Node::Group { weight, .. } | Node::Leaf { weight, .. } => *weight,
        }
    }

    fn any_available(&self, available: &impl Fn(usize) -> bool) -> bool {
        match self {
            Node::Leaf { set, .. } => available(*set),
            Node::Group { children, .. } => children.iter().any(|c| c.any_available(available)),
        }
    }
}

impl Library {
    /// Walks the tree by weight, skipping branches with nothing available. Returns a set index.
    pub fn choose(&self, rng: &mut impl Rng, available: impl Fn(usize) -> bool) -> Option<usize> {
        choose_among(&self.roots, rng, &available)
    }
}

fn choose_among(nodes: &[Node], rng: &mut impl Rng, available: &impl Fn(usize) -> bool) -> Option<usize> {
    let weights: Vec<u32> = nodes
        .iter()
        .map(|n| if n.any_available(available) { n.weight() } else { 0 })
        .collect();
    let node = &nodes[pick_weighted(&weights, rng)?];
    match node {
        Node::Leaf { set, .. } => Some(*set),
        Node::Group { children, .. } => choose_among(children, rng, available),
    }
}

/// Index chosen with probability proportional to its weight. `None` when every weight is zero.
pub fn pick_weighted(weights: &[u32], rng: &mut impl Rng) -> Option<usize> {
    let total: u64 = weights.iter().map(|&w| u64::from(w)).sum();
    if total == 0 {
        return None;
    }
    let mut roll = rng.random_range(0..total);
    for (i, &w) in weights.iter().enumerate() {
        let w = u64::from(w);
        if roll < w {
            return Some(i);
        }
        roll -= w;
    }
    unreachable!("roll is always below the total weight")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn leaf(weight: u32, set: usize) -> Node {
        Node::Leaf { weight, set }
    }

    #[test]
    fn weighted_pick_follows_weights() {
        let mut rng = StdRng::seed_from_u64(7);
        let mut counts = [0u32; 3];
        for _ in 0..30_000 {
            counts[pick_weighted(&[1, 0, 9], &mut rng).unwrap()] += 1;
        }
        assert_eq!(counts[1], 0);
        let ratio = counts[2] as f64 / counts[0] as f64;
        assert!((7.0..11.0).contains(&ratio), "ratio {ratio}");
    }

    #[test]
    fn all_zero_weights_pick_nothing() {
        let mut rng = StdRng::seed_from_u64(7);
        assert_eq!(pick_weighted(&[0, 0], &mut rng), None);
        assert_eq!(pick_weighted(&[], &mut rng), None);
    }

    #[test]
    fn unavailable_branches_are_skipped() {
        let lib = Library {
            roots: vec![
                Node::Group {
                    weight: 1000,
                    children: vec![leaf(1, 0), leaf(1, 1)],
                },
                leaf(1, 2),
            ],
        };
        let mut rng = StdRng::seed_from_u64(1);
        for _ in 0..100 {
            assert_eq!(lib.choose(&mut rng, |s| s == 2), Some(2));
        }
        assert_eq!(lib.choose(&mut rng, |_| false), None);
    }

    #[test]
    fn same_seed_same_choices() {
        let lib = Library {
            roots: vec![
                leaf(3, 0),
                leaf(5, 1),
                Node::Group {
                    weight: 2,
                    children: vec![leaf(1, 2)],
                },
            ],
        };
        let run = |seed| {
            let mut rng = StdRng::seed_from_u64(seed);
            (0..50)
                .map(|_| lib.choose(&mut rng, |_| true).unwrap())
                .collect::<Vec<_>>()
        };
        assert_eq!(run(42), run(42));
    }
}
