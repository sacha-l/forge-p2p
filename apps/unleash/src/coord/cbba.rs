//! Consensus-Based Bundle Algorithm — simplified.
//!
//! Real CBBA (Choi/Brunet/How 2009) alternates bundle-building and
//! consensus phases. We implement a condensed variant that preserves the
//! observable dynamics (bids propagate, highest wins, winner gossiped)
//! while fitting the M3 time budget. See `decisions.md`.
//!
//! Score: `score(t) = dot(cap, req(t)) * urgency(t) - α·travel_cost - β·risk`.
//! α and β are held small (0.1) so capability dominates.

use std::collections::HashMap;
use std::sync::RwLock;

use crate::config::{Pose3, TaskSpec};
use crate::keyspace::{Bid, Capability, TaskWinner};

const ALPHA: f32 = 0.02; // per-metre travel penalty (small so capability dominates)
const BETA: f32 = 0.2; // risk penalty
const MAX_BUNDLE: usize = 3;

pub struct Cbba {
    pub self_id: String,
    cap: Capability,
    inner: RwLock<Inner>,
}

struct Inner {
    known_tasks: HashMap<String, TaskSpec>,
    /// Highest bid seen for each task so far (robot_id, score).
    leader: HashMap<String, (String, f32)>,
    /// Tasks won *by self* (i.e. self is the current leader).
    own_assigned: Vec<String>,
    /// Tasks this robot is considering for its own bundle (pending bid submission).
    own_bundle: Vec<(String, f32)>,
    /// Last round at which `leader` for a task changed (for convergence detection).
    last_change: HashMap<String, u64>,
    round: u64,
    /// Peer IDs blacklisted (e.g., byzantine). Ignored in bid + winner updates.
    blacklisted: std::collections::HashSet<String>,
}

impl Cbba {
    pub fn new(self_id: String, cap: Capability) -> Self {
        Self {
            self_id,
            cap,
            inner: RwLock::new(Inner {
                known_tasks: HashMap::new(),
                leader: HashMap::new(),
                own_assigned: Vec::new(),
                own_bundle: Vec::new(),
                last_change: HashMap::new(),
                round: 0,
                blacklisted: Default::default(),
            }),
        }
    }

    /// Record a new task announcement.
    pub fn on_task_announce(&self, t: TaskSpec, self_pose: Pose3) -> Option<Bid> {
        let mut inner = self.inner.write().expect("cbba poisoned");
        if inner.known_tasks.contains_key(&t.id) {
            return None;
        }
        let score = self.score_for(&t, self_pose);
        inner.known_tasks.insert(t.id.clone(), t.clone());
        if inner.own_bundle.len() < MAX_BUNDLE && score > 0.0 {
            inner.own_bundle.push((t.id.clone(), score));
            inner.own_bundle.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        }
        // Seed the leader with self's bid so gossip can improve on it.
        let entry = inner
            .leader
            .entry(t.id.clone())
            .or_insert((self.self_id.clone(), score));
        if score > entry.1 {
            *entry = (self.self_id.clone(), score);
        }
        let round = inner.round;
        inner.last_change.insert(t.id.clone(), round);
        Some(Bid {
            task_id: t.id,
            robot_id: self.self_id.clone(),
            score,
            ts_ms: crate::keyspace::now_ms(),
        })
    }

    /// Bids from peers (and self) come through here.
    pub fn on_bid(&self, bid: Bid) -> Option<TaskWinner> {
        let mut inner = self.inner.write().expect("cbba poisoned");
        if inner.blacklisted.contains(&bid.robot_id) {
            return None;
        }
        let (new_leader, changed) = {
            let entry = inner
                .leader
                .entry(bid.task_id.clone())
                .or_insert((bid.robot_id.clone(), f32::NEG_INFINITY));
            let changed = bid.score > entry.1 + 1e-4
                || (bid.score > entry.1 - 1e-4 && bid.robot_id < entry.0);
            if changed {
                *entry = (bid.robot_id.clone(), bid.score);
            }
            (entry.clone(), changed)
        };
        if changed {
            let round = inner.round;
            inner.last_change.insert(bid.task_id.clone(), round);
            Some(TaskWinner {
                task_id: bid.task_id,
                winner: new_leader.0,
                bid_score: new_leader.1,
                ts_ms: crate::keyspace::now_ms(),
            })
        } else {
            None
        }
    }

    pub fn tick_round(&self) {
        let mut inner = self.inner.write().expect("cbba poisoned");
        inner.round += 1;
        // Recompute own_assigned from leader: which tasks does self currently lead?
        let me = self.self_id.clone();
        inner.own_assigned = inner
            .leader
            .iter()
            .filter_map(|(tid, (winner, _))| if winner == &me { Some(tid.clone()) } else { None })
            .collect();
        inner.own_assigned.sort();
    }

    pub fn blacklist(&self, peer: &str) {
        let mut inner = self.inner.write().expect("cbba poisoned");
        inner.blacklisted.insert(peer.to_string());
        // Also retract any win the blacklisted peer holds.
        let bad: Vec<String> = inner
            .leader
            .iter()
            .filter_map(|(k, (w, _))| if w == peer { Some(k.clone()) } else { None })
            .collect();
        for k in bad {
            inner.leader.remove(&k);
        }
    }

