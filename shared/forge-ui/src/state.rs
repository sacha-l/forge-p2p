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

/// A peer discovered by one of forge-ui's discovery backends (localhost scan
/// or mDNS). Identified uniquely by `peer_id`; the latest entry wins on
/// update. `source` is a stable tag (`"localhost"` or `"mdns"`) used by
/// eviction logic to scope `PeerLost` events to the backend that owns them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveredPeer {
    pub peer_id: String,
    pub addr: String,
    pub source: String,
}

/// Request from forge-ui → app: "please dial this peer".
///
/// Apps receive these on the `mpsc::Receiver` whose sender was passed to
/// [`crate::ForgeUI::with_dial_sender`]. Sources:
///
/// - the `POST /api/peer/dial` HTTP route (manual dial from the UI), and
/// - auto-dial on first sight of a discovered peer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DialRequest {
    /// libp2p-style base58 PeerId of the target node.
    pub peer_id: String,
    /// Multiaddr the app should use to dial, e.g. `/ip4/127.0.0.1/tcp/3000`.
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

pub(crate) async fn apply_event(state: &ForgeState, event: MeshEvent) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_state() -> Arc<ForgeState> {
        let (tx, _rx) = broadcast::channel::<MeshEvent>(16);
        ForgeState::new(tx, None, None, 8080, (8080, 8089), "test".into())
    }

    #[tokio::test]
    async fn node_started_populates_node_info() {
        let s = mk_state();
        apply_event(
            &s,
            MeshEvent::NodeStarted {
                peer_id: "p1".into(),
                listen_addrs: vec!["/ip4/127.0.0.1/tcp/3000".into()],
            },
        )
        .await;
        let info = s.node_info.read().await.clone().expect("node_info set");
        assert_eq!(info.peer_id, "p1");
        assert_eq!(info.listen_addrs, vec!["/ip4/127.0.0.1/tcp/3000"]);
    }

    #[tokio::test]
    async fn node_started_overwrites_previous() {
        let s = mk_state();
        apply_event(
            &s,
            MeshEvent::NodeStarted {
                peer_id: "old".into(),
                listen_addrs: vec![],
            },
        )
        .await;
        apply_event(
            &s,
            MeshEvent::NodeStarted {
                peer_id: "new".into(),
                listen_addrs: vec!["/ip4/127.0.0.1/tcp/1".into()],
            },
        )
        .await;
        assert_eq!(s.node_info.read().await.as_ref().unwrap().peer_id, "new");
    }

    #[tokio::test]
    async fn peer_connected_is_idempotent() {
        let s = mk_state();
        for _ in 0..3 {
            apply_event(
                &s,
                MeshEvent::PeerConnected {
                    peer_id: "p1".into(),
                    addr: "/ip4/127.0.0.1/tcp/1".into(),
                },
            )
            .await;
        }
        let connected = s.connected.read().await;
        assert_eq!(connected.len(), 1);
        assert!(connected.contains("p1"));
    }

    #[tokio::test]
    async fn peer_disconnected_for_unknown_is_noop() {
        let s = mk_state();
        apply_event(
            &s,
            MeshEvent::PeerDisconnected {
                peer_id: "never-seen".into(),
            },
        )
        .await;
        assert!(s.connected.read().await.is_empty());
    }

    #[tokio::test]
    async fn peer_disconnected_removes_existing() {
        let s = mk_state();
        apply_event(
            &s,
            MeshEvent::PeerConnected {
                peer_id: "p1".into(),
                addr: "/ip4/127.0.0.1/tcp/1".into(),
            },
        )
        .await;
        apply_event(
            &s,
            MeshEvent::PeerDisconnected {
                peer_id: "p1".into(),
            },
        )
        .await;
        assert!(s.connected.read().await.is_empty());
    }

    #[tokio::test]
    async fn peer_discovered_inserts_and_updates() {
        let s = mk_state();
        apply_event(
            &s,
            MeshEvent::PeerDiscovered {
                peer_id: "p1".into(),
                addr: "/ip4/127.0.0.1/tcp/1".into(),
                source: "localhost".into(),
            },
        )
        .await;
        apply_event(
            &s,
            MeshEvent::PeerDiscovered {
                peer_id: "p1".into(),
                addr: "/ip4/127.0.0.1/tcp/2".into(),
                source: "mdns".into(),
            },
        )
        .await;
        let d = s.discovered.read().await;
        assert_eq!(d.len(), 1);
        let entry = d.get("p1").unwrap();
        assert_eq!(entry.addr, "/ip4/127.0.0.1/tcp/2");
        assert_eq!(entry.source, "mdns");
    }

    #[tokio::test]
    async fn peer_lost_removes_entry_regardless_of_source() {
        let s = mk_state();
        apply_event(
            &s,
            MeshEvent::PeerDiscovered {
                peer_id: "p1".into(),
                addr: "/ip4/127.0.0.1/tcp/1".into(),
                source: "mdns".into(),
            },
        )
        .await;
        apply_event(
            &s,
            MeshEvent::PeerLost {
                peer_id: "p1".into(),
                source: "mdns".into(),
            },
        )
        .await;
        assert!(s.discovered.read().await.is_empty());
    }

    #[tokio::test]
    async fn unrelated_events_dont_touch_state() {
        let s = mk_state();
        apply_event(
            &s,
            MeshEvent::MessageSent {
                to: "p1".into(),
                topic: "t".into(),
                size_bytes: 0,
            },
        )
        .await;
        apply_event(
            &s,
            MeshEvent::GossipJoined { topic: "t".into() },
        )
        .await;
        assert!(s.node_info.read().await.is_none());
        assert!(s.connected.read().await.is_empty());
        assert!(s.discovered.read().await.is_empty());
    }
}
