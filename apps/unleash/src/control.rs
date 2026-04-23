//! Per-robot HTTP control port. Used by the scenario runner to inject
//! test-only signals (Byzantine flip, emergency shutdown, link-profile
//! override to be relayed via gossip).

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::Result;
use axum::{extract::State, routing::post, Json, Router};
use serde::Deserialize;
use tokio::sync::mpsc;

use crate::keyspace::LinkProfileOverride;

#[derive(Debug)]
pub struct ControlFlags {
    pub byzantine: AtomicBool,
    pub killswitch: AtomicBool,
    /// When a Phase 3 override arrives, we push it here for the robot's
    /// net loop to broadcast on the control topic.
    pub link_override_tx: Mutex<Option<mpsc::Sender<LinkProfileOverride>>>,
}

use tokio::sync::Mutex;

impl ControlFlags {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            byzantine: AtomicBool::new(false),
            killswitch: AtomicBool::new(false),
            link_override_tx: Mutex::new(None),
        })
    }

    pub fn is_byzantine(&self) -> bool {
        self.byzantine.load(Ordering::Relaxed)
    }

    pub fn is_killed(&self) -> bool {
        self.killswitch.load(Ordering::Relaxed)
    }

    pub async fn set_link_override_sink(&self, tx: mpsc::Sender<LinkProfileOverride>) {
        *self.link_override_tx.lock().await = Some(tx);
    }
}

impl Default for ControlFlags {
    fn default() -> Self {
        Self {
            byzantine: AtomicBool::new(false),
            killswitch: AtomicBool::new(false),
            link_override_tx: Mutex::new(None),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ByzReq {
    byzantine: bool,
}

#[derive(Debug, Deserialize)]
struct DegradeReq {
    profile: String,
    #[serde(default = "default_duration")]
    duration_ms: u64,
}

fn default_duration() -> u64 {
    120_000
}

pub async fn serve(port: u16, flags: Arc<ControlFlags>) -> Result<()> {
    let app = Router::new()
        .route(
            "/byzantine",
            post(|State(f): State<Arc<ControlFlags>>, Json(b): Json<ByzReq>| async move {
                f.byzantine.store(b.byzantine, Ordering::Relaxed);
                Json(serde_json::json!({"byzantine": b.byzantine}))
            }),
        )
        .route(
            "/kill",
            post(|State(f): State<Arc<ControlFlags>>| async move {
                f.killswitch.store(true, Ordering::Relaxed);
                Json(serde_json::json!({"killed": true}))
            }),
        )
        .route(
            "/degrade",
            post(|State(f): State<Arc<ControlFlags>>, Json(b): Json<DegradeReq>| async move {
                let sink = f.link_override_tx.lock().await.clone();
                let msg = LinkProfileOverride {
                    profile: b.profile.clone(),
                    started_at_ms: crate::keyspace::now_ms(),
                    duration_ms: b.duration_ms,
                };
                if let Some(tx) = sink {
                    let _ = tx.send(msg).await;
                }
                Json(serde_json::json!({"profile": b.profile, "duration_ms": b.duration_ms}))
            }),
        )
        .with_state(flags);

    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Ok(())
}
