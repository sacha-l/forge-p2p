mod chat;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use clap::{Parser, ValueEnum};
use forge_ui::{DialRequest, ForgeUI, MeshEvent, UiHandle};
use serde::Deserialize;
use swarm_nl::core::{AppData, Core, CoreBuilder, NetworkEvent};
use swarm_nl::setup::BootstrapConfig;
use swarm_nl::PeerId;
use tokio::sync::mpsc;

use crate::chat::{handle_event, ChatLine, CHAT_TOPIC};

#[derive(Copy, Clone, Debug, ValueEnum)]
enum PeerName {
    Al,
    Bobby,
}

impl PeerName {
    fn display(self) -> &'static str {
        match self {
            PeerName::Al => "Al",
            PeerName::Bobby => "Bobby",
        }
    }

    fn tcp_port(self) -> u16 {
        match self {
            PeerName::Al => 50000,
            PeerName::Bobby => 50200,
        }
    }

    fn udp_port(self) -> u16 {
        self.tcp_port() + 1
    }

    fn ui_port(self) -> u16 {
        match self {
            PeerName::Al => 8080,
            PeerName::Bobby => 8081,
        }
    }
}

#[derive(Parser, Debug)]
#[command(name = "mesh-chat", about = "Two-peer gossip chat demo")]
struct Cli {
    /// Which named peer this process is.
    #[arg(long)]
    peer: PeerName,

    /// Optional: PeerId of a bootnode to dial on startup.
    /// Usually left unset — the browser UI auto-discovers and dials the other peer.
    #[arg(long)]
    bootnode_peer_id: Option<String>,

    /// Optional: multiaddr of the bootnode, e.g. /ip4/127.0.0.1/tcp/50000.
    #[arg(long)]
    bootnode_addr: Option<String>,
}

#[derive(Deserialize)]
struct SendReq {
    text: String,
}

struct AppState {
    tx: mpsc::Sender<String>,
}

async fn send_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SendReq>,
) -> StatusCode {
    let text = req.text.trim().to_string();
    if text.is_empty() {
        return StatusCode::BAD_REQUEST;
    }
    match state.tx.send(text).await {
        Ok(()) => StatusCode::ACCEPTED,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "mesh_chat=info,forge_ui=info".into()),
        )
        .init();

    let cli = Cli::parse();
    let name = cli.peer.display().to_string();

    // Optional CLI-provided bootnode. The forge-ui peers tab can dial at any
    // time too, so most demo runs leave these unset.
    //
    // NOTE: `with_bootnodes` takes HashMap<String, String> despite the reference
    // doc's claim of HashMap<PeerId, String>. See library-feedback.md.
    let mut bootnodes: HashMap<String, String> = HashMap::new();
    if let Some(pid_str) = cli.bootnode_peer_id.as_deref() {
        let addr = cli
            .bootnode_addr
            .clone()
            .unwrap_or_else(|| format!("/ip4/127.0.0.1/tcp/{}", PeerName::Al.tcp_port()));
        bootnodes.insert(pid_str.to_string(), addr);
    }

    let config = BootstrapConfig::new()
        .with_tcp(cli.peer.tcp_port())
        .with_udp(cli.peer.udp_port())
        .with_bootnodes(bootnodes);

    let mut node = CoreBuilder::with_config(config)
        .build()
        .await
        .map_err(|e| anyhow!("failed to build swarm-nl core: {e:?}"))?;

    // Drain initial setup events to collect PeerId + listen addresses.
    let mut peer_id: Option<String> = None;
    let mut listen_addrs: Vec<String> = Vec::new();
    for _ in 0..20 {
        if let Some(event) = node.next_event().await {
            if let NetworkEvent::NewListenAddr {
                local_peer_id,
                address,
                ..
            } = event
            {
                peer_id.get_or_insert_with(|| local_peer_id.to_string());
                listen_addrs.push(address.to_string());
            }
        } else {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    let peer_id = peer_id.unwrap_or_else(|| "unknown".to_string());

    println!("=== mesh-chat :: {name} ===");
    println!("PeerId:  {peer_id}");
    for a in &listen_addrs {
        println!("Listen:  {a}");
    }
    println!("UI:      http://127.0.0.1:{}", cli.peer.ui_port());
    println!("Tip:     open the UI; the other peer should appear under 'Discovered' and auto-connect.");

    // Two channels:
    // - `send_rx` — our own POST /api/chat/send route dispatches user-typed
    //   chat lines here.
    // - `dial_rx` — forge-ui dispatches DialRequests here (manual dial form
    //   in the Peers tab, or auto-connect from discovery).
    let (send_tx, mut send_rx) = mpsc::channel::<String>(64);
    let (dial_tx, mut dial_rx) = mpsc::channel::<DialRequest>(64);

    let app_state = Arc::new(AppState { tx: send_tx });
    let routes = Router::new()
        .route("/api/chat/send", post(send_handler))
        .with_state(app_state);

    let static_dir = format!("{}/static", env!("CARGO_MANIFEST_DIR"));
    let ui = ForgeUI::new()
        .with_port(cli.peer.ui_port())
        .with_app_name(&format!("mesh-chat :: {name}"))
        .with_app_static_dir(&static_dir)
        .with_local_peer_id(&peer_id)
        .with_routes(routes)
        .with_dial_sender(dial_tx)
        .start()
        .await?;

    ui.push(MeshEvent::NodeStarted {
        peer_id: peer_id.clone(),
        listen_addrs: listen_addrs.clone(),
    })
    .await;

    // Join the chat gossip topic.
    match node
        .query_network(AppData::GossipsubJoinNetwork(CHAT_TOPIC.to_string()))
        .await
    {
        Ok(_) => {
            ui.push(MeshEvent::GossipJoined {
                topic: CHAT_TOPIC.to_string(),
            })
            .await;
            tracing::info!(topic = CHAT_TOPIC, "joined gossip topic");
        }
        Err(e) => {
            tracing::warn!(?e, "failed to join gossip topic");
        }
    }

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                println!("shutting down {name}");
                return Ok(());
            }
            Some(text) = send_rx.recv() => {
                broadcast(&mut node, &ui, &name, text).await;
            }
            Some(req) = dial_rx.recv() => {
                dial(&mut node, &ui, req.peer_id, req.addr).await;
            }
            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                while let Some(event) = node.next_event().await {
                    handle_event(event, &ui).await;
                }
            }
        }
    }
}

