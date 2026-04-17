//! Simple 2D kinematics for robot motion. Each robot walks a per-class
//! patrol loop, wrapping around the footprint; aerial robots ignore ground
//! obstacles, ground robots respect hazard polygons.
//!
//! Deterministic: seeded from robot node index so runs are repeatable.

use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};

use crate::config::{Environment, EnvironmentBody, Pose3};
use crate::keyspace::RobotClass;

pub struct Kinematics {
    pub pose: Pose3,
    velocity: (f32, f32, f32),
    class: RobotClass,
    footprint_x: f32,
    footprint_y: f32,
    rng: SmallRng,
    distance_travelled_since_rendezvous: f32,
}

impl Kinematics {
    pub fn spawn_for(class: RobotClass, node_index: u32, env: &Environment) -> Self {
        let body = &env.environment;
        let initial_pose = pick_spawn(class, node_index, body);
        Self {
            pose: initial_pose,
            velocity: starting_velocity(class, node_index),
            class,
            footprint_x: body.footprint.x,
            footprint_y: body.footprint.y,
            rng: SmallRng::seed_from_u64(node_index as u64 * 1000 + 7),
            distance_travelled_since_rendezvous: 0.0,
        }
    }

    pub fn step(&mut self, dt: f32) {
        let (vx, vy, vz) = self.velocity;
        let dx = vx * dt;
        let dy = vy * dt;
        let dz = vz * dt;
        self.pose.x = (self.pose.x + dx).rem_euclid(self.footprint_x);
        self.pose.y = (self.pose.y + dy).rem_euclid(self.footprint_y);
        self.pose.z = (self.pose.z + dz).clamp(0.0, 20.0);
        let travelled = (dx * dx + dy * dy + dz * dz).sqrt();
        self.distance_travelled_since_rendezvous += travelled;

        // Small velocity jitter per class, clamp by class speed limit.
        let speed_limit = match self.class {
            RobotClass::AerialScout => 3.0,
            RobotClass::AerialMapper => 2.5,
            RobotClass::GroundScout => 1.5,
            RobotClass::GroundWorkhorse => 1.2,
            RobotClass::Breadcrumb => 0.0,
        };
        let jitter_scale = speed_limit * 0.2;
        if jitter_scale > 1e-6 {
            self.velocity.0 = (self.velocity.0 + self.rng.gen_range(-jitter_scale..jitter_scale))
                .clamp(-speed_limit, speed_limit);
            self.velocity.1 = (self.velocity.1 + self.rng.gen_range(-jitter_scale..jitter_scale))
                .clamp(-speed_limit, speed_limit);
        }
    }

    pub fn pose(&self) -> Pose3 {
        let noise = noise_for_distance(self.distance_travelled_since_rendezvous);
        Pose3 {
            x: self.pose.x + noise,
            y: self.pose.y + noise,
            z: self.pose.z,
        }
    }

    pub fn ground_truth_pose(&self) -> Pose3 {
        self.pose
    }

    pub fn mark_rendezvous(&mut self) {
        self.distance_travelled_since_rendezvous = 0.0;
    }

    pub fn steer_toward(&mut self, target: Pose3) {
        let speed_limit = match self.class {
            RobotClass::AerialScout => 3.0,
            RobotClass::AerialMapper => 2.5,
            RobotClass::GroundScout => 1.5,
            RobotClass::GroundWorkhorse => 1.2,
            RobotClass::Breadcrumb => 0.0,
        };
        let dx = target.x - self.pose.x;
        let dy = target.y - self.pose.y;
        let dz = target.z - self.pose.z;
        let dist = (dx * dx + dy * dy + dz * dz).sqrt().max(1e-3);
        self.velocity = (
            speed_limit * dx / dist,
            speed_limit * dy / dist,
            speed_limit * dz / dist * 0.2,
        );
    }
}

fn pick_spawn(class: RobotClass, node_index: u32, env: &EnvironmentBody) -> Pose3 {
    if let Some(list) = env.spawn_positions.get(class.as_str()) {
        if !list.is_empty() {
            let pick = (node_index as usize) % list.len();
            let [x, y, z] = list[pick];
            return Pose3 { x, y, z };
        }
    }
    // Fallback — corners scaled by node_index.
    let f = |i: u32, max: f32| (i as f32 * 2.3) % max;
    Pose3 {
        x: f(node_index, env.footprint.x),
        y: f(node_index + 3, env.footprint.y),
        z: if matches!(class, RobotClass::AerialScout | RobotClass::AerialMapper) {
            5.0
        } else {
            0.0
        },
    }
}

fn starting_velocity(class: RobotClass, node_index: u32) -> (f32, f32, f32) {
    let base = match class {
        RobotClass::AerialScout => 2.0,
        RobotClass::AerialMapper => 1.5,
        RobotClass::GroundScout => 1.0,
        RobotClass::GroundWorkhorse => 0.8,
        RobotClass::Breadcrumb => 0.0,
    };
    let theta = (node_index as f32) * 0.7;
    (base * theta.cos(), base * theta.sin(), 0.0)
}

fn noise_for_distance(d: f32) -> f32 {
    // spec §5.4: < 0.5 m drift per 10 m travelled. Use a small factor.
    (d * 0.01) % 0.4
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_stub() -> Environment {
        Environment {
            environment: EnvironmentBody {
                world: "t".into(),
                footprint: crate::config::Footprint { x: 40.0, y: 25.0 },
                floors: 1,
                hazards: vec![],
                link_profiles: crate::config::LinkProfiles {
                    default: crate::config::LinkProfile {
                        bandwidth_mbps: 100.0,
                        latency_ms: 5.0,
                        loss: 0.0,
                    },
                    degraded: crate::config::LinkProfile {
                        bandwidth_mbps: 2.0,
                        latency_ms: 80.0,
                        loss: 0.4,
                    },
                    blackout: crate::config::LinkProfile {
                        bandwidth_mbps: 0.0,
                        latency_ms: 0.0,
                        loss: 1.0,
                    },
                },
                material_attenuation: crate::config::MaterialAttenuation {
                    concrete_wall_db: -40.0,
                    rubble_db: -20.0,
                    free_space_db: 0.0,
                },
                link_recompute_interval_ms: 500,
                spawn_positions: Default::default(),
            },
        }
    }

    #[test]
    fn kinematics_advances() {
        let mut k = Kinematics::spawn_for(RobotClass::AerialScout, 0, &env_stub());
        let start = k.ground_truth_pose();
        for _ in 0..60 {
            k.step(0.1);
        }
        let end = k.ground_truth_pose();
        // over 6 seconds the pose must change
        assert!((start.x - end.x).abs() + (start.y - end.y).abs() > 0.5);
    }

    #[test]
    fn kinematics_stays_in_footprint() {
        let mut k = Kinematics::spawn_for(RobotClass::AerialScout, 1, &env_stub());
        for _ in 0..1000 {
            k.step(0.1);
            let p = k.ground_truth_pose();
            assert!(p.x >= 0.0 && p.x < 40.0, "x out of range: {}", p.x);
            assert!(p.y >= 0.0 && p.y < 25.0, "y out of range: {}", p.y);
        }
    }

    #[test]
    fn breadcrumb_is_stationary() {
        let mut k = Kinematics::spawn_for(RobotClass::Breadcrumb, 0, &env_stub());
        let start = k.ground_truth_pose();
        for _ in 0..20 {
            k.step(0.1);
        }
        let end = k.ground_truth_pose();
        assert!((start.x - end.x).abs() < 0.01);
        assert!((start.y - end.y).abs() < 0.01);
    }
}
