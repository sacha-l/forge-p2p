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

    // Phase 1 — Nominal
    sup.phase_announce(
        "nominal",
        "All robots operational. CBBA converges on the initial task auction.",
        120, 1, 4,
    )
    .await;
    run_phase(&mut sup, "Phase 1 — Nominal", Duration::from_secs(120), start, |_| async { Ok(()) }).await?;

    // Phase 2 — Dropout at t=120s
    sup.phase_dropout().await?;
    sup.phase_announce(
        "dropout",
        "Two robots SIGKILL'd. Orphaned tasks re-enter the bidding pool; expect reassignment within 15 s.",
        60, 2, 4,
    )
    .await;
    run_phase(&mut sup, "Phase 2 — Dropout", Duration::from_secs(60), start, |_| async { Ok(()) }).await?;

    // Phase 3 — Comms degradation at t=180s
    sup.phase_degrade().await?;
    sup.phase_announce(
        "degraded",
        "Link profile dropped to 2 Mbps / 80 ms / 40 % loss. Replication lag climbs; map merges slow.",
        120, 3, 4,
    )
    .await;
    run_phase(&mut sup, "Phase 3 — Degraded", Duration::from_secs(120), start, |_| async { Ok(()) }).await?;
    sup.phase_restore().await?;

    // Phase 4 — Byzantine at t=300s
    sup.phase_byzantine().await?;
    sup.phase_announce(
        "byzantine",
        "One ground scout is now adversarial. W-MSR should reject its inflated victim count within 5 rounds.",
        120, 4, 4,
    )
    .await;
    run_phase(&mut sup, "Phase 4 — Byzantine", Duration::from_secs(120), start, |_| async { Ok(()) }).await?;

    sup.phase_announce(
        "complete",
        "Scenario complete. All 4 phase reports printed to stdout.",
        0, 4, 4,
    )
    .await;

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
