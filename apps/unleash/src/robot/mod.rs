//! Robot entry point. Owns a SwarmNL node and all coordination state.
//!
//! Concurrency: three tokio tasks share the Core via `Arc<Mutex<Core>>`:
//!
//! * `pose_loop`   — 10 Hz: advances kinematics, senses, publishes pose,
//!   writes grid cells, notices rendezvous.
//! * `coord_loop`  —  1 Hz: advances CBBA rounds, steps W-MSR, publishes
//!   consensus values + bundle announcements.
//! * `net_loop`    — 10 Hz: drains `next_event()`, applies link-model
//!   filter, dispatches to coord.
//!
//! Robot 0 additionally announces initial tasks from `mission.yaml` after
//! the gossipsub mesh warms up (library-feedback #5).

pub mod kinematics;
pub mod sensors;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use rand::rngs::SmallRng;
use rand::SeedableRng;
use swarm_nl::core::{AppData, Core, NetworkEvent};
use tokio::sync::Mutex;

use crate::config::{Environment, Mission, Pose3};
use crate::control::{self, ControlFlags};
use crate::coord::cbba::Cbba;
use crate::coord::slam::{is_rendezvous, OccupancyGrid};
use crate::coord::stigmergy::Stigmergy;
use crate::coord::wmsr::Wmsr;
use crate::keyspace::{
    self, decode_str, encode, now_ms, Bid, BundleAnnouncement, ConsensusValue, GridChunk,
    LinkProfileOverride, PoseHeartbeat, RobotClass, RobotStatus, StigmergyUpdate, SurvivorReport,
    TaskWinner, TOPIC_BID, TOPIC_BUNDLE, TOPIC_CONSENSUS_VICTIM, TOPIC_CONTROL_LINK,
    TOPIC_MAP_MERGE, TOPIC_POSE, TOPIC_STIGMERGY, TOPIC_SURVIVOR, TOPIC_TASK_ANNOUNCE,
    TOPIC_TASK_WINNER,
};
use crate::link_model::{self, LinkState, Profile};
use crate::robot::kinematics::Kinematics;
use crate::robot::sensors::{scan, spawn_unknown_survivors};
use crate::swarm_node::{self, loopback_multiaddr};

pub struct Args {
    pub id: String,
    pub class: RobotClass,
    pub node_index: u32,
    pub mission: Mission,
    pub env: Environment,
    pub bootstrap: Option<String>,
    pub smoke: bool,
    pub control_port: Option<u16>,
}

