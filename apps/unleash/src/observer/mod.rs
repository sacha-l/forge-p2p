//! Observer: a SwarmNL peer (no robot behaviour) that subscribes to every
//! Unleash gossip topic, aggregates state, and hosts the forge-ui dashboard.
//!
//! The observer produces no ground truth — dashboard state is strictly what
//! was heard on the mesh. Two app-specific HTTP routes are mounted via
//! `ForgeUI::with_routes`:
//!
//! * `POST /api/phase` — supervisor posts scenario phase transitions here;
//!   handler stores state and pushes a `MeshEvent::Custom` the frontend
//!   consumes.
//! * `GET  /api/mission` — returns a condensed mission briefing so the
//!   Mission panel renders without re-parsing `mission.yaml` in JS.

pub mod panels;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use axum::{extract::State, routing::{get, post}, Json, Router};
use forge_ui::{MeshEvent, UiHandle};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};

use crate::config::{Environment, Mission};
use crate::keyspace;
use crate::swarm_node;

pub struct Args {
    pub mission: Mission,
    pub env: Environment,
    pub ui_port: u16,
    pub tcp_port: u16,
    pub bootstrap: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhasePayload {
    pub phase: String,
    pub description: String,
    #[serde(default)]
    pub duration_s: u64,
    #[serde(default)]
    pub index: u32,
    #[serde(default)]
    pub total: u32,
    #[serde(default)]
    pub started_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
struct MissionSummary {
    id: String,
    objective: String,
    time_limit_s: u64,
    footprint: [f32; 2],
    floors: u8,
    known_survivors: Vec<SurvivorBriefing>,
    unknown_count: u32,
    target_count: u32,
    initial_tasks: Vec<TaskBriefing>,
    fleet: FleetBriefing,
    hazards: Vec<HazardBriefing>,
    phases: Vec<PhaseBriefing>,
}

#[derive(Debug, Clone, Serialize)]
struct SurvivorBriefing {
    id: String,
    pose: [f32; 3],
}

#[derive(Debug, Clone, Serialize)]
struct TaskBriefing {
    id: String,
    kind: String,
    pretty: String,
    urgency: f32,
}

#[derive(Debug, Clone, Serialize)]
struct FleetBriefing {
    size: u32,
    aerial_scout: u32,
    aerial_mapper: u32,
    ground_scout: u32,
    ground_workhorse: u32,
    breadcrumb: u32,
}

#[derive(Debug, Clone, Serialize)]
struct HazardBriefing {
    kind: String,
    polygon: Vec<[f32; 2]>,
    risk: f32,
}

#[derive(Debug, Clone, Serialize)]
struct PhaseBriefing {
    index: u32,
    name: &'static str,
    description: &'static str,
    duration_s: u64,
}

fn canonical_phases() -> Vec<PhaseBriefing> {
    vec![
        PhaseBriefing {
            index: 1,
            name: "Nominal",
            description: "All robots operational. CBBA converges on initial task allocation.",
            duration_s: 120,
        },
        PhaseBriefing {
            index: 2,
            name: "Dropout",
            description: "2 robots SIGKILL'd. Orphaned tasks re-enter the bidding pool; fleet reallocates within 15 s.",
            duration_s: 60,
        },
        PhaseBriefing {
            index: 3,
            name: "Degraded",
            description: "Link profile drops to 2 Mbps / 80 ms / 40 % loss. Replication lag climbs; map merges slow.",
            duration_s: 120,
        },
        PhaseBriefing {
            index: 4,
            name: "Byzantine",
            description: "One ground scout flips adversarial — inflates victim counts, falsifies pose. W-MSR rejects outlier within 5 rounds.",
            duration_s: 120,
        },
    ]
}

fn mission_summary(m: &Mission, e: &Environment) -> MissionSummary {
    MissionSummary {
        id: m.mission.id.clone(),
        objective: m.mission.objective.clone(),
        time_limit_s: m.mission.time_limit_s,
        footprint: [e.environment.footprint.x, e.environment.footprint.y],
        floors: e.environment.floors,
        known_survivors: m
            .mission
            .known_targets
            .iter()
            .map(|t| SurvivorBriefing {
                id: t.id.clone(),
                pose: [t.pose.x, t.pose.y, t.pose.z],
            })
            .collect(),
        unknown_count: m.mission.unknown_targets,
        target_count: m.mission.target_count,
        initial_tasks: m
            .mission
            .initial_tasks
            .iter()
            .map(|t| TaskBriefing {
                id: t.id.clone(),
                kind: t.kind.clone(),
                pretty: pretty_task_name(&t.kind, &t.id),
                urgency: t.urgency,
            })
            .collect(),
        fleet: FleetBriefing {
            size: m.fleet.size,
            aerial_scout: m.fleet.composition.aerial_scout,
            aerial_mapper: m.fleet.composition.aerial_mapper,
            ground_scout: m.fleet.composition.ground_scout,
            ground_workhorse: m.fleet.composition.ground_workhorse,
            breadcrumb: m.fleet.composition.breadcrumb,
        },
        hazards: e
            .environment
            .hazards
            .iter()
            .map(|h| HazardBriefing {
                kind: h.kind.clone(),
                polygon: h.polygon.clone(),
                risk: h.risk,
            })
            .collect(),
        phases: canonical_phases(),
    }
}

fn pretty_task_name(kind: &str, id: &str) -> String {
    match kind {
        "survey_area" => "Survey footprint".into(),
        "establish_perimeter_mesh" => "Establish perimeter mesh".into(),
        "inspect_poi" => "Inspect point of interest".into(),
        "find_victim" => "Rescue — find victim".into(),
        "relay_hold" => "Hold relay position".into(),
        "escort" => "Escort".into(),
        "deploy_node" => "Deploy breadcrumb node".into(),
        _ => id.replace('_', " "),
    }
}

struct ObserverState {
    ui: UiHandle,
    mission: MissionSummary,
    current_phase: RwLock<Option<PhasePayload>>,
}

pub async fn run(args: Args) -> Result<()> {
    let Args {
        mission,
        env,
        ui_port,
        tcp_port,
        bootstrap,
    } = args;

    tracing::info!(tcp_port, ui_port, "starting observer");

    let udp_port = tcp_port + 1;
    let mut node = swarm_node::build_node(tcp_port, udp_port, bootstrap.as_deref()).await?;
    let (peer_id, addrs) = swarm_node::drain_listen_addrs(&mut node).await;
    println!(
        "UNLEASH_OBSERVER_READY peer_id={} tcp={} addr={}",
        peer_id,
        tcp_port,
        swarm_node::loopback_multiaddr(tcp_port)
    );

    for topic in keyspace::default_topics() {
        let req = swarm_nl::core::AppData::GossipsubJoinNetwork(topic.to_string());
        let _ = node.send_to_network(req).await;
    }
    let _ = tokio::time::timeout(
        Duration::from_secs(2),
        node.join_repl_network(keyspace::REPL_SURVIVORS.to_string()),
    )
    .await;

    let static_dir = resolve_static_dir()?;
    tracing::info!(static_dir = ?static_dir, "mounting forge-ui");

    // Prepare forge-ui with app-specific routes (/api/phase, /api/mission).
    let (ui_tx, _ui_rx) = tokio::sync::broadcast::channel::<MeshEvent>(1);
    let _ = ui_tx; // unused; real tx lives inside forge-ui
    let mission_json = mission_summary(&mission, &env);
    // We need the UiHandle inside the /api/phase handler. Build forge-ui
    // first, then register routes on a fresh Router and let forge-ui merge.
    // Since `with_routes` must be called before `start`, we construct a
    // placeholder state now and populate it with the UiHandle after start.
    let shared_ui: Arc<RwLock<Option<UiHandle>>> = Arc::new(RwLock::new(None));
    let shared_phase: Arc<RwLock<Option<PhasePayload>>> = Arc::new(RwLock::new(None));
    let shared_mission = Arc::new(mission_json);

    let api_state = Arc::new(ApiState {
        ui: Arc::clone(&shared_ui),
        phase: Arc::clone(&shared_phase),
        mission: Arc::clone(&shared_mission),
    });
    let app_routes = build_api_router(api_state);

    let ui = forge_ui::ForgeUI::new()
        .with_port(ui_port)
        .with_app_name("Unleash")
        .with_app_static_dir(static_dir.to_str().expect("utf8 static dir"))
        .with_local_peer_id(&peer_id)
        .with_routes(app_routes)
        .start()
        .await?;
    *shared_ui.write().await = Some(ui.clone());

    ui.push(MeshEvent::NodeStarted {
        peer_id: peer_id.clone(),
        listen_addrs: addrs.clone(),
    })
    .await;

    // Emit an initial "booting" phase so the dashboard banner has text
    // before the supervisor announces Phase 1.
    ui.push(MeshEvent::Custom {
        label: "unleash/phase".into(),
        detail: serde_json::to_string(&PhasePayload {
            phase: "booting".into(),
            description: "Scenario starting — waiting for the mesh to warm up.".into(),
            duration_s: 0,
            index: 0,
            total: 4,
            started_ms: keyspace::now_ms(),
        })
        .unwrap_or_default(),
    })
    .await;

    let aggregator = panels::Aggregator::new(mission.clone(), env.clone(), ui.clone());
    let node = Arc::new(Mutex::new(node));

    loop {
        {
            let mut node = node.lock().await;
            while let Some(event) = node.next_event().await {
                aggregator.apply_event(event).await;
            }
        }
        aggregator.tick().await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

struct ApiState {
    ui: Arc<RwLock<Option<UiHandle>>>,
    phase: Arc<RwLock<Option<PhasePayload>>>,
    mission: Arc<MissionSummary>,
}

fn build_api_router(state: Arc<ApiState>) -> Router {
    Router::new()
        .route("/api/phase", post(phase_handler))
        .route("/api/phase", get(phase_get_handler))
        .route("/api/mission", get(mission_handler))
        .with_state(state)
}

async fn phase_handler(
    State(state): State<Arc<ApiState>>,
    Json(mut payload): Json<PhasePayload>,
) -> Json<PhasePayload> {
    if payload.started_ms == 0 {
        payload.started_ms = keyspace::now_ms();
    }
    *state.phase.write().await = Some(payload.clone());
    if let Some(ui) = state.ui.read().await.clone() {
        ui.push(MeshEvent::Custom {
            label: "unleash/phase".into(),
            detail: serde_json::to_string(&payload).unwrap_or_default(),
        })
        .await;
    }
    Json(payload)
}

async fn phase_get_handler(State(state): State<Arc<ApiState>>) -> Json<Option<PhasePayload>> {
    Json(state.phase.read().await.clone())
}

async fn mission_handler(State(state): State<Arc<ApiState>>) -> Json<MissionSummary> {
    Json((*state.mission).clone())
}

fn resolve_static_dir() -> Result<PathBuf> {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let candidate = crate_dir.join("static");
    if candidate.is_dir() {
        return Ok(candidate);
    }
    let cwd = std::env::current_dir()?.join("static");
    if cwd.is_dir() {
        return Ok(cwd);
    }
    anyhow::bail!(
        "could not locate `static/` directory — tried {:?} and {:?}",
        candidate,
        cwd
    );
}
