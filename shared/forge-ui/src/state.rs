//! Shared state for forge-ui's built-in peering routes + discovery tasks.
//!
//! Apps interact with this indirectly:
//! - they push `MeshEvent`s via `UiHandle`; a subscriber task mirrors the relevant
//!   ones (`NodeStarted`, `PeerConnected`, `PeerDisconnected`, `PeerDiscovered`,
//!   `PeerLost`) into the caches below so the built-in HTTP routes can serve them,
//! - they optionally pass a `mpsc::Sender<DialRequest>` so forge-ui can ask the
//!   app's SwarmNL event loop to dial a peer.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc, Mutex, RwLock};

use crate::events::MeshEvent;

/// Opaque handle to a running mDNS backend (advertiser + browser).
/// Stored in `ForgeState` so the toggle route can start/stop it.
pub struct MdnsBackend {
    pub shutdown_tx: tokio::sync::oneshot::Sender<()>,
}

/// Snapshot of the local node's identity, surfaced via `GET /api/node/info`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeInfo {
    pub peer_id: String,
    pub listen_addrs: Vec<String>,
}

/// A peer discovered by one of forge-ui's discovery backends (localhost scan or mDNS).
/// Identified uniquely by `peer_id`; the latest entry wins on update.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveredPeer {
    pub peer_id: String,
    pub addr: String,
    pub source: String,
}

/// Request from forge-ui → app: "please dial this peer".
/// Apps receive these on the `mpsc::Receiver` whose sender was passed to
/// `ForgeUI::with_dial_sender(...)`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DialRequest {
    pub peer_id: String,
    pub addr: String,
}

/// Process-wide state for forge-ui. Shared across axum handlers and background tasks
/// via `Arc<ForgeState>`.
pub struct ForgeState {
    /// Broadcast channel for outbound MeshEvents to WebSocket clients.
    pub tx: broadcast::Sender<MeshEvent>,
    /// The local node's identity. Populated from `MeshEvent::NodeStarted`
    /// (or seeded via `ForgeUI::with_local_peer_id`).
    pub node_info: RwLock<Option<NodeInfo>>,
    /// Peers seen by discovery, keyed by peer_id.
    pub discovered: RwLock<HashMap<String, DiscoveredPeer>>,
    /// Peers currently connected, maintained from PeerConnected/PeerDisconnected events.
    pub connected: RwLock<HashSet<String>>,
    /// Whether the mDNS discovery backend is currently advertising + browsing.
    pub mdns_enabled: AtomicBool,
    /// Running mDNS backend handle (Some while enabled).
    pub mdns_backend: Mutex<Option<MdnsBackend>>,
    /// Channel into the app's event loop for dial requests. `None` means dialing
    /// is disabled (the app didn't pass a sender).
    pub dial_tx: Option<mpsc::Sender<DialRequest>>,
    /// forge-ui's own HTTP port — used by discovery to skip self during scan.
    pub local_http_port: u16,
    /// Inclusive range of ports to probe during localhost discovery (default 8080..=8089).
    pub discovery_port_range: (u16, u16),
    /// App name — surfaced in `GET /api/node/info` so discovered peers can be labelled.
    pub app_name: String,
}

impl ForgeState {
    pub fn new(
        tx: broadcast::Sender<MeshEvent>,
        dial_tx: Option<mpsc::Sender<DialRequest>>,
        local_peer_id: Option<String>,
        local_http_port: u16,
        discovery_port_range: (u16, u16),
        app_name: String,
    ) -> Arc<Self> {
        let node_info = local_peer_id.map(|pid| NodeInfo {
            peer_id: pid,
            listen_addrs: Vec::new(),
        });
        Arc::new(Self {
            tx,
            node_info: RwLock::new(node_info),
            discovered: RwLock::new(HashMap::new()),
            connected: RwLock::new(HashSet::new()),
            mdns_enabled: AtomicBool::new(false),
            mdns_backend: Mutex::new(None),
            dial_tx,
            local_http_port,
            discovery_port_range,
            app_name,
        })
    }
}

/// Background task: subscribe to the MeshEvent broadcast and keep the state
/// caches (`node_info`, `connected`, `discovered`) up to date.
pub fn spawn_state_mirror(state: Arc<ForgeState>) {
    let mut rx = state.tx.subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => apply_event(&state, event).await,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

async fn apply_event(state: &ForgeState, event: MeshEvent) {
    match event {
        MeshEvent::NodeStarted {
            peer_id,
            listen_addrs,
        } => {
            *state.node_info.write().await = Some(NodeInfo {
                peer_id,
                listen_addrs,
            });
        }
        MeshEvent::PeerConnected { peer_id, .. } => {
            state.connected.write().await.insert(peer_id);
        }
        MeshEvent::PeerDisconnected { peer_id } => {
            state.connected.write().await.remove(&peer_id);
        }
        MeshEvent::PeerDiscovered {
            peer_id,
            addr,
            source,
        } => {
            state.discovered.write().await.insert(
                peer_id.clone(),
                DiscoveredPeer {
                    peer_id,
                    addr,
                    source,
                },
            );
        }
        MeshEvent::PeerLost { peer_id, .. } => {
            state.discovered.write().await.remove(&peer_id);
        }
        _ => {}
    }
}
