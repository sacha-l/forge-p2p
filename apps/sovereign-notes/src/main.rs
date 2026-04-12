use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use clap::{Parser, Subcommand};

mod net;
mod rpc;
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
    /// Start web UI and long-running event loop
    Serve {
        /// Port for the web UI
        #[arg(long, default_value_t = 8080)]
        ui_port: u16,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize RPC data directory (must happen before node build)
    rpc::init_data_dir(cli.data_dir.clone());

    // Build node with replication and RPC
    let mut node = net::build_node(
        cli.tcp_port,
        cli.udp_port,
        cli.boot_peer_id.as_deref(),
        cli.boot_addr.as_deref(),
    )
    .await?;

    // Drain setup events
    let peer_id = net::drain_setup_events(&mut node).await;

    // Join replication network and gossip topic
    net::join_repl_network(&mut node).await?;
    net::join_gossip(&mut node).await?;

    // Initialize note store
    let note_store = store::NoteStore::new(&cli.data_dir)?;

    // Dispatch to subcommand
    match cli.command {
        Command::New { title } => {
            let note = note_store.create(&title)?;
            println!("Created note: {} ({})", note.title, note.id);

            // Replicate metadata and announce via gossip
            net::replicate_note_meta(
                &mut node,
                &note.id,
                &note.title,
                note.version,
                &note.updated_at.to_rfc3339(),
            )
            .await?;
            net::announce_change(&mut node, &note.id, &note.title, note.version).await?;
        }
        Command::Edit { id, content } => {
            let note = note_store.update(&id, &content)?;
            println!("Updated note: {} (v{})", note.title, note.version);

            // Replicate updated metadata and announce via gossip
            net::replicate_note_meta(
                &mut node,
                &note.id,
                &note.title,
                note.version,
                &note.updated_at.to_rfc3339(),
            )
            .await?;
            net::announce_change(&mut node, &note.id, &note.title, note.version).await?;
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
            let remote_index = net::new_remote_index();

            // Poll for gossip announcements for a few seconds to learn about remote state
            println!("Listening for peer announcements...");
            let poll_deadline =
                tokio::time::Instant::now() + Duration::from_secs(5);
            while tokio::time::Instant::now() < poll_deadline {
                if let Some(
                    swarm_nl::core::NetworkEvent::GossipsubIncomingMessageHandled {
                        source,
                        data,
                    },
                ) = node.next_event().await
                {
                    net::handle_gossip_message(&data, &source.to_string(), &remote_index);
                }
                // Also consume replication buffer data
                if let Some(repl_data) = node.consume_repl_data(net::REPL_NETWORK).await {
                    // Parse replicated note metadata: [id, title, version, updated_at]
                    if repl_data.data.len() >= 3 {
                        let note_id = &repl_data.data[0];
                        let title = &repl_data.data[1];
                        if let Ok(version) = repl_data.data[2].parse::<u64>() {
                            let mut index =
                                remote_index.lock().expect("remote index lock poisoned");
                            let entry = index
                                .entry(note_id.clone())
                                .or_insert((String::new(), 0, String::new()));
                            if version > entry.1 {
                                *entry = (
                                    title.clone(),
                                    version,
                                    repl_data.sender.to_string(),
                                );
                            }
                        }
                    }
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }

            // Diff local vs remote
            let local_notes = note_store.list()?;
            let local_versions: std::collections::HashMap<String, u64> = local_notes
                .iter()
                .map(|m| (m.id.clone(), m.version))
                .collect();

            // Collect remote state and release the lock before async work
            let to_fetch: Vec<(String, String, u64, String)> = {
                let remote = remote_index.lock().expect("remote index lock poisoned");
                remote
                    .iter()
                    .filter_map(|(note_id, (title, remote_ver, peer_str))| {
                        let local_ver = local_versions.get(note_id).copied().unwrap_or(0);
                        if *remote_ver > local_ver {
                            Some((
                                note_id.clone(),
                                title.clone(),
                                *remote_ver,
                                peer_str.clone(),
                            ))
                        } else {
                            None
                        }
                    })
                    .collect()
            };

            let remote_count = remote_index.lock().expect("lock").len();
            let up_to_date = remote_count.saturating_sub(to_fetch.len());
            let mut pulled = 0u32;

            for (note_id, title, _remote_ver, peer_str) in &to_fetch {
                match peer_str.parse::<swarm_nl::PeerId>() {
                    Ok(peer_id) => {
                        match net::fetch_note_via_rpc(&mut node, &peer_id, note_id).await {
                            Ok(note) => {
                                note_store.save(&note)?;
                                println!(
                                    "Pulled: {} '{}' v{}",
                                    note.id, note.title, note.version
                                );
                                pulled += 1;
                            }
                            Err(e) => {
                                println!("Failed to fetch {note_id}: {e}");
                            }
                        }
                    }
                    Err(e) => {
                        println!("Cannot parse peer ID for '{title}': {e}");
                    }
                }
            }

            if pulled == 0 && up_to_date == 0 && remote_count == 0 {
                println!("No remote notes discovered. Is another device connected?");
            } else {
                println!("\nSync complete: {pulled} pulled, {up_to_date} up to date");
            }
        }
        Command::Status => {
            // Get network info
            let net_info = node
                .query_network(swarm_nl::core::AppData::GetNetworkInfo)
                .await;
            match net_info {
                Ok(swarm_nl::core::AppResponse::GetNetworkInfo {
                    peer_id,
                    connected_peers,
                    external_addresses,
                }) => {
                    println!("=== Sovereign Notes Status ===");
                    println!("PeerId: {peer_id}");
                    println!("Connected peers: {}", connected_peers.len());
                    for peer in &connected_peers {
                        println!("  - {peer}");
                    }
                    if !external_addresses.is_empty() {
                        println!("External addresses:");
                        for addr in &external_addresses {
                            println!("  - {addr}");
                        }
                    }
                }
                _ => {
                    println!("Could not retrieve network info");
                }
            }

            // Get gossip info
            let gossip_info = node
                .query_network(swarm_nl::core::AppData::GossipsubGetInfo)
                .await;
            if let Ok(swarm_nl::core::AppResponse::GossipsubGetInfo {
                topics,
                mesh_peers,
                ..
            }) = gossip_info
            {
                println!("Subscribed topics: {}", topics.join(", "));
                println!("Mesh peers: {}", mesh_peers.len());
            }

            // Local note count
            let notes = note_store.list()?;
            println!("Local notes: {}", notes.len());
        }
        Command::Serve { ui_port } => {
            run_serve(node, note_store, peer_id, ui_port).await?;
        }
    }

    Ok(())
}

/// Run the web UI server with a long-running event loop.
async fn run_serve(
    mut node: swarm_nl::core::Core,
    note_store: store::NoteStore,
    peer_id: String,
    ui_port: u16,
) -> Result<()> {
    use axum::{extract::Path, routing, Json, Router};
    use forge_ui::{ForgeUI, MeshEvent};

    let store = Arc::new(note_store);

    // Build app-specific API routes
    let api_store = Arc::clone(&store);
    let api_routes = Router::new()
        .route("/api/notes", routing::get({
            let s = Arc::clone(&api_store);
            move || {
                let s = Arc::clone(&s);
                async move {
                    match s.list() {
                        Ok(notes) => {
                            let items: Vec<serde_json::Value> = notes
                                .iter()
                                .map(|m| serde_json::json!({
                                    "id": m.id,
                                    "title": m.title,
                                    "version": m.version,
                                    "updated_at": m.updated_at.to_rfc3339(),
                                }))
                                .collect();
                            Json(serde_json::json!(items))
                        }
                        Err(_) => Json(serde_json::json!([])),
                    }
                }
            }
        }))
        .route("/api/notes", routing::post({
            let s = Arc::clone(&api_store);
            move |Json(body): Json<serde_json::Value>| {
                let s = Arc::clone(&s);
                async move {
                    let title = body.get("title").and_then(|v| v.as_str()).unwrap_or("Untitled");
                    match s.create(title) {
                        Ok(note) => Json(serde_json::json!({
                            "id": note.id,
                            "title": note.title,
                            "version": note.version,
                            "content": note.content,
                            "updated_at": note.updated_at.to_rfc3339(),
                        })),
                        Err(e) => Json(serde_json::json!({"error": e.to_string()})),
                    }
                }
            }
        }))
        .route("/api/notes/{id}", routing::get({
            let s = Arc::clone(&api_store);
            move |Path(id): Path<String>| {
                let s = Arc::clone(&s);
                async move {
                    match s.get(&id) {
                        Ok(note) => Json(serde_json::json!({
                            "id": note.id,
                            "title": note.title,
                            "version": note.version,
                            "content": note.content,
                            "updated_at": note.updated_at.to_rfc3339(),
                        })),
                        Err(e) => Json(serde_json::json!({"error": e.to_string()})),
                    }
                }
            }
        }))
        .route("/api/notes/{id}", routing::put({
            let s = Arc::clone(&api_store);
            move |Path(id): Path<String>, Json(body): Json<serde_json::Value>| {
                let s = Arc::clone(&s);
                async move {
                    let content = body.get("content").and_then(|v| v.as_str()).unwrap_or("");
                    match s.update(&id, content) {
                        Ok(note) => Json(serde_json::json!({
                            "id": note.id,
                            "title": note.title,
                            "version": note.version,
                            "content": note.content,
                            "updated_at": note.updated_at.to_rfc3339(),
                        })),
                        Err(e) => Json(serde_json::json!({"error": e.to_string()})),
                    }
                }
            }
        }));

    // Resolve the static dir relative to CWD
    let static_dir = std::env::current_dir()
        .unwrap_or_default()
        .join("static");

    // Start forge-ui with app routes merged in
    let ui = ForgeUI::new()
        .with_port(ui_port)
        .with_app_name("Sovereign Notes")
        .with_app_static_dir(static_dir.to_str().unwrap_or("./static"))
        .with_routes(api_routes)
        .start()
        .await?;

    println!("Web UI running at http://127.0.0.1:{ui_port}");

    // Push initial NodeStarted event
    ui.push(MeshEvent::NodeStarted {
        peer_id: peer_id.clone(),
        listen_addrs: vec![format!("/ip4/127.0.0.1/tcp/{}", 51000)],
    })
    .await;

    // Long-running event loop
    loop {
        if let Some(event) = node.next_event().await {
            match event {
                swarm_nl::core::NetworkEvent::ConnectionEstablished {
                    peer_id: pid,
                    endpoint,
                    ..
                } => {
                    let addr = endpoint.get_remote_address().to_string();
                    println!("Peer connected: {pid} @ {addr}");
                    ui.push(MeshEvent::PeerConnected {
                        peer_id: pid.to_string(),
                        addr,
                    })
                    .await;
                }
                swarm_nl::core::NetworkEvent::ConnectionClosed {
                    peer_id: pid, ..
                } => {
                    println!("Peer disconnected: {pid}");
                    ui.push(MeshEvent::PeerDisconnected {
                        peer_id: pid.to_string(),
                    })
                    .await;
                }
                swarm_nl::core::NetworkEvent::GossipsubIncomingMessageHandled {
                    source,
                    data,
                } => {
                    let size: usize = data.iter().map(|s| s.len()).sum();
                    ui.push(MeshEvent::MessageReceived {
                        from: source.to_string(),
                        topic: net::GOSSIP_TOPIC.to_string(),
                        size_bytes: size,
                    })
                    .await;
                }
                swarm_nl::core::NetworkEvent::ReplicaDataIncoming {
                    source, network, ..
                } => {
                    ui.push(MeshEvent::ReplicaSync {
                        peer_id: source.to_string(),
                        network,
                        status: "incoming".to_string(),
                    })
                    .await;
                }
                _ => {}
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
