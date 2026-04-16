# SwarmNL Library Reference for Coding Agents

> This document is the authoritative reference for building applications with SwarmNL.
> Read this BEFORE writing any code. Follow the patterns exactly.

## 1. What SwarmNL Is (and Isn't)

SwarmNL is a **networking layer library** built on top of libp2p. It handles:
- Peer discovery and connection (Kademlia DHT, mDNS)
- Gossip-based pub/sub messaging (Gossipsub 1.1)
- Direct peer-to-peer RPC (request-response)
- Data replication across replica networks
- Network sharding with data forwarding

**SwarmNL does NOT handle:**
- Consensus / state machines (no blockchain built in)
- Persistent storage (you bring your own)
- Frontend / UI
- Cryptographic signing of application data
- Smart contracts or on-chain logic

Your application is a **Rust binary** that uses `swarm-nl` as a dependency. The library gives you a `Core` node object. You send requests to it, poll events from it, and build your application logic around those interactions.

## 2. Crate & Dependencies

```toml
[dependencies]
swarm-nl = { version = "0.2.1", features = ["tokio-runtime"] }
# OR for async-std:
# swarm-nl = { version = "0.2.1", features = ["async-std-runtime"] }
tokio = { version = "1", features = ["full"] }
```

SwarmNL re-exports key libp2p types. The correct imports are:
```rust
use swarm_nl::*;
use std::collections::HashMap;
```

> **VERIFIED (echo-gossip build):** `swarm_nl::core::prelude` is private in v0.2.1.
> All public types are re-exported from `swarm_nl` directly.
> Use `use swarm_nl::*;` — do NOT use `use swarm_nl::core::prelude::*;`.

## 3. Core Architecture

### The Build Pattern

Every SwarmNL app follows this sequence:

```
BootstrapConfig → CoreBuilder → .build() → Core
```

The `Core` is your node. It runs the libp2p swarm internally. You interact with it through:

| Method | Purpose | Pattern |
|--------|---------|---------|
| `node.query_network(request)` | Send request, wait for response | `async`, returns `Result<AppResponse>` |
| `node.send_to_network(request)` | Send request, get stream ID | `async`, returns `Result<StreamId>` |
| `node.recv_from_network(stream_id)` | Poll for response by stream ID | `async`, returns `Result<AppResponse>` |
| `node.next_event()` | Get next buffered network event | `async`, returns `Option<NetworkEvent>`, **non-blocking** (returns `None` immediately if buffer empty) |
| `node.events()` | Get iterator over all buffered events | `async`, returns iterator |

> **CRITICAL (verified in echo-gossip build):** `next_event()` does NOT block.
> It returns `None` instantly when the buffer is empty. You MUST add a small
> `tokio::time::sleep` in your event loop to avoid busy-spinning:
> ```rust
> loop {
>     if let Some(event) = node.next_event().await {
>         // handle event
>     }
>     tokio::time::sleep(std::time::Duration::from_millis(100)).await;
> }
> ```

### AppData Enum (Requests)

All requests to the network are variants of `AppData`:

```rust
// === DHT Operations ===
AppData::KademliaStoreRecord {
    key: Vec<u8>,
    value: Vec<u8>,
    expiration_time: Option<Instant>,
    explicit_peers: Option<Vec<PeerId>>,
}

AppData::KademliaLookupRecord {
    key: Vec<u8>,
}

AppData::KademliaDeleteRecord {
    key: Vec<u8>,
}

AppData::KademliaGetProviders {
    key: Vec<u8>,
}

AppData::KademliaStartProviding {
    key: Vec<u8>,
}

AppData::KademliaGetRoutingTableInfo

// === RPC (Request-Response) ===
AppData::SendRpc {
    keys: Vec<Vec<u8>>,
    peer: PeerId,
}

// === Gossipsub ===
AppData::GossipsubBroadcastMessage {
    topic: String,
    message: Vec<Vec<u8>>,     // ByteVector, NOT Vec<String>
}

AppData::GossipsubJoinNetwork {
    topic: String,
}

AppData::GossipsubExitNetwork {
    topic: String,
}

AppData::GossipsubGetInfo
```

