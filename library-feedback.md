# SwarmNL Library Feedback

> Improvements, issues, and suggestions discovered while building apps with SwarmNL.
> Each entry is logged by the coding agent during development.

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
