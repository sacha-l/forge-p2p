//! Aggregator: consume the mesh, fold into dashboard state, emit MeshEvents.
//!
//! M0 stub — the aggregator just forwards PeerConnected / PeerDisconnected.
//! M5 adds per-panel state (cluster, latency ring, replication lag, tasks,
//! consensus, map).

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Arc;

use forge_ui::{MeshEvent, UiHandle};
use swarm_nl::core::NetworkEvent;
use tokio::sync::RwLock;

use crate::config::{Environment, Mission, Pose3};
use crate::keyspace::{self, decode_str, BundleAnnouncement, ConsensusValue, GridChunk, LinkProfileOverride, PoseHeartbeat, RobotClass, RobotStatus, SurvivorReport, TaskWinner};

#[derive(Debug, Clone)]
pub struct PanelRobot {
    pub robot_id: String,
    pub class: RobotClass,
    pub pose: Pose3,
    pub battery: f32,
    pub status: RobotStatus,
    pub last_update_ms: u64,
}

#[derive(Debug, Default)]
pub struct AggregatorState {
    pub robots: HashMap<String, PanelRobot>,
    pub tasks: BTreeMap<String, TaskState>,
    pub survivors: HashMap<String, SurvivorReport>,
    pub consensus: HashMap<String, VecDeque<ConsensusValue>>,
    pub grid: HashMap<(u8, u16, u16), (u8, u64, String)>, // (floor,x,y) -> (occupancy,lamport,robot)
    pub link_override: Option<String>,
    pub replication_lag_ms: VecDeque<(u64, f32, f32)>, // (ts, p50, p95)
    pub bundles: HashMap<String, Vec<(String, f32)>>,
}

#[derive(Debug, Clone)]
pub struct TaskState {
    pub task_id: String,
    pub winner: Option<String>,
    pub score: Option<f32>,
    pub updated_ms: u64,
}

pub struct Aggregator {
    mission: Mission,
    env: Environment,
    ui: UiHandle,
    state: Arc<RwLock<AggregatorState>>,
}

impl Aggregator {
    pub fn new(mission: Mission, env: Environment, ui: UiHandle) -> Arc<Self> {
        Arc::new(Self {
            mission,
            env,
            ui,
            state: Arc::new(RwLock::new(AggregatorState::default())),
        })
    }

    pub fn state(&self) -> Arc<RwLock<AggregatorState>> {
        Arc::clone(&self.state)
    }

    pub async fn apply_event(&self, event: NetworkEvent) {
        match event {
            NetworkEvent::ConnectionEstablished { peer_id, .. } => {
                self.ui
                    .push(MeshEvent::PeerConnected {
                        peer_id: peer_id.to_string(),
                        addr: String::new(),
                    })
                    .await;
            }
            NetworkEvent::ConnectionClosed { peer_id, .. } => {
                self.ui
                    .push(MeshEvent::PeerDisconnected {
                        peer_id: peer_id.to_string(),
                    })
                    .await;
            }
            NetworkEvent::GossipsubIncomingMessageHandled { source, data } => {
                self.handle_gossip(&source.to_string(), &data).await;
            }
            NetworkEvent::ReplicaDataIncoming {
                data: _,
                network,
                source,
                ..
            } => {
                if network == keyspace::REPL_SURVIVORS {
                    self.ui
                        .push(MeshEvent::ReplicaSync {
                            peer_id: source.to_string(),
                            network,
                            status: "incoming".into(),
                        })
                        .await;
                }
            }
            _ => {}
        }
    }

    async fn handle_gossip(&self, source: &str, data: &[String]) {
        // Try each schema in turn. The first that decodes wins.
        if let Some(hb) = decode_str::<PoseHeartbeat>(data) {
            self.handle_pose(hb).await;
            return;
        }
        if let Some(tw) = decode_str::<TaskWinner>(data) {
            self.handle_task_winner(tw).await;
            return;
        }
        if let Some(ba) = decode_str::<BundleAnnouncement>(data) {
            self.handle_bundle(ba).await;
            return;
        }
        if let Some(cv) = decode_str::<ConsensusValue>(data) {
            self.handle_consensus(cv).await;
            return;
        }
        if let Some(sr) = decode_str::<SurvivorReport>(data) {
            self.handle_survivor(sr).await;
            return;
        }
        if let Some(gc) = decode_str::<GridChunk>(data) {
            self.handle_grid(gc).await;
            return;
        }
        if let Some(lp) = decode_str::<LinkProfileOverride>(data) {
            self.handle_link_override(lp).await;
            return;
        }
        tracing::trace!(source, "unhandled gossip message");
    }

