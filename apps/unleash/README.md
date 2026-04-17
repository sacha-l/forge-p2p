# Unleash — Heterogeneous Robot Swarm Coordination Demo

A ForgeP2P cookbook app demonstrating every SwarmNL primitive (Kademlia DHT, GossipSub, request-response RPC, replication, sharding) load-bearing in a simulated 10-robot disaster-relief swarm.

Spec: see `../../../spec.md` (in repo root) for the full scenario, fleet roster, and success criteria. Scope forks taken in this implementation: see `decisions.md`.

## Quick start

```bash
# from forge-p2p/apps/unleash/
cargo build --release
cargo run --release -- run scenarios/disaster_relief/
# then open http://localhost:8080
```

The run spawns 10 robot processes + 1 observer process and plays the 420 s 4-phase scenario:

| t | Phase | What happens |
|---|---|---|
| 0 s | Nominal | All robots operational. CBBA allocates initial tasks. |
| 120 s | Dropout | 2 robots SIGKILL'd. Remaining fleet reallocates within 15 s. |
| 180 s | Degraded | Link profile drops to 2 Mbps / 80 ms / 40 % loss. Replication lag visible. |
| 300 s | Byzantine | One ground scout flips to adversarial mode. W-MSR rejects. |
| 420 s | End | Final phase report printed. |

## Subcommands

```
unleash robot     --id <name> --class <class> --node-index <i> --scenario <dir> [--bootstrap <peer:addr>]
unleash observer  --scenario <dir> [--ui-port 8080] [--bootstrap <peer:addr>]
unleash run       <scenario-dir>
```

`run` is the standard entry point. `robot` and `observer` are used internally by the runner but can be invoked by hand for debugging.

## Scenario files

`scenarios/<name>/mission.yaml` + `scenarios/<name>/environment.yaml` drive everything. See `scenarios/disaster_relief/` for the reference; alternative scenarios are new YAML pairs, not new code.

## Architecture

```
runner  ──spawns──▶ robot_0 (SwarmNL node, port 53000)
                    robot_1 (SwarmNL node, port 53100)
                    ...
                    robot_N (SwarmNL node, port 53N00)
                    observer (SwarmNL node, port 53900, forge-ui on :8080)
```

No central coordinator. Robots discover each other via Kademlia DHT bootstrap, advertise capabilities via DHT records, announce tasks + bid via RPC, and replicate survivor findings via SwarmNL's replication primitive.