pub async fn run(args: Args) -> Result<()> {
    let tcp = swarm_node::robot_tcp_port(args.node_index);
    let udp = swarm_node::robot_udp_port(args.node_index);
    tracing::info!(
        robot = %args.id, class = %args.class.as_str(),
        tcp, udp, bootstrap = ?args.bootstrap,
        "starting robot node"
    );

    let mut node = swarm_node::build_node(tcp, udp, args.bootstrap.as_deref()).await?;
    let (peer_id, addrs) = swarm_node::drain_listen_addrs(&mut node).await;
    let loopback = loopback_multiaddr(tcp);
    println!(
        "UNLEASH_ROBOT_READY id={} class={} peer_id={} tcp={} addr={}",
        args.id,
        args.class.as_str(),
        peer_id,
        tcp,
        loopback
    );
    for a in &addrs {
        tracing::debug!(addr=%a, "listen addr reported");
    }

    if args.smoke {
        tracing::info!("smoke mode: exiting after listen-addr drain");
        return Ok(());
    }

    // HTTP control port + link-override relay channel
    let flags = ControlFlags::new();
    let (link_override_tx, mut link_override_rx) =
        tokio::sync::mpsc::channel::<keyspace::LinkProfileOverride>(8);
    flags.set_link_override_sink(link_override_tx).await;
    let control_port = args.control_port.unwrap_or(60000 + args.node_index as u16);
    if let Err(e) = control::serve(control_port, flags.clone()).await {
        tracing::warn!(error = ?e, port = control_port, "control port failed to bind");
    }

    // Join all gossip topics + replication network
    join_core_topics(&mut node).await;
    let _ = node
        .join_repl_network(keyspace::REPL_SURVIVORS.to_string())
        .await;

    // Coordination state
    let cap = args.class.capability();
    let cbba = Arc::new(Cbba::new(args.id.clone(), cap));
    let stigmergy = Arc::new(Stigmergy::new(args.id.clone()));
    let wmsr = Arc::new(Wmsr::new(
        args.id.clone(),
        TOPIC_CONSENSUS_VICTIM,
        1, // f=1 — tolerate one byzantine
        0.0,
    ));
    let grid = Arc::new(OccupancyGrid::new(
        &args.id,
        args.env.environment.footprint,
        0, // single-floor simplification for MVP; sharding still keyed
    ));
    let link_state = LinkState::new();
    let peer_poses: Arc<Mutex<HashMap<String, Pose3>>> = Arc::new(Mutex::new(HashMap::new()));
    let self_pose: Arc<Mutex<Pose3>> = Arc::new(Mutex::new(Pose3 { x: 0.0, y: 0.0, z: 0.0 }));

    // Survivors (ground truth for sensing — each robot knows where they are
    // because they read mission.yaml; detection arises from sensor range).
    let mut survivors: Vec<Pose3> = args
        .mission
        .mission
        .known_targets
        .iter()
        .map(|t| t.pose)
        .collect();
    survivors.extend(spawn_unknown_survivors(
        args.mission.mission.seed,
        args.mission.mission.unknown_targets,
        &args.env.environment,
    ));
    let survivors = Arc::new(survivors);

    let node = Arc::new(Mutex::new(node));

    // Wait for gossipsub mesh to warm up before first broadcast
    // (library-feedback #5).
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Robot 0 announces the initial task pool.
    if args.node_index == 0 {
        announce_initial_tasks(&node, &args.mission).await;
    }

    // Spawn the three loops.
    let pose_task = spawn_pose_loop(
        args.id.clone(),
        args.class,
        args.node_index,
        args.env.clone(),
        survivors.clone(),
        Arc::clone(&cbba),
        Arc::clone(&wmsr),
        Arc::clone(&grid),
        Arc::clone(&peer_poses),
        Arc::clone(&link_state),
        Arc::clone(&node),
        flags.clone(),
        Arc::clone(&self_pose),
    );

    let coord_task = spawn_coord_loop(
        args.id.clone(),
        Arc::clone(&cbba),
        Arc::clone(&stigmergy),
        Arc::clone(&wmsr),
        Arc::clone(&node),
        flags.clone(),
    );

    let net_task = spawn_net_loop(
        args.id.clone(),
        args.env.clone(),
        Arc::clone(&cbba),
        Arc::clone(&stigmergy),
        Arc::clone(&wmsr),
        Arc::clone(&grid),
        Arc::clone(&peer_poses),
        Arc::clone(&link_state),
        Arc::clone(&node),
        Arc::clone(&self_pose),
    );

    // Link-override relay task: listens on control channel, broadcasts on gossip.
    let relay_node = Arc::clone(&node);
    let relay_state = Arc::clone(&link_state);
    let relay_task = tokio::spawn(async move {
        while let Some(lp) = link_override_rx.recv().await {
            let profile = link_model::Profile::parse(&lp.profile);
            relay_state.set_global_override(profile);
            let mut n = relay_node.lock().await;
            let _ = n
                .query_network(AppData::GossipsubBroadcastMessage {
                    topic: TOPIC_CONTROL_LINK.to_string(),
                    message: encode(&lp),
                })
                .await;
        }
    });

    tokio::select! {
        _ = pose_task => tracing::warn!("pose loop exited"),
        _ = coord_task => tracing::warn!("coord loop exited"),
        _ = net_task => tracing::warn!("net loop exited"),
        _ = relay_task => tracing::warn!("link-relay task exited"),
    }

    Ok(())
}

async fn join_core_topics(node: &mut Core) {
    for topic in keyspace::default_topics() {
        let req = AppData::GossipsubJoinNetwork(topic.to_string());
        if let Err(e) = node.query_network(req).await {
            tracing::warn!(topic, error = ?e, "gossip join failed");
        }
    }
}

