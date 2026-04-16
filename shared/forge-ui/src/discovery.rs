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
use crate::state::{DialRequest, DiscoveredPeer, ForgeState, MdnsBackend};

/// mDNS service type advertised by forge-ui.
const MDNS_SERVICE_TYPE: &str = "_forge-p2p._tcp.local.";
/// Prefix prepended to the local peer_id when forming an mDNS instance name.
/// The instance name round-trips through [`instance_name_for`] /
/// [`peer_id_from_instance`], so anything that formats instance names must go
/// through those helpers to keep the encoding consistent.
const MDNS_INSTANCE_PREFIX: &str = "forge-";

/// Interval between localhost-scan passes.
const SCAN_INTERVAL: Duration = Duration::from_secs(5);
/// Per-request timeout for the `/api/node/info` probes.
const PROBE_TIMEOUT: Duration = Duration::from_millis(500);

/// Format the mDNS instance label for a given peer. Uses the full `peer_id`
/// so two peers that happen to share a prefix cannot collide.
fn instance_name_for(peer_id: &str) -> String {
    format!("{MDNS_INSTANCE_PREFIX}{peer_id}")
}

/// Inverse of [`instance_name_for`]. Returns `None` if `instance` is not a
/// forge-ui instance name.
fn peer_id_from_instance(instance: &str) -> Option<&str> {
    instance.strip_prefix(MDNS_INSTANCE_PREFIX)
}

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
                    let peer_id = body.peer_id.clone();
                    if let Err(e) = tx
                        .send(DialRequest {
                            peer_id: body.peer_id,
                            addr,
                        })
                        .await
                    {
                        tracing::warn!(
                            peer_id = %peer_id,
                            error = ?e,
                            "forge-ui: auto-dial failed — app dial channel closed"
                        );
                    }
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

/// Start the mDNS advertiser + browser. Idempotent: if already running,
/// returns immediately.
pub async fn start_mdns(state: Arc<ForgeState>) -> anyhow::Result<()> {
    let mut slot = state.mdns_backend.lock().await;
    if slot.is_some() {
        return Ok(());
    }
    let info = state
        .node_info
        .read()
        .await
        .clone()
        .ok_or_else(|| anyhow::anyhow!("node_info not ready; cannot advertise mDNS"))?;

    // Pick a LAN multiaddr to publish. Prefer the first non-loopback listen
    // addr; fall back to loopback (single-machine mDNS demo). If we have no
    // listen addr at all, refuse to advertise — an empty multiaddr is worse
    // than not advertising, because resolvers would cache a dead entry.
    let multiaddr = info
        .listen_addrs
        .iter()
        .find(|a| a.starts_with("/ip4/") && !a.starts_with("/ip4/127.0.0.1/"))
        .cloned()
        .or_else(|| info.listen_addrs.first().cloned())
        .ok_or_else(|| {
            anyhow::anyhow!("cannot advertise mDNS: node has no listen addresses yet")
        })?;

    let daemon = mdns_sd::ServiceDaemon::new()?;
    let instance_name = instance_name_for(&info.peer_id);
    let host_ip = pick_host_ip();
    let mut props = std::collections::HashMap::new();
    props.insert("peer_id".to_string(), info.peer_id.clone());
    props.insert("multiaddr".to_string(), multiaddr);
    props.insert("app".to_string(), state.app_name.clone());
    let service = mdns_sd::ServiceInfo::new(
        MDNS_SERVICE_TYPE,
        &instance_name,
        &format!("{instance_name}.local."),
        host_ip.as_str(),
        state.local_http_port,
        Some(props),
    )?;
    daemon.register(service)?;

    let receiver = daemon.browse(MDNS_SERVICE_TYPE)?;
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel();

    let state_clone = state.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut shutdown_rx => break,
                event = async { receiver.recv_async().await } => {
                    match event {
                        Ok(mdns_sd::ServiceEvent::ServiceResolved(info)) => {
                            on_resolved(&state_clone, info).await;
                        }
                        Ok(mdns_sd::ServiceEvent::ServiceRemoved(_, fullname)) => {
                            on_removed(&state_clone, &fullname).await;
                        }
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
            }
        }
        // Best-effort daemon shutdown.
        let _ = daemon.shutdown();
    });

    *slot = Some(MdnsBackend { shutdown_tx });
    Ok(())
}

