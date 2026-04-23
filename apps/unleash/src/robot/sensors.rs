//! Sensor stubs — LiDAR, RGB survivor detection, gas sensor.
//!
//! Each sensor produces a "decision signal" (the output that would be
//! emitted by an on-device TFLite model on the hardware path, per spec §9.2).
//! SwarmNL never sees raw sensor data.

use crate::config::{EnvironmentBody, Hazard, Pose3};
use crate::keyspace::RobotClass;

/// Decision signal emitted by an on-device model. On the hardware path this
/// is the actual output of TFLite / TensorRT — here we synthesise it from
/// ground truth + class sensing range.
#[derive(Debug, Clone, Copy)]
pub struct DecisionSignal {
    pub detected_survivor: Option<Pose3>,
    pub confidence: f32,
    pub gas_detected: bool,
    pub in_exclusion_zone: bool,
}

/// Scan at `pose` for survivors (ground truth) and hazards.
pub fn scan(
    pose: Pose3,
    class: RobotClass,
    survivors: &[Pose3],
    env: &EnvironmentBody,
) -> DecisionSignal {
    let range = match class {
        RobotClass::AerialScout => 8.0,      // optical range w/ dust
        RobotClass::AerialMapper => 30.0,    // 3D LiDAR
        RobotClass::GroundScout => 6.0,      // 2D LiDAR through dust
        RobotClass::GroundWorkhorse => 10.0, // 3D LiDAR
        RobotClass::Breadcrumb => 0.0,       // no sensors
    };

    let in_gas = inside_any(pose, env, "gas_leak");
    let gas_detected = in_gas && class == RobotClass::GroundScout;
    let in_exclusion_zone = in_gas
        && matches!(class, RobotClass::AerialScout | RobotClass::AerialMapper);

    let (nearest, dist2) = survivors
        .iter()
        .map(|s| (*s, sq_dist(pose, *s)))
        .fold((Pose3 { x: 0.0, y: 0.0, z: 0.0 }, f32::INFINITY), |acc, x| {
            if x.1 < acc.1 {
                x
            } else {
                acc
            }
        });
    let detected = if dist2.sqrt() <= range && !in_exclusion_zone {
        let conf = (1.0 - dist2.sqrt() / range).clamp(0.2, 1.0);
        Some((nearest, conf))
    } else {
        None
    };

    DecisionSignal {
        detected_survivor: detected.map(|(p, _)| p),
        confidence: detected.map(|(_, c)| c).unwrap_or(0.0),
        gas_detected,
        in_exclusion_zone,
    }
}

fn sq_dist(a: Pose3, b: Pose3) -> f32 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    let dz = a.z - b.z;
    dx * dx + dy * dy + dz * dz
}

fn inside_any(pose: Pose3, env: &EnvironmentBody, kind: &str) -> bool {
    env.hazards
        .iter()
        .filter(|h| h.kind == kind)
        .any(|h| point_in_polygon(pose, h))
}

fn point_in_polygon(pose: Pose3, hz: &Hazard) -> bool {
    let (x, y) = (pose.x, pose.y);
    let mut inside = false;
    let n = hz.polygon.len();
    if n < 3 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let xi = hz.polygon[i][0];
        let yi = hz.polygon[i][1];
        let xj = hz.polygon[j][0];
        let yj = hz.polygon[j][1];
        let intersect = ((yi > y) != (yj > y))
            && (x < (xj - xi) * (y - yi) / (yj - yi).max(1e-9) + xi);
        if intersect {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Seeded spawn of unknown survivors from `mission.seed`.
pub fn spawn_unknown_survivors(seed: u64, count: u32, env: &EnvironmentBody) -> Vec<Pose3> {
    use rand::rngs::SmallRng;
    use rand::{Rng, SeedableRng};
    let mut rng = SmallRng::seed_from_u64(seed);
    (0..count)
        .map(|_| Pose3 {
            x: rng.gen_range(1.0..env.footprint.x - 1.0),
            y: rng.gen_range(1.0..env.footprint.y - 1.0),
            z: rng.gen_range(0.0..2.0),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Footprint, LinkProfile, LinkProfiles, MaterialAttenuation};

    fn env() -> EnvironmentBody {
        EnvironmentBody {
            world: "t".into(),
            footprint: Footprint { x: 40.0, y: 25.0 },
            floors: 1,
            hazards: vec![Hazard {
                kind: "gas_leak".into(),
                polygon: vec![[10.0, 5.0], [18.0, 5.0], [18.0, 12.0], [10.0, 12.0]],
                risk: 1.0,
                drone_exclusion: true,
                pressure_trigger: false,
            }],
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
            material_attenuation: MaterialAttenuation {
                concrete_wall_db: -40.0,
                rubble_db: -20.0,
                free_space_db: 0.0,
            },
            link_recompute_interval_ms: 500,
            spawn_positions: Default::default(),
        }
    }

    #[test]
    fn aerial_scout_excluded_from_gas() {
        let p = Pose3 { x: 14.0, y: 8.0, z: 3.0 };
        let sig = scan(p, RobotClass::AerialScout, &[], &env());
        assert!(sig.in_exclusion_zone);
    }

    #[test]
    fn ground_scout_detects_gas() {
        let p = Pose3 { x: 14.0, y: 8.0, z: 0.0 };
        let sig = scan(p, RobotClass::GroundScout, &[], &env());
        assert!(sig.gas_detected);
        assert!(!sig.in_exclusion_zone);
    }

    #[test]
    fn detects_nearby_survivor() {
        let p = Pose3 { x: 0.0, y: 0.0, z: 0.0 };
        let s = Pose3 { x: 3.0, y: 2.0, z: 0.0 };
        let sig = scan(p, RobotClass::AerialScout, &[s], &env());
        assert!(sig.detected_survivor.is_some());
        assert!(sig.confidence > 0.0);
    }

    #[test]
    fn unknown_survivors_deterministic() {
        let e = env();
        let a = spawn_unknown_survivors(42, 2, &e);
        let b = spawn_unknown_survivors(42, 2, &e);
        assert_eq!(a.len(), 2);
        assert_eq!(a[0].x, b[0].x);
        assert_eq!(a[1].y, b[1].y);
    }
}