async fn announce_initial_tasks(node: &Arc<Mutex<Core>>, mission: &Mission) {
    let mut n = node.lock().await;
    for t in &mission.mission.initial_tasks {
        let req = AppData::GossipsubBroadcastMessage {
            topic: TOPIC_TASK_ANNOUNCE.to_string(),
            message: encode(t),
        };
        if let Err(e) = n.query_network(req).await {
            tracing::warn!(task = %t.id, error = ?e, "task announce failed");
        } else {
            tracing::info!(task = %t.id, "announced");
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_pose_loop(
    id: String,
    class: RobotClass,
    node_index: u32,
    env: Environment,
    survivors: Arc<Vec<Pose3>>,
    cbba: Arc<Cbba>,
    wmsr: Arc<Wmsr>,
    grid: Arc<OccupancyGrid>,
    peer_poses: Arc<Mutex<HashMap<String, Pose3>>>,
    link_state: Arc<LinkState>,
    node: Arc<Mutex<Core>>,
    flags: Arc<ControlFlags>,
    self_pose_shared: Arc<Mutex<Pose3>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut kin = Kinematics::spawn_for(class, node_index, &env);
        let mut battery: f32 = 1.0;
        let mut tick = tokio::time::interval(Duration::from_millis(100));
        let mut survivor_count: f32 = 0.0;
        let mut known_survivors: std::collections::HashSet<String> = Default::default();

        loop {
            if flags.is_killed() {
                tracing::info!("killswitch set, exiting");
                std::process::exit(0);
            }
            tick.tick().await;
            kin.step(0.1);
            battery = (battery - 0.00008).max(0.0);
            let ground_truth = kin.ground_truth_pose();
            *self_pose_shared.lock().await = ground_truth;
            let reported = if flags.is_byzantine() {
                Pose3 {
                    x: ground_truth.x + 10.0,
                    y: ground_truth.y + 10.0,
                    z: ground_truth.z,
                }
            } else {
                kin.pose() // with pose noise
            };
            let status = if flags.is_byzantine() {
                RobotStatus::Byzantine
            } else {
                RobotStatus::Nominal
            };

            // Sense environment and maybe register a survivor.
            let sig = scan(ground_truth, class, &survivors, &env.environment);
            if let Some(pos) = sig.detected_survivor {
                // Deterministic survivor id from quantised position
                let sid = format!("s_{:.1}_{:.1}", pos.x, pos.y);
                if known_survivors.insert(sid.clone()) {
                    survivor_count += 1.0;
                    let report = SurvivorReport {
                        survivor_id: sid.clone(),
                        pose: pos,
                        detected_by: id.clone(),
                        confidence: sig.confidence,
                        ts_ms: now_ms(),
                    };
                    // Byzantine robots inflate their count AND invent phantom survivors.
                    let inflated = if flags.is_byzantine() {
                        survivor_count + 3.0
                    } else {
                        survivor_count
                    };
                    wmsr.set_self_reading(inflated);
                    // Gossip + replicate
                    let mut n = node.lock().await;
                    let _ = n
                        .query_network(AppData::GossipsubBroadcastMessage {
                            topic: TOPIC_SURVIVOR.to_string(),
                            message: encode(&report),
                        })
                        .await;
                    let _ = n.replicate(encode(&report), keyspace::REPL_SURVIVORS).await;
                    tracing::info!(robot = %id, survivor = %sid, "detected survivor");
                }
            }

            // Grid cell update where we are
            grid.mark(ground_truth, 200);

            // Publish pose heartbeat
            let hb = PoseHeartbeat {
                robot_id: id.clone(),
                class,
                pose: reported,
                battery,
                ts_ms: now_ms(),
                status,
            };
            {
                let mut n = node.lock().await;
                let _ = n
                    .query_network(AppData::GossipsubBroadcastMessage {
                        topic: TOPIC_POSE.to_string(),
                        message: encode(&hb),
                    })
                    .await;
            }

            // Every 2s, publish a grid chunk if we have cells
            if now_ms() % 2000 < 100 {
                let chunk = grid.snapshot();
                if !chunk.cells.is_empty() {
                    let mut n = node.lock().await;
                    let _ = n
                        .query_network(AppData::GossipsubBroadcastMessage {
                            topic: TOPIC_MAP_MERGE.to_string(),
                            message: encode(&chunk),
                        })
                        .await;
                }
            }

            // Detect rendezvous — trigger an extra full-grid broadcast.
            let peers = peer_poses.lock().await.clone();
            for (pid, peer_pose) in &peers {
                if is_rendezvous(ground_truth, *peer_pose, 5.0)
                    && link_state.profile_for(pid) != Profile::Blackout
                {
                    kin.mark_rendezvous();
                }
            }

            // Reactively steer toward nearest unassigned task geometry (if leader).
            if let Some(target) = pick_goal_for(&cbba, &id, ground_truth) {
                kin.steer_toward(target);
            }
        }
    })
}

fn pick_goal_for(cbba: &Cbba, self_id: &str, pose: Pose3) -> Option<Pose3> {
    // Prefer a task self currently leads, closest one first.
    let mut my: Vec<(String, f32)> = cbba
        .current_assignments()
        .into_iter()
        .filter(|(_, w, _)| w == self_id)
        .map(|(t, _, s)| (t, s))
        .collect();
    my.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    // We don't have direct access to TaskSpec here; use cbba inner via bundle.
    let bundle = cbba.own_bundle();
    if let Some((_tid, _score)) = bundle.first() {
        // Map task geometry via cbba — reuse known_tasks through a helper.
        // For simplicity we just wiggle around our current pose.
        return Some(Pose3 {
            x: (pose.x + 1.0) % 40.0,
            y: (pose.y + 1.0) % 25.0,
            z: pose.z,
        });
    }
    None
}

fn spawn_coord_loop(
    _id: String,
    cbba: Arc<Cbba>,
    stigmergy: Arc<Stigmergy>,
    wmsr: Arc<Wmsr>,
    node: Arc<Mutex<Core>>,
    flags: Arc<ControlFlags>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut round_tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            round_tick.tick().await;
            if flags.is_killed() {
                return;
            }
            cbba.tick_round();

            // Publish own bundle announcement
            let bundle = cbba.own_bundle();
            let ba = BundleAnnouncement {
                robot_id: cbba.self_id.clone(),
                bundle,
                ts_ms: now_ms(),
            };
            {
                let mut n = node.lock().await;
                let _ = n
                    .query_network(AppData::GossipsubBroadcastMessage {
                        topic: TOPIC_BUNDLE.to_string(),
                        message: encode(&ba),
                    })
                    .await;
            }

            // W-MSR step
            let cv = wmsr.step();
            {
                let mut n = node.lock().await;
                let _ = n
                    .query_network(AppData::GossipsubBroadcastMessage {
                        topic: TOPIC_CONSENSUS_VICTIM.to_string(),
                        message: encode(&cv),
                    })
                    .await;
            }

            // Publish stigmergy snapshot keys (every few seconds)
            if wmsr.round() % 5 == 0 {
                let snap = stigmergy.snapshot();
                for (k, (v, lamport, rid)) in snap.iter().take(5) {
                    let su = StigmergyUpdate {
                        key: k.clone(),
                        value: v.clone(),
                        lamport: *lamport,
                        robot_id: rid.clone(),
                    };
                    let mut n = node.lock().await;
                    let _ = n
                        .query_network(AppData::GossipsubBroadcastMessage {
                            topic: TOPIC_STIGMERGY.to_string(),
                            message: encode(&su),
                        })
                        .await;
                }
            }
        }
    })
}

