use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use swarm_nl::core::replication::{ConsistencyModel, ReplNetworkConfig};
use swarm_nl::core::{AppData, AppResponse, Core, CoreBuilder, NetworkEvent, RpcConfig};
use swarm_nl::setup::BootstrapConfig;

use swarm_nl::PeerId;

use crate::rpc;
use crate::store::Note;

pub const REPL_NETWORK: &str = "sovereign-notes-sync";
pub const GOSSIP_TOPIC: &str = "sovereign-notes-changes";

/// A gossip announcement for a note change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeAnnouncement {
    pub note_id: String,
    pub title: String,
    pub version: u64,
}

/// Tracks remote note versions learned from gossip announcements.
/// Maps note_id -> (title, version, source_peer_id).
pub type RemoteIndex = Arc<Mutex<HashMap<String, (String, u64, String)>>>;

/// Create a new empty remote index.
#[allow(dead_code)]
pub fn new_remote_index() -> RemoteIndex {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Build and configure a sovereign-notes node with replication.
pub async fn build_node(
    tcp_port: u16,
    udp_port: u16,
    boot_peer_id: Option<&str>,
    boot_addr: Option<&str>,
) -> Result<Core> {
    let mut config = BootstrapConfig::new()
        .with_tcp(tcp_port)
        .with_udp(udp_port);

    if let (Some(peer_id), Some(addr)) = (boot_peer_id, boot_addr) {
        let mut bootnodes = HashMap::new();
        bootnodes.insert(peer_id.to_string(), addr.to_string());
        config = config.with_bootnodes(bootnodes);
    }

    let repl_config = ReplNetworkConfig::Custom {
        queue_length: 150,
        expiry_time: Some(60),
        sync_wait_time: 5,
        consistency_model: ConsistencyModel::Eventual,
        data_aging_period: 2,
    };

    let node = CoreBuilder::with_config(config)
        .with_replication(repl_config)
        .with_rpc(RpcConfig::Default, rpc::handle_rpc)
        .build()
        .await
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    Ok(node)
}

/// Drain setup events and print PeerId / listen addresses.
/// Returns (peer_id, listen_addrs).
pub async fn drain_setup_events(node: &mut Core) -> (String, Vec<String>) {
    let mut peer_id = String::new();
    let mut addrs = Vec::new();
    while let Some(event) = node.next_event().await {
        match event {
            NetworkEvent::NewListenAddr {
                local_peer_id,
                address,
                ..
            } => {
                peer_id = local_peer_id.to_string();
                println!("PeerId: {local_peer_id}");
                println!("Listening on: {address}");
                addrs.push(address.to_string());
            }
            _ => break,
        }
    }
    (peer_id, addrs)
}

/// Join the replication network.
pub async fn join_repl_network(node: &mut Core) -> Result<()> {
    node.join_repl_network(REPL_NETWORK.to_string())
        .await
        .map_err(|e| anyhow::anyhow!("failed to join replication network: {e:?}"))?;
    println!("Joined replication network: {REPL_NETWORK}");
    Ok(())
}

/// Join the gossip topic for change announcements.
pub async fn join_gossip(node: &mut Core) -> Result<()> {
    let join_request = AppData::GossipsubJoinNetwork(GOSSIP_TOPIC.to_string());
    match node.query_network(join_request).await {
        Ok(AppResponse::GossipsubJoinSuccess) => {
            println!("Joined gossip topic: {GOSSIP_TOPIC}");
            Ok(())
        }
        Ok(other) => anyhow::bail!("unexpected response joining gossip: {other:?}"),
        Err(e) => anyhow::bail!("failed to join gossip topic: {e:?}"),
    }
}

/// Broadcast a note change announcement via gossip.
pub async fn announce_change(
    node: &mut Core,
    note_id: &str,
    title: &str,
    version: u64,
) -> Result<()> {
    let announcement = ChangeAnnouncement {
        note_id: note_id.to_string(),
        title: title.to_string(),
        version,
    };
    let json = serde_json::to_string(&announcement)?;
    let broadcast = AppData::GossipsubBroadcastMessage {
        topic: GOSSIP_TOPIC.to_string(),
        message: vec![json.as_bytes().to_vec()],
    };
    // Broadcast may fail if no peers are in the mesh yet — that's ok for single-node usage
    match node.query_network(broadcast).await {
        Ok(AppResponse::GossipsubBroadcastSuccess) => {
            println!("Announced change: {} v{}", title, version);
        }
        _ => {
            println!("No peers to announce to (will sync via replication)");
        }
    }
    Ok(())
}

/// Replicate note metadata to the network.
/// Payload format: [note_id, title, version_str, updated_at_str]
pub async fn replicate_note_meta(
    node: &mut Core,
    note_id: &str,
    title: &str,
    version: u64,
    updated_at: &str,
) -> Result<()> {
    let payload = vec![
        note_id.as_bytes().to_vec(),
        title.as_bytes().to_vec(),
        version.to_string().as_bytes().to_vec(),
        updated_at.as_bytes().to_vec(),
    ];
    match node.replicate(payload, REPL_NETWORK).await {
        Ok(()) => {}
        Err(_) => {
            // Replication fails when no peers are connected — that's fine for single-node usage.
            // Data is saved locally and will replicate when peers connect.
            println!("No peers to replicate to (will sync when peers connect)");
        }
    }
    Ok(())
}

/// Process a single gossip event, updating the remote index.
pub fn handle_gossip_message(data: &[String], source: &str, remote_index: &RemoteIndex) {
    for msg in data {
        match serde_json::from_str::<ChangeAnnouncement>(msg) {
            Ok(ann) => {
                println!(
                    "Change from {source}: '{}' v{}",
                    ann.title, ann.version
                );
                let mut index = remote_index.lock().expect("remote index lock poisoned");
                let entry = index
                    .entry(ann.note_id.clone())
                    .or_insert((String::new(), 0, String::new()));
                if ann.version > entry.1 {
                    *entry = (ann.title, ann.version, source.to_string());
                }
            }
            Err(e) => {
                println!("Failed to parse gossip message: {e}");
            }
        }
    }
}

/// Fetch a note from a remote peer via RPC.
#[allow(dead_code)]
pub async fn fetch_note_via_rpc(
    node: &mut Core,
    peer_id: &PeerId,
    note_id: &str,
) -> Result<Note> {
    let request = AppData::SendRpc {
        keys: rpc::make_fetch_request(note_id),
        peer: *peer_id,
    };
    match node.query_network(request).await {
        Ok(AppResponse::SendRpc(response)) => {
            if response.len() < 2 {
                anyhow::bail!("invalid RPC response: too short");
            }
            let status = String::from_utf8_lossy(&response[0]);
            if status == "OK" {
                let note: Note = serde_json::from_slice(&response[1])?;
                Ok(note)
            } else {
                let msg = String::from_utf8_lossy(&response[1]);
                anyhow::bail!("RPC error: {msg}");
            }
        }
        Ok(other) => anyhow::bail!("unexpected RPC response: {other:?}"),
        Err(e) => anyhow::bail!("RPC request failed: {e:?}"),
    }
}

/// Run the event loop, processing gossip and replica events.
/// This is a blocking loop — call from a spawned task.
#[allow(dead_code)]
pub async fn run_event_loop(node: &mut Core, remote_index: &RemoteIndex) {
    loop {
        if let Some(event) = node.next_event().await {
            match event {
                NetworkEvent::GossipsubIncomingMessageHandled { source, data } => {
                    handle_gossip_message(&data, &source.to_string(), remote_index);
                }
                NetworkEvent::ReplicaDataIncoming { source, .. } => {
                    println!("Replica data from {source}");
                }
                _ => {}
            }
        }
        if let Some(repl_data) = node.consume_repl_data(REPL_NETWORK).await {
            println!(
                "Replicated data (clock={}): {:?}",
                repl_data.lamport_clock, repl_data.data
            );
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
