use forge_ui::{DialRequest, ForgeUI, MeshEvent};
use futures_util::StreamExt;
use tokio::sync::mpsc;
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

#[tokio::test]
async fn node_info_is_cached_from_node_started() {
    let ui = ForgeUI::new()
        .with_port(49011)
        .with_app_name("info-test")
        .start()
        .await
        .expect("start");

    // No NodeStarted yet → 503.
    let res = reqwest::get("http://127.0.0.1:49011/api/node/info")
        .await
        .expect("get");
    assert_eq!(res.status(), 503);

    ui.push(MeshEvent::NodeStarted {
        peer_id: "12D3KooWInfoTest".into(),
        listen_addrs: vec!["/ip4/127.0.0.1/tcp/49111".into()],
    })
    .await;

    // Cache-write is async — poll briefly.
    let body = loop_until(
        || async {
            let r = reqwest::get("http://127.0.0.1:49011/api/node/info")
                .await
                .ok()?;
            if r.status() == 200 {
                Some(r.json::<serde_json::Value>().await.ok()?)
            } else {
                None
            }
        },
        Duration::from_secs(2),
    )
    .await
    .expect("node_info should appear");

    assert_eq!(body["peer_id"], "12D3KooWInfoTest");
    assert_eq!(body["app_name"], "info-test");
    assert_eq!(body["http_port"], 49011);
    assert_eq!(body["listen_addrs"][0], "/ip4/127.0.0.1/tcp/49111");
}

#[tokio::test]
async fn dial_route_delivers_to_app_sender() {
    let (tx, mut rx) = mpsc::channel::<DialRequest>(4);
    let _ui = ForgeUI::new()
        .with_port(49012)
        .with_app_name("dial-test")
        .with_dial_sender(tx)
        .start()
        .await
        .expect("start");

    let client = reqwest::Client::new();
    let res = client
        .post("http://127.0.0.1:49012/api/peer/dial")
        .json(&serde_json::json!({
            "peer_id": "12D3KooWDialTarget",
            "addr": "/ip4/127.0.0.1/tcp/50000",
        }))
        .send()
        .await
        .expect("post");
    assert_eq!(res.status(), 202);

    let req = timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("timeout")
        .expect("sender closed");
    assert_eq!(req.peer_id, "12D3KooWDialTarget");
    assert_eq!(req.addr, "/ip4/127.0.0.1/tcp/50000");
}

#[tokio::test]
async fn localhost_discovery_finds_peer_on_adjacent_port() {
    // Two forge-ui instances on adjacent ports should discover each other
    // via the localhost port-scan within ~10 seconds.
    let handle_a = ForgeUI::new()
        .with_port(49022)
        .with_app_name("scan-A")
        .with_local_peer_id("12D3KooWScanA")
        .with_discovery_port_range(49022, 49023)
        .start()
        .await
        .expect("ui-a start");
    let handle_b = ForgeUI::new()
        .with_port(49023)
        .with_app_name("scan-B")
        .with_local_peer_id("12D3KooWScanB")
        .with_discovery_port_range(49022, 49023)
        .start()
        .await
        .expect("ui-b start");

    // Seed listen_addrs so each side can pick a usable multiaddr when scanning.
    handle_a
        .push(MeshEvent::NodeStarted {
            peer_id: "12D3KooWScanA".into(),
            listen_addrs: vec!["/ip4/127.0.0.1/tcp/59022".into()],
        })
        .await;
    handle_b
        .push(MeshEvent::NodeStarted {
            peer_id: "12D3KooWScanB".into(),
            listen_addrs: vec!["/ip4/127.0.0.1/tcp/59023".into()],
        })
        .await;

    let client = reqwest::Client::new();
    let found = loop_until(
        || async {
            let a = client
                .get("http://127.0.0.1:49022/api/peers/discovered")
                .send()
                .await
                .ok()?
                .json::<serde_json::Value>()
                .await
                .ok()?;
            let b = client
                .get("http://127.0.0.1:49023/api/peers/discovered")
                .send()
                .await
                .ok()?
                .json::<serde_json::Value>()
                .await
                .ok()?;
            let a_sees_b = a["peers"]
                .as_array()
                .map(|v| v.iter().any(|p| p["peer_id"] == "12D3KooWScanB"))
                .unwrap_or(false);
            let b_sees_a = b["peers"]
                .as_array()
                .map(|v| v.iter().any(|p| p["peer_id"] == "12D3KooWScanA"))
                .unwrap_or(false);
            (a_sees_b && b_sees_a).then_some(())
        },
        Duration::from_secs(15),
    )
    .await;
    assert!(
        found.is_some(),
        "both sides should discover each other within 15s"
    );
}

