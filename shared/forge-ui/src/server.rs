//! HTTP routing layer: builds the axum `Router` that drives forge-ui.
//!
//! See [`build_router`] for the route map. Application-specific routes get
//! merged in via `ForgeUI::with_routes` before the static file services are
//! registered, so apps cannot accidentally shadow `/ws` or the built-in
//! `/api/*` endpoints.

use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderValue, Request, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use tower_http::services::ServeDir;

use crate::discovery;
use crate::state::{DialRequest, ForgeState};
use crate::ws::ws_handler;

/// Build the axum router for the forge-ui server.
///
/// Serves:
/// - `/ws` — WebSocket endpoint for real-time mesh events
/// - `/config` — JSON with `{app_name}` (kept for backwards compat with older panels)
/// - `/api/node/info` — cached PeerId + listen addrs (+ app_name + http_port)
/// - `/api/peer/dial` — POST {peer_id, addr}: enqueues a `DialRequest` on the app's channel
/// - `/api/peers/discovered` — cache of peers seen by forge-ui's discovery backends
/// - `/api/discovery/mdns` — POST {enabled}: toggles the mDNS backend (A4)
/// - App-specific API routes (from `extra_routes`)
/// - `/app/*` — app-specific static files (from `app_static_dir`)
/// - `/*` — forge-ui's own static files (index.html, mesh.js, style.css, peers.js)
pub fn build_router(
    state: Arc<ForgeState>,
    app_static_dir: Option<PathBuf>,
    extra_routes: Option<Router>,
) -> Router {
    let mut router = Router::new()
        .route("/ws", get(ws_handler))
        .route("/config", get(config_handler))
        .route("/api/node/info", get(node_info_handler))
        .route("/api/peer/dial", post(dial_handler))
        .route("/api/peers/discovered", get(discovered_handler))
        .route("/api/discovery/mdns", post(mdns_toggle_handler))
        .with_state(state);

    // Merge app-specific API routes (before static file services).
    if let Some(extra) = extra_routes {
        router = router.merge(extra);
    }

    // Serve app-specific static files under /app/.
    if let Some(dir) = app_static_dir {
        router = router.nest_service("/app", ServeDir::new(dir));
    } else {
        router = router.nest_service(
            "/app",
            get(|| async { Html("<p>No app panel configured.</p>".to_string()) }),
        );
    }

    // Serve forge-ui's own static files (index.html, mesh.js, peers.js, style.css) at root.
    let ui_static = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("static");
    router = router.fallback_service(ServeDir::new(ui_static));

    // Dev server: keep caches off so HTML/JS/CSS iteration is painless.
    router.layer(middleware::from_fn(no_cache_headers))
}

async fn config_handler(State(state): State<Arc<ForgeState>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "app_name": state.app_name }))
}

async fn node_info_handler(State(state): State<Arc<ForgeState>>) -> Response {
    let info = state.node_info.read().await.clone();
    match info {
        Some(info) => Json(serde_json::json!({
            "peer_id": info.peer_id,
            "listen_addrs": info.listen_addrs,
            "app_name": state.app_name,
            "http_port": state.local_http_port,
        }))
        .into_response(),
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            "node has not started yet",
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct DialReqBody {
    peer_id: String,
    addr: String,
}

async fn dial_handler(
    State(state): State<Arc<ForgeState>>,
    Json(body): Json<DialReqBody>,
) -> Response {
    let peer_id = body.peer_id.trim().to_string();
    let addr = body.addr.trim().to_string();
    if peer_id.is_empty() || addr.is_empty() {
        return (StatusCode::BAD_REQUEST, "peer_id and addr are required").into_response();
    }
    let Some(tx) = state.dial_tx.as_ref() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "dialing is not enabled (app did not provide a dial sender)",
        )
            .into_response();
    };
    match tx.send(DialRequest { peer_id, addr }).await {
        Ok(()) => StatusCode::ACCEPTED.into_response(),
        Err(_) => {
            (StatusCode::SERVICE_UNAVAILABLE, "dial channel closed").into_response()
        }
    }
}

async fn discovered_handler(State(state): State<Arc<ForgeState>>) -> Json<serde_json::Value> {
    let map = state.discovered.read().await;
    let peers: Vec<_> = map.values().cloned().collect();
    Json(serde_json::json!({ "peers": peers }))
}

#[derive(Deserialize)]
struct MdnsToggle {
    enabled: bool,
}

async fn mdns_toggle_handler(
    State(state): State<Arc<ForgeState>>,
    Json(body): Json<MdnsToggle>,
) -> Response {
    if body.enabled {
        match discovery::start_mdns(state.clone()).await {
            Ok(()) => {
                state
                    .mdns_enabled
                    .store(true, std::sync::atomic::Ordering::Relaxed);
                StatusCode::ACCEPTED.into_response()
            }
            Err(e) => (
                StatusCode::SERVICE_UNAVAILABLE,
                format!("mdns start failed: {e}"),
            )
                .into_response(),
        }
    } else {
        discovery::stop_mdns(state.clone()).await;
        state
            .mdns_enabled
            .store(false, std::sync::atomic::Ordering::Relaxed);
        StatusCode::ACCEPTED.into_response()
    }
}

/// Middleware that adds `Cache-Control: no-store` to every response.
async fn no_cache_headers(req: Request<axum::body::Body>, next: Next) -> Response {
    let mut response = next.run(req).await;
    response.headers_mut().insert(
        "cache-control",
        HeaderValue::from_static("no-store, no-cache, must-revalidate"),
    );
    response
}
