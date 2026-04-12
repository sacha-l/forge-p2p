# Echo Gossip — Design Decisions

## Step 1 — Import paths differ from reference doc
- **Context**: The swarm-nl-reference.md suggests `use swarm_nl::core::prelude::*` but `prelude` is `pub(crate)` in swarm-nl 0.2.1.
- **Decision**: Import types directly: `swarm_nl::core::{CoreBuilder, NetworkEvent}` and `swarm_nl::setup::BootstrapConfig`.
- **Trade-off**: More verbose imports, but correct. Reference doc should be updated.

## Step 1 — `Core::next_event()` requires `&mut self`
- **Context**: Reference doc examples use immutable `node`, but `next_event()` needs `&mut self`.
- **Decision**: Declare `let mut node = ...`.
- **Trade-off**: None, this is just the correct usage.

## Step 3 — GossipsubBroadcastMessage.message is ByteVector, not Vec<String>
- **Context**: Reference doc shows `message: vec!["hello".to_string()]` but actual type is `ByteVector` (`Vec<Vec<u8>>`).
- **Decision**: Use `"msg".as_bytes().to_vec()` to convert strings to bytes.
- **Trade-off**: None, just a doc/reality mismatch.

## Step 5 — Gossipsub events differ from reference doc
- **Context**: Reference doc lists `GossipsubIncomingMessage { source, data, topic }` and `GossipsubSubscribed`. Actual names are `GossipsubIncomingMessageHandled { source, data }` (no topic) and `GossipsubSubscribeMessageReceived { peer_id, topic }`.
- **Decision**: Use the actual API names from the source code.
- **Trade-off**: Reference doc needs updating.

## Step 5 — next_event() is non-blocking
- **Context**: `next_event()` returns `None` immediately when the event buffer is empty (it's a `VecDeque::pop_front()`). Need polling loops with sleeps.
- **Decision**: Poll in a loop with `tokio::time::sleep(100ms)` between iterations.
- **Trade-off**: Test takes ~20s due to mesh formation time + polling overhead.

## Step 5 — Gossipsub mesh needs ~5s to form
- **Context**: Broadcasting immediately after joining fails with `GossipsubBroadcastMessageError` because the mesh hasn't formed yet.
- **Decision**: Sleep 5 seconds after both nodes join before broadcasting.
- **Trade-off**: Slower test, but reliable.
