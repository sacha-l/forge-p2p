// Mesh panel: 2D top-down view of the environment footprint with robots as
// icons, connections as colour-coded links, and a latency-ring overlay per
// robot that expands under degraded conditions.

const CLASS_COLOUR = {
  aerial_scout: "#6c9eff",
  aerial_mapper: "#8df0c5",
  ground_scout: "#ffa657",
  ground_workhorse: "#d78cff",
  breadcrumb: "#8d96a8",
};

export function initMesh(svg, env) {
  const robots = new Map(); // robot_id -> {class, pose, status}
  let linkOverride = null;
  const footprint = env.footprint || { x: 40, y: 25 };

  function vx(x) {
    return (x / footprint.x) * 800;
  }
  function vy(y) {
    return 500 - (y / footprint.y) * 500;
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

    // Hazards first
    (env.hazards || []).forEach((h) => {
      const pts = h.polygon.map(([x, y]) => `${vx(x)},${vy(y)}`).join(" ");
      const poly = document.createElementNS(ns, "polygon");
      poly.setAttribute("points", pts);
      poly.setAttribute("fill", h.type === "gas_leak" ? "#c13c3c33" : "#c19a3c33");
      poly.setAttribute("stroke", h.type === "gas_leak" ? "#c13c3c" : "#c19a3c");
      poly.setAttribute("stroke-width", 1);
      svg.appendChild(poly);
    });

    // Links between robots (naive: full graph within 15m, coloured by override)
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
        line.setAttribute("opacity", 0.6);
        svg.appendChild(line);
      }
    }

    // Latency rings + robot dots
    const ringRadius =
      linkOverride === "degraded" ? 28 : linkOverride === "blackout" ? 40 : 12;
    robots.forEach((r) => {
      const cx = vx(r.pose.x);
      const cy = vy(r.pose.y);
      const ring = document.createElementNS(ns, "circle");
      ring.setAttribute("cx", cx);
      ring.setAttribute("cy", cy);
      ring.setAttribute("r", ringRadius);
      ring.setAttribute("fill", "none");
      ring.setAttribute("stroke", CLASS_COLOUR[r.class] || "#999");
      ring.setAttribute("stroke-opacity", 0.35);
      svg.appendChild(ring);
      const dot = document.createElementNS(ns, "circle");
      dot.setAttribute("cx", cx);
      dot.setAttribute("cy", cy);
      dot.setAttribute("r", 6);
      dot.setAttribute("fill", r.status === "byzantine" ? "#ff4c4c" : CLASS_COLOUR[r.class] || "#eee");
      dot.setAttribute("stroke", "#0b0d12");
      dot.setAttribute("stroke-width", 1);
      svg.appendChild(dot);
      const label = document.createElementNS(ns, "text");
      label.setAttribute("x", cx + 8);
      label.setAttribute("y", cy - 8);
      label.setAttribute("fill", "#e0e3eb");
      label.setAttribute("font-size", 10);
      label.textContent = r.robot_id;
      svg.appendChild(label);
    });
  }

  return {
    onPose: (d) => {
      robots.set(d.robot_id, {
        robot_id: d.robot_id,
        class: d.class,
        pose: d.pose,
        status: d.status,
      });
      render();
    },
    onPeerConnected: (_peer) => render(),
    onPeerDisconnected: (_peer) => render(),
    onLinkOverride: (d) => {
      linkOverride = d.profile;
      render();
    },
  };
}
