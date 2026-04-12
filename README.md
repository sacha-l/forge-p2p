# ForgeP2P

An agentic workflow for building peer-to-peer applications with [SwarmNL](https://github.com/algorealmInc/SwarmNL).

## What This Is

ForgeP2P pairs a coding agent with structured SwarmNL documentation to build, test, and showcase P2P networked applications. The agent follows a strict plan-implement-test loop with machine-readable state, so it can resume autonomously across sessions and never ship untested code.

## Quick Start

### Prerequisites
- Rust toolchain (stable, edition 2021)
- A coding agent that reads markdown/TOML instructions (e.g. Claude Code, Cursor, Aider)

### Setup

```bash
# Fork on GitHub, then:
git clone https://github.com/<your-username>/forge-p2p.git
cd forge-p2p
```

### First Command

Open the repo in your coding agent and tell it:

> Read `CLAUDE.md`, then `.forge/workflow.md`, then `.forge/swarm-nl-reference.md`. Plan a new app: **\<describe your app here\>**. Present the plan for my review before writing any code.

### Usage

1. Review the plan in `apps/<name>/plan.toml`
2. Tell the agent: *"Build \<app-name\>"* — it executes step by step with test gates
3. To resume later: *"Continue"* — it reads `forge-state.toml` and picks up where it left off

## How the Agent Works

```
READ state → IMPLEMENT one step → VALIDATE (check + clippy + test)
  ↓ pass                              ↓ fail (max 3 retries)
UPDATE state → COMMIT → next step     LOG blocker → STOP
```

All agent state is in `forge-state.toml` (machine-readable TOML, not prose). Plans are in `plan.toml`. Design decisions go in `decisions.md`. Library issues go in `library-feedback.md`.

## Branching

`main` holds only the workflow templates and reference docs — no app code. Each app is built on its own branch:

```
main
 └─ dev/echo-gossip
 └─ dev/file-index
 └─ dev/your-app
```

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
├── apps/
│   └── <app-name>/
│       ├── forge-state.toml        # Agent state (machine-readable)
│       ├── plan.toml               # Build plan (machine-readable)
│       ├── decisions.md            # Design decisions (human-readable)
│       ├── Cargo.toml
│       ├── src/
│       ├── tests/
│       └── README.md
├── library-feedback.md
└── README.md
```

## Why TOML for State?

Markdown memory files are fragile — an agent can misformat them, and parsing prose to determine "what step am I on?" is unreliable. TOML is unambiguous, trivially parseable, and hard to accidentally corrupt. The agent reads `forge-state.toml` to know exactly where it is, no interpretation needed.

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