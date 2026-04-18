// Tiny shared state store. Fetches /api/mission once; tracks current phase,
// all recent pose heartbeats, detected survivors, task winners, and bundles.
// Panels subscribe via on(<channel>, fn).

const listeners = new Map(); // channel -> Set<fn>

function emit(channel, payload) {
  const set = listeners.get(channel);
  if (!set) return;
  for (const fn of set) {
    try {
      fn(payload);
    } catch (err) {
      console.warn("listener failed", channel, err);
    }
  }
}

export function on(channel, fn) {
  if (!listeners.has(channel)) listeners.set(channel, new Set());
  listeners.get(channel).add(fn);
  return () => listeners.get(channel)?.delete(fn);
}

// Mission store. Populated lazily.
export const store = {
  mission: null,
  phase: null,
  phaseStartedAtMs: null,
  robots: new Map(), // robot_id -> {class, pose, battery, status, ts_ms, peer_id?}
  tasks: new Map(), // task_id -> {winner, score, ts_ms, pretty?}
  bundles: new Map(), // robot_id -> [[task_id, score]]
  survivors: new Map(), // survivor_id -> {pose, detected_by, ts_ms, first_seen_ms}
  consensus: new Map(), // robot_id -> [{round, value, ts}]
  linkOverride: "default",
  peers: new Set(),
};

export async function loadMission() {
  try {
    const r = await fetch("/api/mission");
    if (r.ok) {
      store.mission = await r.json();
      emit("mission", store.mission);
    }
  } catch (e) {
    console.warn("failed to load mission", e);
  }
  return store.mission;
}

export function recordPose(d) {
  const now = Date.now();
  const prev = store.robots.get(d.robot_id) || {};
  store.robots.set(d.robot_id, {
    ...prev,
    robot_id: d.robot_id,
    class: d.class,
    pose: d.pose,
    battery: d.battery,
    status: d.status,
    ts_ms: d.ts_ms || now,
    last_local_ms: now,
  });
  emit("robots", store.robots);
}

export function recordTask(d) {
  const prev = store.tasks.get(d.task_id) || { task_id: d.task_id };
  store.tasks.set(d.task_id, {
    ...prev,
    winner: d.winner,
    score: d.bid_score,
    ts_ms: d.ts_ms,
  });
  emit("tasks", store.tasks);
}

export function recordBundle(d) {
  store.bundles.set(d.robot_id, d.bundle || []);
  emit("bundles", store.bundles);
}

export function recordSurvivor(d) {
  const now = Date.now();
  const prev = store.survivors.get(d.survivor_id);
  if (!prev) {
    store.survivors.set(d.survivor_id, { ...d, first_seen_ms: now });
    emit("survivor-new", d);
  }
  emit("survivors", store.survivors);
}

export function recordConsensus(d) {
  const arr = store.consensus.get(d.robot_id) || [];
  arr.push({ round: d.round, value: d.value, ts: Date.now() });
  if (arr.length > 200) arr.shift();
  store.consensus.set(d.robot_id, arr);
  emit("consensus", store.consensus);
}

export function recordPhase(d) {
  store.phase = d;
  store.phaseStartedAtMs = d.started_ms || Date.now();
  emit("phase", d);
}

export function recordLinkOverride(d) {
  store.linkOverride = d.profile || "default";
  emit("link", store.linkOverride);
}

export function markPeerConnected(id) {
  store.peers.add(id);
  emit("peers", store.peers);
}
export function markPeerDisconnected(id) {
  store.peers.delete(id);
  emit("peers", store.peers);
}

// Aggregate helpers used by the Mission panel + banner.
export function aliveRobots(staleMs = 6000) {
  const now = Date.now();
  const alive = [];
  for (const r of store.robots.values()) {
    if (now - (r.last_local_ms || 0) < staleMs) alive.push(r);
  }
  return alive;
}

export function aliveByClass() {
  const out = {};
  for (const r of aliveRobots()) {
    out[r.class] = (out[r.class] || 0) + 1;
  }
  return out;
}

export function spawnedByClass() {
  const out = {};
  for (const r of store.robots.values()) {
    out[r.class] = (out[r.class] || 0) + 1;
  }
  return out;
}

export function avgBattery() {
  const alive = aliveRobots();
  if (!alive.length) return null;
  return alive.reduce((s, r) => s + (r.battery || 0), 0) / alive.length;
}

export function resolvedTasks() {
  let resolved = 0;
  for (const t of store.tasks.values()) if (t.winner) resolved += 1;
  return resolved;
}

export function consensusVariance() {
  const latest = [];
  for (const arr of store.consensus.values()) {
    if (arr.length) latest.push(arr[arr.length - 1].value);
  }
  if (latest.length < 2) return 0;
  const mean = latest.reduce((s, v) => s + v, 0) / latest.length;
  const variance =
    latest.reduce((s, v) => s + (v - mean) ** 2, 0) / latest.length;
  return Math.sqrt(variance);
}
