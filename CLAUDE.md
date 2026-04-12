# ForgeP2P

A repo for building and showcasing peer-to-peer applications using [SwarmNL](https://github.com/algorealmInc/SwarmNL).

## Project Structure

```
forge-p2p/
├── CLAUDE.md                       # This file — project context
├── .forge/
│   ├── workflow.md                 # Agent workflow rules (READ FIRST)
│   ├── swarm-nl-reference.md       # SwarmNL API reference
│   ├── templates/
│   │   ├── plan.toml               # Plan template (machine-readable)
│   │   ├── Cargo.toml.tmpl         # App Cargo.toml template
│   │   └── main.rs.tmpl            # App main.rs template
│   └── registry.toml              # Catalog of all apps and their status
├── apps/                           # Each app in its own directory
│   └── <app-name>/
│       ├── forge-state.toml        # Machine-readable agent state
│       ├── plan.toml               # Step-by-step plan
│       ├── decisions.md            # Human-readable design decisions log
│       ├── Cargo.toml
│       ├── src/
│       ├── tests/
│       └── README.md
├── library-feedback.md             # SwarmNL improvement suggestions
└── README.md
```

## Key Conventions

- **Rust edition**: 2021
- **Async runtime**: tokio (feature `tokio-runtime` on swarm-nl)
- **Branching**: `main` = templates only. `dev/<app-name>` = active development.
- **Port allocation**: Each app node starts at `50000 + (app_index * 1000) + (node_index * 100)`. Tests start at `49000`.
- **Error handling**: No `.unwrap()` in app code. Use `anyhow` or `thiserror`.

## Before Writing Any Code

1. Read `.forge/workflow.md` — the complete agent operating rules
2. Read `.forge/swarm-nl-reference.md` — the API reference
3. Read `.forge/registry.toml` — what exists already
4. If resuming work, read `apps/<name>/forge-state.toml` first
