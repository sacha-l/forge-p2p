//! Handshake driver: turns `ConnectionEstablished` into a `Challenge →
//! Response` round-trip and reacts to incoming RPCs via the static
//! `HANDLER_CTX` workaround (see `library-feedback.md`).
//!
//! The protocol was simplified from the spec's 4-message flow because
//! SwarmNL v0.2.1's RPC hook is a synchronous `fn(RpcData) -> RpcData`
//! with no peer identity. Each side independently runs one round-trip:
//!
//! ```text
//!   A → B: AppData::SendRpc(Challenge(nonce_a))
//!   B → A (as the RPC response): Response(HMAC(S, nonce_a))
//!
//!   B → A: AppData::SendRpc(Challenge(nonce_b))
//!   A → B (as the RPC response): Response(HMAC(S, nonce_b))
//! ```
//!
//! Mutual authentication falls out of running both directions in parallel.

use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

use anyhow::{anyhow, Result};
use rand::RngCore;
use swarm_nl::core::{AppData, AppResponse, Core};
use swarm_nl::PeerId;

use crate::config::SECRET_LEN;
use crate::pairing::PairingBook;
use crate::wire::{hmac_nonce, mac_eq, WireMsg, NONCE_LEN};

/// Per-handshake timeout. Matches the sweep interval in `PairingBook`.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// Shared state consulted by the sync RPC handler. Installed once at node
/// startup via [`install_handler_ctx`]; cloned `Arc` references live in
/// the main event loop, the per-peer tasks, and the sweeper.
///
/// The `peer` slot holds the single peer currently connected, cached on
/// `ConnectionEstablished`. This is the "n=2 assumption" workaround for
/// SwarmNL's handler having no `PeerId` context; see `library-feedback.md`.
pub struct HandlerCtx {
    pub secret: [u8; SECRET_LEN],
    pub book: PairingBook,
    peer: RwLock<Option<PeerId>>,
}

impl HandlerCtx {
    pub fn new(secret: [u8; SECRET_LEN], book: PairingBook) -> Self {
        Self {
            secret,
            book,
            peer: RwLock::new(None),
        }
    }

    /// Record the "one other peer" cached from `ConnectionEstablished`.
    pub fn set_peer(&self, peer: PeerId) {
        if let Ok(mut g) = self.peer.write() {
            *g = Some(peer);
        }
    }

    /// Read the cached peer, if any.
    pub fn current_peer(&self) -> Option<PeerId> {
        self.peer.read().ok().and_then(|g| *g)
    }
}

static HANDLER_CTX: OnceLock<Arc<HandlerCtx>> = OnceLock::new();

/// Install the process-wide handler context. Must be called once, before
/// any RPC could arrive. Returns an error if called twice.
pub fn install_handler_ctx(ctx: Arc<HandlerCtx>) -> Result<()> {
    HANDLER_CTX
        .set(ctx)
        .map_err(|_| anyhow!("HANDLER_CTX already installed"))
}

/// Fetch the handler context. Returns `None` before install.
pub fn handler_ctx() -> Option<Arc<HandlerCtx>> {
    HANDLER_CTX.get().cloned()
}

/// Random 16-byte nonce for a Challenge.
pub fn fresh_nonce() -> [u8; NONCE_LEN] {
    let mut nonce = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce);
    nonce
}

/// The sync RPC handler registered via `CoreBuilder::with_rpc`.
///
/// `data` is the request payload (`Vec<Vec<u8>>`). We take the first frame,
/// decode it as a [`WireMsg`], and return the appropriate response frame.
/// Malformed input or messages we can't meaningfully answer return an
/// empty `RpcData`; the sender can treat that as a failure.
pub fn rpc_handler(data: swarm_nl::core::RpcData) -> swarm_nl::core::RpcData {
    let Some(ctx) = HANDLER_CTX.get() else {
        return empty();
    };
    let Some(frame) = data.first() else {
        return empty();
    };
    let msg = match WireMsg::decode(frame) {
        Ok(m) => m,
        Err(_) => return empty(),
    };

    match msg {
        WireMsg::Challenge(nonce) => {
            let mac = hmac_nonce(&ctx.secret, &nonce);
            vec![WireMsg::Response(mac).encode()]
        }
        WireMsg::DataPing(seq) => {
            // Gate #2 of the three-if gate: handler-side trust check.
            // Uses the cached "one other peer" from ConnectionEstablished.
            let trusted = ctx
                .current_peer()
                .map(|p| ctx.book.is_trusted(&p))
                .unwrap_or(false);
            if trusted {
                vec![WireMsg::DataPong(seq).encode()]
            } else {
                empty()
            }
        }
        // Response / ResponseAndChallenge / Ack / DataPong should never
        // arrive as a *request* — they are response-shaped frames. Drop.
        _ => empty(),
    }
}

