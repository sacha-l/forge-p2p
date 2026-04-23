// Translate MeshEvent::Custom payloads into one-line English sentences for
// the event log. Emits objects: { ts, tone: "info"|"success"|"warn"|"error",
// glyph, text } that the renderer maps to colour-coded rows.

import { prettyRobotId, classMeta } from "/app/panels/classes.js";

const THROTTLE = {
  "unleash/pose": 15_000, // never log more often than 15 s per robot
  "unleash/consensus": 30_000, // per robot
  "unleash/bundle": 20_000, // per robot
  "unleash/tick": Infinity, // never log
  "unleash/grid": Infinity, // never log (visualised in map panel)
};

const lastLog = new Map(); // key -> ts

function throttleOk(label, bucketKey) {
  const limit = THROTTLE[label];
  if (limit === undefined) return true;
  if (limit === Infinity) return false;
  const k = `${label}:${bucketKey}`;
  const now = Date.now();
  const last = lastLog.get(k) || 0;
  if (now - last < limit) return false;
  lastLog.set(k, now);
  return true;
}

export function narrate(label, payload) {
  const ts = Date.now();
  switch (label) {
    case "unleash/phase": {
      const idx = payload.index ?? "?";
      const desc = payload.description || "";
      const phase = (payload.phase || "").toString();
      const glyph =
        phase === "byzantine"
          ? "☠"
          : phase === "degraded"
            ? "⚠"
            : phase === "dropout"
              ? "✖"
              : phase === "complete"
                ? "✔"
                : "▶";
      const tone =
        phase === "byzantine" || phase === "dropout"
          ? "error"
          : phase === "degraded"
            ? "warn"
            : phase === "complete"
              ? "success"
              : "info";
      return [
        {
          ts,
          tone,
          glyph,
          text: `Phase ${idx} · ${capitalise(phase)} — ${desc}`,
        },
      ];
    }
    case "unleash/task_winner": {
      if (!payload.winner) return [];
      const bucket = payload.task_id || "unknown";
      if (!throttleOk("unleash/task_winner:" + bucket, bucket)) {
        // Only re-announce a winner change; bootstrap.js suppresses repeats.
      }
      return [
        {
          ts,
          tone: "success",
          glyph: "⚑",
          text: `Task "${payload.task_pretty || payload.task_id}" → ${prettyRobotId(payload.winner)} (score ${fmt(payload.bid_score)})`,
        },
      ];
    }
    case "unleash/survivor": {
      const who = prettyRobotId(payload.detected_by);
      const loc = payload.pose
        ? `(${payload.pose.x.toFixed(1)}, ${payload.pose.y.toFixed(1)})`
        : "";
      return [
        {
          ts,
          tone: "success",
          glyph: "🧍",
          text: `Survivor ${payload.survivor_id} found by ${who} ${loc}`.trim(),
        },
      ];
    }
    case "unleash/link_override": {
      const p = payload.profile || "default";
      const glyph = p === "default" ? "↺" : p === "degraded" ? "⚠" : "✖";
      const tone = p === "default" ? "info" : p === "degraded" ? "warn" : "error";
      const label =
        p === "degraded"
          ? "degraded (2 Mbps, 80 ms, 40% loss)"
          : p === "blackout"
            ? "blackout"
            : "default restored";
      return [{ ts, tone, glyph, text: `Link profile: ${label}` }];
    }
    case "unleash/pose": {
      // Only log one line per robot every 15 s, and only when something
      // meaningful changed (class / status transition).
      if (!payload.robot_id) return [];
      if (!throttleOk("unleash/pose", payload.robot_id)) return [];
      if (payload.status === "byzantine") {
        return [
          {
            ts,
            tone: "error",
            glyph: "☠",
            text: `${prettyRobotId(payload.robot_id)} is reporting Byzantine telemetry`,
          },
        ];
      }
      return [];
    }
    case "unleash/bundle": {
      if (!payload.robot_id) return [];
      if (!throttleOk("unleash/bundle", payload.robot_id)) return [];
      if (!payload.bundle || payload.bundle.length === 0) return [];
      const summary = payload.bundle.map(([t]) => t).join(", ");
      return [
        {
          ts,
          tone: "info",
          glyph: "⇌",
          text: `${prettyRobotId(payload.robot_id)} bundle: ${summary}`,
        },
      ];
    }
    case "unleash/consensus": {
      if (!payload.robot_id) return [];
      if (!throttleOk("unleash/consensus", payload.robot_id)) return [];
      return [];
    }
    case "peer-connected": {
      return [
        {
          ts,
          tone: "info",
          glyph: "+",
          text: `peer ${short(payload.peer_id)} joined the mesh`,
        },
      ];
    }
    case "peer-disconnected": {
      return [
        {
          ts,
          tone: "warn",
          glyph: "−",
          text: `peer ${short(payload.peer_id)} left`,
        },
      ];
    }
    default:
      return [];
  }
}

function fmt(n) {
  if (typeof n !== "number" || Number.isNaN(n)) return "—";
  return n.toFixed(2);
}

function capitalise(s) {
  return s ? s[0].toUpperCase() + s.slice(1) : s;
}

function short(pid) {
  return pid ? pid.slice(0, 8) + "…" + pid.slice(-4) : "?";
}

export function renderLog(rootEl, entries) {
  rootEl.innerHTML = "";
  for (const e of entries) {
    const row = document.createElement("div");
    row.className = `log-row log-${e.tone}`;
    const time = document.createElement("span");
    time.className = "log-time";
    const d = new Date(e.ts);
    time.textContent = `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
    const glyph = document.createElement("span");
    glyph.className = "log-glyph";
    glyph.textContent = e.glyph;
    const text = document.createElement("span");
    text.className = "log-text";
    text.textContent = e.text;
    row.append(time, glyph, text);
    rootEl.appendChild(row);
  }
}

function pad(n) {
  return String(n).padStart(2, "0");
}

// Also exposed for panels that want to narrate their own events.
export { classMeta };
