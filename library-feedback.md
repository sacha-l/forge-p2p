# SwarmNL Library Feedback

> Improvements, issues, and suggestions discovered while building apps with SwarmNL.
> Each entry is logged by the coding agent during development.

---

## [2026-04-12] echo-gossip — `core::prelude` module is private
- **Context**: Bootstrapping the first app following the reference doc's import pattern
- **Problem**: `swarm_nl::core::prelude` is declared `pub(crate)` in v0.2.1, so external crates cannot use `use swarm_nl::core::prelude::*`
- **Suggestion**: Either make the `prelude` module public, or document the correct import paths (`swarm_nl::core::{CoreBuilder, NetworkEvent}`, `swarm_nl::setup::BootstrapConfig`)
- **Workaround**: Import types individually from their actual modules
- **Severity**: nice-to-have
- **Relevant API**: `swarm_nl::core::prelude`
