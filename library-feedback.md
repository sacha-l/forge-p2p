# SwarmNL Library Feedback

> Improvements, issues, and suggestions discovered while building apps with SwarmNL.
> Each entry is logged by the coding agent during development.

**Contributing an entry?** See [CONTRIBUTING.md](CONTRIBUTING.md) — a PR that
touches only this file is fastest to merge. If you're working in a fork, only
your `library-feedback.md` diff needs to travel back upstream; your app
artifacts stay in your fork.

---

## [2026-04-12] echo-gossip — `swarm_nl::core::prelude` module is private

- **Context**: Starting the first app following the reference doc's import examples.
- **Problem**: `swarm_nl::core::prelude` is declared `pub(crate)` in v0.2.1, so external crates cannot `use swarm_nl::core::prelude::*;`. The compile failure is unhelpful.
- **Suggestion**: Make `prelude` public, OR fix the reference doc examples to use the working import paths.
- **Workaround**: `use swarm_nl::*;` (crate root re-exports everything needed) or import individually: `use swarm_nl::core::{CoreBuilder, NetworkEvent, AppData, AppResponse}; use swarm_nl::setup::BootstrapConfig;`.
- **Severity**: important

## [2026-04-12] echo-gossip — AppData/NetworkEvent variant names differ from documentation

- **Context**: Joining a gossip topic and handling incoming messages.
- **Problem**: Real variant names:
  - `AppData::GossipsubJoinNetwork(String)` — tuple variant, not struct `{ topic }`
  - `AppData::GossipsubBroadcastMessage { topic, message }` where `message: ByteVector` (`Vec<Vec<u8>>`), not `Vec<String>`
  - `NetworkEvent::GossipsubIncomingMessageHandled { source, data }` — no `topic` field
  - `NetworkEvent::GossipsubSubscribeMessageReceived { peer_id, topic }` — not `GossipsubSubscribed`
  - `NetworkEvent::GossipsubUnsubscribeMessageReceived { peer_id, topic }` — not `GossipsubUnsubscribed`
  - `NetworkEvent::RpcIncomingMessageHandled { data }` — no `peer` field
- **Suggestion**: Align docs with source, or rename variants to the shorter documented names.
- **Workaround**: Check `~/.cargo/registry/.../swarm-nl-*/src/core/prelude.rs` for the real definitions.
- **Severity**: important

## [2026-04-12] echo-gossip — `Core::next_event()` requires `&mut self`

- **Context**: Reference doc examples use `let node = CoreBuilder::...; while let Some(event) = node.next_event().await`.
- **Problem**: `next_event()` takes `&mut self`, so `node` must be `let mut`. Docs show it immutable.
- **Suggestion**: Update reference examples to use `let mut node = ...`.
- **Workaround**: Declare `let mut node`.
- **Severity**: nice-to-have

## [2026-04-12] echo-gossip — `next_event()` is non-blocking and undocumented

- **Context**: Building an event loop that reacts to network events.
- **Problem**: `next_event()` returns `None` instantly if the internal buffer is empty; it does NOT await an event. A naive `loop { if let Some(e) = node.next_event().await { ... } }` pegs CPU at 100%.
- **Suggestion**: Document the non-blocking behaviour. Or offer a `next_event_blocking`/`wait_event` variant that actually suspends.
- **Workaround**: `tokio::time::sleep(Duration::from_millis(100))` at the end of every iteration.
- **Severity**: important

## [2026-04-12] echo-gossip — Gossipsub mesh takes ~5 s to form; broadcasts before then silently fail

- **Context**: Writing an integration test that broadcasts immediately after both nodes join a topic.
- **Problem**: `AppData::GossipsubBroadcastMessage` right after `GossipsubJoinNetwork` returns `GossipsubBroadcastMessageError` because the gossipsub mesh hasn't formed yet. The error is generic — doesn't indicate "retry later".
- **Suggestion**: Buffer broadcasts until the mesh is ready, or expose a distinct "mesh-not-ready" error variant so apps can retry automatically.
- **Workaround**: `tokio::time::sleep(Duration::from_secs(5))` after joins before the first broadcast in tests.
- **Severity**: important

## [2026-04-12] echo-gossip — Use `/ip4/127.0.0.1/…` for local bootnodes; `NewListenAddr` isn't reliable

- **Context**: Test connecting node2 to node1 as a bootnode using the address reported by `NewListenAddr`.
- **Problem**: `NewListenAddr` emits multiple entries — `0.0.0.0`, `127.0.0.1`, LAN IP, docker bridge, etc. Picking the first one often breaks in-process tests.
- **Suggestion**: Document that local tests should construct bootnode addresses manually, or expose a filter helper on `Core` that returns only loopback addresses.
- **Workaround**: Build the addr string manually: `format!("/ip4/127.0.0.1/tcp/{port}")`.
- **Severity**: nice-to-have