### AppResponse Enum (Responses)

```rust
AppResponse::KademliaStoreRecordSuccess
AppResponse::KademliaLookupSuccess(Vec<u8>)       // the value
AppResponse::KademliaDeleteRecordSuccess
AppResponse::KademliaGetProviders { key, providers }
AppResponse::KademliaStartProvidingSuccess
AppResponse::KademliaNoProvidersFound
AppResponse::KademliaRoutingTableInfo { protocol }

AppResponse::SendRpc(Vec<Vec<u8>>)                 // response data

AppResponse::GossipsubBroadcastSuccess
AppResponse::GossipsubJoinSuccess
AppResponse::GossipsubExitSuccess
AppResponse::GossipsubGetInfo { topics, connected_peers }
```

### NetworkEvent Enum (Events)

Events are buffered internally. Poll them with `next_event()` or `events()`.

> **WARNING:** The variant names and field names below were derived from documentation.
> The echo-gossip build found some names differ in actual v0.2.1 source code.
> When in doubt, check `cargo doc --open` on your project to see the actual variants.
> If you find a discrepancy, update this file and log it in decisions.md.

```rust
NetworkEvent::NewListenAddr { local_peer_id, listener_id, address }
NetworkEvent::ConnectionEstablished { peer_id, connection_id, endpoint, num_established, established_in }
NetworkEvent::ConnectionClosed { peer_id, connection_id, endpoint, num_established }
NetworkEvent::ExpiredListenAddr { listener_id, address }
NetworkEvent::ListenerClosed { listener_id, addresses }
NetworkEvent::OutgoingConnectionError { peer_id, connection_id }
NetworkEvent::IncomingConnectionError { local_addr, send_back_addr, connection_id }

// Gossip events
NetworkEvent::GossipsubIncomingMessage { source, data, topic }
NetworkEvent::GossipsubSubscribed { peer_id, topic }
NetworkEvent::GossipsubUnsubscribed { peer_id, topic }

// RPC events
NetworkEvent::RpcIncomingMessage { data, peer }

// Replication events
NetworkEvent::ReplicaDataIncoming { data, network, source }
NetworkEvent::ReplicaNodeJoin { peer_id, network }
NetworkEvent::ReplicaNodeExit { peer_id, network }

// Sharding events
NetworkEvent::IncomingForwardedData { data, source }
```

## 4. Application Patterns

### Pattern A: Basic Node (Echo Server)

The simplest SwarmNL app. Sets up a node and reads events.

```rust
use swarm_nl::*;

#[tokio::main]
async fn main() {
    // 1. Configure
    let config = BootstrapConfig::default();

    // 2. Build node
    let node = CoreBuilder::with_config(config)
        .build()
        .await
        .unwrap();

    // 3. Read setup events (listen addresses, etc.)
    while let Some(event) = node.next_event().await {
        match event {
            NetworkEvent::NewListenAddr { local_peer_id, address, .. } => {
                println!("Peer id: {}", local_peer_id);
                println!("Listening on: {}", address);
            },
            _ => {}
        }
    }

    // 4. Application event loop
    loop {
        if let Some(event) = node.next_event().await {
            // Handle events...
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}
```

### Pattern B: Gossip-Based App (Pub/Sub Messaging)

For apps where peers broadcast messages to topics.

```rust
const GOSSIP_TOPIC: &str = "my_app_network";

#[tokio::main]
async fn main() {
    let config = BootstrapConfig::default();
    let node = CoreBuilder::with_config(config)
        .build()
        .await
        .unwrap();

    // Join gossip network
    let join_request = AppData::GossipsubJoinNetwork {
        topic: GOSSIP_TOPIC.to_string(),
    };
    let _ = node.query_network(join_request).await;

    // Broadcast a message
    let gossip = AppData::GossipsubBroadcastMessage {
        topic: GOSSIP_TOPIC.to_string(),
        message: vec!["hello from peer".as_bytes().to_vec()],  // ByteVector
    };
    let _ = node.query_network(gossip).await;

    // Listen for incoming gossip
    loop {
        if let Some(event) = node.next_event().await {
            if let NetworkEvent::GossipsubIncomingMessage { source, data, topic } = event {
                println!("Got message from {}: {:?} on topic {}", source, data, topic);
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}
```

