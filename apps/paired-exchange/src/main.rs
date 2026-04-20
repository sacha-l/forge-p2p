use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use clap::{Parser, ValueEnum};
use forge_ui::{DialRequest, ForgeUI, MeshEvent, UiHandle};
use paired_exchange::config::Config;
use paired_exchange::datagate::{new_rtt_log, spawn_ping_task, RttLog};
use paired_exchange::handshake::{
    handler_ctx, initiate_handshake, install_handler_ctx, rpc_handler, HandlerCtx,
};
use paired_exchange::pairing::{PairState, PairingBook};
use serde::Serialize;
use swarm_nl::core::{AppData, Core, CoreBuilder, NetworkEvent, RpcConfig};
use swarm_nl::setup::BootstrapConfig;
use swarm_nl::PeerId;
use tokio::sync::mpsc;

#[derive(Serialize)]
struct PeerView {
    peer_id: String,
    state: &'static str,
    since_ms: Option<u128>,
    reason: Option<String>,
}

#[derive(Serialize)]
struct RttView {
    peer_id: String,
    seq: u64,
    rtt_ms: u128,
}

#[derive(Serialize)]
struct StateView {
    role: &'static str,
    peers: Vec<PeerView>,
    rtts: Vec<RttView>,
}

struct AppState {
    role_label: &'static str,
    book: PairingBook,
    rtt_log: RttLog,
}

async fn state_handler(State(state): State<std::sync::Arc<AppState>>) -> Json<StateView> {
    let peers = state
        .book
        .snapshot()
        .into_iter()
        .map(|(peer_id, st)| {
            let (label, since_ms, reason) = match st {
                PairState::Unknown => ("unknown", None, None),
                PairState::AwaitingResponse { started_at, .. } => {
                    ("awaiting", Some(started_at.elapsed().as_millis()), None)
                }
                PairState::Trusted { since } => {
                    ("trusted", Some(since.elapsed().as_millis()), None)
                }
                PairState::Failed { reason } => ("failed", None, Some(reason.to_string())),
            };
            PeerView {
                peer_id: peer_id.to_string(),
                state: label,
                since_ms,
                reason,
            }
        })
        .collect();

    let rtts = {
        let log = state
            .rtt_log
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        log.iter()
            .rev()
            .take(20)
            .rev()
            .map(|(peer_id, seq, rtt)| RttView {
                peer_id: peer_id.to_string(),
                seq: *seq,
                rtt_ms: rtt.as_millis(),
            })
            .collect()
    };

    Json(StateView {
        role: state.role_label,
        peers,
        rtts,
    })
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum Role {
    A,
    B,
}

impl Role {
    fn label(self) -> &'static str {
        match self {
            Role::A => "A",
            Role::B => "B",
        }
    }

    fn tcp_port(self) -> u16 {
        match self {
            Role::A => 53000,
            Role::B => 53100,
        }
    }

    fn udp_port(self) -> u16 {
        self.tcp_port() + 1
    }

    fn ui_port(self) -> u16 {
        match self {
            Role::A => 8080,
            Role::B => 8081,
        }
    }
}

#[derive(Parser, Debug)]
#[command(
    name = "paired-exchange",
    about = "Two-node authorized data exchange gated by pre-shared-secret pairing"
)]
struct Cli {
    /// Which role this process plays (A and B pair).
    #[arg(long)]
    role: Role,

