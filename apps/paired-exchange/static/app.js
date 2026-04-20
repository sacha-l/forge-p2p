(function () {
  const subtitle = document.getElementById("subtitle");
  const peerList = document.getElementById("peer-list");
  const rttSummary = document.getElementById("rtt-summary");
  const rttChart = document.getElementById("rtt-chart");
  const rttLog = document.getElementById("rtt-log");

  async function refresh() {
    let state;
    try {
      const r = await fetch("/api/paired/state", { cache: "no-store" });
      if (!r.ok) throw new Error("HTTP " + r.status);
      state = await r.json();
    } catch (e) {
      subtitle.textContent = "state poll failed: " + e.message;
      return;
    }

    subtitle.textContent = "role " + state.role + " — polling /api/paired/state";
    renderPeers(state.peers || []);
    renderRtts(state.rtts || []);
  }

  function renderPeers(peers) {
    peerList.classList.toggle("empty", peers.length === 0);
    if (peers.length === 0) {
      peerList.textContent = "No peers yet. Waiting for a connection.";
      return;
    }
    peerList.innerHTML = "";
    for (const p of peers) {
      const row = document.createElement("div");
      row.className = "peer";

      const id = document.createElement("span");
      id.className = "id";
      id.textContent = abbreviate(p.peer_id);
      id.title = p.peer_id;

      const badge = document.createElement("span");
      badge.className = "badge " + (p.state || "unknown");
      badge.textContent = p.state || "unknown";

      const meta = document.createElement("span");
      meta.className = "meta";
      meta.textContent = buildMeta(p);

      row.append(id, badge, meta);
      peerList.appendChild(row);
    }
  }

  function renderRtts(rtts) {
    if (rtts.length === 0) {
      rttSummary.textContent = "Ping-pong will appear here once pairing completes.";
      rttLog.innerHTML = "";
      clearChart();
      return;
    }
    const last = rtts[rtts.length - 1];
    const avg = Math.round(rtts.reduce((s, r) => s + r.rtt_ms, 0) / rtts.length);
    rttSummary.textContent =
      rtts.length + " round-trip" + (rtts.length === 1 ? "" : "s") +
      " — last " + last.rtt_ms + " ms, avg " + avg + " ms";

    drawChart(rtts);

    rttLog.innerHTML = "";
    for (const r of rtts.slice().reverse()) {
      const li = document.createElement("li");
      li.textContent =
        "seq " + r.seq +
        "  " + r.rtt_ms + " ms  " +
        abbreviate(r.peer_id);
      rttLog.appendChild(li);
    }
  }

  function drawChart(rtts) {
    const w = 400;
    const h = 120;
    const pad = 4;
    const xs = w - pad * 2;
    const ys = h - pad * 2;
    const max = Math.max(1, ...rtts.map((r) => r.rtt_ms));
    const n = rtts.length;

    let d = "";
    rtts.forEach((r, i) => {
      const x = pad + (n === 1 ? xs / 2 : (i * xs) / (n - 1));
      const y = pad + ys - (r.rtt_ms / max) * ys;
      d += (i === 0 ? "M" : "L") + x.toFixed(1) + "," + y.toFixed(1) + " ";
    });

    rttChart.innerHTML =
      '<path d="' + d + '" fill="none" stroke="#1b5e20" stroke-width="1.4" />';
  }

  function clearChart() {
    rttChart.innerHTML = "";
  }

  function buildMeta(p) {
    if (p.state === "trusted" && p.since_ms != null) {
      return "trusted " + formatDuration(p.since_ms);
    }
    if (p.state === "awaiting" && p.since_ms != null) {
      return "waiting " + formatDuration(p.since_ms);
    }
    if (p.state === "failed" && p.reason) {
      return p.reason;
    }
    return "";
  }

  function formatDuration(ms) {
    if (ms < 1000) return ms + "ms";
    const s = Math.floor(ms / 1000);
    if (s < 60) return s + "s ago";
    const m = Math.floor(s / 60);
    return m + "m " + (s % 60) + "s ago";
  }

  function abbreviate(peerId) {
    if (!peerId) return "?";
    if (peerId.length <= 14) return peerId;
    return peerId.slice(0, 8) + "…" + peerId.slice(-4);
  }

  refresh();
  setInterval(refresh, 1000);
})();