    async fn handle_pose(&self, hb: PoseHeartbeat) {
        {
            let mut s = self.state.write().await;
            s.robots.insert(
                hb.robot_id.clone(),
                PanelRobot {
                    robot_id: hb.robot_id.clone(),
                    class: hb.class,
                    pose: hb.pose,
                    battery: hb.battery,
                    status: hb.status,
                    last_update_ms: hb.ts_ms,
                },
            );
        }
        self.push_custom(
            "unleash/pose",
            &serde_json::json!({
                "robot_id": hb.robot_id,
                "class": hb.class.as_str(),
                "pose": hb.pose,
                "battery": hb.battery,
                "status": hb.status,
                "ts_ms": hb.ts_ms
            }),
        )
        .await;
    }

    async fn handle_task_winner(&self, tw: TaskWinner) {
        {
            let mut s = self.state.write().await;
            s.tasks.insert(
                tw.task_id.clone(),
                TaskState {
                    task_id: tw.task_id.clone(),
                    winner: Some(tw.winner.clone()),
                    score: Some(tw.bid_score),
                    updated_ms: tw.ts_ms,
                },
            );
        }
        self.push_custom("unleash/task_winner", &serde_json::to_value(&tw).unwrap_or_default())
            .await;
    }

    async fn handle_bundle(&self, ba: BundleAnnouncement) {
        {
            let mut s = self.state.write().await;
            s.bundles.insert(ba.robot_id.clone(), ba.bundle.clone());
        }
        self.push_custom(
            "unleash/bundle",
            &serde_json::to_value(&ba).unwrap_or_default(),
        )
        .await;
    }

    async fn handle_consensus(&self, cv: ConsensusValue) {
        {
            let mut s = self.state.write().await;
            let q = s.consensus.entry(cv.robot_id.clone()).or_default();
            q.push_back(cv.clone());
            while q.len() > 200 {
                q.pop_front();
            }
        }
        self.push_custom(
            "unleash/consensus",
            &serde_json::to_value(&cv).unwrap_or_default(),
        )
        .await;
    }

    async fn handle_survivor(&self, sr: SurvivorReport) {
        {
            let mut s = self.state.write().await;
            s.survivors.insert(sr.survivor_id.clone(), sr.clone());
        }
        self.push_custom(
            "unleash/survivor",
            &serde_json::to_value(&sr).unwrap_or_default(),
        )
        .await;
    }

    async fn handle_grid(&self, gc: GridChunk) {
        {
            let mut s = self.state.write().await;
            for cell in &gc.cells {
                let key = (gc.floor, cell.x, cell.y);
                let entry = s
                    .grid
                    .entry(key)
                    .or_insert((cell.occupancy, cell.lamport, cell.updated_by.clone()));
                if cell.lamport > entry.1
                    || (cell.lamport == entry.1 && cell.updated_by > entry.2)
                {
                    *entry = (cell.occupancy, cell.lamport, cell.updated_by.clone());
                }
            }
        }
        self.push_custom(
            "unleash/grid",
            &serde_json::json!({
                "floor": gc.floor,
                "robot_id": gc.robot_id,
                "cell_count": gc.cells.len(),
                "ts_ms": gc.ts_ms
            }),
        )
        .await;
    }

    async fn handle_link_override(&self, lp: LinkProfileOverride) {
        {
            let mut s = self.state.write().await;
            s.link_override = Some(lp.profile.clone());
        }
        self.push_custom(
            "unleash/link_override",
            &serde_json::to_value(&lp).unwrap_or_default(),
        )
        .await;
    }

    pub async fn tick(&self) {
        // Emit a heartbeat with aggregate state, useful for the UI init
        // and for the consensus-panel scrolling charts.
        let s = self.state.read().await;
        self.push_custom(
            "unleash/tick",
            &serde_json::json!({
                "robot_count": s.robots.len(),
                "task_count": s.tasks.len(),
                "survivor_count": s.survivors.len(),
                "link_override": s.link_override,
            }),
        )
        .await;
    }

    async fn push_custom(&self, label: &str, v: &serde_json::Value) {
        self.ui
            .push(MeshEvent::Custom {
                label: label.into(),
                detail: v.to_string(),
            })
            .await;
    }
}

