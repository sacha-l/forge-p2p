// Unleash dashboard bootstrap.
//
// We live inside the forge-ui iframe. Forge-ui's outer chrome owns the
// WebSocket + the mesh panel; but the iframe doesn't share JS context, so
// we open our own WS to the observer and render five panels locally.

import { initMesh } from "/app/panels/mesh.js";
import { initTasks } from "/app/panels/task.js";
import { initConsensus } from "/app/panels/consensus.js";
import { initReplication } from "/app/panels/replication.js";
import { initMap } from "/app/panels/map.js";

const ws = new WebSocket(`ws://${location.host}/ws`);

const subs = [];
function on(label, fn) {
  subs.push([label, fn]);
}

const env = { footprint: { x: 40, y: 25 }, hazards: [] };
fetch("/app/env.json").then(r => (r.ok ? r.json() : env)).then(e => Object.assign(env, e)).catch(() => {});

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

const summary = {
  robots: document.getElementById("s-robots"),
  tasks: document.getElementById("s-tasks"),
  survivors: document.getElementById("s-survivors"),
  link: document.getElementById("s-link"),
};

const mesh = initMesh(document.getElementById("mesh-svg"), env);
const tasksUi = initTasks(document.getElementById("task-cards"));
const consensusUi = initConsensus(
  document.getElementById("consensus-chart"),
  document.getElementById("consensus-legend"),
);
const replUi = initReplication(
  document.getElementById("repl-swimlanes"),
  document.getElementById("repl-chart"),
);
const mapUi = initMap(document.getElementById("map-canvas"), env);

on("unleash/pose", (d) => {
  mesh.onPose(d);
  mapUi.onPose(d);
});
on("unleash/task_winner", (d) => tasksUi.onWinner(d));
on("unleash/bundle", (d) => tasksUi.onBundle(d));
on("unleash/consensus", (d) => consensusUi.onValue(d));
on("unleash/survivor", (d) => {
  replUi.onSurvivor(d);
  mapUi.onSurvivor(d);
});
on("unleash/grid", (d) => mapUi.onGridChunk(d));
on("unleash/link_override", (d) => {
  summary.link.textContent = d.profile;
  mesh.onLinkOverride(d);
});
on("unleash/tick", (d) => {
  summary.robots.textContent = d.robot_count;
  summary.tasks.textContent = d.task_count;
  summary.survivors.textContent = d.survivor_count;
  if (d.link_override) summary.link.textContent = d.link_override;
});

ws.addEventListener("message", (ev) => {
  try {
    const m = JSON.parse(ev.data);
    if (m.type === "Custom" && typeof m.label === "string") {
      const payload = safeParse(m.detail);
      subs.filter(([l]) => l === m.label).forEach(([, fn]) => fn(payload));
    }
    if (m.type === "PeerConnected") mesh.onPeerConnected(m.peer_id);
    if (m.type === "PeerDisconnected") mesh.onPeerDisconnected(m.peer_id);
  } catch (err) {
    console.warn("bad WS message", err, ev.data);
  }
});

ws.addEventListener("open", () => {
  document.getElementById("phase").textContent = "Phase: connected, awaiting telemetry…";
});

function safeParse(s) {
  try {
    return JSON.parse(s);
  } catch {
    return { raw: s };
  }
}

// Expose hooks for debugging
window.unleash = { mesh, tasksUi, consensusUi, replUi, mapUi };
