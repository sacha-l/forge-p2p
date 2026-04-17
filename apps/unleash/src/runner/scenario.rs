//! Four-phase state machine. Broadcast control messages on reserved topics,
//! capture per-phase metrics, emit phase reports to stdout.
//!
//! Phases follow spec §6. M0 stub — M4 wires in actual control.

use std::time::{Duration, Instant};

use anyhow::Result;

use super::report::PhaseReport;
use super::supervisor::Supervisor;
use crate::runner::RunArgs;

pub async fn run(args: RunArgs) -> Result<()> {
    let mut sup = Supervisor::new(args);
    sup.launch_all().await?;

    let start = Instant::now();

    // Phase 1 — Nominal (0–120s)
    run_phase(&mut sup, "Phase 1 — Nominal", Duration::from_secs(120), start, |_| async { Ok(()) }).await?;

    // Phase 2 — Dropout at t=120s
    sup.phase_dropout().await?;
    run_phase(&mut sup, "Phase 2 — Dropout", Duration::from_secs(60), start, |_| async { Ok(()) }).await?;

    // Phase 3 — Comms degradation at t=180s
    sup.phase_degrade().await?;
    run_phase(&mut sup, "Phase 3 — Degraded", Duration::from_secs(120), start, |_| async { Ok(()) }).await?;
    // Restore link profile before Phase 4.
    sup.phase_restore().await?;

    // Phase 4 — Byzantine at t=300s
    sup.phase_byzantine().await?;
    run_phase(&mut sup, "Phase 4 — Byzantine", Duration::from_secs(120), start, |_| async { Ok(()) }).await?;

    sup.shutdown().await;
    Ok(())
}

async fn run_phase<F, Fut>(
    sup: &mut Supervisor,
    label: &str,
    duration: Duration,
    start: Instant,
    _hook: F,
) -> Result<()>
where
    F: FnOnce(&mut Supervisor) -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    tracing::info!(
        elapsed_s = start.elapsed().as_secs(),
        duration_s = duration.as_secs(),
        "{label}"
    );
    let until = tokio::time::Instant::now() + duration;
    while tokio::time::Instant::now() < until {
        if !sup.is_running() {
            tracing::warn!("supervisor reports no running children, aborting phase");
            break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    let report = PhaseReport::capture(label, sup.snapshot().await);
    println!("{report}");
    Ok(())
}
