use forge_ui::{ForgeUI, MeshEvent};
use futures_util::StreamExt;
use tokio::time::{timeout, Duration};
use tokio_tungstenite::connect_async;

#[test]
fn mesh_event_serialization() {
    let event = MeshEvent::PeerConnected {
        peer_id: "12D3KooWTest".to_string(),
        addr: "/ip4/127.0.0.1/tcp/3000".to_string(),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"type\":\"PeerConnected\""));
    assert!(json.contains("12D3KooWTest"));

    let decoded: MeshEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, decoded);
}

#[test]
fn mesh_event_all_variants_serialize() {
    let events = vec![
        MeshEvent::NodeStarted {
            peer_id: "peer1".into(),
            listen_addrs: vec!["/ip4/127.0.0.1/tcp/3000".into()],
        },
        MeshEvent::PeerDisconnected {
            peer_id: "peer2".into(),
        },
        MeshEvent::MessageSent {
            to: "peer3".into(),
            topic: "chat".into(),
            size_bytes: 42,
        },
        MeshEvent::MessageReceived {
            from: "peer4".into(),
            topic: "chat".into(),
            size_bytes: 100,
        },
        MeshEvent::GossipJoined {
            topic: "news".into(),
        },
        MeshEvent::ReplicaSync {
            peer_id: "peer5".into(),
            network: "net1".into(),
            status: "synced".into(),
        },
        MeshEvent::Custom {
            label: "TEST".into(),
            detail: "hello".into(),
        },
    ];
    for event in events {
        let json = serde_json::to_string(&event).unwrap();
        let decoded: MeshEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, decoded);
    }
}

#[tokio::test]
async fn server_starts_and_accepts_ws() {
    let ui = ForgeUI::new()
        .with_port(49010)
        .with_app_name("Test App")
        .start()
        .await
        .expect("server should start");

    // Give server a moment to bind
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Connect via WebSocket
    let (mut ws, _) = timeout(
        Duration::from_secs(5),
        connect_async("ws://127.0.0.1:49010/ws"),
    )
    .await
    .expect("should not timeout")
    .expect("ws connect should succeed");

    // Push an event and verify we receive it
    ui.push(MeshEvent::PeerConnected {
        peer_id: "test-peer".into(),
        addr: "/ip4/127.0.0.1/tcp/5000".into(),
    })
    .await;

    let msg = timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("should not timeout")
        .expect("should get message")
        .expect("message should be ok");

    let text = msg.to_text().expect("should be text");
    let event: MeshEvent = serde_json::from_str(text).unwrap();
    assert!(matches!(event, MeshEvent::PeerConnected { .. }));
}
