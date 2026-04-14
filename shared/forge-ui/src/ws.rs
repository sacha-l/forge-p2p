//! WebSocket handler for streaming `MeshEvent`s to browser clients.
//!
//! Each connection replays the cached `NodeStarted` (so late joiners see
//! identity/addrs) and then streams live events. If a client lags behind the
//! broadcast buffer, we log a warning with the number of dropped events — the
//! UI will have gaps in its history until the next full state refresh.

use std::sync::Arc;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
};
use tokio::sync::broadcast;

use crate::events::MeshEvent;
use crate::state::ForgeState;

/// Axum handler that upgrades HTTP to WebSocket.
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<ForgeState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: Arc<ForgeState>) {
    let mut rx = state.tx.subscribe();
    if let Some(info) = state.node_info.read().await.clone() {
        let synthetic = MeshEvent::NodeStarted {
            peer_id: info.peer_id,
            listen_addrs: info.listen_addrs,
        };
        if let Ok(json) = serde_json::to_string(&synthetic) {
            if socket.send(Message::Text(json)).await.is_err() {
                return;
            }
        }
    }
    loop {
        match rx.recv().await {
            Ok(event) => {
                let json = match serde_json::to_string(&event) {
                    Ok(j) => j,
                    Err(e) => {
                        tracing::warn!(?e, "forge-ui: failed to serialize MeshEvent; dropping");
                        continue;
                    }
                };
                if socket.send(Message::Text(json)).await.is_err() {
                    break;
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!(
                    skipped = n,
                    "forge-ui WebSocket client lagged; events dropped"
                );
                continue;
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}
