//! Link quality model — the "netem middleware" replacement (see `decisions.md`).
//!
//! Each robot maintains a view of every other robot's pose (from 10 Hz gossip
//! heartbeats) and recomputes reachability locally on a timer. For each peer,
//! the ray-cast from self to peer intersects hazard polygons; attenuation is
//! summed and a profile (`default` / `degraded` / `blackout`) is selected.
//!
//! The filter is applied at the app layer on every incoming gossip/RPC: drop
//! with `loss_rate` probability, inject `latency_ms` delay. Phase 3 broadcasts
//! a global override on `unleash/control/link_profile`.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot_stub::RwLock;
use rand::Rng;

use crate::config::{EnvironmentBody, LinkProfile, LinkProfiles, Pose3};
#[cfg(test)]
use crate::config::Hazard;

// Re-export RwLock with a shim name so we don't need to add parking_lot
// as a dep; std's RwLock is fine for our use (non-poisoning by convention).
mod parking_lot_stub {
    pub use std::sync::RwLock;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Default,
    Degraded,
    Blackout,
}

impl Profile {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "default" => Some(Self::Default),
            "degraded" => Some(Self::Degraded),
            "blackout" => Some(Self::Blackout),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Degraded => "degraded",
            Self::Blackout => "blackout",
        }
    }

    fn lookup(self, profiles: &LinkProfiles) -> LinkProfile {
        match self {
            Self::Default => profiles.default,
            Self::Degraded => profiles.degraded,
            Self::Blackout => profiles.blackout,
        }
    }
}

/// Ray-cast segment vs hazard polygon: count the intersected edges and
/// multiply by rubble attenuation. Very simplified — good enough for demo
/// scoring, honest about being a stub.
fn attenuation_db(
    self_pose: Pose3,
    peer_pose: Pose3,
    env: &EnvironmentBody,
) -> f32 {
    let mut db = env.material_attenuation.free_space_db;
    for hz in &env.hazards {
        let xings = segment_polygon_intersections(self_pose, peer_pose, &hz.polygon);
        // Each crossing pair = "inside" the hazard; each segment crossing pair contributes rubble_db.
        // Half the xings count (entry+exit) -- attenuation per passage.
        let passages = (xings / 2).max(0);
        db += passages as f32 * env.material_attenuation.rubble_db;
    }
    db
}

fn segment_polygon_intersections(a: Pose3, b: Pose3, poly: &[[f32; 2]]) -> i32 {
    if poly.len() < 3 {
        return 0;
    }
    let mut n = 0;
    for i in 0..poly.len() {
        let p1 = poly[i];
        let p2 = poly[(i + 1) % poly.len()];
        if segments_intersect([a.x, a.y], [b.x, b.y], p1, p2) {
            n += 1;
        }
    }
    n
}

fn segments_intersect(a1: [f32; 2], a2: [f32; 2], b1: [f32; 2], b2: [f32; 2]) -> bool {
    let d = (a2[0] - a1[0]) * (b2[1] - b1[1]) - (a2[1] - a1[1]) * (b2[0] - b1[0]);
    if d.abs() < 1e-9 {
        return false;
    }
    let t = ((b1[0] - a1[0]) * (b2[1] - b1[1]) - (b1[1] - a1[1]) * (b2[0] - b1[0])) / d;
    let u = ((b1[0] - a1[0]) * (a2[1] - a1[1]) - (b1[1] - a1[1]) * (a2[0] - a1[0])) / d;
    (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)
}

/// Classify a link by summed attenuation (more negative = worse).
/// Thresholds tuned so one rubble passage (-20 dB) → degraded, three
/// passages (-60 dB) → blackout.
pub fn classify_profile(att_db: f32) -> Profile {
    if att_db >= -10.0 {
        Profile::Default
    } else if att_db >= -50.0 {
        Profile::Degraded
    } else {
        Profile::Blackout
    }
}

/// State shared between the robot's receive loop and the link-recompute task.
#[derive(Default)]
pub struct LinkState {
    per_peer: RwLock<HashMap<String, Profile>>,
    /// Global override set by the Phase 3 broadcast on `unleash/control/link_profile`.
    global_override: RwLock<Option<Profile>>,
}

impl LinkState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn update_peer(&self, peer_id: &str, profile: Profile) {
        self.per_peer
            .write()
            .expect("link state poisoned")
            .insert(peer_id.to_string(), profile);
    }

    pub fn set_global_override(&self, profile: Option<Profile>) {
        *self.global_override.write().expect("link state poisoned") = profile;
    }

    pub fn profile_for(&self, peer_id: &str) -> Profile {
        if let Some(p) = *self.global_override.read().expect("link state poisoned") {
            return p;
        }
        self.per_peer
            .read()
            .expect("link state poisoned")
            .get(peer_id)
            .copied()
            .unwrap_or(Profile::Default)
    }

    pub fn snapshot(&self) -> Vec<(String, Profile)> {
        self.per_peer
            .read()
            .expect("link state poisoned")
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect()
    }
}

