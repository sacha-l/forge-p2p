use std::collections::HashMap;
use std::time::Duration;

use anyhow::{anyhow, Result};
use clap::{Parser, ValueEnum};
use forge_ui::{DialRequest, ForgeUI, MeshEvent};
use swarm_nl::core::{AppData, Core, CoreBuilder, NetworkEvent};
use swarm_nl::setup::BootstrapConfig;
use swarm_nl::PeerId;
use tokio::sync::mpsc;

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
    /// Parsed but unused in step 1; wired into the handshake from step 2 on.
    #[arg(long, env = "SECRET")]
    secret: Option<String>,
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

    // Programmatic bootstrap config. Explicit 127.0.0.1 addresses for local
    // dials (library-feedback: don't trust NewListenAddr's reported address).
    let bootnodes: HashMap<String, String> = HashMap::new();
    let config = BootstrapConfig::new()
        .with_tcp(role.tcp_port())
        .with_udp(role.udp_port())
        .with_bootnodes(bootnodes);

    let mut node = CoreBuilder::with_config(config)
        .build()
        .await
        .map_err(|e| anyhow!("failed to build swarm-nl core: {e:?}"))?;

    // Drain the initial NewListenAddr burst so we can show PeerId + addrs.
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

    // Parsed secret is not used yet in step 1 but we touch it so rustc does
    // not warn about the unused CLI field during this early step.
    if cli.secret.is_some() {
        tracing::debug!("secret flag supplied (not yet used)");
    }

    let (dial_tx, mut dial_rx) = mpsc::channel::<DialRequest>(64);

    let static_dir = format!("{}/static", env!("CARGO_MANIFEST_DIR"));
    let ui = ForgeUI::new()
        .with_port(role.ui_port())
        .with_app_name(&format!("paired-exchange :: role {}", role.label()))
        .with_app_static_dir(&static_dir)
        .with_local_peer_id(&peer_id)
        .with_dial_sender(dial_tx)
        .start()
        .await?;

    ui.push(MeshEvent::NodeStarted {
        peer_id: peer_id.clone(),
        listen_addrs: listen_addrs.clone(),
    })
    .await;

    // Main event loop. Step 1 only prints ConnectionEstablished; later steps
    // layer the handshake driver and data gate on top of the same select!.
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
                    handle_event(event, &ui).await;
                }
            }
        }
    }
}

async fn handle_event(event: NetworkEvent, ui: &forge_ui::UiHandle) {
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
