// Consensus panel: W-MSR scalar estimates as a scrolling time series, one
// line per robot. Byzantine robots inflate their own reading but the honest
// W-MSR filters them — those lines diverge visibly from the convergent band.

const WINDOW = 300; // samples kept per robot

export function initConsensus(svg, legend) {
  const series = new Map(); // robot_id -> {color, values: [{round, value}]}
  const byzantine = new Set();
  const palette = [
    "#6c9eff",
    "#8df0c5",
    "#ffa657",
    "#d78cff",
    "#ffd166",
    "#ef476f",
    "#06d6a0",
    "#118ab2",
    "#ffb4a2",
    "#b8c0ff",
  ];
  let paletteIx = 0;

  function colorFor(id) {
    if (!series.has(id)) {
      series.set(id, {
        color: palette[paletteIx++ % palette.length],
        values: [],
      });
    }
    return series.get(id);
  }

  function render() {
    const ns = "http://www.w3.org/2000/svg";
    svg.innerHTML = "";
    const bg = document.createElementNS(ns, "rect");
    bg.setAttribute("x", 0);
    bg.setAttribute("y", 0);
    bg.setAttribute("width", 800);
    bg.setAttribute("height", 300);
    bg.setAttribute("fill", "#0b0d12");
    svg.appendChild(bg);

    // Compute scales
    let allVals = [];
    series.forEach((s) => s.values.forEach((v) => allVals.push(v.value)));
    if (allVals.length === 0) return;
    let vmin = Math.min(...allVals);
    let vmax = Math.max(...allVals);
    if (vmax - vmin < 1) {
      vmin -= 1;
      vmax += 1;
    }
    const padding = (vmax - vmin) * 0.1;
    vmin -= padding;
    vmax += padding;

    const maxRound = Math.max(
      ...[...series.values()].flatMap((s) => s.values.map((v) => v.round))
    );
    const minRound = Math.max(0, maxRound - WINDOW);

    const x = (r) => ((r - minRound) / Math.max(1, maxRound - minRound)) * 760 + 30;
    const y = (v) => 280 - ((v - vmin) / (vmax - vmin)) * 260;

    // Axis lines
    const axisY = document.createElementNS(ns, "line");
    axisY.setAttribute("x1", 30);
    axisY.setAttribute("y1", 20);
    axisY.setAttribute("x2", 30);
    axisY.setAttribute("y2", 280);
    axisY.setAttribute("stroke", "#2a2f3a");
    svg.appendChild(axisY);

    legend.innerHTML = "";
    series.forEach((s, id) => {
      if (s.values.length === 0) return;
      const pts = s.values
        .filter((v) => v.round >= minRound)
        .map((v) => `${x(v.round)},${y(v.value)}`)
        .join(" ");
      const poly = document.createElementNS(ns, "polyline");
      poly.setAttribute("points", pts);
      poly.setAttribute("fill", "none");
      poly.setAttribute(
        "stroke",
        byzantine.has(id) ? "#ff4c4c" : s.color
      );
      poly.setAttribute("stroke-width", byzantine.has(id) ? 2 : 1.2);
      svg.appendChild(poly);

      const chip = document.createElement("span");
      chip.className = "legend-chip";
      chip.innerHTML = `<i style="background:${byzantine.has(id) ? "#ff4c4c" : s.color}"></i>${id}${byzantine.has(id) ? " (byz)" : ""}`;
      legend.appendChild(chip);
    });
  }

  return {
    onValue: (d) => {
      const s = colorFor(d.robot_id);
      s.values.push({ round: d.round, value: d.value });
      if (s.values.length > WINDOW) s.values.shift();
      // heuristic byzantine flag: value far from others
      if (Math.abs(d.value) > 100) byzantine.add(d.robot_id);
      render();
    },
  };
}
