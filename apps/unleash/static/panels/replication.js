// Replication panel: one swimlane per survivor, marking which nodes have
// acknowledged it. Below, a p50/p95 lag time-series across all survivor
// replications (computed locally from first-seen timestamps).

export function initReplication(swimlaneRoot, chartSvg) {
  const survivors = new Map(); // id -> {first_ms, by_node: Set, first_by: String}
  const lagSamples = []; // [{ts_ms, lag_ms}]
  let renderTimer = null;

  function scheduleRender() {
    if (renderTimer) return;
    renderTimer = setTimeout(() => {
      renderTimer = null;
      render();
    }, 200);
  }

  function render() {
    swimlaneRoot.innerHTML = "";
    if (survivors.size === 0) {
      const e = document.createElement("div");
      e.className = "empty";
      e.textContent = "No survivor reports yet.";
      swimlaneRoot.appendChild(e);
    } else {
      [...survivors.keys()].sort().forEach((sid) => {
        const s = survivors.get(sid);
        const row = document.createElement("div");
        row.className = "swim-row";
        const peers = [...s.by_node].sort();
        row.innerHTML = `
          <span class="swim-id">${sid}</span>
          <span class="swim-count">${peers.length}</span>
          <span class="swim-peers">${peers.map((p) => `<span>${p}</span>`).join("")}</span>
        `;
        swimlaneRoot.appendChild(row);
      });
    }

    const ns = "http://www.w3.org/2000/svg";
    chartSvg.innerHTML = "";
    const bg = document.createElementNS(ns, "rect");
    bg.setAttribute("x", 0);
    bg.setAttribute("y", 0);
    bg.setAttribute("width", 800);
    bg.setAttribute("height", 200);
    bg.setAttribute("fill", "#0b0d12");
    chartSvg.appendChild(bg);
    if (lagSamples.length < 2) return;
    const vmin = 0;
    const vmax = Math.max(1000, ...lagSamples.map((s) => s.lag_ms));
    const n = lagSamples.length;
    const x = (i) => (i / (n - 1)) * 760 + 30;
    const y = (v) => 180 - (v / vmax) * 160;
    const poly = document.createElementNS(ns, "polyline");
    poly.setAttribute(
      "points",
      lagSamples.map((s, i) => `${x(i)},${y(s.lag_ms)}`).join(" ")
    );
    poly.setAttribute("fill", "none");
    poly.setAttribute("stroke", "#8df0c5");
    poly.setAttribute("stroke-width", 1.5);
    chartSvg.appendChild(poly);
    // p95 band
    const sorted = [...lagSamples.map((s) => s.lag_ms)].sort((a, b) => a - b);
    const p95 = sorted[Math.floor(sorted.length * 0.95)] || 0;
    const line = document.createElementNS(ns, "line");
    line.setAttribute("x1", 30);
    line.setAttribute("y1", y(p95));
    line.setAttribute("x2", 790);
    line.setAttribute("y2", y(p95));
    line.setAttribute("stroke", "#ffa657");
    line.setAttribute("stroke-dasharray", "4,4");
    chartSvg.appendChild(line);
    const label = document.createElementNS(ns, "text");
    label.setAttribute("x", 38);
    label.setAttribute("y", y(p95) - 4);
    label.setAttribute("fill", "#ffa657");
    label.setAttribute("font-size", 10);
    label.textContent = `p95 ${p95.toFixed(0)} ms`;
    chartSvg.appendChild(label);
  }

  return {
    onSurvivor: (d) => {
      const now = d.ts_ms || Date.now();
      const key = d.survivor_id;
      let entry = survivors.get(key);
      if (!entry) {
        entry = { first_ms: now, by_node: new Set(), first_by: d.detected_by };
        survivors.set(key, entry);
      } else {
        const lag = now - entry.first_ms;
        lagSamples.push({ ts_ms: now, lag_ms: Math.max(0, lag) });
        if (lagSamples.length > 300) lagSamples.shift();
      }
      entry.by_node.add(d.detected_by);
      scheduleRender();
    },
  };
}
