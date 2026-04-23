//! Coordination layer. All four pieces (CBBA, stigmergy, W-MSR, grid merge)
//! are designed to be driven from the robot's main event loop: they consume
//! incoming gossip/RPC payloads and produce outgoing messages the loop then
//! publishes.
//!
//! Each submodule owns its own state and is `Send + Sync` so it can be shared
//! across the per-robot tokio task that runs kinematics and the one that
//! drives network I/O.

pub mod cbba;
pub mod slam;
pub mod stigmergy;
pub mod wmsr;
