//! Phase-report formatter.
//!
//! Formats the metrics listed in spec §6 for stdout. M4 extends with
//! replication-lag p95, cluster count, and consensus convergence rounds once
//! the aggregator state is available at runtime.

use std::fmt::{self, Display};

use super::supervisor::SupervisorSnapshot;

pub struct PhaseReport {
    pub label: String,
    pub running: usize,
    pub total: usize,
}

impl PhaseReport {
    pub fn capture(label: &str, snapshot: SupervisorSnapshot) -> Self {
        Self {
            label: label.to_string(),
            running: snapshot.running_count,
            total: snapshot.total_count,
        }
    }
}

impl Display for PhaseReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f)?;
        writeln!(f, "=== {} ===", self.label)?;
        writeln!(f, "  Robots running:           {} / {}", self.running, self.total)?;
        writeln!(f, "  (detailed metrics: see observer dashboard)")?;
        Ok(())
    }
}
