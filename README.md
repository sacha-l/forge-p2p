# ForgeP2P

An agentic workflow for building peer-to-peer applications with [SwarmNL](https://github.com/algorealmInc/SwarmNL).

## What This Is

ForgeP2P pairs a coding agent with structured SwarmNL documentation to build, test, and showcase P2P networked applications. The agent follows a strict plan-implement-test loop with machine-readable state, so it can resume autonomously across sessions and never ship untested code.

## Quick Start

### Prerequisites
- Rust toolchain (stable, edition 2021)
- A coding agent that reads markdown/TOML instructions (e.g. Claude Code)

### Usage

1. Fork this repo
2. Point your coding agent at `CLAUDE.md`
3. Ask it to plan: *"Plan a new app: P2P chat using gossip"*
4. Review the plan in `apps/<name>/plan.toml`
5. Ask it to build: *"Build chat-room"* — it executes step by step with test gates
6. Ask it to resume: *"Continue"* — it reads `forge-state.toml` and picks up where it left off

## How the Agent Works

```
READ state → IMPLEMENT one step → VALIDATE (check + clippy + test)
  ↓ pass                              ↓ fail (max 3 retries)
UPDATE state → COMMIT → next step     LOG blocker → STOP
```

All agent state is in `forge-state.toml` (machine-readable TOML, not prose). Plans are in `plan.toml`. Design decisions go in `decisions.md`. Library issues go in `library-feedback.md`.

## Repo Structure

```
forge-p2p/
├── CLAUDE.md                       # Project context (agent reads first)
├── .forge/
│   ├── workflow.md                 # Agent operating rules
│   ├── swarm-nl-reference.md       # SwarmNL API reference
│   ├── registry.toml               # Catalog of all apps + status
│   └── templates/
│       ├── plan.toml               # Plan template
│       └── forge-state.toml        # State template
├── shared/
│   └── forge-ui/                   # Embedded web UI + mesh visualizer
│       ├── src/                    # Axum server, WebSocket, MeshEvent
│       └── static/                 # D3.js mesh graph, layout, styles
├── apps/
│   └── <app-name>/
│       ├── forge-state.toml        # Agent state (machine-readable)
│       ├── plan.toml               # Build plan (machine-readable)
│       ├── decisions.md            # Design decisions (human-readable)
│       ├── Cargo.toml              # depends on forge-ui via path
│       ├── src/
│       ├── static/                 # App-specific UI panel
│       ├── tests/
│       └── README.md
├── library-feedback.md
└── README.md
```

## Why TOML for State?

Markdown memory files are fragile — an agent can misformat them, and parsing prose to determine "what step am I on?" is unreliable. TOML is unambiguous, trivially parseable, and hard to accidentally corrupt. The agent reads `forge-state.toml` to know exactly where it is, no interpretation needed.

## forge-ui

Every app includes a built-in web dashboard powered by [`forge-ui`](shared/forge-ui/). One `cargo run` starts both the SwarmNL node and a local web UI with:

- A **D3.js mesh visualizer** showing peers, connections, and message flow in real time
- An **event log** of network activity
- A **split layout** — your app's custom UI on the left, the mesh graph on the right

Apps push `MeshEvent`s from their SwarmNL event loop, and the browser updates live over WebSocket. See [shared/forge-ui/README.md](shared/forge-ui/README.md) for integration details.

## App Ideas

| App | Pattern | Showcases |
|-----|---------|-----------|
| Echo Gossip | gossip | Topic join, broadcast, incoming message handling |
| P2P File Index | dht + rpc | DHT store/lookup, RPC data transfer |
| Chat Room | gossip | Multi-topic pub/sub, peer presence |
| Replicated KV | replication | Consistency models, replica networks |
| Sharded Store | sharding | Custom shard algorithms, data forwarding |
| Sensor Net | gossip + dht | IoT-style broadcast with DHT discovery |

## License

Apache-2.0
