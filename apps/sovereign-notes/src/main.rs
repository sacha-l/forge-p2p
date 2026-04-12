use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use swarm_nl::core::{CoreBuilder, NetworkEvent};
use swarm_nl::setup::BootstrapConfig;

mod store;

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
    /// Edit an existing note's content
    Edit {
        /// Note ID (UUID)
        id: String,
        /// New content for the note
        content: String,
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

    // Initialize note store
    let note_store = store::NoteStore::new(&cli.data_dir)?;

    // Dispatch to subcommand
    match cli.command {
        Command::New { title } => {
            let note = note_store.create(&title)?;
            println!("Created note: {} ({})", note.title, note.id);
        }
        Command::Edit { id, content } => {
            let note = note_store.update(&id, &content)?;
            println!("Updated note: {} (v{})", note.title, note.version);
        }
        Command::Ls => {
            let notes = note_store.list()?;
            if notes.is_empty() {
                println!("No notes yet. Create one with: sovereign-notes new \"My Note\"");
            } else {
                println!("{:<38} {:<30} {:<5} UPDATED", "ID", "TITLE", "VER");
                for meta in &notes {
                    println!(
                        "{:<38} {:<30} {:<5} {}",
                        meta.id,
                        meta.title,
                        meta.version,
                        meta.updated_at.format("%Y-%m-%d %H:%M")
                    );
                }
                println!("\n{} note(s)", notes.len());
            }
        }
        Command::Read { id } => {
            let note = note_store.get(&id)?;
            println!("--- {} (v{}) ---", note.title, note.version);
            println!("{}", note.content);
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
