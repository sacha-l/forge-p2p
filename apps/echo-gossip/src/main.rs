use anyhow::Result;
use clap::Parser;
use swarm_nl::core::{CoreBuilder, NetworkEvent};
use swarm_nl::setup::BootstrapConfig;

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

    println!("Node started successfully.");

    Ok(())
}
