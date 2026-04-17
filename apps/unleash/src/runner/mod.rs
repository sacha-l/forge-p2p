//! Scenario runner — parent supervisor that spawns the fleet and drives
//! the 4-phase state machine.
//!
//! M0 stub: just enough to compile. M4 fills in process supervision.

pub mod report;
pub mod scenario;
pub mod supervisor;

use std::path::PathBuf;

use anyhow::Result;

use crate::config::{Environment, Mission};

pub struct RunArgs {
    pub scenario_dir: PathBuf,
    pub mission: Mission,
    pub env: Environment,
    pub ui_port: u16,
    pub self_path: PathBuf,
}

pub async fn run_scenario(args: RunArgs) -> Result<()> {
    scenario::run(args).await
}
