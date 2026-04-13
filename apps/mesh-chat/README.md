# mesh-chat

A two-peer gossip chat demo built on SwarmNL + forge-ui. Two named peers,
`Al` and `Bobby`, each run their own SwarmNL node and forge-ui instance.
They join a shared `"chat"` gossip topic and exchange typed messages.
Peering, discovery, node identity, and mesh visualization all come from
forge-ui's parent chrome — the app code only implements the chat feature.

## Quickstart

One command per peer. No CLI bootnode, no copy-paste between terminals —
the nodes auto-discover each other via forge-ui's built-in localhost scan
and auto-dial.

From this directory, in two terminals:

```bash
# Terminal 1
cargo run -- --peer al
```

```bash
# Terminal 2
cargo run -- --peer bobby
```

Open the **dual view** — both chat panels side-by-side in one window:

**http://127.0.0.1:8080/app/dual.html**

(`http://127.0.0.1:8081/app/dual.html` serves the same page; either peer
works as the entry point.)

Within ~5–10 s, forge-ui's built-in discovery finds the other node,
auto-dials, and the gossipsub mesh forms ~5–10 s later. Once both
panels show `chat-status: connected`, type in either side and watch it
round-trip to the other within ~1 s.

### Want peers tab, mesh graph, event log?

The dual view only contains the two chat iframes. For the full forge-ui
chrome (node identity card, Peers tab with discovered/connected lists,
mesh visualizer, event log), open one node's root URL in a new tab:

- http://127.0.0.1:8080 — Al's full UI
- http://127.0.0.1:8081 — Bobby's full UI

The dual-view header has click-through links to both.

### Cross-machine demo with mDNS

Want to demo peering across a LAN? Run `mesh-chat` on two machines on the
same Wi-Fi, open each UI, and flip the **mDNS (cross-machine LAN)**
toggle in the Peers tab on at least one side. Within seconds each node
appears on the other under source `mdns` and auto-dials.

> swarm-nl v0.2.1 does not wire libp2p mDNS; forge-ui provides the mDNS
> advertisement/browser at the HTTP layer (TXT records with `peer_id` +
> `multiaddr`). The actual peer connection is still libp2p over TCP.

### Optional: dial via CLI

For scripted runs:

```bash
cargo run -- --peer bobby \
    --bootnode-peer-id <AL_PEER_ID> \
    --bootnode-addr /ip4/127.0.0.1/tcp/50000
```

## Ports

| Role  | SwarmNL TCP | SwarmNL UDP | forge-ui HTTP |
| ----- | ----------- | ----------- | ------------- |
| Al    | 50000       | 50001       | 8080          |
| Bobby | 50200       | 50201       | 8081          |
| Tests | 49000       | 49001       | —             |
| Tests | 49100       | 49101       | —             |

## What this app actually writes

Per `.forge/workflow.md` §1a (UI scope), the app does NOT implement
peering, dial forms, peer lists, node-identity UI, or discovery — all of
that lives in forge-ui. The app provides:

- **`src/main.rs`**: SwarmNL boot + event loop. Creates a
  `mpsc::Sender<DialRequest>` and hands it to
  `ForgeUI::with_dial_sender(tx)`. A `tokio::select!` arm receives
  `DialRequest`s from forge-ui and calls `AppData::DailPeer`.
- **`src/chat.rs`**: the `ChatLine` JSON envelope + a `handle_event`
  that translates `GossipsubIncomingMessageHandled` into
  `MeshEvent::MessageReceived` + `MeshEvent::Custom{label:"CHAT"}`.
- **`/api/chat/send`** (registered via `ForgeUI::with_routes`): POST
  endpoint that enqueues typed text on the app's own `mpsc::Sender<String>`.
- **`static/index.html` + `chat.js` + `chat.css`**: the chat iframe —
  a message list, text input, and send button. That's the whole UI.

## Test

```bash
cargo test
```

The integration test in `tests/integration.rs` spawns two in-process
SwarmNL nodes on ports 49000 / 49100, joins the chat topic, waits for
the mesh to form, broadcasts from node 1, and asserts node 2 decodes the
exact `ChatLine` payload. Runs in ~25 s.

## Known limitation

Gossipsub mesh formation is sometimes one-way for the first ~10 s of a
session. Bobby may not see Al's messages (and vice versa) until both
ends have exchanged heartbeats. See `../../library-feedback.md`.
