# paired-exchange

Two-node SwarmNL demo of **authorized data exchange gated by an
out-of-band proof-of-knowledge handshake** — the "Bluetooth pairing"
pattern, built entirely in application code on top of SwarmNL's
[`AppData::SendRpc`](../../.forge/swarm-nl-reference.md#pattern-c-rpc-based-app-request-response)
primitive.

Two roles (A and B) each start a SwarmNL node and a
[`forge-ui`](../../shared/forge-ui/) panel. On connection, each side
independently proves to the other that it knows a shared 32-byte secret
via a challenge/response handshake. Only after that mutual
authentication completes does either side act on application data — a
ping-pong payload that measures round-trip time.

## Run it

```sh
# Generate a shared secret (64 hex chars = 32 bytes).
export SECRET=$(python3 -c "import secrets; print(secrets.token_hex(32))")

# Terminal 1 — role A
cargo run -- --role a

# Terminal 2 — role B (same SECRET)
cargo run -- --role b
```

Role A listens on TCP 53000 / UDP 53001, role B on 53100 / 53101.
forge-ui serves:

- role A panel: http://127.0.0.1:8080
- role B panel: http://127.0.0.1:8081

Open either URL to watch the edge between peers go **pending → trusted**
within a few seconds, followed by a live feed of DataPing/DataPong
round-trips.

Mismatched secrets → `trusted` never lights up; zero pings are ever
sent because the gate refuses to open.

## Wire protocol

Every RPC payload starts with a 1-byte tag. Bodies are fixed-length per
variant. Decoders validate the length against the tag and return an
error (never panic) on any mismatch.

| Tag  | Variant                | Body                                      | Direction        |
| ---- | ---------------------- | ----------------------------------------- | ---------------- |
| 0x01 | `Challenge`            | 16 random nonce bytes                     | initiator → peer |
| 0x02 | `Response`             | 32-byte HMAC-SHA256 of the nonce          | peer → initiator |
| 0x03 | `ResponseAndChallenge` | 32-byte HMAC + 16-byte nonce (unused¹)    | —                |
| 0x04 | `Ack`                  | empty (unused¹)                           | —                |
| 0x10 | `DataPing`             | little-endian `u64` sequence              | initiator → peer |
| 0x11 | `DataPong`             | little-endian `u64` sequence (same as in) | peer → initiator |

¹ `ResponseAndChallenge` and `Ack` are defined but not used by the
current driver — they remain reserved for a future swarm-nl release
that surfaces incoming RPC via the event loop with a `PeerId`. See
[`decisions.md`](./decisions.md) and the paired-exchange entries in
[`library-feedback.md`](../../library-feedback.md) for why the protocol
was simplified from the spec's 4-message flow.

## Pairing state machine

```
         ┌──────────────┐  Challenge sent
         │   Unknown    │────────────────────┐
         └──────────────┘                    │
                                             ▼
                                   ┌────────────────────┐
                                   │ AwaitingResponse   │
                                   │ { nonce, started } │
                                   └────────────────────┘
                                     │            │
                         MAC verified│            │no reply in 5s
                                     ▼            ▼
                        ┌──────────────┐   ┌────────────┐
                        │   Trusted    │   │  Failed    │
                        │ { since }    │   │ { reason } │
                        └──────────────┘   └────────────┘
                                     │            ▲
                         MAC mismatch└────────────┘
```

The background sweeper runs every 1s and moves any `AwaitingResponse`
older than 5s into `Failed { reason: "handshake timeout" }`.

## The gate is three `if`s, not a framework

All application-level authorization boils down to three checks against
the shared [`PairingBook`](src/pairing.rs):

1. **Send-side** ([`datagate.rs`](src/datagate.rs)): a per-peer task
   refuses to send `DataPing` unless `book.is_trusted(&peer)`.
2. **Receive-side** ([`handshake.rs`](src/handshake.rs) `rpc_handler`):
   the RPC handler refuses to answer `DataPing` unless the cached peer
   is trusted. (The cache is the "n=2 assumption" workaround for
   swarm-nl's handler having no `PeerId` — see library-feedback.)
3. **Pong-side** ([`datagate.rs`](src/datagate.rs)): after a `DataPong`
   is received, the RTT is only recorded if the peer is still trusted
   at that moment. Guards against a race where trust is revoked mid-flight.

That is the entire authorization layer. No new `NetworkBehaviour`, no
custom libp2p protocol, no framework.

## Security caveats

This is a demo. Do not copy-paste it into production.

- **HMAC-over-nonce assumes a high-entropy secret.** A 6-digit PIN would
  be offline-guessable by a passive eavesdropper who captures
  `Challenge` and `Response`. For low-entropy secrets, swap in a PAKE
  like SPAKE2 — same message pattern, different primitives, so the wire
  protocol slots line up.
- **No replay protection across sessions** beyond the fresh nonce per
  handshake. Production systems should bind the MAC to a channel
  binding or session id.
- **The shared secret lives in process memory.** No zeroization, no
  secure storage.
- **`Trusted` state never expires.** Production systems should rotate
  keys and revoke trust explicitly.
- **The persistence cache is unauthenticated.** Anyone with write access
  to `<role>_trusted_peers.json` can seed the book. Fine on a local dev
  box; encrypt or sign it on shared systems.

## Files

```
apps/paired-exchange/
├── Cargo.toml
├── README.md                     ← this file
├── bootstrap_config.ini          ← human-readable port layout
├── decisions.md                  ← design rationale
├── forge-state.toml              ← agent state
├── plan.toml                     ← step-by-step build plan
├── src/
│   ├── lib.rs                    ← module re-exports
│   ├── main.rs                   ← CLI + event loop + forge-ui wiring
│   ├── config.rs                 ← --secret parsing (32-byte hex)
│   ├── wire.rs                   ← WireMsg + encode/decode + HMAC helper
│   ├── pairing.rs                ← PairingBook state machine
│   ├── handshake.rs              ← RPC handler + initiate_handshake
│   ├── datagate.rs               ← per-peer ping task + gates
│   └── persistence.rs            ← <role>_trusted_peers.json I/O
├── static/                        ← app panel (index.html / app.js / app.css)
└── tests/
    ├── handshake.rs              ← two-node handshake integration test
    ├── datagate.rs               ← two-node ping-gate integration test
    └── persistence.rs            ← cold/warm-start skip-path test
```

## Run the tests

```sh
cargo test           # 29 unit + 3 integration tests, ~40s total
cargo clippy -- -D warnings
```

Integration tests are non-trivially slow because swarm-nl's
`recv_from_network` polls with a hardcoded 3-second granularity (see
`library-feedback.md`) — every RPC has a 3s floor.