### Pattern C: RPC-Based App (Request-Response)

For apps needing direct peer-to-peer data exchange.

```rust
// Sending side:
let rpc_request = AppData::SendRpc {
    keys: vec!["request_data".as_bytes().to_vec()],
    peer: target_peer_id,
};

// Option 1: query_network (blocks until response)
if let Ok(AppResponse::SendRpc(response_data)) = node.query_network(rpc_request).await {
    println!("Got response: {:?}", response_data);
}

// Option 2: send_to_network + recv_from_network (non-blocking send)
let stream_id = node.send_to_network(rpc_request).await.unwrap();
// ... do other work ...
if let Ok(AppResponse::SendRpc(response_data)) = node.recv_from_network(stream_id).await {
    println!("Got response: {:?}", response_data);
}
```

### Pattern D: DHT-Based App (Distributed Storage)

For apps that store and retrieve key-value data across the network.

```rust
// Store a record
let store_req = AppData::KademliaStoreRecord {
    key: "my_key".as_bytes().to_vec(),
    value: "my_value".as_bytes().to_vec(),
    expiration_time: None,
    explicit_peers: None,
};
let _ = node.query_network(store_req).await;

// Lookup a record
let lookup_req = AppData::KademliaLookupRecord {
    key: "my_key".as_bytes().to_vec(),
};
if let Ok(AppResponse::KademliaLookupSuccess(value)) = node.query_network(lookup_req).await {
    println!("Found: {}", String::from_utf8_lossy(&value));
}
```

### Pattern E: Replication

For fault-tolerant apps needing data redundancy.

```rust
const REPL_NETWORK_ID: &str = "my_replica_network";

// Configure replication
let repl_config = ReplNetworkConfig::Custom {
    queue_length: 150,
    expiry_time: Some(10),
    sync_wait_time: 5,
    consistency_model: ConsistencyModel::Strong(ConsensusModel::All),
    data_aging_period: 2,
};

// Build node with replication
let node = CoreBuilder::with_config(config)
    .with_replication(repl_config)
    .build()
    .await
    .unwrap();

// Join replica network
node.join_repl_network(REPL_NETWORK_ID.into()).await;

// Replicate data
let payload = vec!["important_data".as_bytes().to_vec()];
node.replicate(payload, REPL_NETWORK_ID).await;

// Consume replicated data
loop {
    if let Some(event) = node.next_event().await {
        if let NetworkEvent::ReplicaDataIncoming { source, .. } = event {
            println!("Replica data from: {}", source.to_base58());
        }
    }
    if let Some(repl_data) = node.consume_repl_data(REPL_NETWORK_ID).await {
        println!("Data: {:?}, confirmations: {:?}", repl_data.data, repl_data.confirmations);
    }
}
```

### Pattern F: Sharding

For scalable apps that partition data across network segments.

