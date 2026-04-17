//! Robot entry point. Starts one SwarmNL node, runs the per-class behaviour
//! loop, gossips pose at 10 Hz, participates in CBBA + W-MSR + stigmergy,
//! and applies the link-model filter to incoming messages.
//!
//! M0 shape: bring up a node, drain `NewListenAddr`, print PeerId, optionally
//! exit (smoke mode). M2+ fills in kinematics, sensors, and coordination.

pub mod kinematics;
pub mod sensors;

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::sync::Mutex;

use crate::config::{Environment, Mission};
use crate::control::{self, ControlFlags};
use crate::keyspace::{self, RobotClass};
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
        robot = %args.id,
        class = %args.class.as_str(),
        tcp, udp,
        bootstrap = ?args.bootstrap,
        "starting robot node"
    );

    let mut node = swarm_node::build_node(tcp, udp, args.bootstrap.as_deref()).await?;
    let (peer_id, addrs) = swarm_node::drain_listen_addrs(&mut node).await;
    let loopback = loopback_multiaddr(tcp);
    println!("UNLEASH_ROBOT_READY id={} class={} peer_id={} tcp={} addr={}", args.id, args.class.as_str(), peer_id, tcp, loopback);
    for a in &addrs {
        tracing::debug!(addr=%a, "listen addr reported");
    }

    if args.smoke {
        tracing::info!("smoke mode: exiting after listen-addr drain");
        return Ok(());
    }

    // Start per-robot HTTP control port (Byzantine flip, killswitch)
    let flags = ControlFlags::new();
    let control_port = args.control_port.unwrap_or(60000 + args.node_index as u16);
    if let Err(e) = control::serve(control_port, flags.clone()).await {
        tracing::warn!(error = ?e, port = control_port, "control port failed to bind");
    }

    // Join gossip topics + replication network. Under library-feedback #7,
    // we tolerate replicate-on-no-peers failures as a no-op.
    join_core_topics(&mut node).await;
    let _ = node
        .join_repl_network(keyspace::REPL_SURVIVORS.to_string())
        .await;

    // Kick off the behaviour loop. M3+ will plug in the full coordination stack.
    let node = Arc::new(Mutex::new(node));
    run_behaviour_loop(args, node, flags).await;

    Ok(())
}

async fn join_core_topics(node: &mut swarm_nl::core::Core) {
    use swarm_nl::core::AppData;
    for topic in keyspace::default_topics() {
        let req = AppData::GossipsubJoinNetwork(topic.to_string());
        if let Err(e) = node.query_network(req).await {
            tracing::warn!(topic, error = ?e, "gossip join failed (will retry implicitly)");
        }
    }
}

async fn run_behaviour_loop(
    args: Args,
    node: Arc<Mutex<swarm_nl::core::Core>>,
    flags: Arc<ControlFlags>,
) {
    // Wait for the gossipsub mesh to warm up (library-feedback #5).
    tokio::time::sleep(Duration::from_secs(5)).await;

    // M0 minimal behaviour: publish one heartbeat every second, poll events,
    // respect the killswitch. Full behaviours are filled in by M2+.
    use crate::keyspace::{encode, now_ms, PoseHeartbeat, RobotStatus, TOPIC_POSE};
    use crate::robot::kinematics::Kinematics;
    use swarm_nl::core::AppData;

    let mut kin = Kinematics::spawn_for(args.class, args.node_index, &args.env);
    let mut heartbeat = tokio::time::interval(Duration::from_millis(100));
    let mut battery: f32 = 1.0;

    loop {
        if flags.is_killed() {
            tracing::info!("killswitch set, exiting");
            std::process::exit(0);
        }
        heartbeat.tick().await;
        kin.step(0.1);
        battery = (battery - 0.00005).max(0.0);
        let mut pose = kin.pose();
        let status = if flags.is_byzantine() {
            // Byzantine robot falsifies its pose by a constant offset.
            pose.x += 10.0;
            RobotStatus::Byzantine
        } else {
            RobotStatus::Nominal
        };

        let hb = PoseHeartbeat {
            robot_id: args.id.clone(),
            class: args.class,
            pose,
            battery,
            ts_ms: now_ms(),
            status,
        };
        {
            let mut node = node.lock().await;
            let _ = node
                .query_network(AppData::GossipsubBroadcastMessage {
                    topic: TOPIC_POSE.to_string(),
                    message: encode(&hb),
                })
                .await;
            // drain any pending events (non-blocking per library-feedback #4)
            while let Some(_ev) = node.next_event().await {
                // M3+ will dispatch these to the coordination layer
            }
        }
    }
}
