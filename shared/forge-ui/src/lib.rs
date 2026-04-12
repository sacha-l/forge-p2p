pub mod events;
pub mod server;
pub mod ws;

pub use events::MeshEvent;

use std::path::PathBuf;

use anyhow::Result;
use axum::Router;
use tokio::sync::broadcast;

/// Handle for pushing events to the UI after the server is started.
#[derive(Clone)]
pub struct UiHandle {
    tx: broadcast::Sender<MeshEvent>,
}

impl UiHandle {
    /// Push a mesh event to all connected WebSocket clients.
    pub async fn push(&self, event: MeshEvent) {
        // Ignore send errors (no receivers connected yet).
        let _ = self.tx.send(event);
    }
}

/// Builder for configuring and starting the forge-ui web server.
pub struct ForgeUI {
    port: u16,
    app_name: String,
    app_static_dir: Option<PathBuf>,
    extra_routes: Option<Router>,
}

impl ForgeUI {
    /// Create a new builder with sensible defaults.
    pub fn new() -> Self {
        Self {
            port: 8080,
            app_name: "ForgeP2P App".to_string(),
            app_static_dir: None,
            extra_routes: None,
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
    /// These routes take priority over forge-ui's own routes.
    pub fn with_routes(mut self, routes: Router) -> Self {
        self.extra_routes = Some(routes);
        self
    }

    /// Start the web server in the background and return a handle for pushing events.
    pub async fn start(self) -> Result<UiHandle> {
        let (tx, _rx) = broadcast::channel::<MeshEvent>(256);
        let handle = UiHandle { tx: tx.clone() };

        let mut router = server::build_router(tx, self.app_name, self.app_static_dir);
        if let Some(extra) = self.extra_routes {
            router = extra.merge(router);
        }
        let addr = format!("127.0.0.1:{}", self.port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;

        tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, router).await {
                eprintln!("forge-ui server error: {e}");
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
