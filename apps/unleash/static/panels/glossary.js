// Popover shown when the header "?" button is clicked. One-line definitions
// for every acronym or jargon term that appears in the dashboard.

const TERMS = [
  {
    term: "SwarmNL",
    def: "The Rust peer-to-peer networking library this app demonstrates. Handles peer discovery, gossip, request-response RPC, data replication, and sharding.",
  },
  {
    term: "CBBA",
    def: "Consensus-Based Bundle Algorithm (Choi/Brunet/How, 2009). Each robot independently scores every open task, bids for the best ones, and wins the one its bid leads. No central allocator.",
  },
  {
    term: "W-MSR",
    def: "Weighted Mean-Subsequence Reduced consensus. Each round, every robot drops the f highest and f lowest neighbour values and averages the rest — honest estimates converge, adversarial outliers get dropped.",
  },
  {
    term: "Byzantine",
    def: "A robot that lies: inflates survivor counts, falsifies its own pose, or injects fake bids. Phase 4 flips one robot adversarial; W-MSR should reject it within 5 rounds.",
  },
  {
    term: "Stigmergy",
    def: "Lamport-clock-versioned key/value store gossiped among robots; higher clock wins, ties by robot_id. Used for \"the fleet remembers\" state (survivor presence, task status).",
  },
  {
    term: "Gossipsub",
    def: "Pub/sub messaging over a libp2p mesh. Each message is relayed to a fan-out of peers per topic; delivery is probabilistic but tends toward reliable once the mesh forms.",
  },
  {
    term: "Kademlia DHT",
    def: "Distributed hash table for peer discovery and small-value lookups. Robots advertise their capability vector under `robot/<id>/capability`.",
  },
  {
    term: "Replication network",
    def: "A named group of peers where data written by any node is mirrored to all. Unleash replicates `SurvivorReport`s in the `unleash_survivors` network.",
  },
  {
    term: "Rendezvous",
    def: "When two robots come within 5 m of each other, they gossip their occupancy grids and merge cells (higher Lamport clock wins). This is the MVP substitute for Swarm-SLAM.",
  },
  {
    term: "Link profile",
    def: "The simulated network condition between two robots. `default` is clean; `degraded` injects 80 ms latency and 40 % packet loss; `blackout` drops everything.",
  },
  {
    term: "Breadcrumb",
    def: "A static mesh relay node dropped by a ground workhorse at a connectivity bottleneck. No sensors, no motion — just extends the gossip network geographically.",
  },
];

export function initGlossary(root) {
  root.classList.add("glossary");
  root.innerHTML = `
    <header>
      <h3>Glossary</h3>
      <button id="gloss-close" aria-label="Close">×</button>
    </header>
    <ul>
      ${TERMS.map((t) => `<li><b>${t.term}</b> — ${t.def}</li>`).join("")}
    </ul>
  `;
  root.style.display = "none";
  root.querySelector("#gloss-close").addEventListener("click", () => {
    root.style.display = "none";
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") root.style.display = "none";
  });
  return {
    toggle() {
      root.style.display = root.style.display === "none" ? "" : "none";
    },
    close() {
      root.style.display = "none";
    },
  };
}
