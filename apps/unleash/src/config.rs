//! YAML loaders for mission + environment context documents.
//!
//! Every scenario-specific parameter lives in one of these two files. A
//! scenario directory contains `mission.yaml` + `environment.yaml`.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mission {
    pub mission: MissionBody,
    #[serde(default = "default_fleet")]
    pub fleet: Fleet,
}

fn default_fleet() -> Fleet {
    Fleet {
        size: 10,
        composition: Composition {
            aerial_scout: 3,
            aerial_mapper: 1,
            ground_scout: 3,
            ground_workhorse: 2,
            breadcrumb: 8,
        },
        scale_mode: "fixed".into(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionBody {
    pub id: String,
    pub objective: String,
    pub time_limit_s: u64,
    pub target_count: u32,
    pub known_targets: Vec<KnownTarget>,
    pub unknown_targets: u32,
    pub seed: u64,
    pub initial_tasks: Vec<TaskSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnownTarget {
    pub id: String,
    pub pose: Pose3,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Pose3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSpec {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub geometry: Geometry,
    pub urgency: f32,
    pub required_capability: std::collections::BTreeMap<String, f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Geometry {
    Polygon { polygon: Vec<[f32; 2]> },
    Point { point: [f32; 2] },
    Line { line: Vec<[f32; 2]> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fleet {
    pub size: u32,
    pub composition: Composition,
    #[serde(default = "default_scale_mode")]
    pub scale_mode: String,
}

fn default_scale_mode() -> String {
    "fixed".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Composition {
    #[serde(default)]
    pub aerial_scout: u32,
    #[serde(default)]
    pub aerial_mapper: u32,
    #[serde(default)]
    pub ground_scout: u32,
    #[serde(default)]
    pub ground_workhorse: u32,
    #[serde(default)]
    pub breadcrumb: u32,
}

impl Composition {
    pub fn mobile_robot_count(&self) -> u32 {
        self.aerial_scout + self.aerial_mapper + self.ground_scout + self.ground_workhorse
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    pub environment: EnvironmentBody,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentBody {
    pub world: String,
    pub footprint: Footprint,
    pub floors: u8,
    #[serde(default)]
    pub hazards: Vec<Hazard>,
    pub link_profiles: LinkProfiles,
    pub material_attenuation: MaterialAttenuation,
    #[serde(default = "default_link_recompute_ms")]
    pub link_recompute_interval_ms: u64,
    #[serde(default)]
    pub spawn_positions: std::collections::BTreeMap<String, Vec<[f32; 3]>>,
}

fn default_link_recompute_ms() -> u64 {
    500
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Footprint {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hazard {
    #[serde(rename = "type")]
    pub kind: String,
    pub polygon: Vec<[f32; 2]>,
    pub risk: f32,
    #[serde(default)]
    pub drone_exclusion: bool,
    #[serde(default)]
    pub pressure_trigger: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkProfiles {
    pub default: LinkProfile,
    pub degraded: LinkProfile,
    pub blackout: LinkProfile,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct LinkProfile {
    pub bandwidth_mbps: f32,
    pub latency_ms: f32,
    pub loss: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MaterialAttenuation {
    pub concrete_wall_db: f32,
    pub rubble_db: f32,
    pub free_space_db: f32,
}

/// Load both `mission.yaml` and `environment.yaml` from a scenario directory.
pub fn load_scenario(dir: &Path) -> Result<(Mission, Environment)> {
    let mission_path = dir.join("mission.yaml");
    let env_path = dir.join("environment.yaml");
    let mission: Mission = serde_yaml::from_str(
        &std::fs::read_to_string(&mission_path)
            .with_context(|| format!("reading {}", mission_path.display()))?,
    )
    .with_context(|| format!("parsing {}", mission_path.display()))?;
    let env: Environment = serde_yaml::from_str(
        &std::fs::read_to_string(&env_path)
            .with_context(|| format!("reading {}", env_path.display()))?,
    )
    .with_context(|| format!("parsing {}", env_path.display()))?;
    validate(&mission, &env)?;
    Ok((mission, env))
}

fn validate(m: &Mission, e: &Environment) -> Result<()> {
    if m.mission.time_limit_s == 0 {
        anyhow::bail!("mission.time_limit_s must be > 0");
    }
    if e.environment.footprint.x <= 0.0 || e.environment.footprint.y <= 0.0 {
        anyhow::bail!("environment.footprint dimensions must be positive");
    }
    if (e.environment.link_profiles.default.loss + 1e-6) > 1.0 {
        anyhow::bail!("link_profiles.default.loss must be in [0,1]");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scenario_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/disaster_relief")
    }

    #[test]
    fn loads_reference_scenario() {
        let (m, e) = load_scenario(&scenario_dir()).expect("reference scenario must load");
        assert_eq!(m.mission.target_count, 5);
        assert_eq!(m.mission.known_targets.len(), 3);
        assert!(m.mission.initial_tasks.iter().any(|t| t.kind == "survey_area"));
        assert_eq!(e.environment.floors, 4);
        assert!(e.environment.link_profiles.blackout.loss >= 0.999);
    }

    #[test]
    fn fleet_mobile_count() {
        let (m, _) = load_scenario(&scenario_dir()).unwrap();
        assert_eq!(m.fleet.composition.mobile_robot_count(), 9);
    }
}
