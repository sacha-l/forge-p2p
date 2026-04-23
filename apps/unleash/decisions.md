# Unleash — Design Decisions Log

Append-only record of non-obvious choices taken during the build. Read top-to-bottom for the evolution; latest decisions at the bottom.

## Scope forks against `spec.md`

`spec.md` is internally inconsistent: §3.3 names Zenoh + `zenoh-bridge-ros2dds` as the transport, while §3.4 maps behaviours onto SwarmNL primitives. §6 calls for Gazebo + PX4 SITL, §5.4 for MISTLab ROS 2 Swarm-SLAM, and §3.1 for Linux network namespaces with `tc qdisc netem`. None of these run natively on macOS (the development platform for this session), and the ForgeP2P project premise is that all apps demonstrate SwarmNL primitives through `forge-ui`.

Under user approval (2026-04-17), the following adaptations were taken:

| Spec item | Adopted approach | Reason |
|---|---|---|
| Transport: Zenoh 1.x peer mode (§3.3) | **Replaced with SwarmNL** (preserving §3.4 table verbatim) | ForgeP2P thesis; §3.4 already maps onto SwarmNL primitives |
| Physics: Gazebo + PX4 SITL (§6 M2) | **Replaced with Rust 2D kinematics stub** | Gazebo/PX4 require Linux + multi-day setup; 2D carries every coordination signal |
| SLAM: MISTLab Swarm-SLAM (§5.4) | **Replaced with Lamport-versioned grid merge on rendezvous** | ROS 2 out of scope; grid merge preserves the "bounded drift after rendezvous" demo semantics |
| Network isolation: Linux netns + tc netem (§3.1) | **Replaced with in-Rust link middleware** | No Linux; each robot applies attenuation model locally against peer poses and filters at the app layer |
| Task allocation: H-CBBA (Turpin) (§5.1) | **Plain CBBA with capability dot product** | Turpin variant adds complexity without changing the demo; scoring respects §2.6 |
| `no_std` verification for ESP32-S3 (§9.3, §11 criterion 13) | **Deferred** | User MVP slice is M0–M5; no_std is M7 |
| Multi-node testing port base | **53000 + node_index*100** | ForgeP2P convention: `50000 + app_index*1000`. Unleash is app index 3 (after echo-gossip, mesh-chat, sovereign-notes) |
| Byzantine detection (§3.5) | **W-MSR rejection only for MVP** | ed25519 signature mismatch path deferred; W-MSR alone catches the §6 Phase 4 scenario |

Deviations from `.forge/workflow.md`:

- The workflow's "~100 lines per step" rule is intentionally violated to compress the M0–M5 build into a single autonomous session per user direction (2026-04-17 "i want to come back from a walk and see what you've built"). Each milestone is still a single logical unit with its own validation gate.

## Architecture — process model

SwarmNL library limitation (`library-feedback.md` #8 — `with_rpc` handler cannot capture state, uses `OnceLock`) forces one SwarmNL node per OS process. Therefore:

- Each robot is a separate OS process launched by the scenario runner via `tokio::process::Command`.
- One observer process runs `forge-ui` on port 8080; it is also a SwarmNL peer and learns state purely from the mesh.
- The scenario runner is a parent supervisor that never itself participates in the SwarmNL network — it only orchestrates children.

This means "no central coordinator" in spec §11 is honoured: the observer is a passive listener, not an allocator.

## Link model

Each robot computes link quality to peers locally using the `environment.yaml` attenuation function — no ground-truth network view is shared. Robots subscribe to `robot/<id>/pose` (gossip, 10 Hz), and for each known peer:

1. Ray-cast from self pose to peer pose through hazard polygons and structural outline.
2. Sum attenuation (concrete_wall_db / rubble_db / free_space_db).
3. Select link profile (`default` / `degraded` / `blackout`) by attenuation threshold.
4. Filter at the app layer on every incoming gossip/RPC: drop with `loss_rate` probability; inject `latency_ms` delay via `tokio::time::sleep`.

Phase 3 degradation is a broadcast `control/link_profile` message on a reserved control gossip topic that every robot's link middleware applies as a global override for 120 s.

## Map merge substitute

Real Swarm-SLAM is out of scope (ROS 2 dependency). Substitute:

- Each robot maintains a 0.5 m cell-size 2D occupancy grid over the footprint, Lamport-versioned per cell.
- On rendezvous (two robots within 5 m line-of-sight), both publish their full grids on gossip topic `map/merge/<region>`.
- Receiver merges cell-by-cell (higher Lamport wins, ties by `robot_id`).

To make the "drift < 0.5 m per 10 m travelled" metric non-trivial, kinematics injects deterministic Gaussian pose noise scaled by distance travelled since the last rendezvous.