## [2026-04-12] sovereign-notes — `Core::replicate()` errors when no peers are in the mesh

- **Context**: Single-node CLI usage — user creates a note on a fresh machine before a peer has connected.
- **Problem**: `replicate()` returns `Err(GossipsubBroadcastMessageError)` because it broadcasts over gossip internally. There is no "no-op success" path. Apps that replicate on every local write must treat this as non-fatal.
- **Suggestion**: Either succeed silently when there are no peers (data is buffered locally anyway) or expose a dedicated `NoPeers` error variant distinct from broadcast failures.
- **Workaround**: `match node.replicate(...).await { Ok(()) => {}, Err(_) => println!("no peers; will sync later"), }`.
- **Severity**: important

## [2026-04-12] sovereign-notes — `CoreBuilder::with_rpc` handler cannot capture app state

- **Context**: Sovereign-notes needs its RPC handler to read from a per-node data directory (`--data-dir <path>`), and the path is only known at startup.
- **Problem**: `with_rpc(config, handler: fn(RpcData) -> RpcData)` takes a plain function pointer, not a closure or trait object. Handlers can't capture a path, a `NoteStore`, or any app state.
- **Suggestion**: Accept `Arc<dyn Fn(RpcData) -> RpcData + Send + Sync>`, or add a second argument passing an app-provided context handle, so handlers can be closures over per-node state.
- **Workaround**: Use a module-level `OnceLock<PathBuf>` (or similar global) set before `CoreBuilder::...build()` runs. Means you can't run two nodes with different data dirs in one process.
- **Severity**: nice-to-have

## [2026-04-13] mesh-chat — gossipsub mesh is one-way when topic is joined before the first peer connects

- **Context**: Al joins topic `"chat"` on startup, Bobby dials Al and joins the same topic. Both are happy to broadcast; only Al → Bobby delivery is reliable.
- **Problem**: Al receives `GossipsubSubscribeMessageReceived` for Bobby, but Bobby never receives the equivalent event for Al. Result: Bobby's broadcasts do not propagate to Al (no mesh link exists from Bobby's side). Al-side broadcasts deliver fine for the first few seconds, then also drop silently.
- **Suggestion**: On `ConnectionEstablished`, re-emit the local node's current gossip subscriptions to the new peer (libp2p's gossipsub does this automatically but the event layer may be swallowing it). Alternatively, document that apps must call `GossipsubJoinNetwork` *after* establishing connections, or re-join on every new peer.
- **Workaround**: None satisfying. A periodic re-join each heartbeat works but is wasteful. Intermittent delivery means demos should visibly show one direction (e.g. Al → Bobby) and mention the limitation.
- **Severity**: important
- **Relevant API**: `AppData::GossipsubJoinNetwork`, `NetworkEvent::GossipsubSubscribeMessageReceived`, `NetworkEvent::GossipsubIncomingMessageHandled`

## [2026-04-13] mesh-chat — `BootstrapConfig::with_bootnodes` takes `HashMap<String, String>`, not `HashMap<PeerId, String>`

- **Context**: Configuring Bobby to dial Al by peer id + multiaddr.
- **Problem**: Reference doc (and the `test_two_nodes_communicate` pattern) shows `HashMap<PeerId, String>`. Actual signature in `swarm-nl-0.2.1/src/setup.rs:58` is `pub fn with_bootnodes(mut self, boot_nodes: Nodes)` where `Nodes = HashMap<String, String>`. Passing a `HashMap<PeerId, String>` is a type error.
- **Suggestion**: Either change the signature to accept `HashMap<PeerId, String>` (avoids stringly-typed peer ids) or update docs to reflect the current type.
- **Workaround**: `let mut bootnodes: HashMap<String, String> = HashMap::new(); bootnodes.insert(peer_id_string, multiaddr_string);`.
- **Severity**: important
- **Relevant API**: `BootstrapConfig::with_bootnodes`, `setup::Nodes`

## [2026-04-12] sovereign-notes — `RpcIncomingMessageHandled` omits the requesting peer

- **Context**: Writing an RPC handler that wants to log or respond based on the sender.
- **Problem**: The event carries only `{ data: RpcData }`. Peer identity must be re-encoded in the request payload, which defeats the point of having a request-response protocol on top of identified peers.
- **Suggestion**: Add `peer: PeerId` to the event, matching other incoming-message events that already carry `source` or `peer_id`.
- **Workaround**: Include sender `PeerId` in the RPC request bytes.
- **Severity**: nice-to-have

## [2026-04-20] paired-exchange — `Core::recv_from_network` polls with 3-second granularity, so every RPC has a hard 3s floor

