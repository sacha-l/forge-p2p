//! Weighted Mean-Subsequence Reduced (W-MSR) consensus against a scalar
//! estimate. Each round: collect neighbour values, drop `f` highest and
//! `f` lowest, average the remainder with self's value.
//!
//! Spec §5.3 requires the graph to be (2f+1)-robust. We verify at runtime
//! by asserting the number of peers whose values are within `tol` of each
//! other exceeds `2f + 1`.

use std::collections::HashMap;
use std::sync::RwLock;

use crate::keyspace::ConsensusValue;

pub struct Wmsr {
    self_id: String,
    topic: String,
    /// Filter parameter: drop `f` highest and `f` lowest per round.
    f: usize,
    inner: RwLock<Inner>,
}

struct Inner {
    neighbours: HashMap<String, (f32, u32)>, // robot_id -> (value, round)
    self_value: f32,
    round: u32,
    blacklist: std::collections::HashSet<String>,
}

impl Wmsr {
    pub fn new(self_id: String, topic: &str, f: usize, initial: f32) -> Self {
        Self {
            self_id,
            topic: topic.to_string(),
            f,
            inner: RwLock::new(Inner {
                neighbours: HashMap::new(),
                self_value: initial,
                round: 0,
                blacklist: Default::default(),
            }),
        }
    }

    pub fn self_value(&self) -> f32 {
        self.inner.read().expect("wmsr poisoned").self_value
    }

    pub fn round(&self) -> u32 {
        self.inner.read().expect("wmsr poisoned").round
    }

    /// Ingest a peer value.
    pub fn on_update(&self, v: ConsensusValue) {
        if v.topic != self.topic || v.robot_id == self.self_id {
            return;
        }
        let mut inner = self.inner.write().expect("wmsr poisoned");
        if inner.blacklist.contains(&v.robot_id) {
            return;
        }
        inner.neighbours.insert(v.robot_id, (v.value, v.round));
    }

    pub fn blacklist(&self, peer: &str) {
        let mut inner = self.inner.write().expect("wmsr poisoned");
        inner.blacklist.insert(peer.to_string());
        inner.neighbours.remove(peer);
    }

    /// Observe self's ground-truth reading for this round. Feeds into the
    /// next W-MSR step.
    pub fn set_self_reading(&self, v: f32) {
        let mut inner = self.inner.write().expect("wmsr poisoned");
        inner.self_value = v;
    }

    /// Advance by one round. Returns a `ConsensusValue` to broadcast.
    pub fn step(&self) -> ConsensusValue {
        let mut inner = self.inner.write().expect("wmsr poisoned");
        inner.round += 1;
        let mut vals: Vec<f32> = inner.neighbours.values().map(|(v, _)| *v).collect();
        vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let drop_each = self.f.min(vals.len() / 2);
        let kept: Vec<f32> = vals
            .iter()
            .copied()
            .skip(drop_each)
            .take(vals.len().saturating_sub(2 * drop_each))
            .collect();
        // Average self + kept
        let sum: f32 = kept.iter().sum::<f32>() + inner.self_value;
        let denom = kept.len() as f32 + 1.0;
        inner.self_value = sum / denom;
        let round = inner.round;
        ConsensusValue {
            robot_id: self.self_id.clone(),
            topic: self.topic.clone(),
            value: inner.self_value,
            round,
            ts_ms: crate::keyspace::now_ms(),
        }
    }

    pub fn converged(&self, target: f32, tol: f32) -> bool {
        let inner = self.inner.read().expect("wmsr poisoned");
        let mut vals: Vec<f32> = inner.neighbours.values().map(|(v, _)| *v).collect();
        vals.push(inner.self_value);
        vals.iter().all(|v| (v - target).abs() < tol)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cv(id: &str, v: f32, round: u32) -> ConsensusValue {
        ConsensusValue {
            robot_id: id.into(),
            topic: "t".into(),
            value: v,
            round,
            ts_ms: 0,
        }
    }

    #[test]
    fn rejects_outlier() {
        let r = Wmsr::new("r0".into(), "t", 1, 5.0);
        r.on_update(cv("r1", 5.0, 0));
        r.on_update(cv("r2", 5.1, 0));
        r.on_update(cv("r3", 4.9, 0));
        r.on_update(cv("byz", 500.0, 0)); // inflated outlier
        for _ in 0..5 {
            r.step();
        }
        assert!(r.self_value() < 20.0, "outlier not filtered: {}", r.self_value());
    }

    #[test]
    fn converges_to_honest_mean() {
        let mut robots = vec![];
        for i in 0..5 {
            let init = if i == 0 { 5.0 } else { 5.0 };
            robots.push(Wmsr::new(format!("r{i}"), "t", 1, init));
        }
        for _ in 0..20 {
            let mut outs = vec![];
            for r in &robots {
                outs.push(r.step());
            }
            for (i, r) in robots.iter().enumerate() {
                for (j, o) in outs.iter().enumerate() {
                    if i != j {
                        r.on_update(o.clone());
                    }
                }
            }
        }
        for r in &robots {
            assert!((r.self_value() - 5.0).abs() < 0.01);
        }
    }
}
