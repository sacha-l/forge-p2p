//! Thin wrapper around SwarmNL's `CoreBuilder` with replication + sharding,
//! plus helpers for draining setup events and building deterministic bootnode
//! entries.
//!
//! M0 surface: just enough to boot a node, drain `NewListenAddr`, and exit.
//! M1 adds replication, sharding, and RPC registration.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use swarm_nl::core::replication::{ConsensusModel, ConsistencyModel, ReplNetworkConfig};
use swarm_nl::core::sharding::{ShardStorage, Sharding};
use swarm_nl::core::{ByteVector, Core, CoreBuilder, NetworkEvent};
use swarm_nl::setup::BootstrapConfig;
use tokio::sync::Mutex;

use crate::keyspace;

/// Build a standard Unleash node: replication network for survivors,
/// sharding for floor zones.
pub async fn build_node(
    tcp_port: u16,
    udp_port: u16,
    bootstrap: Option<&str>,
) -> Result<Core> {
    let mut cfg = BootstrapConfig::new().with_tcp(tcp_port).with_udp(udp_port);

    if let Some(bootstrap) = bootstrap {
        if let Some((peer_id, addr)) = parse_bootstrap(bootstrap) {
            let mut boot = HashMap::new();
            boot.insert(peer_id, addr);
            cfg = cfg.with_bootnodes(boot);
        }
    }

    let repl = ReplNetworkConfig::Custom {
        queue_length: 256,
        expiry_time: Some(120),
        sync_wait_time: 5,
        consistency_model: ConsistencyModel::Strong(ConsensusModel::MinPeers(1)),
        data_aging_period: 2,
    };

    let storage = Arc::new(Mutex::new(FloorStorage));

    let node = CoreBuilder::with_config(cfg)
        .with_replication(repl)
        .with_sharding(keyspace::SHARD_NETWORK.to_string(), storage)
        .build()
        .await
        .map_err(|e| anyhow::anyhow!("CoreBuilder::build failed: {e:?}"))?;
    Ok(node)
}

/// Parse a bootstrap spec like `12D3KooW...@/ip4/127.0.0.1/tcp/53000`.
pub fn parse_bootstrap(s: &str) -> Option<(String, String)> {
    let (peer_id, addr) = s.split_once('@')?;
    Some((peer_id.to_string(), addr.to_string()))
}

/// Format a bootstrap spec.
pub fn format_bootstrap(peer_id: &str, addr: &str) -> String {
    format!("{peer_id}@{addr}")
}

/// Drain `NewListenAddr` events from the node and return
/// `(peer_id_string, listen_addrs)`.
///
/// `next_event()` is non-blocking (library-feedback #4), so we poll with a
/// short sleep. We collect events for up to `total_ms` total — the first
/// `NewListenAddr` sets `peer_id`; subsequent ones add listen addresses.
/// Other events encountered during this window are discarded (they're
/// internal chatter before the app has joined any topic).
pub async fn drain_listen_addrs(node: &mut Core) -> (String, Vec<String>) {
    let total_ms = 1_500u64;
    let tick_ms = 50u64;
    let ticks = total_ms / tick_ms;
    let mut peer_id = String::new();
    let mut addrs: Vec<String> = Vec::new();
    for _ in 0..ticks {
        while let Some(event) = node.next_event().await {
            if let NetworkEvent::NewListenAddr {
                local_peer_id,
                address,
                ..
            } = event
            {
                peer_id = local_peer_id.to_string();
                addrs.push(address.to_string());
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(tick_ms)).await;
        if !peer_id.is_empty() && !addrs.is_empty() {
            // We have enough to proceed. Remaining discard window is short.
        }
    }
    (peer_id, addrs)
}

/// Compute the TCP port for a robot node: `53000 + node_index * 100`.
/// Observer uses `53900`. Tests use base `49300` (see `tests/`).
pub fn robot_tcp_port(node_index: u32) -> u16 {
    53000 + (node_index as u16 * 100)
}

pub fn robot_udp_port(node_index: u32) -> u16 {
    robot_tcp_port(node_index) + 1
}

/// Local-loopback multiaddr suitable for bootnodes on the same host.
pub fn loopback_multiaddr(tcp_port: u16) -> String {
    format!("/ip4/127.0.0.1/tcp/{tcp_port}")
}

/// Sharding by `floor_<n>` — hash determines shard. One shard per floor.
/// Shard IDs must be stable across processes so all robots agree.
pub struct FloorSharding {
    pub floors: u8,
}

impl Sharding for FloorSharding {
    type Key = str;
    type ShardId = String;

    fn locate_shard(&self, key: &Self::Key) -> Option<Self::ShardId> {
        // Keys are of the form `floor_<n>/…`. Everything else maps to floor_0.
        if let Some(rest) = key.strip_prefix("floor_") {
            if let Some(floor_str) = rest.split('/').next() {
                if let Ok(f) = floor_str.parse::<u8>() {
                    if f < self.floors {
                        return Some(format!("floor_{f}"));
                    }
                }
            }
        }
        Some("floor_0".to_string())
    }
}

/// Minimal `ShardStorage` — data is already held in-process in the coord layer
/// so the shard storage is a no-op. `fetch_data` returns empty when asked,
/// which suppresses cross-shard fetches (all shards are independent domains
/// in our model).
#[derive(Debug)]
pub struct FloorStorage;

impl ShardStorage for FloorStorage {
    fn fetch_data(&mut self, _key: ByteVector) -> ByteVector {
        Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bootstrap_roundtrip() {
        let s = format_bootstrap("12D3KooWabc", "/ip4/127.0.0.1/tcp/53000");
        let (p, a) = parse_bootstrap(&s).unwrap();
        assert_eq!(p, "12D3KooWabc");
        assert_eq!(a, "/ip4/127.0.0.1/tcp/53000");
    }

    #[test]
    fn port_allocation_monotonic() {
        assert_eq!(robot_tcp_port(0), 53000);
        assert_eq!(robot_tcp_port(1), 53100);
        assert_eq!(robot_tcp_port(9), 53900);
    }

    #[test]
    fn sharding_locates_floor_keys() {
        let s = FloorSharding { floors: 4 };
        assert_eq!(s.locate_shard("floor_2/chunk_a"), Some("floor_2".into()));
        assert_eq!(s.locate_shard("floor_0/chunk_x"), Some("floor_0".into()));
        // Out-of-range or non-floor keys default to floor_0
        assert_eq!(s.locate_shard("floor_9/chunk_a"), Some("floor_0".into()));
        assert_eq!(s.locate_shard("random"), Some("floor_0".into()));
    }
}