    pub fn current_assignments(&self) -> Vec<(String, String, f32)> {
        let inner = self.inner.read().expect("cbba poisoned");
        inner
            .leader
            .iter()
            .map(|(t, (w, s))| (t.clone(), w.clone(), *s))
            .collect()
    }

    pub fn own_bundle(&self) -> Vec<(String, f32)> {
        self.inner.read().expect("cbba poisoned").own_bundle.clone()
    }

    pub fn round(&self) -> u64 {
        self.inner.read().expect("cbba poisoned").round
    }

    /// Has every task's leader been stable for at least `rounds` rounds?
    pub fn converged(&self, rounds: u64) -> bool {
        let inner = self.inner.read().expect("cbba poisoned");
        if inner.known_tasks.is_empty() {
            return true;
        }
        inner.known_tasks.keys().all(|t| {
            let changed_at = inner.last_change.get(t).copied().unwrap_or(0);
            inner.round.saturating_sub(changed_at) >= rounds
        })
    }

    fn score_for(&self, t: &TaskSpec, self_pose: Pose3) -> f32 {
        let dot = self.cap.dot(&t.required_capability);
        let travel = geometry_distance(t, self_pose);
        let risk = task_risk(t);
        dot * t.urgency - ALPHA * travel - BETA * risk
    }
}

fn geometry_distance(t: &TaskSpec, self_pose: Pose3) -> f32 {
    use crate::config::Geometry;
    match &t.geometry {
        Geometry::Point { point } => sq_dist2(self_pose, *point).sqrt(),
        Geometry::Polygon { polygon } => {
            // Distance to centroid.
            let n = polygon.len().max(1) as f32;
            let cx: f32 = polygon.iter().map(|p| p[0]).sum::<f32>() / n;
            let cy: f32 = polygon.iter().map(|p| p[1]).sum::<f32>() / n;
            sq_dist2(self_pose, [cx, cy]).sqrt()
        }
        Geometry::Line { line } => {
            if let Some(first) = line.first() {
                sq_dist2(self_pose, *first).sqrt()
            } else {
                0.0
            }
        }
    }
}

fn sq_dist2(a: Pose3, b: [f32; 2]) -> f32 {
    let dx = a.x - b[0];
    let dy = a.y - b[1];
    dx * dx + dy * dy
}

fn task_risk(_t: &TaskSpec) -> f32 {
    // Spec schema has `risk` but mission.yaml omits it. Treat as 0.
    0.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keyspace::RobotClass;

    fn task(id: &str, req: &[(&str, f32)], urgency: f32) -> TaskSpec {
        let mut m = std::collections::BTreeMap::new();
        for (k, v) in req {
            m.insert(k.to_string(), *v);
        }
        TaskSpec {
            id: id.into(),
            kind: "survey_area".into(),
            geometry: crate::config::Geometry::Point { point: [10.0, 10.0] },
            urgency,
            required_capability: m,
        }
    }

    fn p(x: f32, y: f32) -> Pose3 {
        Pose3 { x, y, z: 0.0 }
    }

    #[test]
    fn scout_wins_aerial_survey() {
        let scout = Cbba::new("r_scout".into(), RobotClass::AerialScout.capability());
        let ground = Cbba::new("r_ground".into(), RobotClass::GroundScout.capability());
        let t = task("t1", &[("aerial", 1.0), ("survey", 0.8)], 0.9);
        let bid_s = scout.on_task_announce(t.clone(), p(0.0, 0.0)).unwrap();
        let bid_g = ground.on_task_announce(t, p(0.0, 0.0)).unwrap();
        // Cross-pollinate bids
        scout.on_bid(bid_g.clone());
        ground.on_bid(bid_s.clone());
        let assignments_scout = scout.current_assignments();
        let assignments_ground = ground.current_assignments();
        assert_eq!(assignments_scout[0].1, "r_scout");
        assert_eq!(assignments_ground[0].1, "r_scout");
    }

    #[test]
    fn convergence_after_rounds() {
        let c = Cbba::new("r0".into(), RobotClass::GroundScout.capability());
        let t = task("t1", &[("inspect_narrow", 1.0)], 0.9);
        c.on_task_announce(t, p(0.0, 0.0));
        assert!(!c.converged(5));
        for _ in 0..10 {
            c.tick_round();
        }
        assert!(c.converged(5));
    }

    #[test]
    fn blacklist_retracts_wins() {
        let c = Cbba::new("r0".into(), RobotClass::GroundScout.capability());
        let t = task("t1", &[("inspect_narrow", 1.0)], 0.9);
        c.on_task_announce(t, p(0.0, 0.0));
        let fake = Bid {
            task_id: "t1".into(),
            robot_id: "byz".into(),
            score: 1000.0,
            ts_ms: 0,
        };
        c.on_bid(fake);
        assert_eq!(c.current_assignments()[0].1, "byz");
        c.blacklist("byz");
        assert!(c.current_assignments().is_empty());
    }
}