#[tokio::test]
async fn mdns_toggle_accepted() {
    // We can't easily round-trip mDNS over localhost inside a test
    // (multicast + daemon isn't deterministic enough), but we can verify
    // the toggle starts the backend without error when node_info is ready.
    let handle = ForgeUI::new()
        .with_port(49030)
        .with_app_name("mdns-toggle")
        .with_local_peer_id("12D3KooWMdnsToggle")
        .start()
        .await
        .expect("start");
    handle
        .push(MeshEvent::NodeStarted {
            peer_id: "12D3KooWMdnsToggle".into(),
            listen_addrs: vec!["/ip4/127.0.0.1/tcp/59030".into()],
        })
        .await;
    // Let the state mirror catch the event.
    tokio::time::sleep(Duration::from_millis(100)).await;

    let client = reqwest::Client::new();
    let res = client
        .post("http://127.0.0.1:49030/api/discovery/mdns")
        .json(&serde_json::json!({ "enabled": true }))
        .send()
        .await
        .expect("post enable");
    assert_eq!(res.status(), 202, "body: {:?}", res.text().await);

    let res = client
        .post("http://127.0.0.1:49030/api/discovery/mdns")
        .json(&serde_json::json!({ "enabled": false }))
        .send()
        .await
        .expect("post disable");
    assert_eq!(res.status(), 202);
}

#[tokio::test]
async fn root_serves_forge_ui_static_index() {
    let _ui = ForgeUI::new()
        .with_port(49040)
        .with_app_name("static-test")
        .start()
        .await
        .expect("start");

    let res = reqwest::get("http://127.0.0.1:49040/")
        .await
        .expect("get /");
    assert_eq!(res.status(), 200);
    let body = res.text().await.expect("body");
    // forge-ui's own index.html mentions the mesh visualizer canvas.
    assert!(
        body.contains("mesh") || body.to_lowercase().contains("forge"),
        "unexpected index.html: {body}"
    );
}

#[tokio::test]
async fn missing_app_static_dir_is_rejected_at_builder_time() {
    let result = ForgeUI::new()
        .with_port(49050)
        .with_app_name("bad-dir")
        .with_app_static_dir("/definitely/does/not/exist/forge-ui")
        .start()
        .await;
    let err = match result {
        Ok(_) => panic!("start should reject missing static dir"),
        Err(e) => e,
    };
    let msg = format!("{err:#}");
    assert!(
        msg.contains("does not exist") || msg.contains("directory"),
        "unexpected error: {msg}"
    );
}

#[tokio::test]
async fn ws_handler_survives_broadcast_lag() {
    // Push far more events than the broadcast buffer can hold while a WS
    // client is held open but not draining. The handler must log + continue
    // rather than crash, and a freshly connected client must still work.
    let ui = ForgeUI::new()
        .with_port(49060)
        .with_app_name("lag-test")
        .start()
        .await
        .expect("start");

    // First WS connection: connect, then sit idle so it lags.
    let (slow_ws, _) = timeout(
        Duration::from_secs(5),
        connect_async("ws://127.0.0.1:49060/ws"),
    )
    .await
    .expect("slow connect not timeout")
    .expect("slow connect ok");

    // Push >> EVENT_CHANNEL_CAPACITY (1024) events to force the slow client
    // past the broadcast buffer.
    for i in 0..2048 {
        ui.push(MeshEvent::MessageSent {
            to: format!("p{i}"),
            topic: "flood".into(),
            size_bytes: 1,
        })
        .await;
    }

    // A fresh client must still connect and receive a new event.
    let (mut fresh_ws, _) = timeout(
        Duration::from_secs(5),
        connect_async("ws://127.0.0.1:49060/ws"),
    )
    .await
    .expect("fresh connect not timeout")
    .expect("fresh connect ok");

    ui.push(MeshEvent::Custom {
        label: "after-lag".into(),
        detail: "hello".into(),
    })
    .await;

    let msg = timeout(Duration::from_secs(5), fresh_ws.next())
        .await
        .expect("fresh ws recv not timeout")
        .expect("fresh ws yielded None")
        .expect("fresh ws message ok");
    let text = msg.to_text().expect("text");
    // First frame may be a replayed NodeStarted; drain until we see our marker.
    let mut saw_marker = text.contains("after-lag");
    for _ in 0..4 {
        if saw_marker {
            break;
        }
        let msg = timeout(Duration::from_secs(2), fresh_ws.next())
            .await
            .expect("drain not timeout")
            .expect("drain None")
            .expect("drain ok");
        saw_marker = msg.to_text().map(|t| t.contains("after-lag")).unwrap_or(false);
    }
    assert!(saw_marker, "fresh client should receive events after lag");
    drop(slow_ws);
}

#[tokio::test]
async fn dial_route_returns_503_when_app_did_not_wire_sender() {
    let _ui = ForgeUI::new()
        .with_port(49013)
        .with_app_name("no-dial")
        .start()
        .await
        .expect("start");

    let client = reqwest::Client::new();
    let res = client
        .post("http://127.0.0.1:49013/api/peer/dial")
        .json(&serde_json::json!({
            "peer_id": "12D3KooWAny",
            "addr": "/ip4/127.0.0.1/tcp/50000",
        }))
        .send()
        .await
        .expect("post");
    assert_eq!(res.status(), 503);
}

async fn loop_until<F, Fut, T>(mut f: F, total: Duration) -> Option<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Option<T>>,
{
    let start = std::time::Instant::now();
    while start.elapsed() < total {
        if let Some(v) = f().await {
            return Some(v);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    None
}
