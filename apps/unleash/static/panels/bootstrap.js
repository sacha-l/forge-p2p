// Unleash dashboard bootstrap.
//
// We live inside the forge-ui iframe. Forge-ui's outer chrome owns the
// WebSocket + the mesh panel, but the iframe is its own JS context, so we
// open our own WS to the observer and render every panel locally.

import { initMesh } from "/app/panels/mesh.js";
import { initTasks } from "/app/panels/task.js";
import { initConsensus } from "/app/panels/consensus.js";
import { initReplication } from "/app/panels/replication.js";
import { initMap } from "/app/panels/map.js";
import { initMission } from "/app/panels/mission.js";
import { initBanner } from "/app/panels/banner.js";
import { initLegend } from "/app/panels/legend.js";
import { initInspector } from "/app/panels/inspector.js";
import { initGlossary } from "/app/panels/glossary.js";
import { initOnboarding } from "/app/panels/onboarding.js";
import { narrate, renderLog } from "/app/panels/narration.js";
import {
  loadMission,
  markPeerConnected,
  markPeerDisconnected,
  recordBundle,
  recordConsensus,
  recordLinkOverride,
  recordPhase,
  recordPose,
  recordSurvivor,
  recordTask,
  store,
} from "/app/panels/state.js";

/* ---------- tabs (Mission default-active) ---------- */
const tabs = document.querySelectorAll("#tabs .tab");
const panels = document.querySelectorAll(".panel");
tabs.forEach((t) => {
  t.addEventListener("click", () => {
    tabs.forEach((x) => x.classList.remove("active"));
    panels.forEach((x) => x.classList.remove("active"));
    t.classList.add("active");
    document.getElementById(`panel-${t.dataset.panel}`).classList.add("active");
  });
});

function switchTo(tabName) {
  const btn = [...tabs].find((t) => t.dataset.panel === tabName);
  if (btn) btn.click();
}

/* ---------- env (static for the footprint; hazards come from /api/mission) ---------- */
const env = { footprint: { x: 40, y: 25 }, hazards: [] };

/* ---------- inspector + glossary + onboarding ---------- */
const inspector = initInspector(document.getElementById("inspector"));
const glossary = initGlossary(document.getElementById("glossary"));

document.getElementById("btn-help").addEventListener("click", () => glossary.toggle());

const onboarding = initOnboarding(document.getElementById("onboarding"), {
  getMission: () => store.mission,
  onClose: () => switchTo("mission"),
});
document.getElementById("btn-onboard").addEventListener("click", () => onboarding.open());

/* ---------- banner ---------- */
initBanner(document.getElementById("mission-banner"));

/* ---------- panels ---------- */
const mesh = initMesh(document.getElementById("mesh-svg"), env, {
  onRobotClick: (id) => inspector.open(id),
});
const tasksUi = initTasks(document.getElementById("task-cards"));
const consensusUi = initConsensus(
  document.getElementById("consensus-chart"),
  document.getElementById("consensus-legend"),
);
const replUi = initReplication(
  document.getElementById("repl-swimlanes"),
  document.getElementById("repl-chart"),
);
const mapUi = initMap(document.getElementById("map-canvas"), env, {
  onRobotClick: (id) => inspector.open(id),
});
initMission(document.getElementById("panel-mission"));

const classFilter = new Set();
initLegend(document.getElementById("mesh-legend"), {
  onFilterChange: (active) => {
    classFilter.clear();
    active.forEach((c) => classFilter.add(c));
    mesh.setClassFilter(classFilter);
  },
});
initLegend(document.getElementById("map-legend"), {
  onFilterChange: (active) => {
    classFilter.clear();
    active.forEach((c) => classFilter.add(c));
    mapUi.setClassFilter?.(classFilter);
  },
});

/* ---------- event log ---------- */
const logEntries = [];
const logRoot = document.getElementById("event-log-body");

