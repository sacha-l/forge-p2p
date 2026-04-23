//! Wire schemas gossiped and replicated across the Unleash mesh.
//!
//! Every payload is JSON-encoded `Vec<u8>` wrapped in SwarmNL's `ByteVector`
//! (`Vec<Vec<u8>>`) for transport. Signing is out of scope for the M0–M5 slice
//! (see `decisions.md`); a `sender` field is included on every message so the
//! receiver can apply W-MSR / stigmergy ordering without needing SwarmNL's
//! built-in `source` identity.
//!
//! Gossip topics:
//!
//! | Topic                   | Payload             | Purpose |
//! |-------------------------|---------------------|---------|
//! | `unleash/pose`          | `PoseHeartbeat`     | 10 Hz robot pose + battery |
//! | `unleash/task/<id>/winner` | `TaskWinner`     | CBBA winner propagation |
//! | `unleash/task/announce` | `TaskSpec`          | new task into the auction pool |
//! | `unleash/map/merge/<r>` | `GridChunk`         | rendezvous-triggered grid sync |
//! | `unleash/consensus/victim_count` | `ConsensusValue` | W-MSR value |
//! | `unleash/control/link_profile` | `LinkProfileOverride` | Phase 3 broadcast |
//! | `unleash/survivor`      | `SurvivorReport`    | new detection (also replicated) |
//! | `unleash/bundle`        | `BundleAnnouncement` | CBBA current-bundle gossip |
//!
//! DHT keys:
//!
//! | Key                        | Value                |
//! |----------------------------|----------------------|
//! | `robot/<id>/capability`    | `Capability` JSON    |
//! | `robot/<id>/class`         | `RobotClass` name    |
//!
//! Replication networks:
//!
//! | Network           | Keys                    |
//! |-------------------|-------------------------|
//! | `unleash_survivors` | all `SurvivorReport`s |
//!
//! Shards (one per floor): `floor_<n>` for `n in 0..floors`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::config::Pose3;

/// The five mobile robot classes plus the static breadcrumb relay.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RobotClass {
    AerialScout,
    AerialMapper,
    GroundScout,
    GroundWorkhorse,
    Breadcrumb,
}

impl RobotClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AerialScout => "aerial_scout",
            Self::AerialMapper => "aerial_mapper",
            Self::GroundScout => "ground_scout",
            Self::GroundWorkhorse => "ground_workhorse",
            Self::Breadcrumb => "breadcrumb",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "aerial_scout" => Some(Self::AerialScout),
            "aerial_mapper" => Some(Self::AerialMapper),
            "ground_scout" => Some(Self::GroundScout),
            "ground_workhorse" => Some(Self::GroundWorkhorse),
            "breadcrumb" => Some(Self::Breadcrumb),
            _ => None,
        }
    }

    /// Canonical 9-dim capability vector per spec §2.
    pub fn capability(self) -> Capability {
        let mut cap = Capability::zero();
        match self {
            Self::AerialScout => {
                cap.survey = 1.0;
                cap.inspect_narrow = 0.2;
                cap.relay = 0.8;
                cap.aerial = 1.0;
            }
            Self::AerialMapper => {
                cap.survey = 0.9;
                cap.relay = 1.0;
                cap.aerial = 1.0;
            }
            Self::GroundScout => {
                cap.survey = 0.3;
                cap.inspect_narrow = 1.0;
                cap.relay = 0.4;
                cap.payload = 0.3;
                cap.gas_traverse = 1.0;
                cap.ground = 1.0;
                cap.victim_contact = 1.0;
            }
            Self::GroundWorkhorse => {
                cap.survey = 0.5;
                cap.inspect_narrow = 0.6;
                cap.relay = 1.0;
                cap.payload = 1.0;
                cap.deploy_node = 1.0;
                cap.ground = 1.0;
            }
            Self::Breadcrumb => {
                cap.relay = 1.0;
            }
        }
        cap
    }
}

/// 9-dim capability vector (spec §2.6).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct Capability {
    pub survey: f32,
    pub inspect_narrow: f32,
    pub relay: f32,
    pub payload: f32,
    pub deploy_node: f32,
    pub gas_traverse: f32,
    pub aerial: f32,
    pub ground: f32,
    pub victim_contact: f32,
}

impl Capability {
    pub fn zero() -> Self {
        Self::default()
    }

