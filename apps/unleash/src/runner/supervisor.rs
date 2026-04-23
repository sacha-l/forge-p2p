//! Process supervisor: spawn robot processes + one observer via
//! `tokio::process::Command`. Own their handles; SIGKILL on demand.
//!
//! M0 stub — M4 fills in actual process management. For now the supervisor
//! methods are no-ops so the scenario state machine compiles.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use tokio::process::{Child, Command};

use crate::keyspace::RobotClass;
use crate::runner::RunArgs;
use crate::swarm_node;

pub struct ChildHandle {
    pub label: String,
    pub class: Option<RobotClass>,
    pub node_index: u32,
    pub control_port: Option<u16>,
    pub child: Option<Child>,
    pub peer_id: Option<String>,
    pub listen_addr: Option<String>,
}

pub struct Supervisor {
    pub args: RunArgs,
    pub children: Vec<ChildHandle>,
    pub bootstrap_spec: Option<String>,
}

impl Supervisor {
    pub fn new(args: RunArgs) -> Self {
        Self {
            args,
            children: Vec::new(),
            bootstrap_spec: None,
        }
    }

    pub async fn launch_all(&mut self) -> Result<()> {
        // Robot 0 is the bootnode. Launch it, capture peer_id, then launch
        // the rest with `--bootstrap`.
        let roster = build_roster(&self.args.mission);
        if roster.is_empty() {
            anyhow::bail!("empty fleet roster");
        }

        // Launch bootnode (robot 0)
        let (class, id, node_index) = roster[0].clone();
        let bootstrap = self.launch_robot(&id, class, node_index, None).await?;
        self.bootstrap_spec = Some(bootstrap.clone());
        // Give the bootnode a head start so its listen-addr stabilises.
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Rest of the roster
        for (class, id, node_index) in &roster[1..] {
            self.launch_robot(id, *class, *node_index, Some(bootstrap.clone()))
                .await?;
        }

        // Observer
        self.launch_observer(Some(bootstrap.clone())).await?;

        // Warmup: gossipsub mesh takes ~5s (library-feedback #5)
        tokio::time::sleep(Duration::from_secs(8)).await;
        Ok(())
    }

    async fn launch_robot(
        &mut self,
        id: &str,
        class: RobotClass,
        node_index: u32,
        bootstrap: Option<String>,
    ) -> Result<String> {
        let control_port = 60000u16 + node_index as u16;
        let mut cmd = Command::new(&self.args.self_path);
        cmd.arg("robot")
            .args(["--id", id])
            .args(["--class", class.as_str()])
            .args(["--node-index", &node_index.to_string()])
            .args(["--scenario", self.args.scenario_dir.to_str().unwrap_or(".")])
            .args(["--control-port", &control_port.to_string()]);
        if let Some(b) = &bootstrap {
            cmd.args(["--bootstrap", b]);
        }
        cmd.kill_on_drop(true);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::inherit());

