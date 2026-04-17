# Unleash — Known Issues & Follow-Ups

Items worth fixing in a second pass. Logged during the initial build; feel free to tackle in any order.

## Functional gaps to verify / fix

- **Startup takes ~25 s per process** (observer and robot). The blocking calls to `node.join_repl_network(...)` are wrapped in a 2 s timeout, but when `CoreBuilder::build()` runs under contention from 10 other peers joining simultaneously, listen addresses can take longer to enumerate. The runner's `phase_dropout`/`phase_degrade`/`phase_byzantine` timers start at t = 0 regardless of whether children are truly up — if startup is slow, Phase 2 SIGKILLs may fire before the target robot finishes initialising. Mitigation: bump the warmup sleep in `runner/supervisor.rs::launch_all` from 8 s to something like 25 s, or add explicit "all ready" probes on the control ports.

- **Robot behaviour is reactive drift, not goal-seeking.** `robot/kinematics.rs` jitter-steers rather than planning toward a task's geometry. `pick_goal_for` returns a trivial wiggle. Result: robots rarely approach known survivors, so the Survivor panel mostly stays empty unless a robot happens to drift within 6–8 m. Proper fix: thread the won-task geometry through to `kin.steer_toward(target_geometry)` each tick.

- **GridChunk gossip includes only metadata in the `Custom` event for the Map panel.** Full grid cells arrive over `TOPIC_MAP_MERGE` but aren't rebroadcast to the UI. The Map panel's occupancy overlay is therefore not populated from real gossip today — survivors and robot positions render, grid cells don't. Fix: in `observer/panels.rs::handle_grid`, also emit a `unleash/grid_cells` Custom event carrying the cell coordinates.

- **Byzantine detection uses a magic threshold** (`> 100.0` in `robot/mod.rs::dispatch_event`). If the honest swarm ever legitimately reports > 100 survivors, it breaks. Fix: compute a running median of honest values and flag outliers relative to it, or add the signature-based path per spec §3.5.

- **Task re-announce interval is 10 s and unconditional.** Once every task has a stable winner, re-announcing is noise. Fix: skip re-announce for tasks where `cbba.current_assignments()[tid]` has been stable for ≥ 3 rounds.

- **`library-feedback.md` entries logged upstream-worthy:**
  - `Core::join_repl_network` blocks ~30 s with no peers; workaround is a 2 s `tokio::time::timeout`.
  - `AppData::GossipsubJoinNetwork` via `query_network` blocks ~3 s per topic; workaround is `send_to_network`.

  Please PR these upstream when convenient (see `.forge/workflow.md` §6).

## Port & platform notes

- The observer defaults to port **8088** because 8080 was already bound by a separate running `mesh-chat` on the dev machine. `unleash run scenarios/disaster_relief` uses 8088 unless you override with `--ui-port`.

- On macOS, no Linux network namespaces are used (see `decisions.md`). All link degradation is simulated by the in-Rust middleware in `src/link_model.rs`. If you run this on Linux in the future and want real netem, you can add a shell script per robot that enters a network namespace before exec'ing the binary — nothing in the Rust code assumes in-process networking.

## Out of scope for this slice (M0–M5)

- **M6 scaling validation** (fleet sizes 20 / 50) — the `fleet.size` path is wired and `scenarios/` can hold alternate YAMLs, but a scripted sweep + metrics table aren't built.
- **M7** — entropy test, alt-scenario smoke test, recorded demo, `no_std` compile verification for the breadcrumb target.
- **Full Swarm-SLAM** — replaced with Lamport grid merge (see `decisions.md`). Drift-per-metre metric is stubbed.
- **H-CBBA variant** — plain CBBA only.
- **Ed25519 message signing** — Byzantine detection relies on W-MSR outlier rejection alone.

## Quick smoke-test recipe

```bash
cd forge-p2p/apps/unleash
cargo build --release
./target/release/unleash validate scenarios/disaster_relief   # YAML sanity
./target/release/unleash run scenarios/disaster_relief        # full scenario
# open http://localhost:8088
```

Full scenario runs 420 s. To observe early phases only, Ctrl-C after ~150 s (Phase 2 dropout is visible by then).

## Where to look first when something fails

- **Robot not starting** → `tracing` is routed to stderr. Run with `RUST_LOG=debug,unleash=trace ./target/release/unleash robot …` for per-event logs.
- **Dashboard empty** → check that the observer's SwarmNL node actually joined the mesh: `curl http://localhost:8088/api/node/info` and verify `peer_id` is populated; then watch `/ws` traffic in the browser's Network tab.
- **Phases firing too early** → `runner/supervisor.rs::launch_all` sleeps 8 s before returning control to `scenario::run`. Increase if children aren't ready.
- **Port 8088 in use** → pass `--ui-port 8099` (or any free port) to `unleash run`.
