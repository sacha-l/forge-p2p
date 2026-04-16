# ForgeP2P

A repo for building and showcasing peer-to-peer applications using [SwarmNL](https://github.com/algorealmInc/SwarmNL).

## Project Structure

```
forge-p2p/
├── CLAUDE.md                       # This file -- project context
├── .forge/
│   ├── workflow.md                 # Agent workflow rules (READ FIRST)
│   ├── swarm-nl-reference.md       # SwarmNL API reference
│   ├── templates/
│   │   ├── plan.toml               # Plan template (machine-readable)
│   │   └── forge-state.toml        # State template
│   └── registry.toml               # Catalog of all apps and their status
├── shared/                          # Shared crates used by all apps
│   └── forge-ui/                    # Web UI + mesh visualizer (axum + WebSocket + vanilla JS)
│       ├── Cargo.toml
│       ├── src/                     # Rust: axum server, WS handler, event types
│       └── static/                  # Frontend: mesh visualizer, layout, styles
├── apps/                            # Each app in its own directory
│   └── <app-name>/
│       ├── forge-state.toml         # Machine-readable agent state
│       ├── plan.toml                # Step-by-step plan
│       ├── decisions.md             # Human-readable design decisions log
│       ├── Cargo.toml               # depends on forge-ui via path
│       ├── src/
│       ├── static/                  # App-specific UI panel files
│       ├── tests/
│       └── README.md
├── library-feedback.md              # SwarmNL improvement suggestions
└── README.md
```

## Key Conventions

- **Rust edition**: 2021
- **Async runtime**: tokio (feature `tokio-runtime` on swarm-nl)
- **Branching**: `main` = templates + shared crates. `dev/<app-name>` = complete apps. See `.forge/registry.toml` for the full catalog.
- **Port allocation**: Each app node starts at `50000 + (app_index * 1000) + (node_index * 100)`. Tests start at `49000`.
- **Error handling**: No `.unwrap()` in app code. Use `anyhow` or `thiserror`.
- **UI**: All apps use `forge-ui` for the embedded web server and mesh visualizer. Apps only provide their own application panel (HTML/JS in `static/`).

## forge-ui Overview

`shared/forge-ui` is a Rust crate that every app depends on. It provides:

1. **Axum web server** on localhost — one `cargo run` starts both the SwarmNL node and the UI
2. **WebSocket channel** for pushing real-time network events to the browser
3. **Mesh visualizer** (dependency-free vanilla JS, radial layout) showing peers, connections, and message flow
4. **Event log** showing network activity in real time
5. **Loading states and explanations** — when the node is booting, discovering peers, or forming the gossip mesh, the UI shows what's happening and why
6. **Split layout** — left panel for the app-specific UI, right panel for the mesh visualizer

Apps integrate by:
```rust
use forge_ui::{ForgeUI, MeshEvent};

let ui = ForgeUI::new()
    .with_port(8080)
    .with_app_name("My App")
    .with_app_static_dir("./static")
    .start()
    .await;

// In the event loop:
ui.push(MeshEvent::PeerConnected { peer_id, addr }).await;
```

## Before Writing Any Code

1. Read `.forge/workflow.md` -- the complete agent operating rules
2. Read `library-feedback.md` -- known issues from previous builds
3. Read `.forge/swarm-nl-reference.md` -- the API reference
4. Read `.forge/registry.toml` -- what exists already
5. If resuming work, read `apps/<n>/forge-state.toml` first