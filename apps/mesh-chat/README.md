# mesh-chat

A two-peer gossip chat demo built on SwarmNL + forge-ui. Two named peers,
`Al` and `Bobby`, each run their own SwarmNL node and forge-ui instance.
They join a shared `"chat"` gossip topic and exchange typed messages that
are rendered in a chat panel alongside the live mesh visualizer.

## Run

Two terminals, from this directory.

Terminal 1 — Al (bootnode):

```bash
cargo run -- --peer al
```

This prints Al's `PeerId` and a copy-paste command for Bobby. Example output:

```
=== mesh-chat :: Al ===
PeerId:  12D3KooW...
UI:      http://127.0.0.1:8080
Tip:     to start Bobby, run: cargo run -- --peer bobby \
           --bootnode-peer-id 12D3KooW... \
           --bootnode-addr /ip4/127.0.0.1/tcp/50000
```

Terminal 2 — Bobby:

```bash
cargo run -- --peer bobby \
    --bootnode-peer-id <AL_PEER_ID> \
    --bootnode-addr /ip4/127.0.0.1/tcp/50000
```

Then open:

- http://127.0.0.1:8080 — Al's UI
- http://127.0.0.1:8081 — Bobby's UI

Wait ~10–15 seconds for the gossipsub mesh to form, then type a message in
either panel. It should appear in the other within ~1 s, and the mesh
visualizer animates on each broadcast.

## Ports

| Role   | SwarmNL TCP | SwarmNL UDP | forge-ui HTTP |
| ------ | ----------- | ----------- | ------------- |
| Al     | 50000       | 50001       | 8080          |
| Bobby  | 50100       | 50101       | 8081          |
| Tests  | 49000       | 49001       | —             |
| Tests  | 49100       | 49101       | —             |

## How it works

1. Each node boots a SwarmNL `Core` with fixed ports. Bobby is started with
   Al's `PeerId` + multiaddr as its sole bootnode.
2. Both nodes call `AppData::GossipsubJoinNetwork("chat")` on startup.
3. `NetworkEvent::ConnectionEstablished` and
   `GossipsubSubscribeMessageReceived` are translated into forge-ui
   `MeshEvent`s (`PeerConnected`, `Custom{label:"SUB"}`) so the
   visualization updates live.
4. The browser panel POSTs typed text to `POST /api/chat/send`, a custom
   axum route registered via `ForgeUI::with_routes`. An `mpsc::Sender` in
   the route handler forwards the text into the main event loop.
5. The main loop's `tokio::select!` picks up the text, broadcasts a
   `ChatLine` JSON payload via `AppData::GossipsubBroadcastMessage`, and
   pushes `MessageSent` + `Custom{label:"CHAT"}` to the UI.
6. Incoming gossip (`GossipsubIncomingMessageHandled`) is decoded back
   into a `ChatLine` and rendered the same way on the receiving side.

## Test

```bash
cargo test
```

The integration test in `tests/integration.rs` spawns two in-process nodes
on ports 49000 / 49100, joins the chat topic, waits for the mesh to form,
broadcasts from node 1, and asserts node 2 decodes the exact `ChatLine`
payload. Runs in ~25 s.

## Known limitation

Gossipsub mesh formation is sometimes one-way for the first 10 s of a
session. Bobby may not see Al's messages (and vice versa) until both ends
have exchanged heartbeats. See `../../library-feedback.md` for details.
