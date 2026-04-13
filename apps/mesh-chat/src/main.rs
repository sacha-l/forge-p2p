mod chat;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use clap::{Parser, ValueEnum};
use forge_ui::{ForgeUI, MeshEvent, UiHandle};
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
    /// Can also be supplied later from the browser panel.
    #[arg(long)]
    bootnode_peer_id: Option<String>,

    /// Optional: multiaddr of the bootnode, e.g. /ip4/127.0.0.1/tcp/50000.
    #[arg(long)]
    bootnode_addr: Option<String>,
}

/// Commands dispatched from the HTTP route handlers into the main event loop.
#[derive(Debug)]
enum Command {
    Send(String),
    Dial { peer_id: String, addr: String },
}

#[derive(Deserialize)]
struct SendReq {
    text: String,
}

#[derive(Deserialize)]
struct DialReq {
    peer_id: String,
    addr: String,
}

struct AppState {
    tx: mpsc::Sender<Command>,
}

async fn send_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SendReq>,
) -> StatusCode {
    let text = req.text.trim().to_string();
    if text.is_empty() {
        return StatusCode::BAD_REQUEST;
    }
    match state.tx.send(Command::Send(text)).await {
        Ok(()) => StatusCode::ACCEPTED,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}

async fn dial_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<DialReq>,
) -> Response {
    let peer_id = req.peer_id.trim().to_string();
    let addr = req.addr.trim().to_string();
    if peer_id.is_empty() || addr.is_empty() {
        return (StatusCode::BAD_REQUEST, "peer_id and addr are required").into_response();
    }
    // Validate the peer_id parses early so the browser gets a clear 400
    // rather than a silent dispatch that later fails in the event loop.
    if peer_id.parse::<PeerId>().is_err() {
        return (StatusCode::BAD_REQUEST, "invalid peer_id").into_response();
    }
    match state.tx.send(Command::Dial { peer_id, addr }).await {
        Ok(()) => StatusCode::ACCEPTED.into_response(),
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
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

    // Optional CLI-provided bootnode. Browser-driven dialing covers the
    // case where no CLI bootnode is supplied.
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
    if cli.bootnode_peer_id.is_none() {
        println!(
            "Tip:     no bootnode configured. Either start the other peer and paste its\n           PeerId + multiaddr into the UI 'Connect to peer' form, or pass\n           --bootnode-peer-id <PID> --bootnode-addr /ip4/127.0.0.1/tcp/<port>"
        );
    }

    // Channel fed by the HTTP route handlers, drained by the main event loop.
    let (tx, mut rx) = mpsc::channel::<Command>(64);
    let state = Arc::new(AppState { tx });
    let routes = Router::new()
        .route("/api/chat/send", post(send_handler))
        .route("/api/peer/dial", post(dial_handler))
        .with_state(state);

    let static_dir = format!("{}/static", env!("CARGO_MANIFEST_DIR"));
    let ui = ForgeUI::new()
        .with_port(cli.peer.ui_port())
        .with_app_name(&format!("mesh-chat :: {name}"))
        .with_app_static_dir(&static_dir)
        .with_routes(routes)
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
            Some(cmd) = rx.recv() => {
                match cmd {
                    Command::Send(text) => broadcast(&mut node, &ui, &name, text).await,
                    Command::Dial { peer_id, addr } => dial(&mut node, &ui, peer_id, addr).await,
                }
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
