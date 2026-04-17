//! Observer: a SwarmNL peer (no robot behaviour) that subscribes to every
//! Unleash gossip topic, aggregates state, and hosts the forge-ui dashboard.
//!
//! The observer produces no ground truth — dashboard state is strictly what
//! was heard on the mesh.
//!
//! M0 stub: binds the HTTP port, subscribes to gossip, and emits NodeStarted
//! so forge-ui has a local peer ID to display. M5 fills in the 5-panel
//! aggregation + replication-lag tracking.

pub mod panels;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::sync::Mutex;

use crate::config::{Environment, Mission};
use crate::keyspace;
use crate::swarm_node;

pub struct Args {
    pub mission: Mission,
    pub env: Environment,
    pub ui_port: u16,
    pub tcp_port: u16,
    pub bootstrap: Option<String>,
}

pub async fn run(args: Args) -> Result<()> {
    let Args {
        mission,
        env,
        ui_port,
        tcp_port,
        bootstrap,
    } = args;

    tracing::info!(tcp_port, ui_port, "starting observer");

    let udp_port = tcp_port + 1;
    let mut node = swarm_node::build_node(tcp_port, udp_port, bootstrap.as_deref()).await?;
    let (peer_id, addrs) = swarm_node::drain_listen_addrs(&mut node).await;
    println!(
        "UNLEASH_OBSERVER_READY peer_id={} tcp={} addr={}",
        peer_id,
        tcp_port,
        swarm_node::loopback_multiaddr(tcp_port)
    );

    // Subscribe to every Unleash topic (fire-and-forget; see robot/mod.rs
    // for the backpressure-avoidance rationale).
    for topic in keyspace::default_topics() {
        let req = swarm_nl::core::AppData::GossipsubJoinNetwork(topic.to_string());
        let _ = node.send_to_network(req).await;
    }
    let _ = tokio::time::timeout(
        Duration::from_secs(2),
        node.join_repl_network(keyspace::REPL_SURVIVORS.to_string()),
    )
    .await;

    // Start forge-ui.
    let static_dir = resolve_static_dir()?;
    tracing::info!(static_dir = ?static_dir, "mounting forge-ui");
    let ui = forge_ui::ForgeUI::new()
        .with_port(ui_port)
        .with_app_name("Unleash")
        .with_app_static_dir(static_dir.to_str().expect("utf8 static dir"))
        .with_local_peer_id(&peer_id)
        .start()
        .await?;
    ui.push(forge_ui::MeshEvent::NodeStarted {
        peer_id: peer_id.clone(),
        listen_addrs: addrs.clone(),
    })
    .await;

    let aggregator = panels::Aggregator::new(mission.clone(), env.clone(), ui.clone());
    let node = Arc::new(Mutex::new(node));

    // Keep the process alive + continuously drain events.
    // Per library-feedback #4, use a 100ms sleep between polls.
    loop {
        {
            let mut node = node.lock().await;
            while let Some(event) = node.next_event().await {
                aggregator.apply_event(event).await;
            }
        }
        aggregator.tick().await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn resolve_static_dir() -> Result<PathBuf> {
    // First try alongside the binary (cargo run from project root).
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let candidate = crate_dir.join("static");
    if candidate.is_dir() {
        return Ok(candidate);
    }
    // Fallback to `./static` relative to CWD.
    let cwd = std::env::current_dir()?.join("static");
    if cwd.is_dir() {
        return Ok(cwd);
    }
    anyhow::bail!(
        "could not locate `static/` directory — tried {:?} and {:?}",
        candidate,
        cwd
    );
}
