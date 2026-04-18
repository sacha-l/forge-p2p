// Click-to-inspect drawer. Takes an element root + the shared store; shows
// class, status, task, battery, bundle, last update. Dismisses with the ×
// button or ESC.

import { classMeta, prettyRobotId } from "/app/panels/classes.js";
import { on, store } from "/app/panels/state.js";

export function initInspector(root) {
  root.innerHTML = "";
  root.classList.add("inspector");
  root.style.display = "none";
  let currentId = null;

  root.innerHTML = `
    <header>
      <h3 id="insp-title">Robot</h3>
      <button id="insp-close" aria-label="Close">×</button>
    </header>
    <div id="insp-body"></div>
  `;

  root.querySelector("#insp-close").addEventListener("click", () => close());
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") close();
  });

  function close() {
    currentId = null;
    root.style.display = "none";
  }

  function render() {
    if (!currentId) return;
    const robot = store.robots.get(currentId);
    const body = root.querySelector("#insp-body");
    const title = root.querySelector("#insp-title");
    if (!robot) {
      title.textContent = prettyRobotId(currentId);
      body.innerHTML = `<div class="insp-empty">No telemetry received for ${currentId} yet.</div>`;
      return;
    }
    const meta = classMeta(robot.class);
    const bundle = store.bundles.get(currentId) || [];
    const ageS = Math.max(0, Math.floor((Date.now() - (robot.last_local_ms || Date.now())) / 1000));
    const bat = Math.round((robot.battery ?? 0) * 100);
    const statusClass = robot.status === "byzantine"
      ? "bad"
      : robot.status === "offline"
        ? "muted"
        : "ok";
    title.innerHTML = `<span class="insp-dot" style="background:${meta.color}"></span>${prettyRobotId(currentId)}`;
    body.innerHTML = `
      <div class="insp-row"><em>class</em><span>${meta.label}</span></div>
      <div class="insp-row insp-role"><em>role</em><span>${meta.role}</span></div>
      <div class="insp-row"><em>status</em><span class="insp-status ${statusClass}">${robot.status || "—"}</span></div>
      <div class="insp-row"><em>battery</em><span>${bat}% <span class="insp-bar"><i style="width:${bat}%"></i></span></span></div>
      <div class="insp-row"><em>pose</em><span>(${(robot.pose?.x ?? 0).toFixed(1)}, ${(robot.pose?.y ?? 0).toFixed(1)}, ${(robot.pose?.z ?? 0).toFixed(1)})</span></div>
      <div class="insp-row"><em>last heard</em><span>${ageS}s ago</span></div>
      <div class="insp-row insp-bundle">
        <em>bundle</em>
        <span>${bundle.length === 0 ? "— no tasks won" : bundle.map(([t, s]) => `<code>${t}</code>${typeof s === "number" ? ` (${s.toFixed(2)})` : ""}`).join(" · ")}</span>
      </div>
      <div class="insp-row insp-caps">
        <em>capabilities</em>
        <span>${meta.capabilities.map((c) => `<span class="cap-chip">${c}</span>`).join("")}</span>
      </div>
    `;
  }

  on("robots", () => currentId && render());
  on("bundles", () => currentId && render());

  // Light refresh every second so the "last heard" counter stays fresh.
  setInterval(() => currentId && render(), 1000);

  return {
    open(robotId) {
      currentId = robotId;
      root.style.display = "";
      render();
    },
    close,
  };
}
