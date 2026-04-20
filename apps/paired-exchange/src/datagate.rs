//! Application data plane. A per-peer task sends `DataPing(seq)` every
//! [`PING_INTERVAL`] so long as the peer stays `Trusted`; each pong comes
//! back as the RPC response, so we compute and report round-trip time
//! without reaching for a second primitive.
//!
//! The three authorization gates from the plan, annotated for SwarmNL's
//! actual API surface:
//!
//! 1. **Send-side**: `book.is_trusted(&peer)` is checked before every
//!    `SendRpc(DataPing)`. Enforced in `ping_loop` below.
//! 2. **Receive-side (handler)**: the static `rpc_handler` in
//!    `handshake.rs` consults the same book via the cached "single
//!    peer" — the only workaround available because SwarmNL's handler
//!    has no `PeerId`. See `library-feedback.md`.
//! 3. **Pong-side**: decoded response is acted on only if we still
//!    trust the peer at the moment the pong arrives. Guards against a
//!    race where trust was revoked between Ping send and Pong receipt.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use forge_ui::{MeshEvent, UiHandle};
use swarm_nl::core::{AppData, AppResponse, Core};
use swarm_nl::PeerId;

use crate::pairing::PairingBook;
use crate::wire::WireMsg;

/// How often a trusted peer is pinged.
pub const PING_INTERVAL: Duration = Duration::from_secs(2);
/// Ceiling on one ping's round-trip. The observed real minimum under
/// swarm-nl 0.2.1 is ~3s due to the hardcoded `TASK_SLEEP_DURATION`
/// polling cycle in `Core::recv_from_network` (see `library-feedback.md`);
/// 10s lets transient jitter through without retiring the task.
pub const PING_TIMEOUT: Duration = Duration::from_secs(10);

/// Shared log of per-peer round-trip times. Tests assert against this;
/// production can surface it on the UI panel. Entry: `(peer, seq, rtt)`.
pub type RttLog = Arc<Mutex<Vec<(PeerId, u64, Duration)>>>;

pub fn new_rtt_log() -> RttLog {
    Arc::new(Mutex::new(Vec::new()))
}

/// Spawn a background task that pings `peer` every [`PING_INTERVAL`] for as
/// long as the peer is trusted. Exits cleanly once `is_trusted` flips to
/// false (connection closed or trust revoked).
pub fn spawn_ping_task(
    mut core: Core,
    book: PairingBook,
    peer: PeerId,
    ui: UiHandle,
    rtt_log: RttLog,
) {
    tokio::spawn(async move {
        ping_loop(&mut core, &book, peer, &ui, &rtt_log).await;
    });
}

async fn ping_loop(
    core: &mut Core,
    book: &PairingBook,
    peer: PeerId,
    ui: &UiHandle,
    rtt_log: &RttLog,
) {
    let mut seq: u64 = 0;
    let mut ticker = tokio::time::interval(PING_INTERVAL);
    ticker.tick().await; // discard the immediate first tick

    loop {
        ticker.tick().await;

        // Gate #1: refuse to send unless we currently trust the peer.
        if !book.is_trusted(&peer) {
            tracing::debug!(peer = %peer, "ping_loop: peer no longer trusted — exiting");
            return;
        }

        seq += 1;
        let sent_at = Instant::now();
        let req = AppData::SendRpc {
            keys: vec![WireMsg::DataPing(seq).encode()],
            peer,
        };

        ui.push(MeshEvent::MessageSent {
            to: peer.to_string(),
            topic: "ping".to_string(),
            size_bytes: 9, // tag + u64
        })
        .await;

        let result = tokio::time::timeout(PING_TIMEOUT, core.query_network(req)).await;
        let frames = match result {
            Ok(Ok(AppResponse::SendRpc(frames))) => frames,
            Ok(Ok(other)) => {
                tracing::warn!(peer = %peer, ?other, "ping: unexpected AppResponse");
                continue;
            }
            Ok(Err(e)) => {
                tracing::warn!(peer = %peer, error = ?e, "ping: SendRpc failed");
                continue;
            }
            Err(_) => {
                tracing::warn!(peer = %peer, "ping: timed out");
                continue;
            }
        };

        let frame = match frames.first() {
            Some(f) if !f.is_empty() => f,
            _ => {
                tracing::debug!(peer = %peer, "ping: empty response (peer gate closed)");
                continue;
            }
        };

        let ack_seq = match WireMsg::decode(frame) {
            Ok(WireMsg::DataPong(n)) => n,
            Ok(other) => {
                tracing::warn!(peer = %peer, ?other, "ping: wrong response variant");
                continue;
            }
            Err(e) => {
                tracing::warn!(peer = %peer, error = %e, "ping: undecodable response");
                continue;
            }
        };

        // Gate #3: drop the pong if trust flipped during the round trip.
        if !book.is_trusted(&peer) {
            tracing::debug!(peer = %peer, "ping: trust revoked during flight — dropping pong");
            continue;
        }
        if ack_seq != seq {
            tracing::warn!(peer = %peer, got = ack_seq, want = seq, "ping: seq mismatch");
            continue;
        }

        let rtt = sent_at.elapsed();
        {
            let mut log = rtt_log.lock().unwrap_or_else(|e| e.into_inner());
            log.push((peer, seq, rtt));
        }

        tracing::info!(peer = %peer, seq, rtt_ms = rtt.as_millis() as u64, "ping ok");
        ui.push(MeshEvent::MessageReceived {
            from: peer.to_string(),
            topic: "pong".to_string(),
            size_bytes: 9,
        })
        .await;
        ui.push(MeshEvent::Custom {
            label: "DATA".to_string(),
            detail: format!("{peer} seq={seq} rtt={}ms", rtt.as_millis()),
        })
        .await;
    }
}
