// Mission panel — default-active tab. Assembles the briefing, phase
// timeline, live stat cards, fleet roster, and recent-events feed.

import { CLASSES, LINK_PROFILES, classMeta } from "/app/panels/classes.js";
import {
  aliveByClass,
  aliveRobots,
  avgBattery,
  consensusVariance,
  on,
  resolvedTasks,
  spawnedByClass,
  store,
} from "/app/panels/state.js";

export function initMission(root, { keyEventsEl } = {}) {
  root.classList.add("mission-root");
  root.innerHTML = `
    <section class="brief">
      <h2>Mission briefing</h2>
      <p id="brief-scenario">Loading briefing…</p>
      <p id="brief-objective"></p>
      <p class="brief-why"><b>Why it matters:</b> every SwarmNL coordination primitive (peer discovery, gossip, RPC bids, consensus, replication) is load-bearing. Remove one and the demo fails.</p>
    </section>

    <section class="phase-timeline">
      <h2>Phase timeline</h2>
      <div id="timeline-bar"></div>
      <div id="timeline-current" class="timeline-current"></div>
    </section>

    <section class="stat-grid">
      <h2>Live stats</h2>
      <div id="cards" class="cards"></div>
    </section>

    <section class="fleet-roster">
      <h2>Fleet roster</h2>
      <table>
        <thead>
          <tr><th>Class</th><th>Alive</th><th>Role</th><th>Capabilities</th></tr>
        </thead>
        <tbody id="roster-body"></tbody>
      </table>
    </section>

    <section class="task-briefing">
      <h2>Tasks in this scenario</h2>
      <p class="brief-sub">Each task is a bid target. CBBA score = <code>dot(my_capability, required_capability) · urgency − α·distance − β·risk</code>. The "winner" is the robot whose bid currently leads; winners converge as gossip propagates. A winner can change if a closer robot announces or the current winner is killed.</p>
      <div id="task-briefing-body" class="task-briefing-body"></div>
    </section>

    <section class="key-events">
      <h2>Recent key events</h2>
      <div id="key-events-body" class="log"></div>
    </section>
  `;

  const cardEl = root.querySelector("#cards");
  const timelineEl = root.querySelector("#timeline-bar");
  const timelineCur = root.querySelector("#timeline-current");
  const rosterBody = root.querySelector("#roster-body");
  const taskBriefingBody = root.querySelector("#task-briefing-body");
  const keyEvents = root.querySelector("#key-events-body");

  // Initial briefing render when mission loads.
  on("mission", renderBrief);
  renderBrief(store.mission);
  // Always refresh stats on any update.
  on("robots", renderStats);
  on("tasks", renderStats);
  on("survivors", renderStats);
  on("consensus", renderStats);
  on("link", renderStats);
  on("phase", renderTimeline);
  // 1Hz live tick so elapsed-time segments move even when no event arrives.
  setInterval(() => {
    renderStats();
    renderTimeline();
  }, 1000);

  function renderBrief(mission) {
    if (!mission) return;
    root.querySelector("#brief-scenario").innerHTML = `
      <b>Scenario:</b> 4-story reinforced-concrete pancake collapse, hour 18 post-earthquake.
      The ${mission.footprint[0]} × ${mission.footprint[1]} m footprint has ${mission.floors} floors; GPS-denied inside the structure; comms mesh-only.
    `;
    root.querySelector("#brief-objective").innerHTML = `
      <b>Objective:</b> autonomously locate all ${mission.target_count} survivors —
      ${mission.known_survivors.length} positions are known at launch, ${mission.unknown_count} are seeded randomly from the scenario seed and must be discovered.
      Time limit ${Math.floor(mission.time_limit_s / 60)}m.
    `;
    renderRoster(mission);
    renderTimelineSegments(mission);
    renderTaskBriefing(mission);
  }

  function renderTaskBriefing(mission) {
    const tasks = mission?.initial_tasks || [];
    if (!tasks.length) {
      taskBriefingBody.innerHTML = '<div class="empty">No tasks in mission briefing.</div>';
      return;
    }
    taskBriefingBody.innerHTML = tasks
      .map((t) => `
        <div class="task-brief-card">
          <div class="task-brief-head">
            <b>${escape(t.pretty)}</b>
            <code class="task-id-chip">${escape(t.id)}</code>
            <span class="task-urgency" title="Urgency weight (higher = more important)">urgency ${Number(t.urgency).toFixed(1)}</span>
          </div>
          <div class="task-brief-meaning">${taskMeaning(t.kind)}</div>
          <div class="task-brief-who">${winnerHint(t.kind)}</div>
        </div>
      `)
      .join("");
  }

  function renderRoster(mission) {
    const spawnedMap = mission?.fleet || {
      aerial_scout: 0,
      aerial_mapper: 0,
      ground_scout: 0,
      ground_workhorse: 0,
      breadcrumb: 0,
    };
    const alive = aliveByClass();
    const spawned = spawnedByClass();
    rosterBody.innerHTML = Object.entries(CLASSES)
      .map(([key, meta]) => {
        const spw = Math.max(
          spawnedMap[key] || 0,
          spawned[key] || 0,
        );
        const alv = alive[key] || 0;
        const cls = alv === 0 && spw > 0 ? "row-dim" : "";
        const badge =
          alv < spw
            ? `<b class="alive bad">${alv}</b>/${spw}`
            : `<b class="alive ok">${alv}</b>/${spw}`;
        return `
          <tr class="${cls}">
            <td>
              <span class="roster-dot" style="background:${meta.color}"></span>
              ${meta.label}
            </td>
            <td>${badge}</td>
            <td class="role-cell">${meta.role}</td>
            <td>${meta.capabilities.map((c) => `<span class="cap-chip">${c}</span>`).join("")}</td>
          </tr>
        `;
      })
      .join("");
  }

  function renderTimelineSegments(mission) {
    const phases = mission?.phases || [];
    if (!phases.length) return;
    const total = phases.reduce((s, p) => s + (p.duration_s || 0), 0) || 1;
    timelineEl.innerHTML = phases
      .map((p) => {
        const pct = ((p.duration_s || 0) / total) * 100;
        return `
          <div class="segment" data-phase-index="${p.index}" style="flex-basis:${pct}%" title="${escape(p.description)}">
            <span class="seg-index">${p.index}</span>
            <span class="seg-name">${p.name}</span>
            <span class="seg-dur">${Math.round(p.duration_s / 60)}m</span>
          </div>
        `;
      })
      .join("");
  }

  function renderTimeline() {
    const p = store.phase;
    const phases = store.mission?.phases || [];
    if (!p || !phases.length) return;
    const idx = p.index || 0;
    [...timelineEl.querySelectorAll(".segment")].forEach((seg) => {
      const segIdx = Number(seg.dataset.phaseIndex);
      seg.classList.toggle("past", segIdx < idx);
      seg.classList.toggle("current", segIdx === idx);
      seg.classList.toggle("future", segIdx > idx);
    });
    const started = store.phaseStartedAtMs || Date.now();
    const secInPhase = Math.max(0, Math.floor((Date.now() - started) / 1000));
    const desc = p.description || "";
    timelineCur.textContent = `${PHASE_FULL_LABEL(p.phase)} — ${secInPhase}s in · ${desc}`;
  }

  function renderStats() {
    const mission = store.mission;
    const totalTargets = mission?.target_count ?? 5;
    const survivorsFound = store.survivors.size;
    const resolved = resolvedTasks();
    const totalTasks = mission?.initial_tasks?.length ?? store.tasks.size;
    const alive = aliveRobots();
    // Use the sum of composition fields, not mission.fleet.size — that field
    // only counts mobile robots. Breadcrumb relays are also spawned processes.
    const comp = mission?.fleet || {};
    const spawned =
      (comp.aerial_scout || 0) +
      (comp.aerial_mapper || 0) +
      (comp.ground_scout || 0) +
      (comp.ground_workhorse || 0) +
      (comp.breadcrumb || 0) ||
      Math.max(store.robots.size, 1);
    const bat = avgBattery();
    const peerAvg = alive.length
      ? Math.round((alive.length - 1 + store.peers.size) / Math.max(alive.length, 1) * 10) / 10
      : 0;
    const variance = consensusVariance();
    const cards = [
      card(
        "Survivors",
        `${survivorsFound}/${totalTargets}`,
        `${survivorsFound} located via gossip · ${totalTargets} to find (${mission?.known_survivors?.length || 0} known at launch, ${mission?.unknown_count || 0} randomised)`,
      ),
      card(
        "Tasks",
        `${resolved}/${totalTasks}`,
        "tasks with a CBBA winner / total announced · a winner is the robot whose bid currently leads",
      ),
      card(
        "Robots alive",
        `${alive.length}/${spawned}`,
        `robots whose pose heartbeat arrived in the last 6 s / total spawned (all classes, breadcrumbs included)`,
      ),
      card(
        "Avg battery",
        bat === null ? "—" : `${Math.round(bat * 100)}%`,
        "mean across alive fleet · each class drains at a different rate",
      ),
      card(
        "Mesh peers / robot",
        peerAvg.toFixed(1),
        "avg peer count visible in the observer's gossip mesh",
      ),
      card(
        "Link profile",
        store.linkOverride,
        LINK_PROFILES[store.linkOverride]?.description || "—",
      ),
      card(
        "Consensus variance",
        variance.toFixed(2),
        "std dev across W-MSR victim-count estimates · 0 = all honest robots agree",
      ),
      card(
        "Event clock",
        new Date().toLocaleTimeString(),
        "observer wall time",
      ),
    ];
    cardEl.innerHTML = cards.join("");
    renderRoster(store.mission);
  }

  function card(label, value, sub) {
    return `
      <div class="card">
        <div class="card-label">${label}</div>
        <div class="card-value">${value}</div>
        <div class="card-sub">${sub}</div>
      </div>
    `;
  }

  // Expose key-events log root if the caller wants to pipe specific entries.
  if (keyEventsEl) keyEventsEl.appendChild(keyEvents);
  return {
    keyEvents,
  };
}

