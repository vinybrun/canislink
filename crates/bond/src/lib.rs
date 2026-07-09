//! Mutual bond graph for invite routing.

use protocol::{DogId, W_MIN};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BondEdge {
    pub from: DogId,
    pub to: DogId,
    pub weight: f32,
}

#[derive(Debug, Default, Clone)]
pub struct BondGraph {
    /// directed weights; mutual required for eligibility
    edges: HashMap<(DogId, DogId), f32>,
}

impl BondGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_weight(&mut self, from: DogId, to: DogId, weight: f32) {
        self.edges.insert((from, to), weight.clamp(0.0, 1.0));
    }

    /// Bootstrap a mutual pair (steward).
    pub fn bootstrap_mutual(&mut self, a: DogId, b: DogId, weight: f32) {
        let w = weight.clamp(0.0, 1.0).max(W_MIN);
        self.set_weight(a, b, w);
        self.set_weight(b, a, w);
    }

    pub fn weight(&self, from: DogId, to: DogId) -> f32 {
        self.edges.get(&(from, to)).copied().unwrap_or(0.0)
    }

    pub fn mutual_eligible(&self, a: DogId, b: DogId) -> bool {
        self.weight(a, b) >= W_MIN && self.weight(b, a) >= W_MIN
    }

    /// Peers dog may call, sorted by weight desc.
    pub fn candidates(&self, from: DogId) -> Vec<(DogId, f32)> {
        let mut out: Vec<_> = self
            .edges
            .iter()
            .filter_map(|((f, t), w)| {
                if *f == from && *w >= W_MIN && self.weight(*t, from) >= W_MIN {
                    Some((*t, *w))
                } else {
                    None
                }
            })
            .collect();
        out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutual_required() {
        let mut g = BondGraph::new();
        let a = DogId::new();
        let b = DogId::new();
        g.set_weight(a, b, 0.9);
        assert!(!g.mutual_eligible(a, b));
        g.set_weight(b, a, 0.9);
        assert!(g.mutual_eligible(a, b));
    }

    #[test]
    fn candidates_sorted() {
        let mut g = BondGraph::new();
        let a = DogId::new();
        let b = DogId::new();
        let c = DogId::new();
        g.bootstrap_mutual(a, b, 0.5);
        g.bootstrap_mutual(a, c, 0.9);
        let cands = g.candidates(a);
        assert_eq!(cands[0].0, c);
        assert_eq!(cands[1].0, b);
    }
}
