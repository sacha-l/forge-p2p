# Echo Gossip — Design Decisions

## Step 1 — Import paths differ from reference doc
- **Context**: The swarm-nl-reference.md suggests `use swarm_nl::core::prelude::*` but `prelude` is `pub(crate)` in swarm-nl 0.2.1.
- **Decision**: Import types directly: `swarm_nl::core::{CoreBuilder, NetworkEvent}` and `swarm_nl::setup::BootstrapConfig`.
- **Trade-off**: More verbose imports, but correct. Reference doc should be updated.

## Step 1 — `Core::next_event()` requires `&mut self`
- **Context**: Reference doc examples use immutable `node`, but `next_event()` needs `&mut self`.
- **Decision**: Declare `let mut node = ...`.
- **Trade-off**: None, this is just the correct usage.
