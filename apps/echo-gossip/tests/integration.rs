use std::collections::HashMap;
use std::time::Duration;

use swarm_nl::core::{AppData, AppResponse, CoreBuilder, NetworkEvent};
use swarm_nl::setup::BootstrapConfig;

/// Two nodes join the same gossip topic. Node1 broadcasts a message.
/// Node2 receives it and echoes it back. Node1 receives the echo.
#[tokio::test]
async fn two_nodes_echo_gossip() {
    const TOPIC: &str = "echo-network";
    const ECHO_PREFIX: &str = "echo: ";

    let result = tokio::time::timeout(Duration::from_secs(30), async {
        // --- Node 1 setup ---
        let config1 = BootstrapConfig::new().with_tcp(49000).with_udp(49001);
        let mut node1 = CoreBuilder::with_config(config1)
            .build()
            .await
            .expect("node1 build failed");

        // Get node1's PeerId from events
        let mut node1_peer_id = String::new();
        while let Some(event) = node1.next_event().await {
            if let NetworkEvent::NewListenAddr {
                local_peer_id, ..
            } = event
            {
                node1_peer_id = local_peer_id.to_string();
            } else {
                break;
            }
        }
        assert!(!node1_peer_id.is_empty(), "node1 should have a PeerId");

        // --- Node 2 setup (connects to node1 via loopback) ---
        let mut bootnodes = HashMap::new();
        bootnodes.insert(
            node1_peer_id.clone(),
            format!("/ip4/127.0.0.1/tcp/{}", 49000),
        );
        let config2 = BootstrapConfig::new()
            .with_tcp(49100)
            .with_udp(49101)
            .with_bootnodes(bootnodes);
        let mut node2 = CoreBuilder::with_config(config2)
            .build()
            .await
            .expect("node2 build failed");

        // Drain node2 setup events
        while node2.next_event().await.is_some() {
            // Wait a bit then check if there are more events
            tokio::time::sleep(Duration::from_millis(100)).await;
            if node2.next_event().await.is_none() {
                break;
            }
        }

        // Both join the gossip topic
        let join1 = node1
            .query_network(AppData::GossipsubJoinNetwork(TOPIC.to_string()))
            .await;
        assert!(
            matches!(join1, Ok(AppResponse::GossipsubJoinSuccess)),
            "node1 join failed: {join1:?}"
        );

        let join2 = node2
            .query_network(AppData::GossipsubJoinNetwork(TOPIC.to_string()))
            .await;
        assert!(
            matches!(join2, Ok(AppResponse::GossipsubJoinSuccess)),
            "node2 join failed: {join2:?}"
        );

        // Wait for gossipsub mesh heartbeat to propagate
        tokio::time::sleep(Duration::from_secs(5)).await;

        // Node1 broadcasts a test message
        let test_message = "ping from node1";
        let broadcast = AppData::GossipsubBroadcastMessage {
            topic: TOPIC.to_string(),
            message: vec![test_message.as_bytes().to_vec()],
        };
        let broadcast_result = node1.query_network(broadcast).await;
        assert!(
            matches!(broadcast_result, Ok(AppResponse::GossipsubBroadcastSuccess)),
            "broadcast failed: {broadcast_result:?}"
        );

        // Node2 polls for the incoming message and echoes it back
        let mut node2_received = false;
        for _ in 0..100 {
            if let Some(NetworkEvent::GossipsubIncomingMessageHandled { data, .. }) =
                node2.next_event().await
            {
                for msg in &data {
                    if msg == test_message {
                        node2_received = true;
                        let echo = format!("{ECHO_PREFIX}{msg}");
                        let echo_broadcast = AppData::GossipsubBroadcastMessage {
                            topic: TOPIC.to_string(),
                            message: vec![echo.as_bytes().to_vec()],
                        };
                        let _ = node2.query_network(echo_broadcast).await;
                    }
                }
                if node2_received {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(node2_received, "node2 should have received the message");

        // Node1 polls for the echo
        let expected_echo = format!("{ECHO_PREFIX}{test_message}");
        let mut node1_got_echo = false;
        for _ in 0..100 {
            if let Some(NetworkEvent::GossipsubIncomingMessageHandled { data, .. }) =
                node1.next_event().await
            {
                for msg in &data {
                    if msg == &expected_echo {
                        node1_got_echo = true;
                    }
                }
                if node1_got_echo {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(node1_got_echo, "node1 should have received the echo");
    })
    .await;

    assert!(result.is_ok(), "test timed out after 30 seconds");
}
