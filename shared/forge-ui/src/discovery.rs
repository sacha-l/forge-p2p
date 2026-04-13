//! Peer discovery backends for forge-ui.
//!
//! Two backends populate `ForgeState::discovered`:
//!
//! 1. **Localhost port scan** (always on) — probes `GET /api/node/info` on each
//!    port in `discovery_port_range`, skipping the local port. Any distinct
//!    `peer_id` that responds with a usable multiaddr becomes a `DiscoveredPeer`
//!    with `source = "localhost"`.
//!
//! 2. **mDNS** (opt-in via `POST /api/discovery/mdns`) — lands in task A4.
//!
//! When a new peer is added to the cache and `dial_tx` is configured, a
//! `DialRequest` is sent so the app dials immediately (the UI will grow a
//! toggle for this in A3).

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;

use crate::events::MeshEvent;
use crate::state::{DialRequest, DiscoveredPeer, ForgeState};

/// Interval between localhost-scan passes.
const SCAN_INTERVAL: Duration = Duration::from_secs(5);
/// Per-request timeout for the `/api/node/info` probes.
const PROBE_TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Deserialize)]
struct NodeInfoBody {
    peer_id: String,
    listen_addrs: Vec<String>,
}

/// Spawn the localhost port-scan discovery task. Cheap and suitable for local
/// demos; no fallback state on failure.
pub fn spawn_localhost_scan(state: Arc<ForgeState>) {
    tokio::spawn(async move {
        let client = match reqwest::Client::builder().timeout(PROBE_TIMEOUT).build() {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(?e, "forge-ui: failed to build reqwest client; discovery disabled");
                return;
            }
        };
        loop {
            if let Err(e) = scan_once(&state, &client).await {
                tracing::debug!(?e, "forge-ui localhost scan error (non-fatal)");
            }
            tokio::time::sleep(SCAN_INTERVAL).await;
        }
    });
}

async fn scan_once(state: &ForgeState, client: &reqwest::Client) -> anyhow::Result<()> {
    let (lo, hi) = state.discovery_port_range;
    let self_peer = state.node_info.read().await.as_ref().map(|n| n.peer_id.clone());
    let mut seen_this_pass: HashSet<String> = HashSet::new();

    for port in lo..=hi {
        if port == state.local_http_port {
            continue;
        }
        let url = format!("http://127.0.0.1:{port}/api/node/info");
        let Ok(resp) = client.get(&url).send().await else {
            continue;
        };
        if !resp.status().is_success() {
            continue;
        }
        let Ok(body) = resp.json::<NodeInfoBody>().await else {
            continue;
        };
        if Some(&body.peer_id) == self_peer.as_ref() {
            continue;
        }
        // Prefer the first loopback listen addr (most reliable in dev setups).
        let Some(addr) = pick_loopback(&body.listen_addrs) else {
            continue;
        };
        seen_this_pass.insert(body.peer_id.clone());

        let is_new = !state
            .discovered
            .read()
            .await
            .contains_key(&body.peer_id);
        state.discovered.write().await.insert(
            body.peer_id.clone(),
            DiscoveredPeer {
                peer_id: body.peer_id.clone(),
                addr: addr.clone(),
                source: "localhost".into(),
            },
        );

        if is_new {
            let _ = state.tx.send(MeshEvent::PeerDiscovered {
                peer_id: body.peer_id.clone(),
                addr: addr.clone(),
                source: "localhost".into(),
            });
            // Auto-dial on first sight if the app wired a dial sender.
            if let Some(tx) = state.dial_tx.as_ref() {
                let already_connected =
                    state.connected.read().await.contains(&body.peer_id);
                if !already_connected {
                    let _ = tx
                        .send(DialRequest {
                            peer_id: body.peer_id,
                            addr,
                        })
                        .await;
                }
            }
        }
    }

    // Anything in the cache that we didn't see this pass AND whose source is
    // localhost has gone away — emit PeerLost and drop it.
    let to_remove: Vec<String> = {
        let discovered = state.discovered.read().await;
        discovered
            .values()
            .filter(|p| p.source == "localhost" && !seen_this_pass.contains(&p.peer_id))
            .map(|p| p.peer_id.clone())
            .collect()
    };
    if !to_remove.is_empty() {
        let mut discovered = state.discovered.write().await;
        for pid in &to_remove {
            discovered.remove(pid);
        }
        drop(discovered);
        for pid in to_remove {
            let _ = state.tx.send(MeshEvent::PeerLost {
                peer_id: pid,
                source: "localhost".into(),
            });
        }
    }
    Ok(())
}

fn pick_loopback(addrs: &[String]) -> Option<String> {
    addrs
        .iter()
        .find(|a| a.starts_with("/ip4/127.0.0.1/tcp/"))
        .cloned()
        .or_else(|| addrs.first().cloned())
}