/// Should a message from `peer_id` be dropped at the app layer? Consults the
/// profile's `loss` rate. Callers should additionally sleep for `latency_ms`
/// if they want to simulate latency.
pub fn should_drop(profile: Profile, profiles: &LinkProfiles, rng: &mut impl Rng) -> bool {
    let p = profile.lookup(profiles);
    if p.loss >= 1.0 {
        return true;
    }
    if p.loss <= 0.0 {
        return false;
    }
    rng.gen::<f32>() < p.loss
}

pub fn latency_ms(profile: Profile, profiles: &LinkProfiles) -> u64 {
    profile.lookup(profiles).latency_ms.max(0.0) as u64
}

/// Recompute per-peer profiles from a table of observed peer poses.
pub fn recompute(
    state: &LinkState,
    self_pose: Pose3,
    peer_poses: &HashMap<String, Pose3>,
    env: &EnvironmentBody,
) {
    for (peer_id, peer_pose) in peer_poses {
        let db = attenuation_db(self_pose, *peer_pose, env);
        state.update_peer(peer_id, classify_profile(db));
    }
}

/// Clamp `other_hz` into the `env.hazards` format so the function stays
/// generic for unit tests.
#[cfg(test)]
pub(crate) fn test_attenuation(
    self_pose: Pose3,
    peer_pose: Pose3,
    hazards: Vec<Hazard>,
    attn: crate::config::MaterialAttenuation,
) -> f32 {
    let env = EnvironmentBody {
        world: "test".into(),
        footprint: crate::config::Footprint { x: 40.0, y: 25.0 },
        floors: 1,
        hazards,
        link_profiles: LinkProfiles {
            default: LinkProfile {
                bandwidth_mbps: 1.0,
                latency_ms: 0.0,
                loss: 0.0,
            },
            degraded: LinkProfile {
                bandwidth_mbps: 1.0,
                latency_ms: 0.0,
                loss: 0.5,
            },
            blackout: LinkProfile {
                bandwidth_mbps: 0.0,
                latency_ms: 0.0,
                loss: 1.0,
            },
        },
        material_attenuation: attn,
        link_recompute_interval_ms: 500,
        spawn_positions: Default::default(),
    };
    attenuation_db(self_pose, peer_pose, &env)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Footprint, Hazard, MaterialAttenuation};

    fn attn() -> MaterialAttenuation {
        MaterialAttenuation {
            concrete_wall_db: -40.0,
            rubble_db: -20.0,
            free_space_db: 0.0,
        }
    }

    #[test]
    fn clear_los_is_default() {
        let a = Pose3 { x: 0.0, y: 0.0, z: 0.0 };
        let b = Pose3 { x: 5.0, y: 0.0, z: 0.0 };
        let db = test_attenuation(a, b, vec![], attn());
        assert_eq!(classify_profile(db), Profile::Default);
    }

    #[test]
    fn through_rubble_is_degraded() {
        let a = Pose3 { x: 0.0, y: 5.0, z: 0.0 };
        let b = Pose3 { x: 40.0, y: 5.0, z: 0.0 };
        let hazards = vec![Hazard {
            kind: "rubble".into(),
            polygon: vec![[10.0, 0.0], [20.0, 0.0], [20.0, 10.0], [10.0, 10.0]],
            risk: 0.5,
            drone_exclusion: false,
            pressure_trigger: false,
        }];
        let db = test_attenuation(a, b, hazards, attn());
        assert!(db <= -20.0, "expected at least one rubble pass: {db}");
        // One rubble pass: should be degraded, not blackout.
        assert_eq!(classify_profile(db), Profile::Degraded);
    }

    #[test]
    fn through_multiple_rubble_is_blackout() {
        let a = Pose3 { x: 0.0, y: 5.0, z: 0.0 };
        let b = Pose3 { x: 40.0, y: 5.0, z: 0.0 };
        let hazards = vec![
            Hazard {
                kind: "r1".into(),
                polygon: vec![[5.0, 0.0], [10.0, 0.0], [10.0, 10.0], [5.0, 10.0]],
                risk: 0.5,
                drone_exclusion: false,
                pressure_trigger: false,
            },
            Hazard {
                kind: "r2".into(),
                polygon: vec![[20.0, 0.0], [25.0, 0.0], [25.0, 10.0], [20.0, 10.0]],
                risk: 0.5,
                drone_exclusion: false,
                pressure_trigger: false,
            },
            Hazard {
                kind: "r3".into(),
                polygon: vec![[30.0, 0.0], [35.0, 0.0], [35.0, 10.0], [30.0, 10.0]],
                risk: 0.5,
                drone_exclusion: false,
                pressure_trigger: false,
            },
        ];
        let db = test_attenuation(a, b, hazards, attn());
        assert!(db <= -60.0, "expected blackout-level attenuation: {db}");
        assert_eq!(classify_profile(db), Profile::Blackout);
    }

    #[test]
    fn global_override_wins() {
        let s = LinkState::new();
        s.update_peer("a", Profile::Default);
        assert_eq!(s.profile_for("a"), Profile::Default);
        s.set_global_override(Some(Profile::Degraded));
        assert_eq!(s.profile_for("a"), Profile::Degraded);
        s.set_global_override(None);
        assert_eq!(s.profile_for("a"), Profile::Default);
    }

    // Helper for readability in other test modules.
    #[allow(dead_code)]
    pub(crate) fn fp() -> Footprint {
        Footprint { x: 40.0, y: 25.0 }
    }
}
