// mesh-chat panel — renders Custom{label:"CHAT"} events from forge-ui's /ws
// feed and (step 5) POSTs user input to /api/chat/send.
//
// The iframe is served from /app/* on the same origin as forge-ui, so
// relative paths and window.location.host both target the correct server.

const messagesEl = document.getElementById("messages");
const statusEl = document.getElementById("chat-status");
const inputEl = document.getElementById("input");
const sendEl = document.getElementById("send");
const formEl = document.getElementById("compose");
const titleEl = document.getElementById("chat-title");

let myName = null;

function setStatus(text, cls) {
  statusEl.textContent = text;
  statusEl.className = "status" + (cls ? " " + cls : "");
}

function appendMessage(from, text) {
  const div = document.createElement("div");
  const mine = myName && from === myName;
  div.className = "msg " + (mine ? "mine" : "theirs");
  const who = document.createElement("div");
  who.className = "who";
  who.textContent = from + (mine ? " (you)" : "");
  const body = document.createElement("div");
  body.textContent = text;
  div.appendChild(who);
  div.appendChild(body);
  messagesEl.appendChild(div);
  messagesEl.scrollTop = messagesEl.scrollHeight;
}

// Pull the app name from /config so we can identify "my" messages vs. theirs.
fetch("/config")
  .then((r) => r.json())
  .then((cfg) => {
    titleEl.textContent = cfg.app_name || "mesh-chat";
    // app_name format: "mesh-chat :: Al"
    const m = /::\s*(.+)$/.exec(cfg.app_name || "");
    if (m) myName = m[1].trim();
  })
  .catch(() => {});

function connect() {
  const url = `ws://${location.host}/ws`;
  const ws = new WebSocket(url);

  ws.addEventListener("open", () => {
    setStatus("connected", "ok");
    inputEl.disabled = false;
    sendEl.disabled = false;
    inputEl.focus();
  });

  ws.addEventListener("close", () => {
    setStatus("disconnected — retrying…", "err");
    inputEl.disabled = true;
    sendEl.disabled = true;
    setTimeout(connect, 1000);
  });

  ws.addEventListener("error", () => {
    setStatus("error", "err");
  });

  ws.addEventListener("message", (ev) => {
    let event;
    try {
      event = JSON.parse(ev.data);
    } catch {
      return;
    }
    if (event.type === "Custom" && event.label === "CHAT") {
      // detail format: "<from>: <text>"
      const idx = event.detail.indexOf(":");
      if (idx < 0) return;
      const from = event.detail.slice(0, idx).trim();
      const text = event.detail.slice(idx + 1).trim();
      appendMessage(from, text);
    }
  });
}

formEl.addEventListener("submit", async (ev) => {
  ev.preventDefault();
  const text = inputEl.value.trim();
  if (!text) return;
  inputEl.value = "";
  try {
    const res = await fetch("/api/chat/send", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ text }),
    });
    if (!res.ok && res.status !== 202) {
      setStatus(`send failed (${res.status})`, "err");
    }
  } catch (e) {
    setStatus("send failed: " + e.message, "err");
  }
});

connect();
