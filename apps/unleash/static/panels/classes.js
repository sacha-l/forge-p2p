// Single source of truth for robot classes — used by every panel that
// wants a colour, an icon, or a human-readable description.
//
// Colour values duplicated in mesh.js for back-compat; when you change
// one, change both.

export const CLASSES = {
  aerial_scout: {
    label: "Aerial Scout",
    short: "scout",
    acronym: "as",
    color: "#6c9eff",
    icon: "▲",
    role: "Quadrotor. Surveys the exterior and through large openings; acts as a comms relay when stationary.",
    capabilities: ["survey", "relay", "aerial"],
  },
  aerial_mapper: {
    label: "Aerial Mapper",
    short: "mapper",
    acronym: "am",
    color: "#8df0c5",
    icon: "◆",
    role: "Heavy quadrotor with 3D LiDAR. High-altitude overwatch and dense mapping.",
    capabilities: ["survey", "relay", "aerial"],
  },
  ground_scout: {
    label: "Ground Scout",
    short: "scout",
    acronym: "gs",
    color: "#ffa657",
    icon: "●",
    role: "Small tracked UGV. Enters interior voids, contacts victims, senses gas (can traverse gas zones).",
    capabilities: ["inspect_narrow", "gas_traverse", "victim_contact", "ground"],
  },
  ground_workhorse: {
    label: "Ground Workhorse",
    short: "workhorse",
    acronym: "gw",
    color: "#d78cff",
    icon: "■",
    role: "Legged / large tracked. Carries and deploys breadcrumb mesh nodes; traverses rubble.",
    capabilities: ["payload", "deploy_node", "relay", "ground"],
  },
  breadcrumb: {
    label: "Breadcrumb Relay",
    short: "relay",
    acronym: "bc",
    color: "#8d96a8",
    icon: "◇",
    role: "Static mesh node dropped by workhorses at connectivity bottlenecks. No sensors, no motion.",
    capabilities: ["relay"],
  },
};

export function classMeta(key) {
  return CLASSES[key] || {
    label: key,
    short: key,
    acronym: key,
    color: "#cccccc",
    icon: "○",
    role: "Unknown class.",
    capabilities: [],
  };
}

export const LINK_PROFILES = {
  default: {
    label: "Default",
    color: "#3e8a5a",
    description: "100 Mbps / 5 ms / 0 % loss — clean line-of-sight.",
  },
  degraded: {
    label: "Degraded",
    color: "#c19a3c",
    description: "2 Mbps / 80 ms / 40 % loss — through rubble or multipath.",
  },
  blackout: {
    label: "Blackout",
    color: "#c13c3c",
    description: "No link — comms-denied zone.",
  },
};

// Pretty-print a robot id like `r3_am` → `r3 ‧ mapper`.
export function prettyRobotId(raw) {
  const m = /^(r\d+)_([a-z]+)$/.exec(raw || "");
  if (!m) return raw || "";
  const [, stem, acr] = m;
  const meta = Object.values(CLASSES).find((c) => c.acronym === acr);
  return meta ? `${stem} ‧ ${meta.short}` : raw;
}

export function classFromId(raw) {
  const m = /^r\d+_([a-z]+)$/.exec(raw || "");
  if (!m) return null;
  const acr = m[1];
  const entry = Object.entries(CLASSES).find(([, c]) => c.acronym === acr);
  return entry ? entry[0] : null;
}
