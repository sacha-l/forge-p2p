// ForgeP2P Mesh Visualizer
// Connects to the forge-ui WebSocket and renders a D3 force-directed graph.

(function () {
  "use strict";

  // --- State ---
  let selfPeerId = null;
  const peers = new Map();   // peer_id -> { id, addrs }
  const links = new Map();   // "src->dst" -> { source, target }

  // --- D3 Setup ---
  const svg = d3.select("#mesh-svg");
  const width = () => svg.node().getBoundingClientRect().width;
  const height = () => svg.node().getBoundingClientRect().height;

  const g = svg.append("g");
  let linkGroup = g.append("g").attr("class", "links");
  let nodeGroup = g.append("g").attr("class", "nodes");

  const simulation = d3.forceSimulation()
    .force("link", d3.forceLink().id(d => d.id).distance(100))
    .force("charge", d3.forceManyBody().strength(-200))
    .force("center", d3.forceCenter(width() / 2, height() / 2))
    .on("tick", ticked);

  simulation.stop();

  function ticked() {
    linkGroup.selectAll(".link")
      .attr("x1", d => d.source.x)
      .attr("y1", d => d.source.y)
      .attr("x2", d => d.target.x)
      .attr("y2", d => d.target.y);

    nodeGroup.selectAll(".node")
      .attr("transform", d => `translate(${d.x},${d.y})`);
  }

  function updateGraph() {
    const nodeData = Array.from(peers.values());
    const linkData = Array.from(links.values());

    // Links
    const linkSel = linkGroup.selectAll(".link").data(linkData, d => d.source.id + "->" + d.target.id);
    linkSel.exit().remove();
    linkSel.enter().append("line").attr("class", "link");

    // Nodes
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
  const phaseEl = loadingEl.querySelector(".phase");
  const phases = [
    "Starting node, generating keypair...",
    "Listening for peers...",
    "Waiting for peer connections...",
  ];
  let phaseIdx = 0;

  function advancePhase(text) {
    phaseEl.textContent = text || phases[Math.min(++phaseIdx, phases.length - 1)];
  }

  function hideLoading() {
    loadingEl.classList.add("hidden");
  }

  // --- Event log ---
  const logEl = document.getElementById("log-entries");
  const MAX_LOG = 200;

  function logEvent(cssClass, label, detail) {
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
    statusDot.className = "dot " + state;
    statusText.textContent = text;
  }

  function connect() {
    const proto = location.protocol === "https:" ? "wss:" : "ws:";
    const ws = new WebSocket(`${proto}//${location.host}/ws`);

    ws.onopen = function () {
      setStatus("connected", "Connected");
      advancePhase("Connected to node, waiting for events...");
    };

    ws.onclose = function () {
      setStatus("connecting", "Reconnecting...");
      setTimeout(connect, 2000);
    };

    ws.onerror = function () {
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
        // Node is running — hide the loading overlay after a brief pause
        // so the user can read the phase message
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
        // Remove all links involving this peer
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
      document.getElementById("app-title").textContent = cfg.app_name || "ForgeP2P";
      document.title = cfg.app_name || "ForgeP2P";
    })
    .catch(() => {});

  // If loading doesn't clear within 30s, hide it anyway
  setTimeout(() => {
    if (!loadingEl.classList.contains("hidden")) {
      hideLoading();
    }
  }, 30000);

  connect();
})();
