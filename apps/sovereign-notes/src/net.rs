use anyhow::Result;
use swarm_nl::core::replication::{ConsistencyModel, ReplNetworkConfig};
use swarm_nl::core::{Core, NetworkEvent};
use swarm_nl::setup::BootstrapConfig;
use swarm_nl::core::CoreBuilder;
use std::collections::HashMap;
use std::time::Duration;

pub const REPL_NETWORK: &str = "sovereign-notes-sync";

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
        .build()
        .await
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    Ok(node)
}

/// Drain setup events and print PeerId / listen addresses.
/// Returns the local PeerId as a string.
pub async fn drain_setup_events(node: &mut Core) -> String {
    let mut peer_id = String::new();
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
            }
            _ => break,
        }
    }
    peer_id
}

/// Join the replication network.
pub async fn join_repl_network(node: &mut Core) -> Result<()> {
    node.join_repl_network(REPL_NETWORK.to_string())
        .await
        .map_err(|e| anyhow::anyhow!("failed to join replication network: {e:?}"))?;
    println!("Joined replication network: {REPL_NETWORK}");
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
    node.replicate(payload, REPL_NETWORK)
        .await
        .map_err(|e| anyhow::anyhow!("replication failed: {e:?}"))?;
    Ok(())
}

/// Run the event loop, processing incoming replica data.
/// This is a blocking loop — call from a spawned task.
#[allow(dead_code)]
pub async fn run_event_loop(node: &mut Core) {
    loop {
        if let Some(NetworkEvent::ReplicaDataIncoming { data, source, .. }) =
            node.next_event().await
        {
            println!("Replica data from {source}: {data:?}");
        }
        // Consume any ready replication buffer data
        if let Some(repl_data) = node.consume_repl_data(REPL_NETWORK).await {
            println!(
                "Replicated data (clock={}): {:?}",
                repl_data.lamport_clock, repl_data.data
            );
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