    /// 32-byte shared secret, hex-encoded (64 hex chars).
    /// Falls back to the SECRET environment variable if the flag is absent.
    #[arg(long, env = "SECRET")]
    secret: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "paired_exchange=info,forge_ui=info".into()),
        )
        .init();

    let cli = Cli::parse();
    let role = cli.role;
    let config =
        Config::from_hex(&cli.secret).context("failed to parse --secret / SECRET env var")?;

    let book = PairingBook::new();
    let ctx = Arc::new(HandlerCtx::new(config.secret, book.clone()));
    install_handler_ctx(ctx.clone()).map_err(|e| anyhow!("install handler ctx: {e}"))?;
    let rtt_log: RttLog = new_rtt_log();

    let bootnodes: HashMap<String, String> = HashMap::new();
    let swarm_config = BootstrapConfig::new()
        .with_tcp(role.tcp_port())
        .with_udp(role.udp_port())
        .with_bootnodes(bootnodes);

    let mut node = CoreBuilder::with_config(swarm_config)
        .with_rpc(RpcConfig::Default, rpc_handler)
        .build()
        .await
        .map_err(|e| anyhow!("failed to build swarm-nl core: {e:?}"))?;

    // Drain the initial NewListenAddr burst.
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

    println!("=== paired-exchange :: role {} ===", role.label());
    println!("PeerId:  {peer_id}");
    for a in &listen_addrs {
        println!("Listen:  {a}");
    }
    println!("UI:      http://127.0.0.1:{}", role.ui_port());
    println!(
        "Tip:     start role A first, then role B; forge-ui auto-discovers on localhost and dials."
    );

    let (dial_tx, mut dial_rx) = mpsc::channel::<DialRequest>(64);

    let app_state = std::sync::Arc::new(AppState {
        role_label: role.label(),
        book: book.clone(),
        rtt_log: rtt_log.clone(),
    });
    let routes = Router::new()
        .route("/api/paired/state", get(state_handler))
        .with_state(app_state);

    let static_dir = format!("{}/static", env!("CARGO_MANIFEST_DIR"));
    let ui = ForgeUI::new()
        .with_port(role.ui_port())
        .with_app_name(&format!("paired-exchange :: role {}", role.label()))
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

    // Background sweeper: every 1s, expire AwaitingResponse older than 5s.
    let sweep_book = book.clone();
    let sweep_ui = ui.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        interval.tick().await; // discard immediate first tick
        loop {
            interval.tick().await;
            let swept = sweep_book.sweep_stale(paired_exchange::handshake::HANDSHAKE_TIMEOUT);
            if swept > 0 {
                tracing::warn!(count = swept, "sweeper moved stale handshakes to Failed");
                sweep_ui
                    .push(MeshEvent::Custom {
                        label: "PAIR".to_string(),
                        detail: format!("{swept} handshake(s) timed out"),
                    })
                    .await;
            }
        }
    });

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                println!("shutting down role {}", role.label());
                return Ok(());
            }
            Some(req) = dial_rx.recv() => {
                dial(&mut node, req.peer_id, req.addr).await;
            }
            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                while let Some(event) = node.next_event().await {
                    handle_event(&mut node, event, &ui, &book, &config.secret, &rtt_log).await;
                }
            }
        }
    }
}

async fn handle_event(
    node: &mut Core,
    event: NetworkEvent,
    ui: &UiHandle,
    book: &PairingBook,
    secret: &[u8; paired_exchange::config::SECRET_LEN],
    rtt_log: &RttLog,
) {
    match event {
        NetworkEvent::ConnectionEstablished {
            peer_id, endpoint, ..
        } => {
            let addr = format!("{endpoint:?}");
            tracing::info!(peer = %peer_id, addr = %addr, "ConnectionEstablished");
            println!("ConnectionEstablished: {peer_id}");
            ui.push(MeshEvent::PeerConnected {
                peer_id: peer_id.to_string(),
                addr,
            })
            .await;

            // Cache the peer for the sync RPC handler's gate check.
            if let Some(ctx) = handler_ctx() {
                ctx.set_peer(peer_id);
            }

            ui.push(MeshEvent::Custom {
                label: "PAIR".to_string(),
                detail: format!("challenging {peer_id}"),
            })
            .await;

            initiate_handshake(node, book, secret, peer_id).await;

            let trusted = book.is_trusted(&peer_id);
            ui.push(MeshEvent::Custom {
                label: "PAIR".to_string(),
                detail: format!(
                    "{peer_id} → {}",
                    if trusted { "trusted" } else { "failed" }
                ),
            })
            .await;

            // Gate-open side effect: spawn the per-peer ping task only
            // after the handshake lands on Trusted. Sending `DataPing`
            // is gated a second time inside the task (gate #1 in the
            // three-if gate) so it stops the moment trust is revoked.
            if trusted {
                spawn_ping_task(
                    node.clone(),
                    book.clone(),
                    peer_id,
                    ui.clone(),
                    rtt_log.clone(),
                );
            }
        }
        NetworkEvent::ConnectionClosed { peer_id, .. } => {
            tracing::info!(peer = %peer_id, "ConnectionClosed");
            ui.push(MeshEvent::PeerDisconnected {
                peer_id: peer_id.to_string(),
            })
            .await;
        }
        NetworkEvent::NewListenAddr {
            local_peer_id,
            address,
            ..
        } => {
            tracing::debug!(peer = %local_peer_id, addr = %address, "new listen addr (late)");
        }
        other => {
            tracing::debug!(event = ?other, "unhandled event");
        }
    }
}

async fn dial(node: &mut Core, peer_id_str: String, addr: String) {
    let peer_id: PeerId = match peer_id_str.parse() {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(peer = %peer_id_str, error = %e, "dial: invalid peer_id");
            return;
        }
    };
    match node
        .query_network(AppData::DailPeer(peer_id, addr.clone()))
        .await
    {
        Ok(_) => tracing::info!(peer = %peer_id_str, addr = %addr, "dial ok"),
        Err(e) => tracing::warn!(peer = %peer_id_str, error = ?e, "dial failed"),
    }
}
