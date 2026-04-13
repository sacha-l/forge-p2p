// Sovereign Notes — App Panel
// Talks to the sovereign-notes API routes served alongside forge-ui.

(function () {
  "use strict";

  // If loaded standalone (not in the forge-ui iframe), redirect to the full shell
  // so the user sees the mesh visualizer and event log too.
  if (window.self === window.top) {
    window.location.replace("/");
    return;
  }

  const API = "";  // same origin
  let selectedNoteId = null;
  let peerCount = 0;

  // --- API helpers ---

  async function api(path, opts) {
    const res = await fetch(API + path, opts);
    if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
    return res.json();
  }

  async function loadNotes() {
    try {
      const notes = await api("/api/notes");
      renderNoteList(notes);
      updateStatus(notes.length);
    } catch (e) {
      document.getElementById("note-list").innerHTML =
        '<div class="empty-state">Failed to load notes</div>';
    }
  }

  async function createNote(title) {
    const note = await api("/api/notes", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ title }),
    });
    await loadNotes();
    selectNote(note.id);
    return note;
  }

  async function loadNote(id) {
    return api("/api/notes/" + id);
  }

  async function saveNote(id, content) {
    return api("/api/notes/" + id, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ content }),
    });
  }

  // --- Rendering ---

  function renderNoteList(notes) {
    const el = document.getElementById("note-list");
    if (notes.length === 0) {
      el.innerHTML = '<div class="empty-state">No notes yet. Click "+ New Note" to create one.</div>';
      return;
    }
    el.innerHTML = notes.map(n => {
      const sel = n.id === selectedNoteId ? " selected" : "";
      const date = new Date(n.updated_at).toLocaleString();
      return `<div class="note-item${sel}" data-id="${n.id}">
        <span class="title">${esc(n.title)}</span>
        <span class="meta">v${n.version} &middot; ${date}</span>
      </div>`;
    }).join("");

    el.querySelectorAll(".note-item").forEach(item => {
      item.addEventListener("click", () => selectNote(item.dataset.id));
    });
  }

  async function selectNote(id) {
    selectedNoteId = id;
    const note = await loadNote(id);
    document.getElementById("editor-section").classList.add("visible");
    document.getElementById("note-title-display").textContent = note.title;
    document.getElementById("note-editor").value = note.content;
    document.getElementById("note-version").textContent = `v${note.version}`;

    // Update selection highlight
    document.querySelectorAll(".note-item").forEach(el => {
      el.classList.toggle("selected", el.dataset.id === id);
    });
  }

  function updateStatus(noteCount) {
    const bar = document.getElementById("status-bar");
    bar.textContent = `${noteCount} note(s) | ${peerCount} peer(s) connected`;
  }

  function esc(s) {
    const d = document.createElement("div");
    d.textContent = s;
    return d.innerHTML;
  }

  // --- WebSocket for live updates ---

  function connectWs() {
    const proto = location.protocol === "https:" ? "wss:" : "ws:";
    const ws = new WebSocket(`${proto}//${location.host}/ws`);
    const dot = document.getElementById("sync-dot");
    const txt = document.getElementById("sync-text");

    ws.onopen = () => {
      dot.className = "dot online";
      txt.textContent = "connected";
    };

    ws.onclose = () => {
      dot.className = "dot offline";
      txt.textContent = "reconnecting...";
      setTimeout(connectWs, 2000);
    };

    ws.onmessage = (evt) => {
      let event;
      try { event = JSON.parse(evt.data); } catch { return; }

      if (event.type === "PeerConnected") {
        peerCount++;
        updateStatus(document.querySelectorAll(".note-item").length);
      } else if (event.type === "PeerDisconnected") {
        peerCount = Math.max(0, peerCount - 1);
        updateStatus(document.querySelectorAll(".note-item").length);
      } else if (event.type === "Custom" && event.label === "NOTE_SYNCED") {
        // Refresh note list when a sync happens
        loadNotes();
      }
    };
  }

  // --- Event handlers ---

  document.getElementById("btn-new").addEventListener("click", () => {
    const dialog = document.getElementById("new-note-dialog");
    dialog.classList.toggle("visible");
    if (dialog.classList.contains("visible")) {
      document.getElementById("new-title").focus();
    }
  });

  document.getElementById("btn-create").addEventListener("click", async () => {
    const input = document.getElementById("new-title");
    const title = input.value.trim();
    if (!title) return;
    await createNote(title);
    input.value = "";
    document.getElementById("new-note-dialog").classList.remove("visible");
  });

  document.getElementById("new-title").addEventListener("keydown", (e) => {
    if (e.key === "Enter") document.getElementById("btn-create").click();
  });

  document.getElementById("btn-refresh").addEventListener("click", loadNotes);

  document.getElementById("btn-spawn-peer").addEventListener("click", async () => {
    const btn = document.getElementById("btn-spawn-peer");
    btn.disabled = true;
    btn.textContent = "Spawning...";
    try {
      const res = await fetch("/api/spawn-peer", { method: "POST" });
      const info = await res.json();
      const peersEl = document.getElementById("spawned-peers");
      if (info.error) {
        peersEl.innerHTML += `<div class="spawned-peer error">Spawn failed: ${esc(info.error)}</div>`;
      } else {
        peersEl.innerHTML += `<div class="spawned-peer">
          Peer #${peersEl.children.length + 1}: pid ${info.pid} &middot;
          tcp ${info.tcp_port} &middot;
          <a href="${info.ui_url}" target="_blank">${info.ui_url}</a>
        </div>`;
      }
    } catch (e) {
      console.error("Spawn peer failed:", e);
    } finally {
      btn.disabled = false;
      btn.textContent = "+ Spawn Peer";
    }
  });

  document.getElementById("btn-save").addEventListener("click", async () => {
    if (!selectedNoteId) return;
    const content = document.getElementById("note-editor").value;
    const updated = await saveNote(selectedNoteId, content);
    document.getElementById("note-version").textContent = `v${updated.version}`;
    await loadNotes();
  });

  // --- Init ---
  loadNotes();
  connectWs();
})();
