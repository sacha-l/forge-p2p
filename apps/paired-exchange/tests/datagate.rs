//! Integration test for the data-plane gate.
//!
//! Two SwarmNL nodes boot in-process. The RPC handler is installed with
//! a known secret. We then exercise two scenarios sequentially — a
//! single `#[tokio::test]` because the handler context is a process-wide
//! `OnceLock`:
//!
//! 1. **Trusted peer → pings flow.** Mark the remote peer Trusted in
//!    node2's book, spawn a ping task, wait a few intervals, and assert
//!    that at least three `(seq, rtt)` entries landed in the RTT log.
//!
//! 2. **Untrusted peer → zero pings.** Clear trust, spawn a fresh
//!    ping task, wait a full 4 seconds (two ping intervals) and assert
//!    that the RTT log gained zero entries. The task exits cleanly on
//!    its first loop iteration rather than sitting idle.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use paired_exchange::config::SECRET_LEN;
use paired_exchange::datagate::{new_rtt_log, spawn_ping_task};
use paired_exchange::handshake::{install_handler_ctx, rpc_handler, HandlerCtx};
use paired_exchange::pairing::PairingBook;
use swarm_nl::core::{Core, CoreBuilder, NetworkEvent, RpcConfig};
use swarm_nl::setup::BootstrapConfig;

const TEST_INDEX: u16 = 1;
const SECRET: [u8; SECRET_LEN] = [0x33; SECRET_LEN];

fn node_port(node_index: u16) -> u16 {
    49000 + TEST_INDEX * 100 + node_index * 10
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn datagate_end_to_end() {
    tokio::time::timeout(Duration::from_secs(60), run()).await.unwrap();
}

async fn run() {
    let book = PairingBook::new();
    let ctx = Arc::new(HandlerCtx::new(SECRET, book.clone()));
    // Best-effort install — another test binary may have already claimed
    // the OnceLock, in which case we rely on the existing secret.
    let _ = install_handler_ctx(ctx);

    let mut node1 = boot_node(node_port(1), None).await;
    let node1_peer_id = harvest_peer_id(&mut node1).await;

    let mut bootnodes: HashMap<String, String> = HashMap::new();
    bootnodes.insert(
        node1_peer_id.to_string(),
        format!("/ip4/127.0.0.1/tcp/{}", node_port(1)),
    );
    let mut node2 = boot_node(node_port(2), Some(bootnodes)).await;

    // The handler ctx cache needs to know "the other peer" so gate #2
    // (receive-side) accepts DataPings. In the binary, main.rs sets this
    // from `ConnectionEstablished`; here we do it directly.
    if let Some(ctx) = paired_exchange::handshake::handler_ctx() {
        ctx.set_peer(node1_peer_id);
    }

    // Drain both sides' event queues until each sees the other. If only
    // node1's side is drained, node2's request_response behaviour can
    // remain in a "not yet upgraded" state long enough that the first
    // few pings time out silently.
    wait_for_both_sides_connected(&mut node1, &mut node2, Duration::from_secs(10)).await;
    // Give libp2p's request_response a beat to finish negotiating on
    // node2's side before the first SendRpc.
    tokio::time::sleep(Duration::from_millis(500)).await;

    // -----------------------------------------------------------------
    // 1. Trusted → pings flow.
    // -----------------------------------------------------------------
    book.mark_trusted(node1_peer_id);
    let rtt_log_trusted = new_rtt_log();
    let ui = start_test_ui().await;
    spawn_ping_task(
        node2.clone(),
        book.clone(),
        node1_peer_id,
        ui.clone(),
        rtt_log_trusted.clone(),
    );

    // Each RPC has a 3s floor inside `Core::recv_from_network` (see
    // library-feedback.md), so pings serialize at roughly that rate no
    // matter what `PING_INTERVAL` says. 18s therefore gives us a safe
    // ≥2-ping budget even on slow CI.
    tokio::time::sleep(Duration::from_secs(18)).await;
    let count = rtt_log_trusted
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .len();
    assert!(
        count >= 2,
        "expected ≥2 ping round-trips in 18s; got {count}"
    );

    // -----------------------------------------------------------------
    // 2. Untrusted → zero pings.
    // -----------------------------------------------------------------
    book.mark_failed(node1_peer_id, "revoked for test");
    let rtt_log_untrusted = new_rtt_log();
    spawn_ping_task(
        node2.clone(),
        book.clone(),
        node1_peer_id,
        ui,
        rtt_log_untrusted.clone(),
    );
    tokio::time::sleep(Duration::from_secs(4)).await;
    let count = rtt_log_untrusted
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .len();
    assert_eq!(count, 0, "untrusted peer should not record any RTTs");
}

async fn boot_node(tcp_port: u16, bootnodes: Option<HashMap<String, String>>) -> Core {
    let mut cfg = BootstrapConfig::new().with_tcp(tcp_port).with_udp(tcp_port + 1);
    if let Some(bn) = bootnodes {
        cfg = cfg.with_bootnodes(bn);
    }
    CoreBuilder::with_config(cfg)
        .with_rpc(RpcConfig::Default, rpc_handler)
        .build()
        .await
        .expect("build node")
}

async fn harvest_peer_id(node: &mut Core) -> swarm_nl::PeerId {
    for _ in 0..40 {
        if let Some(NetworkEvent::NewListenAddr { local_peer_id, .. }) = node.next_event().await {
            return local_peer_id;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("node never emitted NewListenAddr");
}

async fn wait_for_both_sides_connected(
    node1: &mut Core,
    node2: &mut Core,
    within: Duration,
) {
    let deadline = std::time::Instant::now() + within;
    let mut n1_seen = false;
    let mut n2_seen = false;
    while !(n1_seen && n2_seen) {
        while let Some(ev) = node1.next_event().await {
            if matches!(ev, NetworkEvent::ConnectionEstablished { .. }) {
                n1_seen = true;
            }
        }
        while let Some(ev) = node2.next_event().await {
            if matches!(ev, NetworkEvent::ConnectionEstablished { .. }) {
                n2_seen = true;
            }
        }
        if std::time::Instant::now() > deadline {
            panic!(
                "nodes did not both see ConnectionEstablished within {within:?} (n1={n1_seen}, n2={n2_seen})"
            );
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Start a real forge-ui server on a test-scoped port so the ping task
/// has a live `UiHandle` to push into. `UiHandle` has no public
/// constructor, so the bind-a-port route is the simplest path; the
/// server is torn down when the test ends.
async fn start_test_ui() -> forge_ui::UiHandle {
    use std::path::Path;
    let static_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("static");
    forge_ui::ForgeUI::new()
        .with_port(38080 + TEST_INDEX)
        .with_app_name("paired-exchange datagate test")
        .with_app_static_dir(static_dir.to_str().unwrap())
        .start()
        .await
        .expect("forge-ui test start")
}
