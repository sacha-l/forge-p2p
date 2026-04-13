//! Two-node gossip round-trip for mesh-chat.
//!
//! Spawns two SwarmNL nodes in-process on test ports, joins the shared chat
//! topic, and asserts node 2 receives a JSON `ChatLine` broadcast from node 1.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use swarm_nl::core::{AppData, Core, CoreBuilder, NetworkEvent};
use swarm_nl::setup::BootstrapConfig;
use tokio::time::timeout;

const TOPIC: &str = "chat";
const NODE1_TCP: u16 = 49000;
const NODE1_UDP: u16 = 49001;
const NODE2_TCP: u16 = 49100;
const NODE2_UDP: u16 = 49101;
const MESH_WARMUP: Duration = Duration::from_secs(12);
const OVERALL_TIMEOUT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct ChatLine {
    from: String,
    text: String,
}

async fn boot(config: BootstrapConfig) -> Result<(Core, String)> {
    let mut node = CoreBuilder::with_config(config)
        .build()
        .await
        .map_err(|e| anyhow!("failed to build node: {e:?}"))?;

    // Drain setup events for ~1s to collect the local peer id.
    let mut peer_id = None;
    for _ in 0..20 {
        if let Some(NetworkEvent::NewListenAddr { local_peer_id, .. }) = node.next_event().await {
            peer_id.get_or_insert_with(|| local_peer_id.to_string());
        } else {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    let peer_id = peer_id.ok_or_else(|| anyhow!("never saw NewListenAddr"))?;
    Ok((node, peer_id))
}

#[tokio::test]
async fn two_node_gossip_roundtrip() -> Result<()> {
    timeout(OVERALL_TIMEOUT, async {
        // Node 1: no bootnodes.
        let cfg1 = BootstrapConfig::new()
            .with_tcp(NODE1_TCP)
            .with_udp(NODE1_UDP);
        let (mut node1, peer1) = boot(cfg1).await?;

        // Node 2: dial node 1 on loopback.
        let mut bootnodes: HashMap<String, String> = HashMap::new();
        bootnodes.insert(
            peer1.clone(),
            format!("/ip4/127.0.0.1/tcp/{NODE1_TCP}"),
        );
        let cfg2 = BootstrapConfig::new()
            .with_tcp(NODE2_TCP)
            .with_udp(NODE2_UDP)
            .with_bootnodes(bootnodes);
        let (mut node2, _peer2) = boot(cfg2).await?;

        // Both nodes join the chat topic.
        node1
            .query_network(AppData::GossipsubJoinNetwork(TOPIC.into()))
            .await
            .map_err(|e| anyhow!("node1 join failed: {e:?}"))?;
        node2
            .query_network(AppData::GossipsubJoinNetwork(TOPIC.into()))
            .await
            .map_err(|e| anyhow!("node2 join failed: {e:?}"))?;

        // Mesh needs time to form before broadcasts propagate.
        // Drain events on both nodes during the warmup to avoid buffer overruns.
        let warmup_end = tokio::time::Instant::now() + MESH_WARMUP;
        while tokio::time::Instant::now() < warmup_end {
            while node1.next_event().await.is_some() {}
            while node2.next_event().await.is_some() {}
            tokio::time::sleep(POLL_INTERVAL).await;
        }

        // Node 1 broadcasts a ChatLine JSON payload.
        let line = ChatLine {
            from: "Al".into(),
            text: "integration-hello".into(),
        };
        let payload = serde_json::to_vec(&line)?;
        node1
            .query_network(AppData::GossipsubBroadcastMessage {
                topic: TOPIC.into(),
                message: vec![payload],
            })
            .await
            .map_err(|e| anyhow!("node1 broadcast failed: {e:?}"))?;

        // Wait for node 2 to observe the message.
        loop {
            if let Some(NetworkEvent::GossipsubIncomingMessageHandled { data, .. }) =
                node2.next_event().await
            {
                let raw = data.into_iter().next().ok_or_else(|| anyhow!("empty data"))?;
                let got: ChatLine = serde_json::from_str(&raw)
                    .map_err(|e| anyhow!("received non-JSON payload {raw:?}: {e}"))?;
                assert_eq!(got, line);
                return Ok::<(), anyhow::Error>(());
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    })
    .await
    .map_err(|_| anyhow!("integration test timed out after {OVERALL_TIMEOUT:?}"))?
}
