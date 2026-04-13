use std::path::PathBuf;

use axum::{
    http::{HeaderValue, Request},
    middleware::{self, Next},
    response::{Html, Response},
    routing::get,
    Router,
};
use tokio::sync::broadcast;
use tower_http::services::ServeDir;

use crate::events::MeshEvent;
use crate::ws::{ws_handler, WsState};

/// Build the axum router for the forge-ui server.
///
/// Serves:
/// - `/ws` — WebSocket endpoint for real-time mesh events
/// - App-specific API routes (from `extra_routes`)
/// - `/app/*` — app-specific static files (from `app_static_dir`)
/// - `/*` — forge-ui's own static files (index.html, mesh.js, style.css)
pub fn build_router(
    tx: broadcast::Sender<MeshEvent>,
    app_name: String,
    app_static_dir: Option<PathBuf>,
    extra_routes: Option<Router>,
) -> Router {
    let ws_state = WsState { tx };

    let mut router = Router::new()
        .route("/ws", get(ws_handler))
        .route(
            "/config",
            get({
                let name = app_name.clone();
                move || async move {
                    axum::Json(serde_json::json!({ "app_name": name }))
                }
            }),
        )
        .with_state(ws_state);

    // Merge app-specific API routes (before static file services)
    if let Some(extra) = extra_routes {
        router = router.merge(extra);
    }

    // Serve app-specific static files under /app/
    if let Some(dir) = app_static_dir {
        router = router.nest_service("/app", ServeDir::new(dir));
    } else {
        router = router.nest_service(
            "/app",
            get(|| async { Html("<p>No app panel configured.</p>".to_string()) }),
        );
    }

    // Serve forge-ui's own static files (index.html, mesh.js, style.css) at root
    let ui_static = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("static");
    router = router.fallback_service(ServeDir::new(ui_static));

    // Disable browser caching — this is a dev server, stale JS/CSS causes
    // pain when iterating on the frontend.
    router.layer(middleware::from_fn(no_cache_headers))
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
