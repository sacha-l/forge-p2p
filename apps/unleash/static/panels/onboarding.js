// First-visit walkthrough. 4 slides; stored-seen flag in localStorage.

import { CLASSES } from "/app/panels/classes.js";

const STORAGE_KEY = "unleash_onboarded_v1";

const SLIDES = [
  {
    title: "What is Unleash?",
    body: () => `
      <p>Unleash is a simulated robot swarm coordinating over a peer-to-peer mesh — <b>no central coordinator</b>. Every robot talks directly to every other robot through gossip, consensus, and replication protocols.</p>
      <p>This dashboard shows the mesh <i>from inside</i>: everything you see is state that the observer learned by listening to the same gossip topics the robots use. Nothing is ground-truth injected.</p>
    `,
  },
  {
    title: "Who's in the swarm?",
    body: () => `
      <p>Eighteen robots in four mobile classes plus static breadcrumb relays:</p>
      <ul class="onboard-roster">
        ${Object.entries(CLASSES)
          .map(
            ([, c]) => `
          <li>
            <span class="roster-dot" style="background:${c.color}"></span>
            <b>${c.label}</b> — ${c.role}
          </li>
        `,
          )
          .join("")}
      </ul>
    `,
  },
  {
    title: "What are they doing?",
    body: (mission) => {
      if (!mission) {
        return `<p>Loading mission briefing…</p>`;
      }
      return `
        <p><b>Scenario:</b> 4-story reinforced-concrete pancake collapse, hour 18 post-earthquake. GPS-denied inside the structure. Communications mesh-only — no cellular backhaul.</p>
        <p><b>Objective:</b> autonomously locate all ${mission.target_count} survivors inside the golden window. ${mission.known_survivors.length} positions are known at launch; ${mission.unknown_count} are seeded randomly and must be discovered.</p>
        <p><b>Why it matters:</b> every coordination primitive (peer discovery, gossip, RPC bids, consensus, replication) is load-bearing. Remove any one and the demo fails.</p>
      `;
    },
  },
  {
    title: "What you'll see",
    body: () => `
      <ul class="onboard-tabs">
        <li><b>Mission</b> — live briefing, phase timeline, and the stats that matter.</li>
        <li><b>Mesh</b> — top-down view of the 40×25 m footprint; dots are robots, lines are active gossip links.</li>
        <li><b>Tasks</b> — live CBBA auction; each card is one task with its current winner.</li>
        <li><b>Consensus</b> — W-MSR estimate per robot; Byzantine robots diverge and turn red.</li>
        <li><b>Replication</b> — how fast a survivor detection propagates; lag p50/p95 chart.</li>
        <li><b>Map</b> — merged occupancy grid + located survivors, built from gossip only.</li>
      </ul>
      <p>Click any robot dot in the Mesh or Map to open its inspector. Click <b>?</b> in the header for a glossary of every acronym.</p>
    `,
  },
];

export function initOnboarding(root, { getMission, onClose } = {}) {
  root.classList.add("modal-backdrop");
  root.innerHTML = `
    <div class="modal-card">
      <header>
        <h2 id="onboard-title"></h2>
        <button id="onboard-close" aria-label="Close">×</button>
      </header>
      <div id="onboard-body"></div>
      <footer>
        <div id="onboard-dots"></div>
        <div class="onboard-actions">
          <button id="onboard-prev">Back</button>
          <button id="onboard-next" class="primary">Next</button>
        </div>
      </footer>
    </div>
  `;
  root.style.display = "none";

  let idx = 0;

  const titleEl = root.querySelector("#onboard-title");
  const bodyEl = root.querySelector("#onboard-body");
  const dotsEl = root.querySelector("#onboard-dots");
  const prevBtn = root.querySelector("#onboard-prev");
  const nextBtn = root.querySelector("#onboard-next");

  function render() {
    const slide = SLIDES[idx];
    const mission = getMission?.();
    titleEl.textContent = slide.title;
    bodyEl.innerHTML = slide.body(mission);
    dotsEl.innerHTML = SLIDES.map(
      (_, i) => `<span class="onboard-dot${i === idx ? " active" : ""}"></span>`,
    ).join("");
    prevBtn.disabled = idx === 0;
    nextBtn.textContent = idx === SLIDES.length - 1 ? "Done" : "Next";
  }

  prevBtn.addEventListener("click", () => {
    if (idx > 0) {
      idx -= 1;
      render();
    }
  });
  nextBtn.addEventListener("click", () => {
    if (idx < SLIDES.length - 1) {
      idx += 1;
      render();
    } else {
      close();
    }
  });
  root.querySelector("#onboard-close").addEventListener("click", () => close());
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && root.style.display !== "none") close();
  });

  function close() {
    root.style.display = "none";
    try {
      localStorage.setItem(STORAGE_KEY, "1");
    } catch {}
    onClose?.();
  }

  function open() {
    idx = 0;
    render();
    root.style.display = "";
  }

  // Auto-open on first visit.
  try {
    if (!localStorage.getItem(STORAGE_KEY)) open();
  } catch {}

  return { open, close };
}