```rust
use std::sync::{Arc, Mutex};

const NETWORK_SHARDING_ID: &str = "my_shard_network";

// 1. Implement ShardStorage trait
#[derive(Debug)]
struct MyStorage;

impl ShardStorage for MyStorage {
    fn fetch_data(&mut self, key: ByteVector) -> ByteVector {
        // Read from your storage layer
        // Return the data corresponding to the key
        Default::default()
    }
}

// 2. Implement Sharding trait
struct MySharding;

impl Sharding for MySharding {
    type Key = str;
    type ShardId = String;

    fn locate_shard(&self, key: &Self::Key) -> Option<Self::ShardId> {
        // Your sharding algorithm (hash-based, range-based, etc.)
        let hash = key.as_bytes().iter().fold(0u64, |acc, &b| acc.wrapping_add(b as u64));
        Some(hash.to_string())
    }
}

// 3. Configure node for sharding (requires replication)
let shard_storage = Arc::new(Mutex::new(MyStorage));
let repl_config = ReplNetworkConfig::Custom {
    queue_length: 150,
    expiry_time: Some(10),
    sync_wait_time: 5,
    consistency_model: ConsistencyModel::Eventual,
    data_aging_period: 2,
};

let node = CoreBuilder::with_config(config)
    .with_replication(repl_config)
    .with_sharding(NETWORK_SHARDING_ID.into(), shard_storage)
    .build()
    .await
    .unwrap();

// 4. Join a shard
let shard_exec = MySharding;
let shard_id = shard_exec.locate_shard("my_data_key").unwrap();
shard_exec.join_network(node.clone(), &shard_id).await.unwrap();

// 5. Store data (routes to correct shard automatically)
let payload = vec!["file_data".as_bytes().to_vec()];
shard_exec.shard(node.clone(), "my_data_key", payload).await.unwrap();

// 6. Handle forwarded data and replica events
loop {
    if let Some(event) = node.next_event().await {
        match event {
            NetworkEvent::IncomingForwardedData { data, source } => {
                println!("Forwarded data from {}: {:?}", source.to_base58(), data);
            },
            NetworkEvent::ReplicaDataIncoming { data, network, source, .. } => {
                if let Some(repl_data) = node.consume_repl_data(&network).await {
                    println!("Replica data: {:?}", repl_data.data);
                }
            },
            _ => {}
        }
    }
}
```

## 5. Node Configuration Reference

### BootstrapConfig Methods

```rust
// Default (random keypair, random ports)
BootstrapConfig::default()

// From .ini file
BootstrapConfig::from_file("bootstrap_config.ini")

// Programmatic builder
BootstrapConfig::new()
    .with_bootnodes(bootnode_map)     // HashMap<PeerId, String>
    .with_tcp(1509)                    // TCP port
    .with_udp(2710)                    // UDP port
```

### INI File Format

```ini
[ports]
tcp=3000
udp=4000

[auth]
crypto=Ed25519       ; Ed25519, RSA, Secp256k1, Ecdsa
protoc_prefix=/custom-protocol/1.0

[bootstrap]
boot_nodes=[12D3KooW...:/ip4/192.168.1.1/tcp/3000, ...]
```

### CoreBuilder Chain

```rust
CoreBuilder::with_config(config)
    // Optional: add replication
    .with_replication(repl_config)
    // Optional: add sharding (requires replication)
    .with_sharding(shard_network_id, shard_storage_arc)
    // Build the node
    .build()
    .await
    .unwrap()
```

## 6. Multi-Node Testing Pattern

Most SwarmNL apps involve multiple nodes. The standard testing approach:

```rust
#[tokio::test]
async fn test_two_nodes_communicate() {
    // Node 1
    let config1 = BootstrapConfig::new().with_tcp(49600).with_udp(49601);
    let node1 = CoreBuilder::with_config(config1).build().await.unwrap();

    // Get node1's peer ID and listen address from events
    let mut node1_peer_id = None;
    let mut node1_addr = None;
    while let Some(event) = node1.next_event().await {
        if let NetworkEvent::NewListenAddr { local_peer_id, address, .. } = event {
            node1_peer_id = Some(local_peer_id);
            node1_addr = Some(address.to_string());
        }
    }

    // Node 2 connects to Node 1 as bootnode
    let mut bootnodes = HashMap::new();
    bootnodes.insert(node1_peer_id.unwrap(), node1_addr.unwrap());
    let config2 = BootstrapConfig::new()
        .with_tcp(49700)
        .with_udp(49701)
        .with_bootnodes(bootnodes);
    let node2 = CoreBuilder::with_config(config2).build().await.unwrap();

    // Now node1 and node2 are connected
    // Test your communication logic...
}
```

## 7. Key Types Quick Reference

