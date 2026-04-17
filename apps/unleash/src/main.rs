#![allow(dead_code)]
//! Unleash — heterogeneous robot swarm coordination demo.
//!
//! Three subcommands:
//!
//! * `robot`    — one SwarmNL node running robot behaviours
//! * `observer` — one SwarmNL node hosting the forge-ui dashboard at :8080
//! * `run`      — parent supervisor that spawns the fleet and orchestrates
//!   the 4-phase scenario
//!
//! See `README.md` and `spec.md` for the full scenario.

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

mod config;
mod control;
mod coord;
mod keyspace;
mod link_model;
mod observer;
mod robot;
mod runner;
mod swarm_node;

use crate::keyspace::RobotClass;

#[derive(Parser, Debug)]
#[command(name = "unleash", about = "Heterogeneous robot swarm coordination demo")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run a single robot node.
    Robot {
        #[arg(long)]
        id: String,
        #[arg(long)]
        class: String,
        #[arg(long)]
        node_index: u32,
        #[arg(long)]
        scenario: PathBuf,
        /// Bootstrap peer in `peer_id@multiaddr` form.
        #[arg(long)]
        bootstrap: Option<String>,
        /// Exit after listening to setup events (used by `cargo test`).
        #[arg(long, default_value_t = false)]
        smoke: bool,
        /// Per-robot HTTP control port (Byzantine flip, runtime knobs).
        #[arg(long)]
        control_port: Option<u16>,
    },
    /// Run the observer node (SwarmNL peer + forge-ui dashboard).
    Observer {
        #[arg(long)]
        scenario: PathBuf,
        #[arg(long, default_value_t = 8080)]
        ui_port: u16,
        #[arg(long, default_value_t = 53900)]
        tcp_port: u16,
        #[arg(long)]
        bootstrap: Option<String>,
    },
    /// Run the full scenario: spawn robots + observer, drive 4 phases.
    Run {
        scenario: PathBuf,
        #[arg(long, default_value_t = 8080)]
        ui_port: u16,
        /// Path to the unleash binary; defaults to the current executable.
        #[arg(long)]
        self_path: Option<PathBuf>,
    },
    /// Validate YAML scenario files without starting the network.
    Validate { scenario: PathBuf },
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let cli = Cli::parse();

    match cli.command {
        Command::Validate { scenario } => {
            let (mission, env) = config::load_scenario(&scenario)?;
            println!(
                "OK: mission id={} tasks={} fleet_size={} floors={}",
                mission.mission.id,
                mission.mission.initial_tasks.len(),
                mission.fleet.size,
                env.environment.floors
            );
            Ok(())
        }
        Command::Robot {
            id,
            class,
            node_index,
            scenario,
            bootstrap,
            smoke,
            control_port,
        } => {
            let (mission, env) = config::load_scenario(&scenario)?;
            let class = RobotClass::parse(&class)
                .ok_or_else(|| anyhow::anyhow!("unknown robot class: {class}"))?;
            robot::run(robot::Args {
                id,
                class,
                node_index,
                mission,
                env,
                bootstrap,
                smoke,
                control_port,
            })
            .await
        }
        Command::Observer {
            scenario,
            ui_port,
            tcp_port,
            bootstrap,
        } => {
            let (mission, env) = config::load_scenario(&scenario)?;
            observer::run(observer::Args {
                mission,
                env,
                ui_port,
                tcp_port,
                bootstrap,
            })
            .await
        }
        Command::Run {
            scenario,
            ui_port,
            self_path,
        } => {
            let (mission, env) = config::load_scenario(&scenario)?;
            let self_path = self_path.unwrap_or_else(|| std::env::current_exe().expect("self path"));
            runner::run_scenario(runner::RunArgs {
                scenario_dir: scenario,
                mission,
                env,
                ui_port,
                self_path,
            })
            .await
        }
    }
}

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,swarm_nl=warn,h2=warn,hyper=warn,reqwest=warn"));
    fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_line_number(false)
        .compact()
        .init();
}
