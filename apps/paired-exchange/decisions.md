# paired-exchange — Design Decisions

## Architecture — chose application-level gate over custom NetworkBehaviour

- **Context**: Needed to refuse SwarmNL application data until peers mutually prove knowledge of a pre-shared secret ("Bluetooth pairing" model).
- **Options**:
  1. Write a custom libp2p `NetworkBehaviour` that chains with ping/identify.
  2. Sit entirely on top of SwarmNL's `AppData::SendRpc` and keep authorization in application state.
- **Decision**: Option 2. The handshake is a per-peer state machine (`Unknown → AwaitingResponse → Trusted | Failed`); SwarmNL's `next_event().await` loop is the natural driver. `SendRpc` accepts opaque `Vec<Vec<u8>>`, so a 1-byte tag multiplexes pairing and data messages on the same channel — no new protocol registration needed.
- **Trade-off**: The "gate" is three `if book.is_trusted(&peer)` checks around the data plane, not a type-system guarantee. A malicious `SwarmNL`-level message will still reach the event loop; we just refuse to act on it. Acceptable for the demo and for most application-level authorization problems. A transport-level refusal would need the per-peer delivery filter hook noted in `library-feedback.md` §7 of the plan.

## Crypto — HMAC-over-nonce, not PAKE

- **Context**: Mutual proof-of-knowledge of a shared secret `S`.
- **Decision**: HMAC-SHA256 over a fresh 16-byte nonce per direction. Simple, stdlib-friendly (via `hmac` + `sha2`), and correct when `S` is high-entropy (≥128 bits).
- **Trade-off**: Offline-guessable for low-entropy secrets (6-digit PINs etc.). README will call this out and list SPAKE2 as future work — same message pattern, different primitives, so the wire protocol slot is forward-compatible.

## Wire framing — 1-byte tag + body, no length prefix

- **Context**: Multiplex pairing and data traffic on one RPC channel.
- **Decision**: First byte is a variant tag (0x01..0x04 pairing, 0x10..0x11 data). Body length is implicit per variant (nonce=16, mac=32, seq=8, Ack=0). `decode` validates the length matches the tag and returns `Err` on any mismatch — never panics.
- **Trade-off**: Not self-describing; adding a new variant requires both sides to update. Acceptable for a two-node demo where both sides ship together.

## Port allocation

- Per `CLAUDE.md`: `50000 + (app_index * 1000) + (node_index * 100)`.
- `paired-exchange` is the 4th app (echo-gossip=0, mesh-chat=1, sovereign-notes=2, paired-exchange=3), so:
  - Role A: TCP 53000, UDP 53001
  - Role B: TCP 53100, UDP 53101
  - Integration tests: 49000 + test_index \* 100 per the reference doc pattern.

## Known SwarmNL friction applied from `library-feedback.md`

Reading the shared knowledge base up front so we don't rediscover these:

- `use swarm_nl::*;` — `core::prelude` is private in v0.2.1.
- `let mut node` — `next_event()` takes `&mut self`.
- `tokio::time::sleep(Duration::from_millis(100))` at the end of the event loop — `next_event()` is non-blocking.
- Build bootnode addresses as `/ip4/127.0.0.1/tcp/<port>` manually; don't trust `NewListenAddr`.
- `BootstrapConfig::with_bootnodes` wants `HashMap<String, String>`, not `HashMap<PeerId, String>`.
- The incoming-RPC event is `RpcIncomingMessageHandled { data }` and carries no sender `PeerId` — we'll have to encode the sender inside the wire payload, or rely on `ConnectionEstablished` to register the single expected peer. For a two-node demo we can cache "the one other peer" and treat every incoming RPC as coming from them; we'll note this explicitly in the step-5 implementation and in `library-feedback.md` if it becomes awkward.

## Step 5 — protocol simplified from 4-message to 1-round-trip per direction

- **Context**: The plan called for a 4-message handshake (`Challenge → ResponseAndChallenge → Response → Ack`) driven from the event loop on incoming RPC events.
- **Discovery**: `NetworkEvent::RpcIncomingMessageHandled` is declared but *never emitted* by the library (grep'd `swarm-nl-0.2.1/src`). The only incoming-RPC hook is `CoreBuilder::with_rpc(config, handler: fn(RpcData) -> RpcData)` — a sync bare-fn pointer with no `PeerId` and no app-state capture.
- **Decision**: Collapse to two independent 1-round-trip challenges. Each side, on `ConnectionEstablished`, generates a nonce, sends `AppData::SendRpc(Challenge(nonce))`, verifies the returned `Response(mac)`, and marks the peer `Trusted` locally on success. The peer's handler computes `HMAC(S, nonce)` and returns `Response(mac)` as the RPC response. Running both directions in parallel still yields mutual authentication.
- **Trade-off**: Lose the combined `ResponseAndChallenge + Ack` round; two handshakes instead of one compressed one. Wire variants `ResponseAndChallenge` and `Ack` remain defined in `wire.rs` (forward-compat for a future SwarmNL that exposes peer identity on incoming RPC) but are not used by the current driver.
- **Trade-off (gate 2 of 3)**: The handler-side `is_trusted` check on incoming `DataPing` falls back to a single-peer assumption (cached from `ConnectionEstablished` via a `static OnceLock<Arc<HandlerCtx>>`). This is clean for n=2 but does not generalise; logged in `library-feedback.md`.

## Step 5 — `PairingBook` uses `std::sync::Mutex`, not `tokio::sync::Mutex`

- **Context**: The RPC handler is a sync `fn(RpcData) -> RpcData` and cannot `.await`. It still needs to check `is_trusted`.
- **Decision**: Back `PairingBook` with `std::sync::Mutex<HashMap<PeerId, PairState>>`. Critical sections are short (single-map-op) and never cross an await point, so this is the pattern tokio itself recommends for such cases. Lets the sync handler and all async tasks share one book without `try_lock`-style ugliness.