| Type | Description |
|------|-------------|
| `Core` | The main node handle. Clone-safe (uses Arc internally). |
| `PeerId` | libp2p peer identifier, derived from public key |
| `Multiaddr` | Multi-format network address (e.g. `/ip4/1.2.3.4/tcp/3000`) |
| `AppData` | Enum of all request types you can send to the network |
| `AppResponse` | Enum of all response types you receive |
| `NetworkEvent` | Enum of all events the network generates |
| `StreamId` | Opaque ID returned by `send_to_network` for later polling |
| `BootstrapConfig` | Node startup configuration |
| `CoreBuilder` | Builder for constructing a `Core` node |
| `ReplNetworkConfig` | Replication configuration (queue size, consistency, etc.) |
| `ConsistencyModel` | `Eventual` or `Strong(ConsensusModel)` |
| `ConsensusModel` | `All` or `MinPeers(u64)` |
| `ReplBufferData` | Data item in replication buffer (has data, lamport_clock, confirmations, etc.) |
| `ByteVector` | `Vec<Vec<u8>>` - the standard data container |
| `StringVector` | `Vec<String>` - string data container |

## 8. Common Mistakes

> Items marked **VERIFIED** were discovered during actual app builds.

1. **Not consuming events**: The internal event buffer has a max size. If you don't poll events, you lose them. Always run an event loop.

2. **Port conflicts in tests**: When running multiple nodes, use different TCP/UDP ports for each node.

3. **Forgetting bootnodes**: Nodes won't discover each other without either bootnodes or mDNS. For local testing, connect node2 to node1 via bootnodes.

4. **Sharding without replication**: Sharding is built on top of replication. You MUST configure `with_replication()` before `with_sharding()`.

5. **Inconsistent consistency models**: All nodes in a replica network MUST use the same `ConsistencyModel`. Mixing causes undefined behavior.

6. **Blocking the event loop**: The `node` methods are async. Don't block the tokio runtime. Use `tokio::spawn` for concurrent tasks.

7. **VERIFIED: Busy-spinning on `next_event()`**: `next_event()` is non-blocking and returns `None` immediately when the buffer is empty. Without a sleep in the loop, you'll burn 100% CPU. Always add `tokio::time::sleep(Duration::from_millis(100))` in your event loop.

8. **VERIFIED: Gossipsub mesh formation takes ~5 seconds**: After two nodes connect via bootnodes, the gossipsub mesh is NOT immediately ready. Broadcasts sent before the mesh forms will silently fail. In tests, add a 5-second sleep after connection before broadcasting. In production, retry broadcasts or wait for `GossipsubSubscribed` events.

9. **VERIFIED: Use 127.0.0.1 for local bootnode addresses**: `NewListenAddr` events may report `0.0.0.0` or other interface addresses. For local testing, always construct bootnode addresses with `127.0.0.1` explicitly, e.g. `/ip4/127.0.0.1/tcp/<port>`.

10. **VERIFIED: Import from `swarm_nl::*`, not `swarm_nl::core::prelude::*`**: The `core::prelude` module is private in v0.2.1. All public types are re-exported from the crate root.

11. **VERIFIED: swarm-nl v0.2.1 does NOT wire libp2p mDNS.** The `CoreBehaviour` derives only `request_response`, `kademlia`, `ping`, `identify`, `gossipsub` — no mDNS. If you need local auto-discovery, use `shared/forge-ui`'s mDNS backend (the UI toggle in the Peers tab), which advertises `_forge-p2p._tcp.local.` at the HTTP layer and fires a dial request when a peer is found.

## 9. Feature Flags

- `tokio-runtime` — Use tokio as the async runtime (recommended)
- `async-std-runtime` — Use async-std as the async runtime

Choose one. Do not enable both.

## 10. Useful App Ideas by Communication Pattern

| Pattern | Good For | Example Apps |
|---------|----------|--------------|
| Gossip only | Broadcasting state, pub/sub | Chat rooms, live game state, sensor networks |
| RPC only | Direct data exchange, file transfer | File sharing, database queries between peers |
| DHT only | Distributed key-value storage | Name registries, content addressing |
| Gossip + DHT | Discovery + broadcasting | Decentralized social feeds, marketplaces |
| Gossip + RPC | Broadcasting + direct exchange | Multiplayer games, collaborative editing |
| Replication | Fault tolerance, redundancy | Distributed databases, backup networks |
| Sharding | Scalability, partitioned data | Large-scale storage, CDN-like networks |
| Sharding + Replication | Scale + fault tolerance | Production distributed systems |