fn empty() -> swarm_nl::core::RpcData {
    vec![Vec::new()]
}

/// Drive one Challenge → Response handshake against `peer`. Updates the
/// book to `Trusted` or `Failed` by the time it returns.
///
/// Logs at `info` / `warn` rather than returning errors so the caller
/// (typically spawned from the event loop) can be fire-and-forget.
pub async fn initiate_handshake(
    node: &mut Core,
    book: &PairingBook,
    secret: &[u8; SECRET_LEN],
    peer: PeerId,
) {
    let nonce = fresh_nonce();
    book.mark_challenged(peer, nonce);
    tracing::info!(peer = %peer, "handshake: sending Challenge");

    let req = AppData::SendRpc {
        keys: vec![WireMsg::Challenge(nonce).encode()],
        peer,
    };

    let response = match tokio::time::timeout(HANDSHAKE_TIMEOUT, node.query_network(req)).await {
        Ok(Ok(resp)) => resp,
        Ok(Err(e)) => {
            tracing::warn!(peer = %peer, error = ?e, "handshake: SendRpc failed");
            book.mark_failed(peer, "rpc error");
            return;
        }
        Err(_) => {
            tracing::warn!(peer = %peer, "handshake: timed out waiting for Response");
            book.mark_failed(peer, "handshake timeout");
            return;
        }
    };

    let frames = match response {
        AppResponse::SendRpc(frames) => frames,
        other => {
            tracing::warn!(peer = %peer, ?other, "handshake: unexpected AppResponse variant");
            book.mark_failed(peer, "unexpected response");
            return;
        }
    };

    let frame = match frames.first() {
        Some(f) if !f.is_empty() => f,
        _ => {
            tracing::warn!(peer = %peer, "handshake: empty RPC response (peer refused or handler not installed)");
            book.mark_failed(peer, "empty response");
            return;
        }
    };

    let mac = match WireMsg::decode(frame) {
        Ok(WireMsg::Response(mac)) => mac,
        Ok(other) => {
            tracing::warn!(peer = %peer, ?other, "handshake: wrong wire variant in response");
            book.mark_failed(peer, "bad response variant");
            return;
        }
        Err(e) => {
            tracing::warn!(peer = %peer, error = %e, "handshake: could not decode response");
            book.mark_failed(peer, "undecodable response");
            return;
        }
    };

    let expected = hmac_nonce(secret, &nonce);
    if mac_eq(&mac, &expected) {
        book.mark_trusted(peer);
        tracing::info!(peer = %peer, "handshake: Trusted");
    } else {
        book.mark_failed(peer, "mac mismatch");
        tracing::warn!(peer = %peer, "handshake: MAC mismatch — Failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handler_returns_valid_response_for_challenge() {
        // Build a ctx with a known secret, install it.
        let secret = [0x42u8; SECRET_LEN];
        let book = PairingBook::new();
        let ctx = Arc::new(HandlerCtx::new(secret, book));
        let _ = install_handler_ctx(ctx);

        let nonce = [0x11u8; NONCE_LEN];
        let req = vec![WireMsg::Challenge(nonce).encode()];
        let resp = rpc_handler(req);
        let decoded = WireMsg::decode(&resp[0]).unwrap();
        match decoded {
            WireMsg::Response(mac) => {
                assert_eq!(mac, hmac_nonce(&secret, &nonce));
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }

    #[test]
    fn handler_drops_unknown_request() {
        // Uses whatever ctx was installed by the previous test — fine
        // because we only care that malformed input does not panic.
        let resp = rpc_handler(vec![vec![0xff, 0xff]]);
        assert_eq!(resp, vec![Vec::<u8>::new()]);
        assert_eq!(rpc_handler(vec![]), vec![Vec::<u8>::new()]);
    }
}