/// Stop the mDNS backend if running. No-op otherwise.
pub async fn stop_mdns(state: Arc<ForgeState>) {
    let mut slot = state.mdns_backend.lock().await;
    if let Some(backend) = slot.take() {
        let _ = backend.shutdown_tx.send(());
    }
    // Emit PeerLost for any mDNS-sourced entries so the UI cleans up.
    let to_remove: Vec<String> = {
        let discovered = state.discovered.read().await;
        discovered
            .values()
            .filter(|p| p.source == "mdns")
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
                source: "mdns".into(),
            });
        }
    }
}

async fn on_resolved(state: &Arc<ForgeState>, info: mdns_sd::ServiceInfo) {
    let props = info.get_properties();
    let peer_id = match props.get("peer_id").map(|p| p.val_str().to_string()) {
        Some(p) if !p.is_empty() => p,
        _ => return,
    };
    let self_peer = state.node_info.read().await.as_ref().map(|n| n.peer_id.clone());
    if Some(&peer_id) == self_peer.as_ref() {
        return;
    }
    let addr = props
        .get("multiaddr")
        .map(|p| p.val_str().to_string())
        .unwrap_or_default();
    if addr.is_empty() {
        return;
    }

    let is_new = !state.discovered.read().await.contains_key(&peer_id);
    state.discovered.write().await.insert(
        peer_id.clone(),
        DiscoveredPeer {
            peer_id: peer_id.clone(),
            addr: addr.clone(),
            source: "mdns".into(),
        },
    );

    if is_new {
        let _ = state.tx.send(MeshEvent::PeerDiscovered {
            peer_id: peer_id.clone(),
            addr: addr.clone(),
            source: "mdns".into(),
        });
        if let Some(tx) = state.dial_tx.as_ref() {
            let already_connected = state.connected.read().await.contains(&peer_id);
            if !already_connected {
                let pid_for_log = peer_id.clone();
                if let Err(e) = tx.send(DialRequest { peer_id, addr }).await {
                    tracing::warn!(
                        peer_id = %pid_for_log,
                        error = ?e,
                        "forge-ui: auto-dial (mDNS) failed — app dial channel closed"
                    );
                }
            }
        }
    }
}

async fn on_removed(state: &Arc<ForgeState>, fullname: &str) {
    // mdns-sd gives us the fullname (`<instance>.<service_type>`). Strip the
    // service suffix, then the `MDNS_INSTANCE_PREFIX` to recover the peer_id
    // that was advertised by `instance_name_for`. If the message isn't one of
    // ours, ignore it.
    let instance = fullname
        .strip_suffix(&format!(".{MDNS_SERVICE_TYPE}"))
        .unwrap_or(fullname);
    let Some(peer_id) = peer_id_from_instance(instance) else {
        return;
    };
    let peer_id = peer_id.to_string();

    let has_entry = state
        .discovered
        .read()
        .await
        .get(&peer_id)
        .is_some_and(|p| p.source == "mdns");
    if !has_entry {
        return;
    }
    state.discovered.write().await.remove(&peer_id);
    let _ = state.tx.send(MeshEvent::PeerLost {
        peer_id,
        source: "mdns".into(),
    });
}

fn pick_host_ip() -> String {
    // Prefer the first non-loopback IPv4 interface for LAN demos.
    if let Ok(ifaces) = if_addrs::get_if_addrs() {
        for iface in ifaces {
            if iface.is_loopback() {
                continue;
            }
            if let if_addrs::IfAddr::V4(v4) = iface.addr {
                return v4.ip.to_string();
            }
        }
    }
    "127.0.0.1".to_string()
}
