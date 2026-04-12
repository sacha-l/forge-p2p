# Sovereign Notes — Design Decisions

## Step 5 — RPC handler uses OnceLock for data directory
- **Context**: `CoreBuilder::with_rpc()` requires `fn(RpcData) -> RpcData` — a plain function pointer, not a closure. Cannot capture the data directory path.
- **Options**: (1) Global static via OnceLock, (2) Thread-local, (3) Encode path in every RPC request
- **Decision**: OnceLock<PathBuf> set once at startup before node build. Simple, safe, no overhead.
- **Trade-off**: Cannot run multiple nodes with different data dirs in the same process (fine for CLI tool, would need refactoring for tests with multiple in-process nodes using different stores).
