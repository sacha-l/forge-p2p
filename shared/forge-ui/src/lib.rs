//! Embedded web UI, mesh visualizer, and peer-discovery for ForgeP2P apps.
//!
//! Apps use the [`ForgeUI`] builder to start an Axum server on localhost that
//! serves a split-pane UI (app panel + mesh visualizer) and a WebSocket feed
//! of [`MeshEvent`]s. The [`UiHandle`] returned from [`ForgeUI::start`] is how
//! apps push events into that feed.
//!
//! See `CLAUDE.md` and `.forge/workflow.md` for the full integration contract.

pub mod discovery;
pub mod events;
pub mod server;
pub mod state;
pub mod ws;

pub use events::MeshEvent;
pub use state::{DialRequest, DiscoveredPeer, NodeInfo};

use std::path::PathBuf;

use anyhow::{Context, Result};
use axum::Router;
use tokio::sync::{broadcast, mpsc};

use crate::state::{spawn_state_mirror, ForgeState};

/// Capacity of the broadcast channel carrying [`MeshEvent`]s to WebSocket clients.
///
/// Bumped beyond the default 256 because busy apps (gossip, chat) can emit
/// many events in quick succession while a browser reconnects. If a client
/// still lags past this window, the WS handler logs a warning and keeps
/// running — no crash, but the client will have gaps in its event log until
/// it gets a fresh snapshot.
const EVENT_CHANNEL_CAPACITY: usize = 1024;

/// Handle for pushing events to the UI after the server is started.
#[derive(Clone)]
pub struct UiHandle {
    tx: broadcast::Sender<MeshEvent>,
}

impl UiHandle {
    /// Broadcast `event` to all connected WebSocket clients.
    ///
    /// This call is fire-and-forget: it succeeds even when no clients are
    /// connected (common during startup) and never blocks the caller. If the
    /// buffer is full (capacity [`EVENT_CHANNEL_CAPACITY`]), lagging clients
    /// drop events and the WS handler logs a warning; the `push` itself still
    /// returns normally.
    pub async fn push(&self, event: MeshEvent) {
        let _ = self.tx.send(event);
    }
}

/// Builder for configuring and starting the forge-ui web server.
pub struct ForgeUI {
    port: u16,
    app_name: String,
    app_static_dir: Option<PathBuf>,
    extra_routes: Option<Router>,
    dial_tx: Option<mpsc::Sender<DialRequest>>,
    local_peer_id: Option<String>,
    discovery_port_range: (u16, u16),
}

impl ForgeUI {
    /// Create a new builder with sensible defaults.
    pub fn new() -> Self {
        Self {
            port: 8080,
            app_name: "ForgeP2P App".to_string(),
            app_static_dir: None,
            extra_routes: None,
            dial_tx: None,
            local_peer_id: None,
            discovery_port_range: (8080, 8089),
        }
    }

    /// Set the port for the web server.
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Set the application name shown in the UI.
    pub fn with_app_name(mut self, name: &str) -> Self {
        self.app_name = name.to_string();
        self
    }

    /// Set the directory for app-specific static files (served under `/app/`).
    pub fn with_app_static_dir(mut self, dir: &str) -> Self {
        self.app_static_dir = Some(PathBuf::from(dir));
        self
    }

    /// Merge additional axum routes into the server (e.g. app-specific API endpoints).
    pub fn with_routes(mut self, routes: Router) -> Self {
        self.extra_routes = Some(routes);
        self
    }

    /// Provide a channel into the app's event loop so forge-ui can ask the app
    /// to dial peers (both from the manual UI form and from auto-discovery).
    /// Without this, `POST /api/peer/dial` returns `503` and auto-connect is a no-op.
    pub fn with_dial_sender(mut self, tx: mpsc::Sender<DialRequest>) -> Self {
        self.dial_tx = Some(tx);
        self
    }

    /// Seed the local `PeerId` before the node emits its first `NodeStarted` event.
    /// Useful so `GET /api/node/info` returns a usable response immediately on startup.
    pub fn with_local_peer_id(mut self, peer_id: &str) -> Self {
        self.local_peer_id = Some(peer_id.to_string());
        self
    }

    /// Override the inclusive port range probed by the localhost discovery backend.
    /// Default is `(8080, 8089)`.
    pub fn with_discovery_port_range(mut self, lo: u16, hi: u16) -> Self {
        self.discovery_port_range = (lo, hi);
        self
    }

    /// Start the web server in the background and return a handle for pushing events.
    ///
    /// Fails fast if the configured `app_static_dir` does not exist or the
    /// HTTP port cannot be bound. After the server has started successfully,
    /// later fatal errors from Axum are logged via `tracing::error!` (the
    /// task cannot propagate them synchronously once the handle is returned).
    pub async fn start(self) -> Result<UiHandle> {
        if let Some(dir) = self.app_static_dir.as_ref() {
            if !dir.is_dir() {
                return Err(anyhow::anyhow!(
                    "forge-ui: app_static_dir {:?} does not exist or is not a directory",
                    dir
                ));
            }
        }

        let (tx, _rx) = broadcast::channel::<MeshEvent>(EVENT_CHANNEL_CAPACITY);
        let handle = UiHandle { tx: tx.clone() };

        let state = ForgeState::new(
            tx,
            self.dial_tx,
            self.local_peer_id,
            self.port,
            self.discovery_port_range,
            self.app_name.clone(),
        );
        spawn_state_mirror(state.clone());
        discovery::spawn_localhost_scan(state.clone());

        let router = server::build_router(state, self.app_static_dir, self.extra_routes);
        let addr = format!("127.0.0.1:{}", self.port);
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .with_context(|| format!("forge-ui: failed to bind {addr}"))?;

        let app_name = self.app_name.clone();
        tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, router).await {
                tracing::error!(error = ?e, app = %app_name, "forge-ui server exited with error");
            }
        });

        Ok(handle)
    }
}

impl Default for ForgeUI {
    fn default() -> Self {
        Self::new()
    }
}
