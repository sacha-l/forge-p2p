use std::collections::HashMap;
use std::time::Duration;

use swarm_nl::core::replication::{ConsistencyModel, ReplNetworkConfig};
use swarm_nl::core::{AppData, AppResponse, CoreBuilder, NetworkEvent, RpcConfig};
use swarm_nl::setup::BootstrapConfig;

mod common {
    use std::path::Path;

    // Re-use the app's store and rpc modules
    // We inline the minimal logic here since we can't import from the binary crate

    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Note {
        pub id: String,
        pub title: String,
        pub content: String,
        pub version: u64,
        pub created_at: String,
        pub updated_at: String,
    }

    /// Create a note JSON file in the given directory.
    pub fn create_note(data_dir: &Path, title: &str, content: &str) -> Note {
        let notes_dir = data_dir.join("notes");
        std::fs::create_dir_all(&notes_dir).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let note = Note {
            id: id.clone(),
            title: title.to_string(),
            content: content.to_string(),
            version: 1,
            created_at: now.clone(),
            updated_at: now,
        };
        let path = notes_dir.join(format!("{id}.json"));
        std::fs::write(&path, serde_json::to_string_pretty(&note).unwrap()).unwrap();
        note
    }

    /// Read a note from disk.
    pub fn read_note(data_dir: &Path, note_id: &str) -> Option<Note> {
        let path = data_dir.join("notes").join(format!("{note_id}.json"));
        let data = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&data).ok()
    }

    /// Minimal RPC handler that serves notes from a directory.
    /// Uses a static OnceLock for the data dir path (same pattern as the app).
    static TEST_DATA_DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

    pub fn init_test_data_dir(path: std::path::PathBuf) {
        let _ = TEST_DATA_DIR.set(path);
    }

    pub fn test_rpc_handler(request: Vec<Vec<u8>>) -> Vec<Vec<u8>> {
        let Some(first) = request.first() else {
            return vec![b"ERR".to_vec(), b"empty".to_vec()];
        };
        let prefix = b"FETCH:";
        if let Some(note_id_bytes) = first.strip_prefix(prefix) {
            let note_id = String::from_utf8_lossy(note_id_bytes);
            let Some(data_dir) = TEST_DATA_DIR.get() else {
                return vec![b"ERR".to_vec(), b"no data dir".to_vec()];
            };
            match read_note(data_dir, &note_id) {
                Some(note) => {
                    let json = serde_json::to_string(&note).unwrap();
                    vec![b"OK".to_vec(), json.into_bytes()]
                }
                None => vec![b"ERR".to_vec(), b"not found".to_vec()],
            }
        } else {
            vec![b"ERR".to_vec(), b"unknown".to_vec()]
        }
    }
}

