use std::path::PathBuf;

use axum::{
    response::Html,
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
/// - `/app/*` — app-specific static files (from `app_static_dir`)
/// - `/*` — forge-ui's own static files (index.html, mesh.js, style.css)
pub fn build_router(
    tx: broadcast::Sender<MeshEvent>,
    app_name: String,
    app_static_dir: Option<PathBuf>,
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

    router
}
