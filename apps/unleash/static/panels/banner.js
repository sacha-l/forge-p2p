// Persistent top-of-page header showing the current phase + elapsed-in-phase
// with colour shift by phase.

import { on, store } from "/app/panels/state.js";

const PHASE_TONES = {
  booting: "neutral",
  nominal: "good",
  dropout: "bad",
  degraded: "warn",
  byzantine: "bad",
  complete: "good",
};

const PHASE_LABELS = {
  booting: "Booting",
  nominal: "Phase 1 · Nominal",
  dropout: "Phase 2 · Dropout",
  degraded: "Phase 3 · Degraded",
  byzantine: "Phase 4 · Byzantine",
  complete: "Complete",
};

const NEXT_PREVIEW = {
  nominal: "next: Dropout — 2 robots SIGKILL'd",
  dropout: "next: Degraded — link profile 2 Mbps / 80 ms / 40 % loss",
  degraded: "next: Byzantine — one ground scout flips adversarial",
  byzantine: "final phase — scenario ends shortly",
  booting: "waiting for Phase 1 announcement…",
  complete: "scenario complete",
};

export function initBanner(root) {
  root.classList.add("mission-banner");
  root.innerHTML = `
    <div class="banner-title">
      <span class="banner-mission">Disaster-relief swarm — locate survivors inside a collapsed building. No central coordinator.</span>
    </div>
    <div class="banner-phase">
      <span id="banner-phase-chip" class="phase-chip neutral">Booting</span>
      <span id="banner-phase-desc" class="banner-desc">Scenario starting — waiting for the mesh to warm up.</span>
      <span id="banner-elapsed" class="banner-elapsed">t +0:00</span>
      <span id="banner-next" class="banner-next"></span>
    </div>
  `;

  const chip = root.querySelector("#banner-phase-chip");
  const desc = root.querySelector("#banner-phase-desc");
  const elapsed = root.querySelector("#banner-elapsed");
  const nextEl = root.querySelector("#banner-next");

  function paint() {
    const p = store.phase;
    const key = p?.phase || "booting";
    chip.textContent = PHASE_LABELS[key] || key;
    chip.className = `phase-chip ${PHASE_TONES[key] || "neutral"}`;
    desc.textContent = p?.description || "Scenario starting — waiting for the mesh to warm up.";
    nextEl.textContent = NEXT_PREVIEW[key] || "";
    const started = store.phaseStartedAtMs || Date.now();
    const dur = p?.duration_s || 0;
    const sec = Math.max(0, Math.floor((Date.now() - started) / 1000));
    const durStr = dur > 0 ? ` / ${fmtDur(dur)}` : "";
    elapsed.textContent = `t +${fmtDur(sec)}${durStr}`;
  }

  on("phase", paint);
  setInterval(paint, 1000);
  paint();
}

function fmtDur(totalS) {
  const m = Math.floor(totalS / 60);
  const s = totalS % 60;
  return `${m}:${String(s).padStart(2, "0")}`;
}
