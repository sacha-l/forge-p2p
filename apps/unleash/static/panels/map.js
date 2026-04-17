// Map panel: canvas rendering of the merged occupancy grid + survivor
// markers + hazard overlays. Zero ground-truth leakage — this is the
// mesh-visible state, gossiped via map/merge.

export function initMap(canvas, env) {
  const footprint = env.footprint || { x: 40, y: 25 };
  const ctx = canvas.getContext("2d");
  const grid = new Map(); // "floor,x,y" -> occupancy (uint8)
  const survivors = new Map(); // id -> pose
  const robots = new Map(); // id -> pose
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
    return (x / footprint.x) * canvas.width;
  }
  function vy(y) {
    return canvas.height - (y / footprint.y) * canvas.height;
  }

  function render() {
    ctx.fillStyle = "#0b0d12";
    ctx.fillRect(0, 0, canvas.width, canvas.height);

    // Hazards
    (env.hazards || []).forEach((h) => {
      ctx.beginPath();
      ctx.fillStyle = h.type === "gas_leak" ? "#c13c3c33" : "#c19a3c33";
      ctx.strokeStyle = h.type === "gas_leak" ? "#c13c3c" : "#c19a3c";
      h.polygon.forEach(([px, py], i) => {
        if (i === 0) ctx.moveTo(vx(px), vy(py));
        else ctx.lineTo(vx(px), vy(py));
      });
      ctx.closePath();
      ctx.fill();
      ctx.stroke();
    });

    // Grid cells (rough 0.5 m resolution)
    const CELL = 0.5;
    const cw = (CELL / footprint.x) * canvas.width + 1;
    const ch = (CELL / footprint.y) * canvas.height + 1;
    grid.forEach((occ, key) => {
      const [, xs, ys] = key.split(",");
      const gx = parseInt(xs, 10) * CELL;
      const gy = parseInt(ys, 10) * CELL;
      const a = Math.min(1, occ / 255) * 0.7;
      ctx.fillStyle = `rgba(108, 158, 255, ${a})`;
      ctx.fillRect(vx(gx), vy(gy) - ch, cw, ch);
    });

    // Survivors
    survivors.forEach((p) => {
      ctx.beginPath();
      ctx.arc(vx(p.x), vy(p.y), 6, 0, Math.PI * 2);
      ctx.fillStyle = "#06d6a0";
      ctx.fill();
      ctx.strokeStyle = "#0b0d12";
      ctx.stroke();
    });

    // Robots on top
    robots.forEach((p, id) => {
      ctx.beginPath();
      ctx.arc(vx(p.x), vy(p.y), 4, 0, Math.PI * 2);
      ctx.fillStyle = "#ffd166";
      ctx.fill();
      ctx.fillStyle = "#e0e3eb";
      ctx.font = "10px system-ui";
      ctx.fillText(id, vx(p.x) + 6, vy(p.y) - 4);
    });
  }

  return {
    onGridChunk: (d) => {
      // `d` has just metadata (cell_count). Fetching the cells themselves
      // would be a second round-trip; we approximate by showing chunk count.
      schedule();
    },
    onSurvivor: (d) => {
      survivors.set(d.survivor_id, d.pose);
      schedule();
    },
    onPose: (d) => {
      robots.set(d.robot_id, d.pose);
      schedule();
    },
  };
}
