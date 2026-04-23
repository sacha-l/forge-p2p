// Map panel: canvas rendering of the merged occupancy grid + survivor
// markers + hazard overlays. Zero ground-truth leakage — this is the
// mesh-visible state, gossiped via map/merge.

import { classMeta, prettyRobotId } from "/app/panels/classes.js";

export function initMap(canvas, initialEnv, { onRobotClick } = {}) {
  let env = initialEnv;
  const ctx = canvas.getContext("2d");
  const grid = new Map();
  const survivors = new Map();
  const robots = new Map();
  let classFilter = null;
  let scheduled = false;

  function schedule() {
    if (scheduled) return;
    scheduled = true;
    requestAnimationFrame(() => {
      scheduled = false;
      render();
    });
  }

  function vx(x) {
    return (x / (env.footprint?.x || 40)) * canvas.width;
  }
  function vy(y) {
    return canvas.height - (y / (env.footprint?.y || 25)) * canvas.height;
  }

  function render() {
    ctx.fillStyle = "#0b0d12";
    ctx.fillRect(0, 0, canvas.width, canvas.height);

    (env.hazards || []).forEach((h) => {
      ctx.beginPath();
      ctx.fillStyle = h.kind === "gas_leak" ? "#c13c3c33" : "#c19a3c33";
      ctx.strokeStyle = h.kind === "gas_leak" ? "#c13c3c" : "#c19a3c";
      h.polygon.forEach(([px, py], i) => {
        if (i === 0) ctx.moveTo(vx(px), vy(py));
        else ctx.lineTo(vx(px), vy(py));
      });
      ctx.closePath();
      ctx.fill();
      ctx.stroke();
      const cx = h.polygon.reduce((s, p) => s + p[0], 0) / h.polygon.length;
      const cy = h.polygon.reduce((s, p) => s + p[1], 0) / h.polygon.length;
      ctx.fillStyle = h.kind === "gas_leak" ? "#ff9696" : "#ffd488";
      ctx.font = "10px system-ui";
      ctx.textAlign = "center";
      ctx.fillText(h.kind.replace("_", " "), vx(cx), vy(cy));
    });

    const CELL = 0.5;
    const cw = (CELL / (env.footprint?.x || 40)) * canvas.width + 1;
    const ch = (CELL / (env.footprint?.y || 25)) * canvas.height + 1;
    grid.forEach((occ, key) => {
      const [, xs, ys] = key.split(",");
      const gx = parseInt(xs, 10) * CELL;
      const gy = parseInt(ys, 10) * CELL;
      const a = Math.min(1, occ / 255) * 0.55;
      ctx.fillStyle = `rgba(108, 158, 255, ${a})`;
      ctx.fillRect(vx(gx), vy(gy) - ch, cw, ch);
    });

    survivors.forEach((p, id) => {
      ctx.beginPath();
      ctx.arc(vx(p.x), vy(p.y), 7, 0, Math.PI * 2);
      ctx.fillStyle = "#06d6a0";
      ctx.fill();
      ctx.strokeStyle = "#0b0d12";
      ctx.lineWidth = 1.5;
      ctx.stroke();
      ctx.fillStyle = "#06d6a0";
      ctx.font = "10px system-ui";
      ctx.textAlign = "left";
      ctx.fillText(id, vx(p.x) + 10, vy(p.y) + 3);
    });

    robots.forEach((r, id) => {
      const meta = classMeta(r.class);
      const dim = classFilter && !classFilter.has(r.class) ? 0.2 : 1.0;
      ctx.save();
      ctx.globalAlpha = dim;
      ctx.beginPath();
      ctx.arc(vx(r.pose.x), vy(r.pose.y), 5, 0, Math.PI * 2);
      ctx.fillStyle = r.status === "byzantine" ? "#ff4c4c" : meta.color;
      ctx.fill();
      ctx.strokeStyle = "#0b0d12";
      ctx.lineWidth = 1;
      ctx.stroke();
      ctx.fillStyle = "#e0e3eb";
      ctx.font = "10px ui-monospace, monospace";
      ctx.textAlign = "left";
      ctx.fillText(prettyRobotId(id), vx(r.pose.x) + 7, vy(r.pose.y) - 6);
      ctx.restore();
    });
  }

  // Hit-testing for click-to-inspect.
  canvas.style.cursor = "pointer";
  canvas.addEventListener("click", (ev) => {
    const rect = canvas.getBoundingClientRect();
    const px = ((ev.clientX - rect.left) / rect.width) * canvas.width;
    const py = ((ev.clientY - rect.top) / rect.height) * canvas.height;
    let best = null;
    let bestD = 100;
    robots.forEach((r, id) => {
      const dx = vx(r.pose.x) - px;
      const dy = vy(r.pose.y) - py;
      const d2 = dx * dx + dy * dy;
      if (d2 < bestD) {
        bestD = d2;
        best = id;
      }
    });
    if (best) onRobotClick?.(best);
  });

  return {
    onGridChunk: () => schedule(),
    onSurvivor: (d) => {
      survivors.set(d.survivor_id, d.pose);
      schedule();
    },
    onPose: (d) => {
      robots.set(d.robot_id, { pose: d.pose, class: d.class, status: d.status });
      schedule();
    },
    setClassFilter: (set) => {
      classFilter = set;
      schedule();
    },
    setEnv: (e) => {
      env = e;
      schedule();
    },
  };
}
