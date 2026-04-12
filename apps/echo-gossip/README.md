# Echo Gossip

Peers join a gossip topic, broadcast messages, and echo back whatever they receive. The simplest possible SwarmNL app.

## How it works

1. A node boots and joins the `echo-network` gossip topic
2. It broadcasts a greeting message to the topic
3. When it receives a message from another peer, it echoes it back with an `echo: ` prefix
4. Messages that already have the `echo: ` prefix are printed but not re-echoed (prevents infinite loops)

## Usage

Start node 1:
```bash
cargo run -- --tcp-port 50000 --udp-port 50001
```

Note the PeerId printed at startup, then start node 2 in another terminal:
```bash
cargo run -- --tcp-port 50100 --udp-port 50101 \
  --boot-peer-id <PEER_ID_FROM_NODE_1> \
  --boot-addr /ip4/127.0.0.1/tcp/50000
```

## SwarmNL pattern

**Gossip only** — uses `GossipsubJoinNetwork`, `GossipsubBroadcastMessage`, and `GossipsubIncomingMessageHandled`.

## Testing

```bash
cargo test -- --test-threads=1
```

The integration test spawns two in-process nodes (ports 49000 and 49100), has them join the same topic, broadcasts a message, and verifies the echo round-trip.
