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
    // Replay the cached node_info as a synthetic NodeStarted so clients that connect
    // after the node booted still see identity/addrs.
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
                    Err(_) => continue,
                };
                if socket.send(Message::Text(json)).await.is_err() {
                    break;
                }
            }
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}
