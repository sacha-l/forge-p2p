# mesh-chat

A two-peer gossip chat demo built on SwarmNL + forge-ui. Two named peers,
`Al` and `Bobby`, each run their own SwarmNL node and forge-ui instance.
They join a shared `"chat"` gossip topic and exchange typed messages that
are rendered in a chat panel alongside the live mesh visualizer.

## Run

Two terminals, from this directory. No CLI bootnode is required — peers
dial each other through the browser UI.

Terminal 1 — Al:

```bash
cargo run -- --peer al
```

Terminal 2 — Bobby:

```bash
cargo run -- --peer bobby
```

Both processes print their own `PeerId` and listen addrs on startup.
Then open:

- http://127.0.0.1:8080 — Al's UI
- http://127.0.0.1:8081 — Bobby's UI

On either UI, expand the **Connect to peer** panel at the top of the
chat column and paste the *other* node's `PeerId` and a loopback
multiaddr (e.g. `/ip4/127.0.0.1/tcp/50000` for Al, `…/50200` for Bobby),
then click **Connect**. Within ~10–15 s the gossip mesh will form; after
that, typed messages should round-trip within ~1 s and the mesh
visualizer on the right will animate on each broadcast.

### Alternative: dial via CLI

If you'd rather skip the UI step, pass the bootnode on the command line:

```bash
cargo run -- --peer bobby \
    --bootnode-peer-id <AL_PEER_ID> \
    --bootnode-addr /ip4/127.0.0.1/tcp/50000
```

## Ports

| Role   | SwarmNL TCP | SwarmNL UDP | forge-ui HTTP |
| ------ | ----------- | ----------- | ------------- |
| Al     | 50000       | 50001       | 8080          |
| Bobby  | 50200       | 50201       | 8081          |
| Tests  | 49000       | 49001       | —             |
| Tests  | 49100       | 49101       | —             |

## How it works

1. Each node boots a SwarmNL `Core` with fixed ports. Neither peer requires
   a CLI bootnode — they can discover each other via the UI dial form.
2. Both nodes call `AppData::GossipsubJoinNetwork("chat")` on startup.
3. `NetworkEvent::ConnectionEstablished` and
   `GossipsubSubscribeMessageReceived` are translated into forge-ui
   `MeshEvent`s (`PeerConnected`, `Custom{label:"SUB"}`) so the
   visualization updates live.
4. The browser panel POSTs to two custom routes registered via
   `ForgeUI::with_routes`:
   - `POST /api/chat/send` — typed message; enqueues `Command::Send`
   - `POST /api/peer/dial` — `{peer_id, addr}`; enqueues `Command::Dial`
   An `mpsc::Sender` in the route handlers forwards commands into the
   main event loop.
5. The main loop's `tokio::select!` picks up commands and dispatches:
   - `Send` → `AppData::GossipsubBroadcastMessage` + `MessageSent` +
     `Custom{label:"CHAT"}` for the local echo.
   - `Dial` → `AppData::DailPeer(peer_id, addr)` + `Custom{label:"DIAL"}`
     status updates.
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
