# mesh-chat — design decisions

## Step 1 — Two hardcoded peer names (`Al`, `Bobby`) selected via `--peer` CLI
- **Context**: Demo needs exactly two named peers. Randomized or runtime-configurable names add scope without helping the demo.
- **Options**: (a) Two named enum variants with fixed ports; (b) free-form `--name <str>` + `--port <u16>`; (c) a YAML manifest.
- **Decision**: (a). One `cargo run -- --peer al` command per peer; Bobby takes Al's printed `PeerId` as `--bootnode-peer-id`.
- **Trade-off**: Not reusable beyond two peers, but the task spec explicitly names Al and Bobby.

## Step 1 — Static app directory resolved via `CARGO_MANIFEST_DIR`
- **Context**: `ForgeUI::with_app_static_dir` resolves paths relative to the binary's CWD.
- **Decision**: Compute `concat!(env!("CARGO_MANIFEST_DIR"), "/static")` at compile time so the panel works regardless of where the binary is launched from.
- **Trade-off**: Absolute path baked into the binary. Fine for dev; we don't ship this binary.

## Step 1 — `with_bootnodes` actually takes `HashMap<String, String>`
- **Context**: Reference doc and prior `HashMap<PeerId, String>` pattern didn't compile.
- **Resolution**: Pass the peer id as a string. Logged in `library-feedback.md` (2026-04-13 entry).

## Step 3 — Accept one-way gossip delivery for the demo
- **Context**: Gossipsub mesh forms asymmetrically when a peer joins a topic before the first connection. Bobby reliably receives Al's broadcasts; the reverse is flaky.
- **Decision**: Ship the demo with the library's current behaviour — users will primarily see Al → Bobby. Logged as library-feedback. Not worth blocking the demo on a library-level mesh fix.
- **Trade-off**: Two open Bobby panels will not always show each other's messages. Both will always show Al's.

## Step 8 — Adopt forge-ui 0.2; delete app-side peer plumbing
- **Context**: forge-ui grew built-in peering (node card, peers tab, localhost scan, mDNS, /api/peer/dial). The app-side dial form and route were now duplicates.
- **Decision**: Replace the app's `Command::Dial` path with a `mpsc::Sender<forge_ui::DialRequest>` wired via `ForgeUI::with_dial_sender`. Keep the app's own `POST /api/chat/send` (that's app-specific). Drop the iframe's dial form + CSS + JS.
- **Trade-off**: The app now depends on forge-ui's auto-discovery for one-command bring-up. The CLI bootnode args still work as a scripted fallback.
