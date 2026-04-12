// ForgeP2P Mesh Visualizer
// Connects to the forge-ui WebSocket and renders a D3 force-directed graph.

(function () {
  "use strict";

  // --- State ---
  let selfPeerId = null;
  const peers = new Map();   // peer_id -> { id, addrs }
  const links = new Map();   // "src->dst" -> { source, target }
  const hasD3 = typeof d3 !== "undefined";

  // --- D3 Setup (only if available) ---
  let simulation = null;
  let linkGroup = null;
  let nodeGroup = null;
  let svg = null;

  function setupD3() {
    if (!hasD3) {
      console.warn("[forge-ui] D3.js failed to load; mesh graph disabled");
      const meshPanel = document.getElementById("mesh-panel");
      if (meshPanel) {
        meshPanel.innerHTML = '<h2>Mesh</h2><div style="padding:20px;color:#888;font-size:12px;">D3.js could not be loaded from CDN.<br>Mesh graph disabled.</div>';
      }
      return;
    }
    svg = d3.select("#mesh-svg");
    const g = svg.append("g");
    linkGroup = g.append("g").attr("class", "links");
    nodeGroup = g.append("g").attr("class", "nodes");

    simulation = d3.forceSimulation()
      .force("link", d3.forceLink().id(d => d.id).distance(100))
      .force("charge", d3.forceManyBody().strength(-200))
      .force("center", d3.forceCenter(width() / 2, height() / 2))
      .on("tick", ticked);

    simulation.stop();
  }

  function width() { return svg ? svg.node().getBoundingClientRect().width : 0; }
  function height() { return svg ? svg.node().getBoundingClientRect().height : 0; }

  function ticked() {
    if (!hasD3) return;
    linkGroup.selectAll(".link")
      .attr("x1", d => d.source.x)
      .attr("y1", d => d.source.y)
      .attr("x2", d => d.target.x)
      .attr("y2", d => d.target.y);
    nodeGroup.selectAll(".node")
      .attr("transform", d => `translate(${d.x},${d.y})`);
  }

  function updateGraph() {
    if (!hasD3) return;
    const nodeData = Array.from(peers.values());
    const linkData = Array.from(links.values());

    const linkSel = linkGroup.selectAll(".link").data(linkData, d => d.source.id + "->" + d.target.id);
    linkSel.exit().remove();
    linkSel.enter().append("line").attr("class", "link");

    const nodeSel = nodeGroup.selectAll(".node").data(nodeData, d => d.id);
    nodeSel.exit().remove();
    const enter = nodeSel.enter().append("g")
      .attr("class", d => d.id === selfPeerId ? "node self" : "node")
      .call(drag(simulation));
    enter.append("circle").attr("r", 14);
    enter.append("text")
      .attr("dy", 28)
      .attr("text-anchor", "middle")
      .text(d => shortId(d.id));

    simulation.nodes(nodeData);
    simulation.force("link").links(linkData);
    simulation.force("center", d3.forceCenter(width() / 2, height() / 2));
    simulation.alpha(0.3).restart();
  }

  function drag(sim) {
    return d3.drag()
      .on("start", (event, d) => {
        if (!event.active) sim.alphaTarget(0.3).restart();
        d.fx = d.x; d.fy = d.y;
      })
      .on("drag", (event, d) => {
        d.fx = event.x; d.fy = event.y;
      })
      .on("end", (event, d) => {
        if (!event.active) sim.alphaTarget(0);
        d.fx = null; d.fy = null;
      });
  }

  function flashLink(srcId, dstId) {
    if (!hasD3) return;
    const key = srcId + "->" + dstId;
    linkGroup.selectAll(".link")
      .filter(d => (d.source.id + "->" + d.target.id) === key ||
                   (d.target.id + "->" + d.source.id) === key)
      .classed("active", true)
      .transition().duration(600)
      .on("end", function () { d3.select(this).classed("active", false); });
  }

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
      // Server is running — hide loading after a short pause
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
  try {
    setupD3();
  } catch (e) {
    console.error("[forge-ui] D3 setup failed:", e);
  }

  fetch("/config")
    .then(r => r.json())
    .then(cfg => {
      const title = document.getElementById("app-title");
      if (title) title.textContent = cfg.app_name || "ForgeP2P";
      document.title = cfg.app_name || "ForgeP2P";
    })
    .catch(err => console.warn("[forge-ui] /config fetch failed:", err));

  // 30s failsafe to hide loading even if nothing happens
  setTimeout(hideLoading, 30000);

  connect();
})();