    /// Score-relevant dot product with a sparse required-capability map.
    pub fn dot(self, required: &BTreeMap<String, f32>) -> f32 {
        let mut s = 0.0;
        for (k, w) in required {
            let v = match k.as_str() {
                "survey" => self.survey,
                "inspect_narrow" => self.inspect_narrow,
                "relay" => self.relay,
                "payload" => self.payload,
                "deploy_node" => self.deploy_node,
                "gas_traverse" => self.gas_traverse,
                "aerial" => self.aerial,
                "ground" => self.ground,
                "victim_contact" => self.victim_contact,
                _ => 0.0,
            };
            s += v * w;
        }
        s
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoseHeartbeat {
    pub robot_id: String,
    pub class: RobotClass,
    pub pose: Pose3,
    pub battery: f32,
    pub ts_ms: u64,
    pub status: RobotStatus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RobotStatus {
    Nominal,
    Degraded,
    Offline,
    Byzantine,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurvivorReport {
    pub survivor_id: String,
    pub pose: Pose3,
    pub detected_by: String,
    pub confidence: f32,
    pub ts_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusValue {
    pub robot_id: String,
    pub topic: String,
    pub value: f32,
    pub round: u32,
    pub ts_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkProfileOverride {
    /// `"default"` | `"degraded"` | `"blackout"`.
    pub profile: String,
    pub started_at_ms: u64,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskWinner {
    pub task_id: String,
    pub winner: String,
    pub bid_score: f32,
    pub ts_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleAnnouncement {
    pub robot_id: String,
    pub bundle: Vec<(String, f32)>, // (task_id, score)
    pub ts_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bid {
    pub task_id: String,
    pub robot_id: String,
    pub score: f32,
    pub ts_ms: u64,
}

/// A cell update from a robot's local occupancy grid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridCell {
    pub x: u16,
    pub y: u16,
    pub occupancy: u8, // 0 = free, 128 = unknown, 255 = occupied
    pub lamport: u64,
    pub updated_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridChunk {
    pub floor: u8,
    pub robot_id: String,
    pub cells: Vec<GridCell>,
    pub ts_ms: u64,
}

/// Stigmergy KV update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StigmergyUpdate {
    pub key: String,
    pub value: String,
    pub lamport: u64,
    pub robot_id: String,
}

// === topic helpers ===
pub const TOPIC_POSE: &str = "unleash/pose";
pub const TOPIC_TASK_ANNOUNCE: &str = "unleash/task/announce";
pub const TOPIC_TASK_WINNER: &str = "unleash/task/winner";
pub const TOPIC_BUNDLE: &str = "unleash/bundle";
pub const TOPIC_BID: &str = "unleash/bid";
pub const TOPIC_SURVIVOR: &str = "unleash/survivor";
pub const TOPIC_CONSENSUS_VICTIM: &str = "unleash/consensus/victim_count";
pub const TOPIC_CONTROL_LINK: &str = "unleash/control/link_profile";
pub const TOPIC_STIGMERGY: &str = "unleash/stigmergy";
pub const TOPIC_MAP_MERGE: &str = "unleash/map/merge";

pub const REPL_SURVIVORS: &str = "unleash_survivors";
pub const SHARD_NETWORK: &str = "unleash_floors";

/// All gossip topics a robot subscribes to, in deterministic order.
pub fn default_topics() -> Vec<&'static str> {
    vec![
        TOPIC_POSE,
        TOPIC_TASK_ANNOUNCE,
        TOPIC_TASK_WINNER,
        TOPIC_BUNDLE,
        TOPIC_BID,
        TOPIC_SURVIVOR,
        TOPIC_CONSENSUS_VICTIM,
        TOPIC_CONTROL_LINK,
        TOPIC_STIGMERGY,
        TOPIC_MAP_MERGE,
    ]
}

/// Encode any serializable payload as a SwarmNL `ByteVector` (single-chunk).
pub fn encode<T: Serialize>(v: &T) -> Vec<Vec<u8>> {
    vec![serde_json::to_vec(v).unwrap_or_default()]
}

/// Decode the first chunk of a SwarmNL `ByteVector` into T.
pub fn decode<T: for<'de> Deserialize<'de>>(data: &[Vec<u8>]) -> Option<T> {
    data.first().and_then(|b| serde_json::from_slice(b).ok())
}

/// Decode from a Vec<String> (what Gossipsub delivers in some variants).
pub fn decode_str<T: for<'de> Deserialize<'de>>(data: &[String]) -> Option<T> {
    data.first().and_then(|s| serde_json::from_str(s).ok())
}

/// Current unix time in milliseconds.
pub fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Pre-keys that would normally come from a multi-robot key server. For MVP
/// we accept unsigned messages and rely on W-MSR for byzantine detection.
#[derive(Debug, Clone, Copy)]
pub struct RobotIdentity;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_are_non_degenerate() {
        for c in [
            RobotClass::AerialScout,
            RobotClass::AerialMapper,
            RobotClass::GroundScout,
            RobotClass::GroundWorkhorse,
            RobotClass::Breadcrumb,
        ] {
            let cap = c.capability();
            // every class must score > 0 on *something*
            let sum = cap.survey
                + cap.inspect_narrow
                + cap.relay
                + cap.payload
                + cap.deploy_node
                + cap.gas_traverse
                + cap.aerial
                + cap.ground
                + cap.victim_contact;
            assert!(sum > 0.0, "{} capability sum is zero", c.as_str());
        }
    }

    #[test]
    fn aerial_scout_matches_survey_task() {
        let cap = RobotClass::AerialScout.capability();
        let mut req = BTreeMap::new();
        req.insert("survey".into(), 0.8);
        req.insert("aerial".into(), 1.0);
        let score = cap.dot(&req);
        assert!(score > 1.5, "survey score should be high, got {score}");
    }

    #[test]
    fn json_round_trip_pose_heartbeat() {
        let hb = PoseHeartbeat {
            robot_id: "r0".into(),
            class: RobotClass::AerialScout,
            pose: Pose3 { x: 1.0, y: 2.0, z: 3.0 },
            battery: 0.75,
            ts_ms: 12345,
            status: RobotStatus::Nominal,
        };
        let encoded = encode(&hb);
        let decoded: PoseHeartbeat = decode(&encoded).unwrap();
        assert_eq!(decoded.robot_id, hb.robot_id);
        assert_eq!(decoded.pose, hb.pose);
    }
}
