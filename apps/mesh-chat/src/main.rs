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
use forge_ui::{ForgeUI, MeshEvent};
use serde::Deserialize;
use swarm_nl::core::{AppData, CoreBuilder, NetworkEvent};
use swarm_nl::setup::BootstrapConfig;
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
            PeerName::Bobby => 50100,
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

    /// PeerId of the bootnode to dial (typically Al's PeerId, for Bobby).
    #[arg(long)]
    bootnode_peer_id: Option<String>,

    /// Multiaddr of the bootnode, e.g. /ip4/127.0.0.1/tcp/50000.
    #[arg(long, default_value = "/ip4/127.0.0.1/tcp/50000")]
    bootnode_addr: String,
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

    // Build bootnodes map (empty for Al, populated for Bobby).
    // NOTE: `with_bootnodes` takes HashMap<String, String> despite the reference
    // doc's claim of HashMap<PeerId, String>. See library-feedback.md.
    let mut bootnodes: HashMap<String, String> = HashMap::new();
    if let Some(pid_str) = cli.bootnode_peer_id.as_deref() {
        bootnodes.insert(pid_str.to_string(), cli.bootnode_addr.clone());
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
            "Tip:     to start Bobby, run: cargo run -- --peer bobby \\\n           --bootnode-peer-id {peer_id} \\\n           --bootnode-addr /ip4/127.0.0.1/tcp/{}",
            cli.peer.tcp_port()
        );
    }

    // Channel fed by the HTTP POST handler, drained by the main event loop.
    let (tx, mut rx) = mpsc::channel::<String>(64);
    let state = Arc::new(AppState { tx });
    let routes = Router::new()
        .route("/api/chat/send", post(send_handler))
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
            Some(text) = rx.recv() => {
                let line = ChatLine { from: name.clone(), text };
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
            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                while let Some(event) = node.next_event().await {
                    handle_event(event, &ui).await;
                }
            }
        }
    }
}
