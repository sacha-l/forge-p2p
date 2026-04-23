//! Rendezvous-triggered occupancy-grid merge — the MVP substitute for
//! Swarm-SLAM (see `decisions.md`).
//!
//! Each robot maintains a 0.5 m cell occupancy grid, Lamport-versioned per
//! cell. When another robot passes within 5 m LoS, both publish their full
//! grids on `unleash/map/merge`. Receivers merge cell-by-cell: higher
//! Lamport wins, ties by `robot_id`.

use std::collections::HashMap;
use std::sync::RwLock;

use crate::config::{Footprint, Pose3};
use crate::keyspace::{GridCell, GridChunk};

pub const CELL_M: f32 = 0.5;

pub struct OccupancyGrid {
    robot_id: String,
    width: u16,
    height: u16,
    floor: u8,
    cells: RwLock<HashMap<(u16, u16), GridCell>>,
    lamport: RwLock<u64>,
}

impl OccupancyGrid {
    pub fn new(robot_id: &str, footprint: Footprint, floor: u8) -> Self {
        Self {
            robot_id: robot_id.into(),
            width: (footprint.x / CELL_M).ceil() as u16,
            height: (footprint.y / CELL_M).ceil() as u16,
            floor,
            cells: RwLock::new(HashMap::new()),
            lamport: RwLock::new(0),
        }
    }

    pub fn dims(&self) -> (u16, u16) {
        (self.width, self.height)
    }

    pub fn mark(&self, pose: Pose3, occupancy: u8) {
        let (cx, cy) = self.cell_coords(pose);
        let mut lamport = self.lamport.write().expect("grid lamport poisoned");
        *lamport += 1;
        let clk = *lamport;
        self.cells
            .write()
            .expect("grid cells poisoned")
            .insert(
                (cx, cy),
                GridCell {
                    x: cx,
                    y: cy,
                    occupancy,
                    lamport: clk,
                    updated_by: self.robot_id.clone(),
                },
            );
    }

    pub fn merge(&self, incoming: &GridChunk) -> usize {
        if incoming.floor != self.floor {
            return 0;
        }
        let mut merged = 0;
        let mut cells = self.cells.write().expect("grid cells poisoned");
        for c in &incoming.cells {
            let entry = cells
                .entry((c.x, c.y))
                .or_insert_with(|| c.clone());
            if c.lamport > entry.lamport
                || (c.lamport == entry.lamport && c.updated_by > entry.updated_by)
            {
                *entry = c.clone();
                merged += 1;
            }
        }
        // Bump local lamport so future writes from us sort after merged cells.
        if let Some(max) = incoming.cells.iter().map(|c| c.lamport).max() {
            let mut lam = self.lamport.write().expect("grid lamport poisoned");
            *lam = (*lam).max(max);
        }
        merged
    }

    pub fn snapshot(&self) -> GridChunk {
        let cells = self
            .cells
            .read()
            .expect("grid cells poisoned")
            .values()
            .cloned()
            .collect();
        GridChunk {
            floor: self.floor,
            robot_id: self.robot_id.clone(),
            cells,
            ts_ms: crate::keyspace::now_ms(),
        }
    }

    fn cell_coords(&self, pose: Pose3) -> (u16, u16) {
        let cx = ((pose.x / CELL_M) as i32).clamp(0, self.width as i32 - 1) as u16;
        let cy = ((pose.y / CELL_M) as i32).clamp(0, self.height as i32 - 1) as u16;
        (cx, cy)
    }
}

/// Detect rendezvous: two poses within `range_m` have LoS (ignoring
/// obstacles for this MVP — the link-model layer handles obstructions).
pub fn is_rendezvous(a: Pose3, b: Pose3, range_m: f32) -> bool {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    let dz = a.z - b.z;
    (dx * dx + dy * dy + dz * dz).sqrt() < range_m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp() -> Footprint {
        Footprint { x: 40.0, y: 25.0 }
    }

    #[test]
    fn grid_dims_match_footprint() {
        let g = OccupancyGrid::new("r0", fp(), 0);
        assert_eq!(g.dims(), (80, 50));
    }

    #[test]
    fn mark_and_snapshot() {
        let g = OccupancyGrid::new("r0", fp(), 0);
        g.mark(Pose3 { x: 2.0, y: 2.0, z: 0.0 }, 200);
        let snap = g.snapshot();
        assert_eq!(snap.cells.len(), 1);
        assert_eq!(snap.cells[0].occupancy, 200);
    }

    #[test]
    fn merge_keeps_higher_lamport() {
        let a = OccupancyGrid::new("a", fp(), 0);
        let b = OccupancyGrid::new("b", fp(), 0);
        a.mark(Pose3 { x: 1.0, y: 1.0, z: 0.0 }, 100);
        b.mark(Pose3 { x: 1.0, y: 1.0, z: 0.0 }, 200);
        b.mark(Pose3 { x: 1.0, y: 1.0, z: 0.0 }, 210); // now lamport=2
        let chunk_b = b.snapshot();
        let merged = a.merge(&chunk_b);
        assert_eq!(merged, 1);
        let snap = a.snapshot();
        assert_eq!(snap.cells[0].occupancy, 210);
    }

    #[test]
    fn is_rendezvous_threshold() {
        let a = Pose3 { x: 0.0, y: 0.0, z: 0.0 };
        let b = Pose3 { x: 3.0, y: 0.0, z: 0.0 };
        assert!(is_rendezvous(a, b, 5.0));
        assert!(!is_rendezvous(a, Pose3 { x: 10.0, y: 0.0, z: 0.0 }, 5.0));
    }
}
