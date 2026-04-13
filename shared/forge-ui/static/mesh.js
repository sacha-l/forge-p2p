// ForgeP2P Mesh Visualizer
// Vanilla JS SVG — no external dependencies, works offline.

(function () {
  "use strict";

  // --- State ---
  let selfPeerId = null;
  const peers = new Map();   // peer_id -> { id, addrs }
  const links = new Map();   // "a->b" -> { source, target }

  const SVG_NS = "http://www.w3.org/2000/svg";
  const svg = document.getElementById("mesh-svg");

  function svgSize() {
    if (!svg) return { w: 0, h: 0 };
    const r = svg.getBoundingClientRect();
    return { w: r.width || 300, h: r.height || 300 };
  }

  // --- Rendering ---

  function computePositions() {
    const { w, h } = svgSize();
    const cx = w / 2;
    const cy = h / 2;
    const radius = Math.min(w, h) / 2 - 40;
    const positions = new Map();

    // Self goes at center
    if (selfPeerId && peers.has(selfPeerId)) {
      positions.set(selfPeerId, { x: cx, y: cy });
    }

    // Other peers in a circle around self
    const others = Array.from(peers.keys()).filter(id => id !== selfPeerId);
    const n = others.length || 1;
    others.forEach((id, i) => {
      const angle = (2 * Math.PI * i) / n - Math.PI / 2;
      positions.set(id, {
        x: cx + radius * Math.cos(angle),
        y: cy + radius * Math.sin(angle),
      });
    });

    return positions;
  }

  function updateGraph() {
    if (!svg) return;
    // Wipe and redraw (small graphs only — fine)
    while (svg.firstChild) svg.removeChild(svg.firstChild);

    const positions = computePositions();

    // Draw links first (so they sit under nodes)
    for (const link of links.values()) {
      const src = positions.get(link.source);
      const dst = positions.get(link.target);
      if (!src || !dst) continue;
      const line = document.createElementNS(SVG_NS, "line");
      line.setAttribute("x1", src.x);
      line.setAttribute("y1", src.y);
      line.setAttribute("x2", dst.x);
      line.setAttribute("y2", dst.y);
      line.setAttribute("class", "link");
      line.dataset.linkKey = link.source + "->" + link.target;
      svg.appendChild(line);
    }

    // Draw nodes
    for (const peer of peers.values()) {
      const pos = positions.get(peer.id);
      if (!pos) continue;
      const g = document.createElementNS(SVG_NS, "g");
      g.setAttribute("class", peer.id === selfPeerId ? "node self" : "node");
      g.setAttribute("transform", `translate(${pos.x},${pos.y})`);

      const circle = document.createElementNS(SVG_NS, "circle");
      circle.setAttribute("r", 14);
      g.appendChild(circle);

      const text = document.createElementNS(SVG_NS, "text");
      text.setAttribute("dy", 28);
      text.setAttribute("text-anchor", "middle");
      text.textContent = shortId(peer.id);
      g.appendChild(text);

      svg.appendChild(g);
    }
  }

  function flashLink(srcId, dstId) {
    if (!svg) return;
    const keys = [srcId + "->" + dstId, dstId + "->" + srcId];
    svg.querySelectorAll("line.link").forEach(line => {
      if (keys.includes(line.dataset.linkKey)) {
        line.classList.add("active");
        setTimeout(() => line.classList.remove("active"), 600);
      }
    });
  }

  // Re-layout on window resize
  window.addEventListener("resize", updateGraph);

  // --- Loading phases ---
  const loadingEl = document.getElementById("loading");
  const phaseEl = loadingEl ? loadingEl.querySelector(".phase") : null;

  function advancePhase(text) {
    if (phaseEl) phaseEl.textContent = text;
  }

  function hideLoading() {
    if (loadingEl) loadingEl.classList.add("hidden");
  }

  // --- Event log ---
  const logEl = document.getElementById("log-entries");
  const MAX_LOG = 200;

  function logEvent(cssClass, label, detail) {
    if (!logEl) return;
    const el = document.createElement("div");
    el.className = "log-entry " + cssClass;
    const now = new Date().toLocaleTimeString();
    el.innerHTML = `<span class="time">${now}</span><span class="label">${label}</span>${detail}`;
    logEl.prepend(el);
    while (logEl.children.length > MAX_LOG) logEl.lastChild.remove();
  }

  // --- WebSocket ---
  const statusDot = document.getElementById("status-dot");
  const statusText = document.getElementById("status-text");

  function setStatus(state, text) {
    if (statusDot) statusDot.className = "dot " + state;
    if (statusText) statusText.textContent = text;
  }

  function connect() {
    const proto = location.protocol === "https:" ? "wss:" : "ws:";
    const url = `${proto}//${location.host}/ws`;
    console.log("[forge-ui] Connecting WebSocket:", url);
    const ws = new WebSocket(url);

    ws.onopen = function () {
      console.log("[forge-ui] WebSocket connected");
      setStatus("connected", "Connected");
      advancePhase("Connected, waiting for node info...");
      setTimeout(hideLoading, 2000);
    };

    ws.onclose = function (e) {
      console.warn("[forge-ui] WebSocket closed:", e.code, e.reason);
      setStatus("connecting", "Reconnecting...");
      setTimeout(connect, 2000);
    };

    ws.onerror = function (e) {
      console.error("[forge-ui] WebSocket error:", e);
      setStatus("error", "Connection error");
    };

    ws.onmessage = function (evt) {
      let event;
      try { event = JSON.parse(evt.data); } catch { return; }
      handleEvent(event);
    };
  }

  function handleEvent(e) {
    switch (e.type) {
      case "NodeStarted":
        selfPeerId = e.peer_id;
        peers.set(e.peer_id, { id: e.peer_id, addrs: e.listen_addrs });
        updateGraph();
        advancePhase("Listening for peers on " + (e.listen_addrs[0] || "..."));
        logEvent("node-started", "NODE", `Started ${shortId(e.peer_id)}`);
        setTimeout(hideLoading, 1500);
        break;

      case "PeerConnected":
        peers.set(e.peer_id, { id: e.peer_id, addrs: [e.addr] });
        if (selfPeerId) {
          links.set(selfPeerId + "->" + e.peer_id, { source: selfPeerId, target: e.peer_id });
        }
        updateGraph();
        hideLoading();
        logEvent("peer-connected", "CONNECT", `${shortId(e.peer_id)} @ ${e.addr}`);
        break;

      case "PeerDisconnected":
        peers.delete(e.peer_id);
        for (const [key] of links) {
          if (key.includes(e.peer_id)) links.delete(key);
        }
        updateGraph();
        logEvent("peer-disconnected", "DISCONNECT", shortId(e.peer_id));
        break;

      case "MessageSent":
        if (selfPeerId) flashLink(selfPeerId, e.to);
        logEvent("message-sent", "SEND", `to ${shortId(e.to)} [${e.topic}] ${e.size_bytes}B`);
        break;

      case "MessageReceived":
        if (selfPeerId) flashLink(e.from, selfPeerId);
        logEvent("message-received", "RECV", `from ${shortId(e.from)} [${e.topic}] ${e.size_bytes}B`);
        break;

      case "GossipJoined":
        logEvent("gossip-joined", "GOSSIP", `Joined topic: ${e.topic}`);
        break;

      case "ReplicaSync":
        logEvent("custom", "REPLICA", `${shortId(e.peer_id)} [${e.network}] ${e.status}`);
        break;

      case "Custom":
        logEvent("custom", e.label, e.detail);
        break;
    }
  }

  function shortId(peerId) {
    if (!peerId) return "?";
    if (peerId.length <= 12) return peerId;
    return peerId.slice(0, 6) + ".." + peerId.slice(-4);
  }

  // --- Init ---
  fetch("/config")
    .then(r => r.json())
    .then(cfg => {
      const title = document.getElementById("app-title");
      if (title) title.textContent = cfg.app_name || "ForgeP2P";
      document.title = cfg.app_name || "ForgeP2P";
    })
    .catch(err => console.warn("[forge-ui] /config fetch failed:", err));

  setTimeout(hideLoading, 30000);
  connect();
})();