#[tokio::test]
async fn two_nodes_sync_note_via_rpc() {
    let result = tokio::time::timeout(Duration::from_secs(60), async {
        let dir1 = tempfile::TempDir::new().unwrap();
        let dir2 = tempfile::TempDir::new().unwrap();

        // Initialize the test RPC handler with node1's data dir
        common::init_test_data_dir(dir1.path().to_path_buf());

        let repl_config = ReplNetworkConfig::Custom {
            queue_length: 150,
            expiry_time: Some(60),
            sync_wait_time: 5,
            consistency_model: ConsistencyModel::Eventual,
            data_aging_period: 2,
        };

        // --- Node 1 ---
        let config1 = BootstrapConfig::new().with_tcp(49200).with_udp(49201);
        let mut node1 = CoreBuilder::with_config(config1)
            .with_replication(repl_config.clone())
            .with_rpc(RpcConfig::Default, common::test_rpc_handler)
            .build()
            .await
            .expect("node1 build failed");

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

        // --- Node 2 ---
        let mut bootnodes = HashMap::new();
        bootnodes.insert(
            node1_peer_id.clone(),
            format!("/ip4/127.0.0.1/tcp/{}", 49200),
        );
        let config2 = BootstrapConfig::new()
            .with_tcp(49300)
            .with_udp(49301)
            .with_bootnodes(bootnodes);
        let mut node2 = CoreBuilder::with_config(config2)
            .with_replication(repl_config)
            .with_rpc(RpcConfig::Default, common::test_rpc_handler)
            .build()
            .await
            .expect("node2 build failed");

        // Drain node2 setup events
        while let Some(event) = node2.next_event().await {
            if !matches!(event, NetworkEvent::NewListenAddr { .. }) {
                break;
            }
        }

        // Both join gossip topic
        let topic = "sovereign-notes-changes";
        let j1 = node1
            .query_network(AppData::GossipsubJoinNetwork(topic.to_string()))
            .await;
        assert!(matches!(j1, Ok(AppResponse::GossipsubJoinSuccess)));

        let j2 = node2
            .query_network(AppData::GossipsubJoinNetwork(topic.to_string()))
            .await;
        assert!(matches!(j2, Ok(AppResponse::GossipsubJoinSuccess)));

        // Wait for mesh to form
        tokio::time::sleep(Duration::from_secs(5)).await;

        // Node1 creates a note
        let note = common::create_note(dir1.path(), "Test Note", "Hello from node1");
        println!("Created note: {} '{}'", note.id, note.title);

        // Node1 broadcasts a change announcement
        let announcement = serde_json::json!({
            "note_id": note.id,
            "title": note.title,
            "version": note.version,
        });
        let broadcast = AppData::GossipsubBroadcastMessage {
            topic: topic.to_string(),
            message: vec![announcement.to_string().as_bytes().to_vec()],
        };
        let br = node1.query_network(broadcast).await;
        assert!(
            matches!(br, Ok(AppResponse::GossipsubBroadcastSuccess)),
            "broadcast failed: {br:?}"
        );

        // Node2 receives the announcement
        let mut received_note_id = String::new();
        for _ in 0..100 {
            if let Some(NetworkEvent::GossipsubIncomingMessageHandled { data, .. }) =
                node2.next_event().await
            {
                for msg in &data {
                    if let Ok(ann) =
                        serde_json::from_str::<serde_json::Value>(msg)
                    {
                        if let Some(id) = ann.get("note_id").and_then(|v| v.as_str()) {
                            received_note_id = id.to_string();
                            println!("Node2 received announcement for note: {id}");
                        }
                    }
                }
                if !received_note_id.is_empty() {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(
            !received_note_id.is_empty(),
            "node2 should have received the announcement"
        );

        // Node2 fetches the note via RPC from node1
        let peer_id: swarm_nl::PeerId = node1_peer_id.parse().expect("parse peer id");
        let mut fetch_key = b"FETCH:".to_vec();
        fetch_key.extend_from_slice(received_note_id.as_bytes());
        let rpc_request = AppData::SendRpc {
            keys: vec![fetch_key],
            peer: peer_id,
        };
        let rpc_response = node2.query_network(rpc_request).await;
        match rpc_response {
            Ok(AppResponse::SendRpc(response)) => {
                assert!(response.len() >= 2, "response too short");
                assert_eq!(response[0], b"OK", "expected OK status");
                let fetched: common::Note =
                    serde_json::from_slice(&response[1]).expect("parse note");
                assert_eq!(fetched.id, note.id);
                assert_eq!(fetched.title, "Test Note");
                assert_eq!(fetched.content, "Hello from node1");
                println!(
                    "Node2 fetched note: {} '{}' v{}",
                    fetched.id, fetched.title, fetched.version
                );

                // Save to node2's data directory
                let notes_dir = dir2.path().join("notes");
                std::fs::create_dir_all(&notes_dir).unwrap();
                let path = notes_dir.join(format!("{}.json", fetched.id));
                std::fs::write(&path, serde_json::to_string_pretty(&fetched).unwrap()).unwrap();
            }
            other => panic!("RPC fetch failed: {other:?}"),
        }

        // Verify the note exists in node2's directory
        let synced = common::read_note(dir2.path(), &note.id);
        assert!(synced.is_some(), "note should exist in node2's data dir");
        let synced = synced.unwrap();
        assert_eq!(synced.title, "Test Note");
        assert_eq!(synced.content, "Hello from node1");
        println!("Sync verified: note exists on both nodes");
    })
    .await;

    assert!(result.is_ok(), "test timed out after 60 seconds");
}
