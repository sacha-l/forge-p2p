# forge-ui

Embedded web UI and mesh visualizer for ForgeP2P apps. One `cargo run` starts both your SwarmNL node and a local web dashboard showing the live network topology.

## What It Does

- **Axum web server** on a configurable localhost port
- **WebSocket push** — your app sends `MeshEvent`s, the browser renders them in real time
- **D3.js mesh visualizer** — force-directed graph where peers are nodes and connections are edges. Edges pulse when messages flow.
- **Event log** — timestamped feed of network activity (connections, messages, gossip joins, etc.)
- **Split layout** — left panel for your app's own UI, right panel for the mesh graph, bottom for the event log
- **Loading states** — contextual messages while the node boots ("Starting node, generating keypair..." → "Listening for peers..." → active view)

## Usage

Add `forge-ui` as a dependency in your app's `Cargo.toml`:

```toml
[dependencies]
forge-ui = { path = "../../shared/forge-ui" }
```

Start the UI alongside your SwarmNL node:

```rust
use forge_ui::{ForgeUI, MeshEvent};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let ui = ForgeUI::new()
        .with_port(8080)
        .with_app_name("My App")
        .with_app_static_dir("./static")  // your app's UI files
        .start()
        .await?;

    // In your SwarmNL event loop, push events to the UI:
    ui.push(MeshEvent::NodeStarted {
        peer_id: peer_id.to_string(),
        listen_addrs: vec![addr.to_string()],
    }).await;

    ui.push(MeshEvent::PeerConnected {
        peer_id: remote_peer.to_string(),
        addr: remote_addr.to_string(),
    }).await;

    Ok(())
}
```

Then open `http://localhost:8080` in a browser.

## MeshEvent Variants

| Event | Fields | When to push |
|-------|--------|--------------|
| `NodeStarted` | `peer_id`, `listen_addrs` | After the node boots and starts listening |
| `PeerConnected` | `peer_id`, `addr` | On `ConnectionEstablished` event |
| `PeerDisconnected` | `peer_id` | On `ConnectionClosed` event |
| `MessageSent` | `to`, `topic`, `size_bytes` | After sending a gossip broadcast or RPC |
| `MessageReceived` | `from`, `topic`, `size_bytes` | On incoming gossip or RPC message |
| `GossipJoined` | `topic` | After joining a gossip topic |
| `ReplicaSync` | `peer_id`, `network`, `status` | On replication sync events |
| `Custom` | `label`, `detail` | Anything app-specific |

Events serialize as JSON over WebSocket with a `"type"` discriminator:

```json
{"type":"PeerConnected","peer_id":"12D3KooW...","addr":"/ip4/127.0.0.1/tcp/3000"}
```

## App Panel

The left panel loads your app's `index.html` from the directory set via `.with_app_static_dir()`. It's served under `/app/`, so your app panel lives at `/app/index.html`.

Your app's JavaScript can open its own WebSocket to `/ws` to receive the same `MeshEvent` stream, or use it to drive app-specific UI updates.

## API

### `ForgeUI`

Builder for configuring and starting the server.

| Method | Description |
|--------|-------------|
| `ForgeUI::new()` | Create builder (defaults: port 8080, name "ForgeP2P App") |
| `.with_port(u16)` | Set the server port |
| `.with_app_name(&str)` | Set the app name shown in the header |
| `.with_app_static_dir(&str)` | Set path to app-specific static files |
| `.start().await` | Start the server, returns `Result<UiHandle>` |

### `UiHandle`

Returned by `.start()`. Clone-safe.

| Method | Description |
|--------|-------------|
| `.push(MeshEvent).await` | Broadcast an event to all connected browsers |

## File Structure

```
shared/forge-ui/
├── Cargo.toml
├── src/
│   ├── lib.rs          # ForgeUI builder, UiHandle
│   ├── events.rs       # MeshEvent enum
│   ├── ws.rs           # WebSocket handler + broadcast channel
│   └── server.rs       # Axum routes, static file serving
├── static/
│   ├── index.html      # Shell layout
│   ├── mesh.js         # D3.js mesh visualizer
│   └── style.css       # Styling
└── tests/
    └── integration.rs  # Serialization + server + WS tests
```

## Port Allocation

Per project convention, apps use ports starting at `50000 + (app_index * 1000)`. Pick a port that doesn't collide with your SwarmNL TCP/UDP ports.