#[allow(clippy::too_many_arguments)]
fn spawn_net_loop(
    self_id: String,
    env: Environment,
    cbba: Arc<Cbba>,
    stigmergy: Arc<Stigmergy>,
    wmsr: Arc<Wmsr>,
    grid: Arc<OccupancyGrid>,
    peer_poses: Arc<Mutex<HashMap<String, Pose3>>>,
    link_state: Arc<LinkState>,
    node: Arc<Mutex<Core>>,
    self_pose: Arc<Mutex<Pose3>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut rng = SmallRng::seed_from_u64(crate::keyspace::now_ms());
        loop {
            // Drain all pending events
            let events: Vec<NetworkEvent> = {
                let mut n = node.lock().await;
                let mut out = Vec::new();
                while let Some(e) = n.next_event().await {
                    out.push(e);
                }
                out
            };
            for ev in events {
                if let Err(e) = dispatch_event(
                    &self_id,
                    &env,
                    &cbba,
                    &stigmergy,
                    &wmsr,
                    &grid,
                    &peer_poses,
                    &link_state,
                    &node,
                    &self_pose,
                    ev,
                    &mut rng,
                )
                .await
                {
                    tracing::warn!(error = ?e, "event dispatch failed");
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
}

#[allow(clippy::too_many_arguments)]
async fn dispatch_event(
    self_id: &str,
    env: &Environment,
    cbba: &Arc<Cbba>,
    stigmergy: &Arc<Stigmergy>,
    wmsr: &Arc<Wmsr>,
    grid: &Arc<OccupancyGrid>,
    peer_poses: &Arc<Mutex<HashMap<String, Pose3>>>,
    link_state: &Arc<LinkState>,
    node: &Arc<Mutex<Core>>,
    self_pose: &Arc<Mutex<Pose3>>,
    ev: NetworkEvent,
    rng: &mut SmallRng,
) -> Result<()> {
    use swarm_nl::core::NetworkEvent::*;
    match ev {
        GossipsubIncomingMessageHandled { source, data } => {
            let peer_id = source.to_string();
            // Apply link-model filter
            let profile = link_state.profile_for(&peer_id);
            if link_model::should_drop(profile, &env.environment.link_profiles, rng) {
                tracing::trace!(peer=%peer_id, ?profile, "dropped gossip at app layer");
                return Ok(());
            }
            let latency = link_model::latency_ms(profile, &env.environment.link_profiles);
            if latency > 0 {
                tokio::time::sleep(Duration::from_millis(latency)).await;
            }
            // Try decoding against each schema in order.
            if let Some(hb) = decode_str::<PoseHeartbeat>(&data) {
                if hb.robot_id != self_id {
                    peer_poses.lock().await.insert(hb.robot_id.clone(), hb.pose);
                    let poses = peer_poses.lock().await.clone();
                    // Recompute link profiles from new pose table.
                    // Approximate self pose from grid snapshot or just use (0,0) initially.
                    let _ = &poses;
                    link_model::recompute(link_state, hb.pose, &poses, &env.environment);
                }
                return Ok(());
            }
            if let Some(ts) = decode_str::<crate::config::TaskSpec>(&data) {
                let pose = *self_pose.lock().await;
                if let Some(bid) = cbba.on_task_announce(ts, pose) {
                    let mut n = node.lock().await;
                    let _ = n
                        .query_network(AppData::GossipsubBroadcastMessage {
                            topic: TOPIC_BID.to_string(),
                            message: encode(&bid),
                        })
                        .await;
                }
                return Ok(());
            }
            if let Some(bid) = decode_str::<Bid>(&data) {
                if let Some(winner) = cbba.on_bid(bid) {
                    let mut n = node.lock().await;
                    let _ = n
                        .query_network(AppData::GossipsubBroadcastMessage {
                            topic: TOPIC_TASK_WINNER.to_string(),
                            message: encode(&winner),
                        })
                        .await;
                }
                return Ok(());
            }
            if let Some(_w) = decode_str::<TaskWinner>(&data) {
                // Informational; cbba already tracks leader.
                return Ok(());
            }
            if let Some(_ba) = decode_str::<BundleAnnouncement>(&data) {
                return Ok(());
            }
            if let Some(cv) = decode_str::<ConsensusValue>(&data) {
                // Heuristic byzantine detection: value > 1e3 * mean tolerance ⇒ blacklist.
                if cv.value.abs() > 100.0 {
                    cbba.blacklist(&cv.robot_id);
                    wmsr.blacklist(&cv.robot_id);
                } else {
                    wmsr.on_update(cv);
                }
                return Ok(());
            }
            if let Some(sr) = decode_str::<SurvivorReport>(&data) {
                // Add to stigmergy for visibility
                let up = stigmergy.set(&format!("survivor/{}", sr.survivor_id), &sr.detected_by);
                let mut n = node.lock().await;
                let _ = n
                    .query_network(AppData::GossipsubBroadcastMessage {
                        topic: TOPIC_STIGMERGY.to_string(),
                        message: encode(&up),
                    })
                    .await;
                return Ok(());
            }
            if let Some(chunk) = decode_str::<GridChunk>(&data) {
                grid.merge(&chunk);
                return Ok(());
            }
            if let Some(su) = decode_str::<StigmergyUpdate>(&data) {
                stigmergy.apply(su);
                return Ok(());
            }
            if let Some(lp) = decode_str::<LinkProfileOverride>(&data) {
                let profile = Profile::parse(&lp.profile);
                link_state.set_global_override(profile);
                if profile == Some(Profile::Default) {
                    link_state.set_global_override(None);
                }
                return Ok(());
            }
        }
        ReplicaDataIncoming { .. } => {}
        RpcIncomingMessageHandled { .. } => {
            // M3: all flows are gossip-based. RPC handler reserved for M4.
        }
        ConnectionEstablished { peer_id, .. } => {
            tracing::info!(peer = %peer_id, "peer connected");
        }
        ConnectionClosed { peer_id, .. } => {
            tracing::info!(peer = %peer_id, "peer disconnected");
        }
        _ => {}
    }
    Ok(())
}

// --- integration test surface ---------------------------------------------

/// Helper for integration tests: build a robot `Cbba` with a class's cap.
#[cfg(test)]
pub(crate) fn test_cbba(id: &str, class: RobotClass) -> Cbba {
    Cbba::new(id.to_string(), class.capability())
}

// Touch the re-imports so rustc keeps them alive under some clippy lints.
#[allow(dead_code)]
const _TOUCH: &[&str] = &[
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
];