async fn broadcast(node: &mut Core, ui: &UiHandle, name: &str, text: String) {
    let line = ChatLine {
        from: name.to_string(),
        text,
    };
    let bytes = line.encode();
    let size = bytes.len();
    let req = AppData::GossipsubBroadcastMessage {
        topic: CHAT_TOPIC.to_string(),
        message: vec![bytes],
    };
    match node.query_network(req).await {
        Ok(_) => {
            ui.push(MeshEvent::MessageSent {
                to: CHAT_TOPIC.to_string(),
                topic: CHAT_TOPIC.to_string(),
                size_bytes: size,
            })
            .await;
            ui.push(MeshEvent::Custom {
                label: "CHAT".to_string(),
                detail: format!("{}: {}", line.from, line.text),
            })
            .await;
            tracing::info!(text = %line.text, "broadcast ok");
        }
        Err(e) => {
            tracing::warn!(?e, "broadcast failed");
            ui.push(MeshEvent::Custom {
                label: "ERROR".to_string(),
                detail: format!("broadcast failed: {e:?}"),
            })
            .await;
        }
    }
}

async fn dial(node: &mut Core, ui: &UiHandle, peer_id_str: String, addr: String) {
    let peer_id: PeerId = match peer_id_str.parse() {
        Ok(p) => p,
        Err(e) => {
            ui.push(MeshEvent::Custom {
                label: "DIAL".to_string(),
                detail: format!("invalid peer_id {peer_id_str:?}: {e}"),
            })
            .await;
            return;
        }
    };
    ui.push(MeshEvent::Custom {
        label: "DIAL".to_string(),
        detail: format!("dialing {peer_id_str} @ {addr}"),
    })
    .await;
    match node
        .query_network(AppData::DailPeer(peer_id, addr.clone()))
        .await
    {
        Ok(_) => {
            tracing::info!(peer = %peer_id_str, addr = %addr, "dial ok");
            ui.push(MeshEvent::Custom {
                label: "DIAL".to_string(),
                detail: format!("dial ok: {peer_id_str}"),
            })
            .await;
        }
        Err(e) => {
            tracing::warn!(?e, "dial failed");
            ui.push(MeshEvent::Custom {
                label: "DIAL".to_string(),
                detail: format!("dial failed: {e:?}"),
            })
            .await;
        }
    }
}
