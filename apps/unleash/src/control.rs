//! Per-robot HTTP control port. Used by the scenario runner to inject
//! test-only signals (Byzantine flip, emergency shutdown).
//!
//! Stubbed in M0; wired into the robot event loop in M4.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::Result;
use axum::{extract::State, routing::post, Json, Router};
use serde::Deserialize;

/// Shared state between a robot and its control port.
#[derive(Debug, Default)]
pub struct ControlFlags {
    pub byzantine: AtomicBool,
    pub killswitch: AtomicBool,
}

impl ControlFlags {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn is_byzantine(&self) -> bool {
        self.byzantine.load(Ordering::Relaxed)
    }

    pub fn is_killed(&self) -> bool {
        self.killswitch.load(Ordering::Relaxed)
    }
}

#[derive(Debug, Deserialize)]
struct ByzReq {
    byzantine: bool,
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
        .with_state(flags);

    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Ok(())
}
