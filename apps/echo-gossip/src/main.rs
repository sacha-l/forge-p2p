use anyhow::Result;
use clap::Parser;
use swarm_nl::core::{AppData, AppResponse, CoreBuilder, NetworkEvent};
use swarm_nl::setup::BootstrapConfig;

const GOSSIP_TOPIC: &str = "echo-network";
const ECHO_PREFIX: &str = "echo: ";

/// Echo Gossip — peers echo back whatever they receive on a gossip topic.
#[derive(Parser)]
#[command(name = "echo-gossip")]
struct Cli {
    /// TCP port for the node
    #[arg(long, default_value_t = 50000)]
    tcp_port: u16,

    /// UDP port for the node
    #[arg(long, default_value_t = 50001)]
    udp_port: u16,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Configure and build the node
    let config = BootstrapConfig::new()
        .with_tcp(cli.tcp_port)
        .with_udp(cli.udp_port);

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

    // Join the gossip topic
    let join_request = AppData::GossipsubJoinNetwork(GOSSIP_TOPIC.to_string());
    match node.query_network(join_request).await {
        Ok(AppResponse::GossipsubJoinSuccess) => {
            println!("Joined gossip topic: {GOSSIP_TOPIC}");
        }
        Ok(other) => {
            anyhow::bail!("Unexpected response joining topic: {other:?}");
        }
        Err(e) => {
            anyhow::bail!("Failed to join gossip topic: {e:?}");
        }
    }

    // Broadcast a greeting
    let greeting = format!("hello from {}", cli.tcp_port);
    let broadcast = AppData::GossipsubBroadcastMessage {
        topic: GOSSIP_TOPIC.to_string(),
        message: vec![greeting.as_bytes().to_vec()],
    };
    match node.query_network(broadcast).await {
        Ok(AppResponse::GossipsubBroadcastSuccess) => {
            println!("Broadcast greeting: {greeting}");
        }
        other => {
            println!("Greeting broadcast result: {other:?}");
        }
    }

    // Event loop — listen for incoming gossip messages and echo them back
    println!("Listening for messages on '{GOSSIP_TOPIC}'...");
    loop {
        if let Some(event) = node.next_event().await {
            match event {
                NetworkEvent::GossipsubIncomingMessageHandled { source, data } => {
                    for msg in &data {
                        println!("[{source}] {msg}");

                        // Echo back messages that don't already have the echo prefix
                        if !msg.starts_with(ECHO_PREFIX) {
                            let echo_msg = format!("{ECHO_PREFIX}{msg}");
                            let echo_broadcast = AppData::GossipsubBroadcastMessage {
                                topic: GOSSIP_TOPIC.to_string(),
                                message: vec![echo_msg.as_bytes().to_vec()],
                            };
                            if let Err(e) = node.query_network(echo_broadcast).await {
                                println!("Failed to echo: {e:?}");
                            }
                        }
                    }
                }
                NetworkEvent::GossipsubSubscribeMessageReceived { peer_id, topic } => {
                    println!("Peer {peer_id} joined topic '{topic}'");
                }
                NetworkEvent::GossipsubUnsubscribeMessageReceived { peer_id, topic } => {
                    println!("Peer {peer_id} left topic '{topic}'");
                }
                _ => {}
            }
        }
    }
}