        let mut child = cmd.spawn()?;
        let bootstrap_spec = wait_for_ready(&mut child, node_index, "UNLEASH_ROBOT_READY").await;
        let stored_bootstrap = bootstrap_spec.clone().unwrap_or_else(|| {
            // Fallback: construct from node index deterministically.
            swarm_node::format_bootstrap("unknown", &swarm_node::loopback_multiaddr(swarm_node::robot_tcp_port(node_index)))
        });
        self.children.push(ChildHandle {
            label: id.to_string(),
            class: Some(class),
            node_index,
            control_port: Some(control_port),
            child: Some(child),
            peer_id: bootstrap_spec
                .as_deref()
                .and_then(swarm_node::parse_bootstrap)
                .map(|(p, _)| p),
            listen_addr: bootstrap_spec
                .as_deref()
                .and_then(swarm_node::parse_bootstrap)
                .map(|(_, a)| a),
        });
        Ok(stored_bootstrap)
    }

    async fn launch_observer(&mut self, bootstrap: Option<String>) -> Result<()> {
        let mut cmd = Command::new(&self.args.self_path);
        cmd.arg("observer")
            .args(["--scenario", self.args.scenario_dir.to_str().unwrap_or(".")])
            .args(["--ui-port", &self.args.ui_port.to_string()]);
        if let Some(b) = bootstrap {
            cmd.args(["--bootstrap", &b]);
        }
        cmd.kill_on_drop(true);
        cmd.stdout(std::process::Stdio::inherit());
        cmd.stderr(std::process::Stdio::inherit());
        let child = cmd.spawn()?;
        self.children.push(ChildHandle {
            label: "observer".into(),
            class: None,
            node_index: 99,
            control_port: None,
            child: Some(child),
            peer_id: None,
            listen_addr: None,
        });
        Ok(())
    }

    pub async fn phase_dropout(&mut self) -> Result<()> {
        // Kill 1 aerial_scout + 1 ground_scout (per spec §6 Phase 2)
        let targets: Vec<u32> = self
            .children
            .iter()
            .filter(|c| {
                matches!(
                    c.class,
                    Some(RobotClass::AerialScout) | Some(RobotClass::GroundScout)
                )
            })
            .take(2)
            .map(|c| c.node_index)
            .collect();
        for idx in targets {
            self.sigkill(idx).await;
        }
        Ok(())
    }

    pub async fn phase_degrade(&mut self) -> Result<()> {
        // Broadcast link-profile override by POST-ing to robot 0's control port
        // (which forwards it over gossip). Stub for M0; M4 broadcasts directly
        // via a dedicated "runner" SwarmNL peer.
        if let Some(c) = self.children.iter().find(|c| c.node_index == 0) {
            if let Some(p) = c.control_port {
                let _ = reqwest::Client::new()
                    .post(format!("http://127.0.0.1:{p}/degrade"))
                    .json(&serde_json::json!({"profile": "degraded", "duration_ms": 120000}))
                    .send()
                    .await;
            }
        }
        Ok(())
    }

    /// Restore link profile to default (used between Phase 3 and Phase 4).
    pub async fn phase_restore(&mut self) -> Result<()> {
        if let Some(c) = self.children.iter().find(|c| c.node_index == 0) {
            if let Some(p) = c.control_port {
                let _ = reqwest::Client::new()
                    .post(format!("http://127.0.0.1:{p}/degrade"))
                    .json(&serde_json::json!({"profile": "default", "duration_ms": 0}))
                    .send()
                    .await;
            }
        }
        Ok(())
    }

    pub async fn phase_byzantine(&mut self) -> Result<()> {
        // Flip one ground_scout to byzantine mode.
        if let Some(c) = self
            .children
            .iter()
            .find(|c| c.class == Some(RobotClass::GroundScout))
        {
            if let Some(p) = c.control_port {
                let _ = reqwest::Client::new()
                    .post(format!("http://127.0.0.1:{p}/byzantine"))
                    .json(&serde_json::json!({"byzantine": true}))
                    .send()
                    .await;
            }
        }
        Ok(())
    }

    /// Announce a scenario-phase transition to the observer so the dashboard
    /// can update its banner and timeline. Fire-and-forget — if the observer
    /// isn't up yet we don't block the scenario runner.
    pub async fn phase_announce(
        &self,
        phase: &str,
        description: &str,
        duration_s: u64,
        index: u32,
        total: u32,
    ) {
        let url = format!("http://127.0.0.1:{}/api/phase", self.args.ui_port);
        let body = serde_json::json!({
            "phase": phase,
            "description": description,
            "duration_s": duration_s,
            "index": index,
            "total": total,
        });
        let _ = tokio::time::timeout(
            Duration::from_millis(500),
            reqwest::Client::new().post(url).json(&body).send(),
        )
        .await;
    }

    async fn sigkill(&mut self, node_index: u32) {
        if let Some(c) = self
            .children
            .iter_mut()
            .find(|c| c.node_index == node_index)
        {
            if let Some(child) = c.child.as_mut() {
                let _ = child.kill().await;
                tracing::info!(
                    node_index,
                    label = %c.label,
                    "SIGKILL'd robot (Phase 2 dropout)"
                );
            }
        }
    }

    pub fn is_running(&self) -> bool {
        self.children.iter().any(|c| c.child.is_some())
    }

    pub async fn shutdown(&mut self) {
        for c in &mut self.children {
            if let Some(child) = c.child.as_mut() {
                let _ = child.kill().await;
            }
        }
    }

    pub async fn snapshot(&self) -> SupervisorSnapshot {
        SupervisorSnapshot {
            running_count: self
                .children
                .iter()
                .filter(|c| c.child.is_some())
                .count(),
            total_count: self.children.len(),
        }
    }
}

#[derive(Debug)]
pub struct SupervisorSnapshot {
    pub running_count: usize,
    pub total_count: usize,
}

fn build_roster(mission: &crate::config::Mission) -> Vec<(RobotClass, String, u32)> {
    let comp = &mission.fleet.composition;
    let mut out = Vec::new();
    let mut node_index = 0u32;
    for _ in 0..comp.aerial_scout {
        out.push((RobotClass::AerialScout, format!("r{node_index}_as"), node_index));
        node_index += 1;
    }
    for _ in 0..comp.aerial_mapper {
        out.push((RobotClass::AerialMapper, format!("r{node_index}_am"), node_index));
        node_index += 1;
    }
    for _ in 0..comp.ground_scout {
        out.push((RobotClass::GroundScout, format!("r{node_index}_gs"), node_index));
        node_index += 1;
    }
    for _ in 0..comp.ground_workhorse {
        out.push((RobotClass::GroundWorkhorse, format!("r{node_index}_gw"), node_index));
        node_index += 1;
    }
    for _ in 0..comp.breadcrumb {
        out.push((RobotClass::Breadcrumb, format!("r{node_index}_bc"), node_index));
        node_index += 1;
    }
    out
}

async fn wait_for_ready(child: &mut Child, node_index: u32, expected_prefix: &str) -> Option<String> {
    use tokio::io::{AsyncBufReadExt, BufReader};
    let stdout = child.stdout.take()?;
    let mut reader = BufReader::new(stdout).lines();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    while tokio::time::Instant::now() < deadline {
        let line = tokio::time::timeout(Duration::from_millis(500), reader.next_line()).await;
        match line {
            Ok(Ok(Some(line))) => {
                tracing::debug!(node_index, line = %line, "child stdout");
                if line.starts_with(expected_prefix) {
                    // parse `peer_id=... tcp=... addr=...`
                    let peer_id = extract_kv(&line, "peer_id");
                    let addr = extract_kv(&line, "addr");
                    if let (Some(p), Some(a)) = (peer_id, addr) {
                        return Some(swarm_node::format_bootstrap(&p, &a));
                    }
                }
            }
            _ => continue,
        }
    }
    tracing::warn!(node_index, "child did not report READY line within deadline");
    None
}

fn extract_kv(line: &str, key: &str) -> Option<String> {
    for token in line.split_whitespace() {
        if let Some(rest) = token.strip_prefix(&format!("{key}=")) {
            return Some(rest.to_string());
        }
    }
    None
}

/// Path type alias so the module compiles in isolation during M0 stubs.
#[allow(dead_code)]
type _P = PathBuf;
