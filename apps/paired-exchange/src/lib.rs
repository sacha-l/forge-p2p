//! Library crate for `paired-exchange`. Holds all logic that benefits from
//! being unit-testable and accessible from integration tests. The `main.rs`
//! binary stays thin: CLI parsing + event-loop wiring.

pub mod config;
