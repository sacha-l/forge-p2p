// ForgeP2P Peers tab — renders the discovered + connected lists, manual dial
// form, and discovery toggles. Subscribes to the shared event bus exposed by
// mesh.js (`window.forgeUI.onEvent`).

(function () {
  "use strict";

  const shortId = (window.forgeUI && window.forgeUI.shortId) || (s => s);

  // State mirroring forge-ui's server caches. Kept in sync via events + a
  // periodic reconcile fetch so a late-opened panel shows current state.
  const discovered = new Map(); // peer_id -> { peer_id, addr, source }
  const connected = new Set();  // peer_id
  let selfPeerId = null;

  // --- DOM ---
  const connectedListEl = document.getElementById("connected-list");
  const connectedCountEl = document.getElementById("connected-count");
  const discoveredListEl = document.getElementById("discovered-list");
  const discoveredCountEl = document.getElementById("discovered-count");
  const manualFormEl = document.getElementById("manual-dial-form");
  const manualPeerIdEl = document.getElementById("manual-peer-id");
  const manualAddrEl = document.getElementById("manual-addr");
  const manualBtnEl = document.getElementById("manual-dial-btn");
  const manualStatusEl = document.getElementById("manual-dial-status");
  const autoConnectToggleEl = document.getElementById("toggle-auto-connect");
  const mdnsToggleEl = document.getElementById("toggle-mdns");

  function escapeHtml(s) {
    return String(s)
      .replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  }

  function renderConnected() {
    if (!connectedListEl) return;
    connectedCountEl.textContent = String(connected.size);
    if (connected.size === 0) {
      connectedListEl.className = "peer-list empty";
      connectedListEl.textContent = "No peers connected";
      return;
    }
    connectedListEl.className = "peer-list";
    connectedListEl.innerHTML = "";
    for (const pid of connected) {
      const row = document.createElement("div");
      row.className = "peer-row";
      row.innerHTML = `
        <div class="info">
          <span class="pid" title="${escapeHtml(pid)}">${escapeHtml(shortId(pid))}</span>
        </div>`;
      connectedListEl.appendChild(row);
    }
  }

  function renderDiscovered() {
    if (!discoveredListEl) return;
    discoveredCountEl.textContent = String(discovered.size);
    if (discovered.size === 0) {
      discoveredListEl.className = "peer-list empty";
      discoveredListEl.textContent = "No peers discovered yet";
      return;
    }
    discoveredListEl.className = "peer-list";
    discoveredListEl.innerHTML = "";
    for (const peer of discovered.values()) {
      const row = document.createElement("div");
      row.className = "peer-row";
      const isConnected = connected.has(peer.peer_id);
      row.innerHTML = `
        <div class="info">
          <span class="pid" title="${escapeHtml(peer.peer_id)}">
            <span class="source-badge ${escapeHtml(peer.source)}">${escapeHtml(peer.source)}</span>${escapeHtml(shortId(peer.peer_id))}
          </span>
          <span class="addr" title="${escapeHtml(peer.addr)}">${escapeHtml(peer.addr)}</span>
        </div>
      `;
      const btn = document.createElement("button");
      btn.type = "button";
      btn.textContent = isConnected ? "Connected" : "Connect";
      btn.disabled = isConnected;
      btn.addEventListener("click", () => dial(peer.peer_id, peer.addr, btn));
      row.appendChild(btn);
      discoveredListEl.appendChild(row);
    }
  }

  async function dial(peerId, addr, btn) {
    if (btn) { btn.disabled = true; btn.textContent = "Dialing…"; }
    try {
      const res = await fetch("/api/peer/dial", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ peer_id: peerId, addr }),
      });
      if (res.status === 202) {
        if (btn) btn.textContent = "Dispatched";
      } else {
        const body = await res.text();
        if (btn) { btn.disabled = false; btn.textContent = "Retry"; }
        console.warn("[peers] dial error", res.status, body);
      }
    } catch (e) {
      if (btn) { btn.disabled = false; btn.textContent = "Retry"; }
      console.warn("[peers] dial error:", e);
    }
  }

  // --- Manual dial form ---
  if (manualFormEl) {
    manualFormEl.addEventListener("submit", async (ev) => {
      ev.preventDefault();
      const peer_id = manualPeerIdEl.value.trim();
      const addr = manualAddrEl.value.trim();
      if (!peer_id || !addr) return;
      manualBtnEl.disabled = true;
      manualStatusEl.className = "status-line";
      manualStatusEl.textContent = "dialing…";
      try {
        const res = await fetch("/api/peer/dial", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ peer_id, addr }),
        });
        if (res.status === 202) {
          manualStatusEl.className = "status-line ok";
          manualStatusEl.textContent = "dispatched";
        } else {
          manualStatusEl.className = "status-line err";
          manualStatusEl.textContent = `error ${res.status}: ${await res.text()}`;
        }
      } catch (e) {
        manualStatusEl.className = "status-line err";
        manualStatusEl.textContent = "error: " + e.message;
      } finally {
        manualBtnEl.disabled = false;
      }
    });
  }

  // --- mDNS toggle ---
  if (mdnsToggleEl) {
    mdnsToggleEl.addEventListener("change", async () => {
      try {
        await fetch("/api/discovery/mdns", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ enabled: mdnsToggleEl.checked }),
        });
      } catch (e) {
        console.warn("[peers] mdns toggle error:", e);
      }
    });
  }

  // --- Event bus subscription ---
  function onEvent(e) {
    switch (e.type) {
      case "NodeStarted":
        selfPeerId = e.peer_id;
        break;
      case "PeerConnected":
        connected.add(e.peer_id);
        renderConnected();
        renderDiscovered(); // update "Connect" → "Connected" badge
        break;
      case "PeerDisconnected":
        connected.delete(e.peer_id);
        renderConnected();
        renderDiscovered();
        break;
      case "PeerDiscovered":
        if (e.peer_id === selfPeerId) break;
        discovered.set(e.peer_id, e);
        renderDiscovered();
        break;
      case "PeerLost":
        discovered.delete(e.peer_id);
        renderDiscovered();
        break;
    }
  }
  (window.forgeUI && window.forgeUI.onEvent || []).push(onEvent);

  // --- Initial reconcile from REST (in case we open after events fired) ---
  fetch("/api/node/info")
    .then(r => r.ok ? r.json() : null)
    .then(info => { if (info) selfPeerId = info.peer_id; })
    .catch(() => {});
  fetch("/api/peers/discovered")
    .then(r => r.json())
    .then(body => {
      for (const p of body.peers || []) discovered.set(p.peer_id, p);
      renderDiscovered();
    })
    .catch(() => {});
  renderConnected();
  renderDiscovered();
})();
