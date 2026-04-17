// Tasks panel: one card per task announced, showing CBBA state.

export function initTasks(root) {
  const tasks = new Map(); // task_id -> {winner, score, ts_ms, bundles}

  function render() {
    root.innerHTML = "";
    if (tasks.size === 0) {
      const e = document.createElement("div");
      e.className = "empty";
      e.textContent = "No tasks announced yet.";
      root.appendChild(e);
      return;
    }
    const sorted = [...tasks.values()].sort((a, b) =>
      a.task_id.localeCompare(b.task_id)
    );
    sorted.forEach((t) => {
      const card = document.createElement("div");
      card.className = "task-card";
      const status = t.winner ? "assigned" : "bidding";
      card.innerHTML = `
        <div class="task-header">
          <span class="task-id">${escape(t.task_id)}</span>
          <span class="task-status ${status}">${status}</span>
        </div>
        <div class="task-body">
          <div><em>winner</em> <b>${escape(t.winner || "—")}</b></div>
          <div><em>score</em> <b>${t.score ? t.score.toFixed(2) : "—"}</b></div>
          <div><em>updated</em> <b>${t.ts_ms ? new Date(t.ts_ms).toLocaleTimeString() : "—"}</b></div>
        </div>
      `;
      root.appendChild(card);
    });
  }

  return {
    onWinner: (d) => {
      const t = tasks.get(d.task_id) || { task_id: d.task_id };
      t.winner = d.winner;
      t.score = d.bid_score;
      t.ts_ms = d.ts_ms;
      tasks.set(d.task_id, t);
      render();
    },
    onBundle: (d) => {
      (d.bundle || []).forEach(([tid]) => {
        const t = tasks.get(tid) || { task_id: tid };
        tasks.set(tid, t);
      });
      render();
    },
  };
}

function escape(s) {
  return String(s).replace(/[<>&]/g, (c) => ({ "<": "&lt;", ">": "&gt;", "&": "&amp;" })[c]);
}