const PHASE_FULL = {
  booting: "Booting",
  nominal: "Phase 1: Nominal",
  dropout: "Phase 2: Dropout",
  degraded: "Phase 3: Degraded",
  byzantine: "Phase 4: Byzantine",
  complete: "Complete",
};
function PHASE_FULL_LABEL(k) {
  return PHASE_FULL[k] || k || "—";
}

function escape(s) {
  return String(s).replace(/[<>&"]/g, (c) => ({ "<": "&lt;", ">": "&gt;", "&": "&amp;", '"': "&quot;" })[c]);
}

// One-line human description of every task kind used in the reference scenario.
function taskMeaning(kind) {
  switch (kind) {
    case "survey_area":
      return "Cover the full footprint from altitude to build the initial map.";
    case "establish_perimeter_mesh":
      return "Drop static breadcrumb relays at connectivity bottlenecks so the mesh survives link failures.";
    case "inspect_poi":
      return "Enter an interior void too small for aerial vehicles and inspect it.";
    case "find_victim":
      return "Make physical contact with a specific survivor and report position + state.";
    case "relay_hold":
      return "Hold a stationary position so gossip can hop through this robot as a relay.";
    case "deploy_node":
      return "Drop a single breadcrumb node at a specific point.";
    case "escort":
      return "Escort another robot while it performs a task.";
    default:
      return `Custom task kind: ${kind}.`;
  }
}

// Who's likely to win this task, derived from the capability-vector match.
function winnerHint(kind) {
  switch (kind) {
    case "survey_area":
      return "<b>Likely winner:</b> Aerial Scout or Aerial Mapper — only they have <code>aerial=1.0</code>.";
    case "establish_perimeter_mesh":
    case "deploy_node":
      return "<b>Likely winner:</b> Ground Workhorse — only class with <code>deploy_node</code> + <code>payload</code>.";
    case "inspect_poi":
      return "<b>Likely winner:</b> Ground Scout — highest <code>inspect_narrow</code>.";
    case "find_victim":
      return "<b>Likely winner:</b> Ground Scout — only class with <code>victim_contact</code>.";
    case "relay_hold":
      return "<b>Likely winner:</b> Aerial Mapper or Ground Workhorse — both score <code>relay=1.0</code>.";
    default:
      return "";
  }
}
