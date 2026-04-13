use std::collections::VecDeque;
use std::sync::Arc;

use forge_ui::{MeshEvent, UiHandle};
use serde::{Deserialize, Serialize};
use swarm_nl::core::NetworkEvent;
use tokio::sync::Mutex;

pub const CHAT_TOPIC: &str = "chat";
/// Maximum number of chat lines retained for WS-reconnect replay.
pub const HISTORY_LIMIT: usize = 50;

/// One chat line sent over the gossip topic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatLine {
    pub from: String,
    pub text: String,
}

impl ChatLine {
    pub fn encode(&self) -> Vec<u8> {
        // Serialisation is infallible for this flat struct.
        serde_json::to_vec(self).unwrap_or_default()
    }
}

/// Ring buffer of the most recent `HISTORY_LIMIT` chat lines. Shared between
/// the broadcast path, the receive path, and `GET /api/chat/history`.
pub type History = Arc<Mutex<VecDeque<ChatLine>>>;

pub fn new_history() -> History {
    Arc::new(Mutex::new(VecDeque::with_capacity(HISTORY_LIMIT)))
}

pub async fn record(history: &History, line: ChatLine) {
    let mut h = history.lock().await;
    if h.len() >= HISTORY_LIMIT {
        h.pop_front();
    }
    h.push_back(line);
}

/// Translate one SwarmNL event into forge-ui events.
pub async fn handle_event(event: NetworkEvent, ui: &UiHandle, history: &History) {
    match event {
        NetworkEvent::ConnectionEstablished {
            peer_id, endpoint, ..
        } => {
            let pid = peer_id.to_string();
            let addr = format!("{endpoint:?}");
            tracing::info!(peer = %pid, "connection established");
            ui.push(MeshEvent::PeerConnected { peer_id: pid, addr })
                .await;
        }
        NetworkEvent::ConnectionClosed { peer_id, .. } => {
            let pid = peer_id.to_string();
            tracing::info!(peer = %pid, "connection closed");
            ui.push(MeshEvent::PeerDisconnected { peer_id: pid }).await;
        }
        NetworkEvent::GossipsubSubscribeMessageReceived { peer_id, topic } => {
            tracing::info!(peer = %peer_id, %topic, "peer subscribed to topic");
            ui.push(MeshEvent::Custom {
                label: "SUB".to_string(),
                detail: format!("{peer_id} subscribed to {topic}"),
            })
            .await;
        }
        NetworkEvent::GossipsubUnsubscribeMessageReceived { peer_id, topic } => {
            ui.push(MeshEvent::Custom {
                label: "UNSUB".to_string(),
                detail: format!("{peer_id} left {topic}"),
            })
            .await;
        }
        NetworkEvent::GossipsubIncomingMessageHandled { source, data } => {
            // `data` is StringVector (Vec<String>); the first element is our JSON payload.
            // Peers from previous app versions may send raw text; fall back gracefully.
            let raw = data.into_iter().next().unwrap_or_default();
            let size = raw.len();
            let line = serde_json::from_str::<ChatLine>(&raw).unwrap_or(ChatLine {
                from: source.to_string(),
                text: raw,
            });
            tracing::info!(from = %line.from, text = %line.text, "incoming chat");
            record(history, line.clone()).await;
            ui.push(MeshEvent::MessageReceived {
                from: source.to_string(),
                topic: CHAT_TOPIC.to_string(),
                size_bytes: size,
            })
            .await;
            ui.push(MeshEvent::Custom {
                label: "CHAT".to_string(),
                detail: format!("{}: {}", line.from, line.text),
            })
            .await;
        }
        other => {
            tracing::debug!(?other, "unhandled network event");
        }
    }
}
