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
  `MeshEvent::MessageReceived` + `MeshEvent::Custom{label:"CHAT"}`. Also
  owns the rolling `History` ring buffer (last 50 lines) used to replay
  chat history to newly-opened panels.
- **`/api/chat/send`** (registered via `ForgeUI::with_routes`): POST
  endpoint that enqueues typed text on the app's own `mpsc::Sender<String>`.
- **`/api/chat/history`**: GET endpoint returning the last 50 chat lines
  so the iframe can replay them on load (survives browser refresh).
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

## Try this

Once both peers are up and chatting, exercise the discovery + recovery
flow:

1. **Send a few messages**, then refresh one browser tab. The chat
   history replays from `/api/chat/history` (last 50 lines) — you won't
   see an empty panel.
2. **Kill one peer** (`Ctrl-C` on, say, Bobby's terminal). On Al's full
   UI (http://127.0.0.1:8080), within ~5–10 s the event log shows
   `DISCONNECT` (libp2p connection dropped) and then `LOST Bobby`
   (forge-ui's localhost scanner noticed). Bobby disappears from the
   **Connected** and **Discovered** lists on the Peers tab.
3. **Restart Bobby** (`cargo run -- --peer bobby`). Al's event log
   quickly shows `DISCOVER` → `DIAL` → `CONNECT` → `SUB`. Auto-connect
   dials Bobby without any clicks; the mesh graph re-adds the edge.
4. **Send another message.** It delivers to the fresh Bobby instance
   — no manual reconnect.

## Troubleshooting

**`rustc 1.86.0 is not supported by … time@0.3.47` on first build.**
The `time` crate has a newer MSRV than the pinned rustc. Pin back:

```bash
cargo update time --precise 0.3.41
```

**`MultiaddressListenError("/ip4/0.0.0.0/tcp/50200")` on Bobby start.**
Something else on your machine is using TCP port 50200. Close the
conflicting process, or change Bobby's port in
`src/main.rs::PeerName::tcp_port` (and note that the localhost
discovery port range in forge-ui defaults to 8080–8089, not SwarmNL
ports, so you only need to edit the SwarmNL port).

**Port 8080 or 8081 already in use.**
`pkill -f mesh-chat` will clear stragglers from a previous run. If
something non-mesh-chat is on the port, pass a different HTTP port by
editing `PeerName::ui_port`.

**mDNS toggle returns 503 on macOS.**
The first time you advertise an mDNS service, macOS may prompt for
local-network permission. If you clicked Deny, revoke it in System
Settings → Privacy & Security → Local Network and restart the binary.

## Known limitation

Gossipsub mesh formation is sometimes one-way for the first ~10 s of a
session. Bobby may not see Al's messages (and vice versa) until both
ends have exchanged heartbeats. See `../../library-feedback.md`.

## Future development ideas

Not scoped for the current demo, but obvious next wins:

- **RTT display.** Timestamp each `ChatLine` at send; on receive,
  subtract from local clock and render `Bobby: hi  (42ms)` in the
  panel. Makes the "it's real P2P gossip" story tangible.
- **Single-command launch.** A `--demo` flag that boots both Al and
  Bobby in one process (two `Core` + two `ForgeUI` under one tokio
  runtime; the integration test already proves the pattern) and prints
  the dual-view URL. Optionally opens the browser via `open`/`xdg-open`.
- **Typing indicators.** Broadcast a tiny `{kind:"typing"}` gossip
  message on keystroke (debounced) so the other panel shows "Bobby is
  typing…". Exercises a second logical message type on the same topic.
- **More than two peers.** Drop the `PeerName` enum, accept
  `--name <str>` + `--ui-port <u16>` + `--swarm-port <u16>`. Would
  stress-test the mesh visualizer with more nodes and make the demo
  more visually interesting.
- **Non-gossip patterns.** Direct-message via RPC (`AppData::SendRpc`)
  and/or a DHT handle registry (`AppData::KademliaStoreRecord`) — same
  UI, different SwarmNL primitives, useful as a teaching reference.
