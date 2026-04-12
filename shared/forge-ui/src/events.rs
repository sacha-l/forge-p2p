use serde::{Deserialize, Serialize};

/// Network events that apps push to the UI for visualization.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum MeshEvent {
    /// The local node has started and is listening.
    NodeStarted {
        peer_id: String,
        listen_addrs: Vec<String>,
    },
    /// A remote peer connected.
    PeerConnected { peer_id: String, addr: String },
    /// A remote peer disconnected.
    PeerDisconnected { peer_id: String },
    /// A message was sent to a peer or topic.
    MessageSent {
        to: String,
        topic: String,
        size_bytes: usize,
    },
    /// A message was received from a peer.
    MessageReceived {
        from: String,
        topic: String,
        size_bytes: usize,
    },
    /// Joined a gossip topic.
    GossipJoined { topic: String },
    /// Replication sync status update.
    ReplicaSync {
        peer_id: String,
        network: String,
        status: String,
    },
    /// App-specific custom event.
    Custom { label: String, detail: String },
}
