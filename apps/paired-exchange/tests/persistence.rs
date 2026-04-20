//! Integration test for the persistence skip-path.
//!
//! The binary's flow is:
//!
//! ```text
//!   load_trusted_peers(file)  →  book.mark_trusted(...)
//!   on ConnectionEstablished:
//!       if book.is_trusted(peer) { skip handshake }
//!       else                      { initiate_handshake; persistence::save_trusted_peer }
//! ```
//!
//! The exit criterion is that a restart with a populated cache produces
//! **zero** `Challenge` sends. We prove that by counting
//! `CHALLENGE_SENT` across two phases of a single process:
//!
//! 1. **Cold start**: empty cache, do one live handshake, persist.
//! 2. **Warm start**: simulate a restart by constructing a fresh book,
//!    loading from the file on disk, then running the same guarded
//!    handle-event branch — asserting the challenge counter is
//!    unchanged.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use paired_exchange::config::SECRET_LEN;
use paired_exchange::handshake::{
    initiate_handshake, install_handler_ctx, rpc_handler, HandlerCtx, CHALLENGE_SENT,
};
use paired_exchange::pairing::PairingBook;
use paired_exchange::persistence;
use swarm_nl::core::{Core, CoreBuilder, NetworkEvent, RpcConfig};
use swarm_nl::setup::BootstrapConfig;

const TEST_INDEX: u16 = 2;
const SECRET: [u8; SECRET_LEN] = [0x55; SECRET_LEN];

fn node_port(node_index: u16) -> u16 {
    49000 + TEST_INDEX * 100 + node_index * 10
}

fn tmp_persist_path(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("paired-exchange-persist-{}", name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("trusted.json")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn persistence_skip_path() {
    tokio::time::timeout(Duration::from_secs(45), run()).await.unwrap();
}

async fn run() {
    let book = PairingBook::new();
    let ctx = Arc::new(HandlerCtx::new(SECRET, book.clone()));
    let _ = install_handler_ctx(ctx);

    let persist_path = tmp_persist_path("skip-path");
    assert!(
        persistence::load_trusted_peers(&persist_path).is_empty(),
        "cache should start empty"
    );

    let mut node1 = boot_node(node_port(1), None).await;
    let node1_peer_id = harvest_peer_id(&mut node1).await;

    let mut bootnodes: HashMap<String, String> = HashMap::new();
    bootnodes.insert(
        node1_peer_id.to_string(),
        format!("/ip4/127.0.0.1/tcp/{}", node_port(1)),
    );
    let mut node2 = boot_node(node_port(2), Some(bootnodes)).await;
    if let Some(ctx) = paired_exchange::handshake::handler_ctx() {
        ctx.set_peer(node1_peer_id);
    }
    wait_for_both(&mut node1, &mut node2, Duration::from_secs(10)).await;
    tokio::time::sleep(Duration::from_millis(500)).await;

    // --- Phase 1: cold start, handshake runs, peer gets persisted. ---
    let challenges_before = CHALLENGE_SENT.load(Ordering::Relaxed);
    initiate_handshake(&mut node2, &book, &SECRET, node1_peer_id).await;
    assert!(
        book.is_trusted(&node1_peer_id),
        "cold-start handshake should produce Trusted"
    );
    let challenges_cold = CHALLENGE_SENT.load(Ordering::Relaxed);
    assert!(
        challenges_cold > challenges_before,
        "cold start should have sent at least one Challenge"
    );
    persistence::save_trusted_peer(&persist_path, &node1_peer_id);
    let loaded = persistence::load_trusted_peers(&persist_path);
    assert_eq!(loaded, vec![node1_peer_id]);

    // --- Phase 2: simulated restart — fresh book populated from disk. ---
    let book2 = PairingBook::new();
    for p in persistence::load_trusted_peers(&persist_path) {
        book2.mark_trusted(p);
    }
    assert!(book2.is_trusted(&node1_peer_id));

    // This is the exact guard main.rs uses on ConnectionEstablished.
    let challenges_before_warm = CHALLENGE_SENT.load(Ordering::Relaxed);
    if !book2.is_trusted(&node1_peer_id) {
        initiate_handshake(&mut node2, &book2, &SECRET, node1_peer_id).await;
    }
    let challenges_after_warm = CHALLENGE_SENT.load(Ordering::Relaxed);
    assert_eq!(
        challenges_after_warm, challenges_before_warm,
        "warm start must send zero Challenges (skip-path)"
    );

    // --- Phase 3: corrupt file → treated as empty, no panic. ---
    let corrupt_path = tmp_persist_path("corrupt");
    std::fs::write(&corrupt_path, b"not json at all").unwrap();
    assert!(persistence::load_trusted_peers(&corrupt_path).is_empty());
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

async fn wait_for_both(node1: &mut Core, node2: &mut Core, within: Duration) {
    let deadline = std::time::Instant::now() + within;
    let mut n1 = false;
    let mut n2 = false;
    while !(n1 && n2) {
        while let Some(ev) = node1.next_event().await {
            if matches!(ev, NetworkEvent::ConnectionEstablished { .. }) {
                n1 = true;
            }
        }
        while let Some(ev) = node2.next_event().await {
            if matches!(ev, NetworkEvent::ConnectionEstablished { .. }) {
                n2 = true;
            }
        }
        if std::time::Instant::now() > deadline {
            panic!("connection not established within {within:?}");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
