use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use swarm_nl::core::{CoreBuilder, NetworkEvent};
use swarm_nl::setup::BootstrapConfig;

/// Sovereign Notes — peer-to-peer note syncing across your devices.
#[derive(Parser)]
#[command(name = "sovereign-notes")]
struct Cli {
    /// TCP port for the node
    #[arg(long, default_value_t = 51000)]
    tcp_port: u16,

    /// UDP port for the node
    #[arg(long, default_value_t = 51001)]
    udp_port: u16,

    /// Directory for note storage
    #[arg(long, default_value = "./notes-data")]
    data_dir: PathBuf,

    /// PeerId of a bootnode to connect to
    #[arg(long)]
    boot_peer_id: Option<String>,

    /// Multiaddr of the bootnode (e.g. /ip4/127.0.0.1/tcp/51000)
    #[arg(long)]
    boot_addr: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new note
    New {
        /// Title of the note
        title: String,
    },
    /// Edit an existing note
    Edit {
        /// Note ID (UUID)
        id: String,
    },
    /// List all notes
    Ls,
    /// Read a note
    Read {
        /// Note ID (UUID)
        id: String,
    },
    /// Sync notes from connected peers
    Sync,
    /// Show network and sync status
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Build bootstrap config with optional bootnode
    let mut config = BootstrapConfig::new()
        .with_tcp(cli.tcp_port)
        .with_udp(cli.udp_port);

    if let (Some(peer_id), Some(addr)) = (&cli.boot_peer_id, &cli.boot_addr) {
        let mut bootnodes = HashMap::new();
        bootnodes.insert(peer_id.clone(), addr.clone());
        config = config.with_bootnodes(bootnodes);
    }

    let mut node = CoreBuilder::with_config(config)
        .build()
        .await
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    // Consume setup events — print PeerId and listen addresses
    while let Some(event) = node.next_event().await {
        match event {
            NetworkEvent::NewListenAddr {
                local_peer_id,
                address,
                ..
            } => {
                println!("PeerId: {local_peer_id}");
                println!("Listening on: {address}");
            }
            _ => break,
        }
    }

    // Dispatch to subcommand
    match cli.command {
        Command::New { title } => {
            println!("new: not implemented (title: {title})");
        }
        Command::Edit { id } => {
            println!("edit: not implemented (id: {id})");
        }
        Command::Ls => {
            println!("ls: not implemented");
        }
        Command::Read { id } => {
            println!("read: not implemented (id: {id})");
        }
        Command::Sync => {
            println!("sync: not implemented");
        }
        Command::Status => {
            println!("status: not implemented");
        }
    }

    Ok(())
}
