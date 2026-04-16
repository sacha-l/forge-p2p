// ForgeP2P Mesh Visualizer + event bus.
//
// This file owns the WebSocket connection to /ws and the SVG mesh graph.
// Other modules (peers.js, app-specific code) subscribe to events via
// `window.forgeUI.onEvent.push(handler)` — no second WebSocket needed.

(function () {
  "use strict";

  // --- Cross-module event bus ---
  window.forgeUI = window.forgeUI || { onEvent: [] };

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

  function computePositions() {
    const { w, h } = svgSize();
    const cx = w / 2;
    const cy = h / 2;
    const radius = Math.min(w, h) / 2 - 40;
    const positions = new Map();

    if (selfPeerId && peers.has(selfPeerId)) {
      positions.set(selfPeerId, { x: cx, y: cy });
    }

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
    while (svg.firstChild) svg.removeChild(svg.firstChild);

    const positions = computePositions();

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

  window.addEventListener("resize", updateGraph);

  // --- Loading overlay ---
  const loadingEl = document.getElementById("loading");
  const phaseEl = loadingEl ? loadingEl.querySelector(".phase") : null;

  function advancePhase(text) { if (phaseEl) phaseEl.textContent = text; }
  function hideLoading() { if (loadingEl) loadingEl.classList.add("hidden"); }

  // --- Event log ---
  const logEl = document.getElementById("log-entries");
  const MAX_LOG = 200;

  function logEvent(cssClass, label, detail) {
    if (!logEl) return;
    const el = document.createElement("div");
    el.className = "log-entry " + cssClass;
    const now = new Date().toLocaleTimeString();
    el.innerHTML = `<span class="time">${now}</span><span class="label">${label}</span>${escapeHtml(detail)}`;
    logEl.prepend(el);
    while (logEl.children.length > MAX_LOG) logEl.lastChild.remove();
  }

  function escapeHtml(s) {
    return String(s)
      .replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  }

  // --- Node identity card in the header ---
  const nodePeerIdEl = document.getElementById("node-peer-id");
  const nodeAddrEl = document.getElementById("node-addr");

  function setNodeCard(peerId, addrs) {
    if (nodePeerIdEl) {
      nodePeerIdEl.textContent = peerId || "—";
      nodePeerIdEl.dataset.value = peerId || "";
    }
    if (nodeAddrEl) {
      const loopback = (addrs || []).find(a => a.startsWith("/ip4/127.0.0.1/")) || (addrs || [])[0] || "";
      nodeAddrEl.textContent = loopback || "—";
      nodeAddrEl.dataset.value = loopback || "";
    }
  }

  function wireCopy(el) {
    if (!el) return;
    el.addEventListener("click", () => {
      const value = el.dataset.value || el.textContent;
      if (!value || value === "—") return;
      navigator.clipboard.writeText(value).then(() => {
        el.classList.add("copied");
        setTimeout(() => el.classList.remove("copied"), 700);
      });
    });
  }
  wireCopy(nodePeerIdEl);
  wireCopy(nodeAddrEl);

  // --- Tabs ---
  document.querySelectorAll("#tabs .tab").forEach(btn => {
    btn.addEventListener("click", () => {
      const name = btn.dataset.tab;
      document.querySelectorAll("#tabs .tab").forEach(b => b.classList.toggle("active", b === btn));
      document.querySelectorAll(".tab-content").forEach(c => {
        c.classList.toggle("active", c.id === "tab-" + name);
      });
    });
  });

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
    const ws = new WebSocket(url);

    ws.onopen = function () {
      setStatus("connected", "Connected");
      advancePhase("Connected, waiting for node info...");
      setTimeout(hideLoading, 2000);
    };

    ws.onclose = function () {
      setStatus("connecting", "Reconnecting...");
      setTimeout(connect, 2000);
    };

    ws.onerror = function () { setStatus("error", "Connection error"); };

    ws.onmessage = function (evt) {
      let event;
      try { event = JSON.parse(evt.data); } catch { return; }
      handleEvent(event);
      // Fan out to subscribed modules (peers.js, app-specific handlers).
      for (const h of window.forgeUI.onEvent) {
        try { h(event); } catch (e) { console.error("[forge-ui] handler error:", e); }
      }
    };
  }

  function handleEvent(e) {
    switch (e.type) {
      case "NodeStarted":
        selfPeerId = e.peer_id;
        peers.set(e.peer_id, { id: e.peer_id, addrs: e.listen_addrs });
        setNodeCard(e.peer_id, e.listen_addrs);
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

      case "PeerDiscovered":
        logEvent("peer-discovered", "DISCOVER", `${shortId(e.peer_id)} @ ${e.addr} (${e.source})`);
        break;

      case "PeerLost":
        logEvent("peer-lost", "LOST", `${shortId(e.peer_id)} (${e.source})`);
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
  window.forgeUI.shortId = shortId;

  // --- Init ---
  fetch("/config")
    .then(r => r.json())
    .then(cfg => {
      const title = document.getElementById("app-title");
      if (title) title.textContent = cfg.app_name || "ForgeP2P";
      document.title = cfg.app_name || "ForgeP2P";
    })
    .catch(err => console.warn("[forge-ui] /config fetch failed:", err));

  // Seed node card from /api/node/info so a late-opened panel still sees identity.
  fetch("/api/node/info")
    .then(r => r.ok ? r.json() : null)
    .then(info => {
      if (!info) return;
      if (!selfPeerId) selfPeerId = info.peer_id;
      setNodeCard(info.peer_id, info.listen_addrs);
    })
    .catch(() => {});

  setTimeout(hideLoading, 30000);
  connect();
})();
