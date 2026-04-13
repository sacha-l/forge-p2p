# mesh-chat

A two-peer gossip chat demo built on SwarmNL + forge-ui. Two named peers,
`Al` and `Bobby`, each run their own SwarmNL node and forge-ui instance.
They join a shared `"chat"` gossip topic and exchange typed messages that
are rendered in a chat panel alongside the live mesh visualizer.

## Quickstart

Each peer is one command. No CLI bootnode, no copy-paste between
terminals — nodes are wired up from the browser.

From this directory, in two terminals:

```bash
# Terminal 1
cargo run -- --peer al
```

```bash
# Terminal 2
cargo run -- --peer bobby
```

Then open each node's UI:

- http://127.0.0.1:8080 — Al
- http://127.0.0.1:8081 — Bobby

**Connect the peers from the UI.** In Bobby's tab, expand the
**Connect to peer** panel at the top of the chat column and fill in:

- **PeerId** — copy Al's `PeerId` from Terminal 1's startup output
- **Multiaddr** — `/ip4/127.0.0.1/tcp/50000`

Click **Connect**. You should see `DIAL` / `CONNECT` / `SUB` entries
appear in the event log on the right, and Al's node appear in the mesh
visualizer. Give gossipsub ~10–15 s to form the mesh, then type a
message in either chat panel — it round-trips within ~1 s.

To connect the other direction, do the same from Al's tab using Bobby's
`PeerId` and `/ip4/127.0.0.1/tcp/50200`.

### Optional: dial via CLI instead of UI

If you want to skip the browser dial step (e.g. scripted runs):

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