function appendLog(entries) {
  if (!entries || entries.length === 0) return;
  for (const e of entries) logEntries.push(e);
  while (logEntries.length > 300) logEntries.shift();
  // newest first (simpler to read as events stream in)
  renderLog(logRoot, [...logEntries].reverse().slice(0, 200));
}

/* ---------- load mission briefing, then wire events ---------- */
loadMission().then(() => {
  // environment hazards for the canvases.
  if (store.mission?.hazards) {
    env.hazards = store.mission.hazards;
    env.footprint = {
      x: store.mission.footprint?.[0] || 40,
      y: store.mission.footprint?.[1] || 25,
    };
    mesh.setEnv?.(env);
    mapUi.setEnv?.(env);
  }
});

/* ---------- WS ---------- */
const ws = new WebSocket(`ws://${location.host}/ws`);
const taskPretty = new Map(); // task_id -> human name (populated from mission briefing)

function ensureTaskPretty() {
  if (taskPretty.size) return;
  for (const t of store.mission?.initial_tasks || []) {
    taskPretty.set(t.id, t.pretty || t.id);
  }
}

const subs = new Map();
function on(label, fn) {
  if (!subs.has(label)) subs.set(label, []);
  subs.get(label).push(fn);
}

on("unleash/pose", (d) => {
  recordPose(d);
  mesh.onPose(d);
  mapUi.onPose(d);
  appendLog(narrate("unleash/pose", d));
});
on("unleash/task_winner", (d) => {
  ensureTaskPretty();
  const enriched = { ...d, task_pretty: taskPretty.get(d.task_id) || d.task_id };
  recordTask(enriched);
  tasksUi.onWinner(enriched);
  appendLog(narrate("unleash/task_winner", enriched));
});
on("unleash/bundle", (d) => {
  recordBundle(d);
  tasksUi.onBundle(d);
  appendLog(narrate("unleash/bundle", d));
});
on("unleash/consensus", (d) => {
  recordConsensus(d);
  consensusUi.onValue(d);
  appendLog(narrate("unleash/consensus", d));
});
on("unleash/survivor", (d) => {
  const first = !store.survivors.has(d.survivor_id);
  recordSurvivor(d);
  replUi.onSurvivor(d);
  mapUi.onSurvivor(d);
  if (first) appendLog(narrate("unleash/survivor", d));
});
on("unleash/grid", (d) => mapUi.onGridChunk(d));
on("unleash/link_override", (d) => {
  const prev = store.linkOverride;
  recordLinkOverride(d);
  mesh.onLinkOverride(d);
  if (prev !== d.profile) appendLog(narrate("unleash/link_override", d));
});
on("unleash/tick", () => {
  // tick used to drive header aggregates; we compute them from state.js now
});
on("unleash/phase", (d) => {
  const prev = store.phase?.phase;
  recordPhase(d);
  if (prev !== d.phase) appendLog(narrate("unleash/phase", d));
});

ws.addEventListener("message", (ev) => {
  try {
    const m = JSON.parse(ev.data);
    if (m.type === "Custom" && typeof m.label === "string") {
      const payload = safeParse(m.detail);
      (subs.get(m.label) || []).forEach((fn) => fn(payload));
    } else if (m.type === "PeerConnected") {
      markPeerConnected(m.peer_id);
      mesh.onPeerConnected?.(m.peer_id);
      appendLog(narrate("peer-connected", { peer_id: m.peer_id }));
    } else if (m.type === "PeerDisconnected") {
      markPeerDisconnected(m.peer_id);
      mesh.onPeerDisconnected?.(m.peer_id);
      appendLog(narrate("peer-disconnected", { peer_id: m.peer_id }));
    }
  } catch (err) {
    console.warn("bad WS message", err, ev.data);
  }
});

function safeParse(s) {
  try {
    return JSON.parse(s);
  } catch {
    return { raw: s };
  }
}

// Expose for debugging.
window.unleash = { mesh, tasksUi, consensusUi, replUi, mapUi, store, inspector, glossary, onboarding };
