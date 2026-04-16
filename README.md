# ForgeP2P

Working peer-to-peer apps built on [SwarmNL](https://github.com/algorealmInc/SwarmNL), plus an agentic workflow for adding new ones.

## What This Is

**A growing cookbook of SwarmNL apps.** Read them, run them, copy the patterns. Every app ships with an embedded web UI and a live mesh visualizer via [`forge-ui`](shared/forge-ui/), so you can actually see gossip propagate, peers dial each other, and DHT lookups resolve — not just watch stdout scroll by.

**An opinionated agentic workflow for building more.** A coding agent reads the reference docs, plans step-by-step, and implements one step at a time behind a `cargo check + clippy + test` gate. State lives in machine-readable TOML so work resumes across sessions without prose interpretation.

You can use ForgeP2P either way: as a reference implementation of SwarmNL patterns, or as a scaffold to ship a new app with the agent.

## Existing Apps

Each app lives on its own branch with a full README, build instructions, and tests.

| App | Branch | Pattern | What it shows |
|-----|--------|---------|---------------|
| **Echo Gossip** | [`dev/echo-gossip`](https://github.com/sacha-l/forge-p2p/tree/dev/echo-gossip/apps/echo-gossip) | gossip | Topic join, broadcast, incoming message handling. The simplest SwarmNL app. |
| **Mesh Chat** | [`dev/mesh-chat`](https://github.com/sacha-l/forge-p2p/tree/dev/mesh-chat/apps/mesh-chat) | gossip | Two peers auto-discover via mDNS, chat over a shared topic, with a dual-view web UI. |
| **Sovereign Notes** | [`dev/sovereign-notes`](https://github.com/sacha-l/forge-p2p/tree/dev/sovereign-notes/apps/sovereign-notes) | replication + gossip + RPC | Distributed note-taking with CRUD, gossip announcements, and RPC content pull. |

**Try one:**

```sh
git checkout dev/mesh-chat
cd apps/mesh-chat
cargo run
# open http://localhost:50100 in your browser
```

## Build Your Own App

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
│       └── static/                 # Vanilla-JS mesh graph, layout, styles
├── apps/                            # Empty on main — complete apps live on dev/* branches
│   └── <app-name>/
│       ├── forge-state.toml        # Agent state (machine-readable)
│       ├── plan.toml               # Build plan (machine-readable)
│       ├── decisions.md            # Design decisions (human-readable)
│       ├── Cargo.toml              # depends on forge-ui via path
│       ├── src/
│       ├── static/                 # App-specific UI panel
│       ├── tests/
│       └── README.md
├── library-feedback.md            # Shared SwarmNL papercut log (PR entries upstream!)
├── CONTRIBUTING.md                # How to PR a library-feedback entry from a fork
└── README.md
```

## Why TOML for State?

Markdown memory files are fragile — an agent can misformat them, and parsing prose to determine "what step am I on?" is unreliable. TOML is unambiguous, trivially parseable, and hard to accidentally corrupt. The agent reads `forge-state.toml` to know exactly where it is, no interpretation needed.

## forge-ui

Every app includes a built-in web dashboard powered by [`forge-ui`](shared/forge-ui/). One `cargo run` starts both the SwarmNL node and a local web UI with:

- A **mesh visualizer** (dependency-free vanilla JS) showing peers, connections, and message flow in real time
- An **event log** of network activity
- A **split layout** — your app's custom UI on the left, the mesh graph on the right

Apps push `MeshEvent`s from their SwarmNL event loop, and the browser updates live over WebSocket. See [shared/forge-ui/README.md](shared/forge-ui/README.md) for integration details.

## More App Ideas

| App | Pattern | Showcases |
|-----|---------|-----------|
| P2P File Index | dht + rpc | DHT store/lookup, RPC data transfer |
| Replicated KV | replication | Consistency models, replica networks |
| Sharded Store | sharding | Custom shard algorithms, data forwarding |
| Sensor Net | gossip + dht | IoT-style broadcast with DHT discovery |

## Using SwarmNL anywhere? Contribute feedback

[`library-feedback.md`](library-feedback.md) is a shared log of SwarmNL API papercuts and workarounds discovered while building real apps. It accumulates across every build so nobody has to rediscover the same issue twice.

**If you hit something in SwarmNL — in this repo, in your own fork, or in an unrelated project — please PR an entry.** A small PR that touches only `library-feedback.md` is the fastest to merge. See [CONTRIBUTING.md](CONTRIBUTING.md) for the 60-second recipe.

## License

Apache-2.0