- **Context**: Per-peer ping task sends `AppData::SendRpc(DataPing)` every 2 seconds and measures round-trip time.
- **Problem**: `Core::recv_from_network` (called by `query_network`) does not suspend on an actual notification from the networking backend. Instead it spin-polls `stream_response_buffer` in a loop gated by `tokio::time::sleep(Duration::from_secs(TASK_SLEEP_DURATION))` where `TASK_SLEEP_DURATION = 3` is a private const in `core/prelude.rs:23`. Its max iteration count is 10, which turns into a 30-second ceiling. Effect: every single RPC has a built-in floor of `0..3s` minimum latency even for in-process nodes on loopback where the real round-trip is well under a millisecond. Observed pings oscillate between ~10ms and ~3s depending on where the response arrives relative to the poll cycle; latency is not monotone with network distance.
- **Suggestion**: Wake the waiter with a notification when the response lands (e.g. `tokio::sync::Notify`, or park a oneshot receiver per `stream_id` and drop it into the buffer alongside the payload). Failing that, at minimum make `TASK_SLEEP_DURATION` configurable through `RpcConfig`.
- **Workaround**: None available from the app side — the 3s sleep is hardcoded and private. Apps that want real-time RPC must either tolerate the floor or fork the library. For this demo, ping timeouts were raised to 10s and integration-test RTT assertions were loosened so the test tolerates the polling-cycle jitter.
- **Severity**: important
- **Relevant API**: `Core::query_network`, `Core::recv_from_network`, `TASK_SLEEP_DURATION`

## [2026-04-20] paired-exchange — `RpcIncomingMessageHandled` is declared but never emitted; RPC handler is a sync `fn(RpcData) -> RpcData` with no peer context

- **Context**: Building an application-level authorization gate (HMAC-based pairing) where both sides refuse to act on incoming RPCs until a challenge/response handshake completes. The spec called for a 4-message handshake (`Challenge → ResponseAndChallenge → Response → Ack`) driven from the event loop on incoming RPC events, with each side reacting to the sender's `PeerId`.
- **Problem**: Two stacked gaps in v0.2.1 force a redesign:
  1. `NetworkEvent::RpcIncomingMessageHandled { data: RpcData }` is declared in `prelude.rs:486` but nothing in `core/mod.rs` ever pushes it onto the event queue (confirmed by full-source grep — only `GossipsubIncomingMessageHandled` has an emit site). Apps cannot react to incoming RPC via the normal `node.next_event()` loop at all.
  2. The only path the library gives you for incoming RPC is the `rpc_handler_fn: fn(RpcData) -> RpcData` registered via `CoreBuilder::with_rpc`. It is a *bare fn pointer* (so no captured app state — same issue as the sovereign-notes `with_rpc` entry above), it is *synchronous* (cannot `.await`, cannot take a `tokio::sync::Mutex`), and it receives no `PeerId` for the caller. The response is whatever bytes the fn returns, sent back on the same RPC channel; there is no other hook.
- **Consequence for this app**: The 4-message event-driven handshake is not expressible on SwarmNL v0.2.1. The protocol was simplified to a single round-trip `Challenge(nonce) → Response(hmac)` per direction, with each side independently initiating against the other on `ConnectionEstablished`. Mutual authentication still holds in aggregate (each side proves knowledge of `S` to the other via its own challenge), at the cost of losing the combined `ResponseAndChallenge + Ack` flow.
- **Consequence for the data-plane gate**: The plan's three `is_trusted(&peer)` checks become "two enforceable + one best-effort". The two initiator-side checks (before sending `DataPing`, after receiving `DataPong` as the RPC response) are checkable. The handler-side check on incoming `DataPing` cannot see the caller, so it falls back to a "single connected peer" assumption cached from `ConnectionEstablished` and consulted via a `static OnceLock<Arc<HandlerCtx>>` — clean for a two-node demo, but does not generalise. Any production app that wants a per-peer receive-side gate is blocked on (2).
- **Suggestion**: Either (a) actually emit `RpcIncomingMessageHandled { data, peer }` from the request-response branch in `core/mod.rs` so apps can handle RPC in the normal async event loop, or (b) change `with_rpc` to accept `Arc<dyn Fn(PeerId, RpcData) -> BoxFuture<'static, RpcData> + Send + Sync>` so handlers can take state *and* see the caller *and* do async work. The proposed per-peer delivery filter hook in §7 of the paired-exchange plan is the narrow-composable version of the same idea.
- **Workaround**: `static HANDLER_CTX: OnceLock<Arc<HandlerCtx>>` holding the shared secret, a `std::sync::Mutex`-backed `PairingBook` (not `tokio::sync::Mutex`, so the sync handler can lock it without blocking the runtime), and an `Option<PeerId>` cache of the one currently-connected peer, set from `ConnectionEstablished`. Document clearly that the handler-side gate only works because n=2.
- **Severity**: blocking (for the 4-message protocol) → important (after the protocol simplification)
- **Relevant API**: `NetworkEvent::RpcIncomingMessageHandled`, `CoreBuilder::with_rpc`, `RpcConfig`, `AppData::SendRpc`
