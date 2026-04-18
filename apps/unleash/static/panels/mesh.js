// Mesh panel: 2D top-down view of the environment footprint with robots as
// icons, connections as colour-coded links, and a latency-ring overlay per
// robot that expands under degraded conditions.
//
// Hooks:
//   onRobotClick(robot_id)  — delegate for inspector drawer
//   setClassFilter(set)     — dim robots whose class is NOT in the set
//   setEnv(env)             — refresh footprint / hazards after /api/mission

import { CLASSES, classMeta, prettyRobotId } from "/app/panels/classes.js";

export function initMesh(svg, initialEnv, { onRobotClick } = {}) {
  const robots = new Map(); // robot_id -> {class, pose, status}
  let linkOverride = null;
  let env = initialEnv;
  let classFilter = null; // null = show all

  function vx(x) {
    return (x / (env.footprint?.x || 40)) * 800;
  }
  function vy(y) {
    return 500 - (y / (env.footprint?.y || 25)) * 500;
  }

  function render() {
    const ns = "http://www.w3.org/2000/svg";
    svg.innerHTML = "";
    const rect = document.createElementNS(ns, "rect");
    rect.setAttribute("x", 0);
    rect.setAttribute("y", 0);
    rect.setAttribute("width", 800);
    rect.setAttribute("height", 500);
    rect.setAttribute("fill", "#0b0d12");
    svg.appendChild(rect);

    (env.hazards || []).forEach((h) => {
      const pts = h.polygon.map(([x, y]) => `${vx(x)},${vy(y)}`).join(" ");
      const poly = document.createElementNS(ns, "polygon");
      poly.setAttribute("points", pts);
      poly.setAttribute("fill", h.kind === "gas_leak" ? "#c13c3c33" : "#c19a3c33");
      poly.setAttribute("stroke", h.kind === "gas_leak" ? "#c13c3c" : "#c19a3c");
      poly.setAttribute("stroke-width", 1);
      svg.appendChild(poly);
      // label
      if (h.polygon.length) {
        const cx = h.polygon.reduce((s, p) => s + p[0], 0) / h.polygon.length;
        const cy = h.polygon.reduce((s, p) => s + p[1], 0) / h.polygon.length;
        const t = document.createElementNS(ns, "text");
        t.setAttribute("x", vx(cx));
        t.setAttribute("y", vy(cy));
        t.setAttribute("fill", h.kind === "gas_leak" ? "#ff9696" : "#ffd488");
        t.setAttribute("font-size", 10);
        t.setAttribute("text-anchor", "middle");
        t.textContent = h.kind.replace("_", " ");
        svg.appendChild(t);
      }
    });

    const ids = [...robots.keys()];
    for (let i = 0; i < ids.length; i++) {
      for (let j = i + 1; j < ids.length; j++) {
        const a = robots.get(ids[i]);
        const b = robots.get(ids[j]);
        const dx = a.pose.x - b.pose.x;
        const dy = a.pose.y - b.pose.y;
        const d = Math.sqrt(dx * dx + dy * dy);
        if (d > 18) continue;
        const line = document.createElementNS(ns, "line");
        line.setAttribute("x1", vx(a.pose.x));
        line.setAttribute("y1", vy(a.pose.y));
        line.setAttribute("x2", vx(b.pose.x));
        line.setAttribute("y2", vy(b.pose.y));
        const colour =
          linkOverride === "degraded"
            ? "#c19a3c"
            : linkOverride === "blackout"
              ? "#c13c3c"
              : "#3e8a5a";
        line.setAttribute("stroke", colour);
        line.setAttribute("stroke-width", 1);
        line.setAttribute("opacity", 0.55);
        svg.appendChild(line);
      }
    }

    const ringRadius =
      linkOverride === "degraded" ? 28 : linkOverride === "blackout" ? 40 : 12;
    robots.forEach((r) => {
      const cx = vx(r.pose.x);
      const cy = vy(r.pose.y);
      const meta = classMeta(r.class);
      const dim =
        classFilter && !classFilter.has(r.class) ? 0.15 : 1.0;
      const ring = document.createElementNS(ns, "circle");
      ring.setAttribute("cx", cx);
      ring.setAttribute("cy", cy);
      ring.setAttribute("r", ringRadius);
      ring.setAttribute("fill", "none");
      ring.setAttribute("stroke", meta.color);
      ring.setAttribute("stroke-opacity", 0.32 * dim);
      svg.appendChild(ring);

      const dot = document.createElementNS(ns, "circle");
      dot.setAttribute("cx", cx);
      dot.setAttribute("cy", cy);
      dot.setAttribute("r", 6);
      dot.setAttribute("fill", r.status === "byzantine" ? "#ff4c4c" : meta.color);
      dot.setAttribute("stroke", "#0b0d12");
      dot.setAttribute("stroke-width", 1);
      dot.setAttribute("opacity", dim);
      dot.style.cursor = "pointer";
      dot.addEventListener("click", () => onRobotClick?.(r.robot_id));
      dot.addEventListener("mouseenter", () => (dot.setAttribute("r", 8)));
      dot.addEventListener("mouseleave", () => (dot.setAttribute("r", 6)));
      svg.appendChild(dot);

      const label = document.createElementNS(ns, "text");
      label.setAttribute("x", cx + 9);
      label.setAttribute("y", cy - 8);
      label.setAttribute("fill", "#e0e3eb");
      label.setAttribute("font-size", 10);
      label.setAttribute("font-family", "ui-monospace, monospace");
      label.setAttribute("opacity", dim);
      label.textContent = prettyRobotId(r.robot_id);
      svg.appendChild(label);

      const title = document.createElementNS(ns, "title");
      title.textContent = `${prettyRobotId(r.robot_id)}\n${meta.label}\nstatus: ${r.status || "—"}\nbattery: ${Math.round((r.battery ?? 0) * 100)}%\n(click to inspect)`;
      dot.appendChild(title);
    });
  }

  return {
    onPose: (d) => {
      robots.set(d.robot_id, {
        robot_id: d.robot_id,
        class: d.class,
        pose: d.pose,
        status: d.status,
        battery: d.battery,
      });
      render();
    },
    onPeerConnected: () => render(),
    onPeerDisconnected: () => render(),
    onLinkOverride: (d) => {
      linkOverride = d.profile;
      render();
    },
    setClassFilter: (set) => {
      classFilter = set;
      render();
    },
    setEnv: (e) => {
      env = e;
      render();
    },
  };
}

// Back-compat export for any consumer relying on the old constant.
export const CLASS_COLOUR = Object.fromEntries(
  Object.entries(CLASSES).map(([k, v]) => [k, v.color]),
);
