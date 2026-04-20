//! Per-peer pairing state machine backing the HMAC handshake.
//!
//! The gate itself is three `if book.is_trusted(&peer)` checks in the data
//! plane (see step 6); everything else is bookkeeping. States progress
//! `Unknown → AwaitingResponse → Trusted`, with any step able to transition
//! into `Failed`. `sweep_stale` moves pending responses that have timed out
//! into `Failed` so a silent peer never leaves us hanging indefinitely.
//!
//! The book is cheap to clone — it wraps an `Arc<Mutex<HashMap<...>>>` —
//! so multiple tasks (the handshake driver, the per-peer ping loop, the
//! sweeper) can share one instance without ceremony.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use swarm_nl::PeerId;
use tokio::sync::Mutex;

use crate::wire::NONCE_LEN;

/// A peer's current pairing status from this node's point of view.
///
/// `AwaitingResponse` stores the nonce we sent so we can verify the MAC
/// when the peer's reply arrives, plus a timestamp used by the sweeper.
#[derive(Debug, Clone)]
pub enum PairState {
    Unknown,
    AwaitingResponse {
        nonce_sent: [u8; NONCE_LEN],
        started_at: Instant,
    },
    Trusted {
        since: Instant,
    },
    Failed {
        reason: &'static str,
    },
}

/// Concurrent map of `PeerId` → [`PairState`].
///
/// `Clone` is cheap: it duplicates the `Arc`, not the map.
#[derive(Clone, Default)]
pub struct PairingBook {
    inner: Arc<Mutex<HashMap<PeerId, PairState>>>,
}

impl PairingBook {
    pub fn new() -> Self {
        Self::default()
    }

    /// `true` iff the peer has reached the `Trusted` state. All data-plane
    /// gates in step 6 consult this method.
    pub async fn is_trusted(&self, peer: &PeerId) -> bool {
        let guard = self.inner.lock().await;
        matches!(guard.get(peer), Some(PairState::Trusted { .. }))
    }

    /// Record that we have sent a Challenge to `peer` carrying `nonce`.
    /// Overwrites any prior state — starting a fresh handshake resets the
    /// record from either `Failed` or a stale `AwaitingResponse`.
    pub async fn mark_challenged(&self, peer: PeerId, nonce: [u8; NONCE_LEN]) {
        let mut guard = self.inner.lock().await;
        guard.insert(
            peer,
            PairState::AwaitingResponse {
                nonce_sent: nonce,
                started_at: Instant::now(),
            },
        );
    }

    /// Promote `peer` to `Trusted`.
    pub async fn mark_trusted(&self, peer: PeerId) {
        let mut guard = self.inner.lock().await;
        guard.insert(
            peer,
            PairState::Trusted {
                since: Instant::now(),
            },
        );
    }

    /// Record a pairing failure. `reason` is a `&'static str` so the book
    /// doesn't allocate on the error path; for structured diagnostics the
    /// caller should also emit a log/MeshEvent.
    pub async fn mark_failed(&self, peer: PeerId, reason: &'static str) {
        let mut guard = self.inner.lock().await;
        guard.insert(peer, PairState::Failed { reason });
    }

    /// Return the nonce we previously sent to `peer`, if we are currently
    /// awaiting their response. Used by the handshake driver to verify the
    /// MAC on an incoming `Response` or `ResponseAndChallenge`.
    pub async fn pending_nonce(&self, peer: &PeerId) -> Option<[u8; NONCE_LEN]> {
        let guard = self.inner.lock().await;
        match guard.get(peer) {
            Some(PairState::AwaitingResponse { nonce_sent, .. }) => Some(*nonce_sent),
            _ => None,
        }
    }

    /// Snapshot a peer's current state, mostly useful for tests and
    /// UI-tab diagnostics.
    pub async fn state_of(&self, peer: &PeerId) -> PairState {
        let guard = self.inner.lock().await;
        guard.get(peer).cloned().unwrap_or(PairState::Unknown)
    }

    /// Move every `AwaitingResponse` entry older than `timeout` into
    /// `Failed { reason: "handshake timeout" }`. `Trusted` / `Failed` /
    /// `Unknown` are untouched. Returns the number of entries swept so
    /// callers can log or surface a metric.
    pub async fn sweep_stale(&self, timeout: Duration) -> usize {
        let mut guard = self.inner.lock().await;
        let now = Instant::now();
        let mut swept = 0usize;
        for state in guard.values_mut() {
            if let PairState::AwaitingResponse { started_at, .. } = state {
                if now.duration_since(*started_at) > timeout {
                    *state = PairState::Failed {
                        reason: "handshake timeout",
                    };
                    swept += 1;
                }
            }
        }
        swept
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn some_peer() -> PeerId {
        PeerId::random()
    }

    #[tokio::test]
    async fn default_is_unknown_and_not_trusted() {
        let book = PairingBook::new();
        let peer = some_peer();
        assert!(!book.is_trusted(&peer).await);
        assert!(matches!(book.state_of(&peer).await, PairState::Unknown));
    }

    #[tokio::test]
    async fn mark_challenged_stores_nonce_and_is_not_trusted() {
        let book = PairingBook::new();
        let peer = some_peer();
        let nonce = [0x77u8; NONCE_LEN];
        book.mark_challenged(peer, nonce).await;

        assert!(!book.is_trusted(&peer).await);
        assert_eq!(book.pending_nonce(&peer).await, Some(nonce));
    }

    #[tokio::test]
    async fn mark_trusted_flips_is_trusted() {
        let book = PairingBook::new();
        let peer = some_peer();
        book.mark_challenged(peer, [0u8; NONCE_LEN]).await;
        book.mark_trusted(peer).await;
        assert!(book.is_trusted(&peer).await);
        assert!(book.pending_nonce(&peer).await.is_none());
    }

    #[tokio::test]
    async fn mark_failed_overrides_trusted() {
        let book = PairingBook::new();
        let peer = some_peer();
        book.mark_trusted(peer).await;
        book.mark_failed(peer, "kicked").await;
        assert!(!book.is_trusted(&peer).await);
        match book.state_of(&peer).await {
            PairState::Failed { reason } => assert_eq!(reason, "kicked"),
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn mark_challenged_after_failed_restarts_handshake() {
        let book = PairingBook::new();
        let peer = some_peer();
        book.mark_failed(peer, "old run").await;
        book.mark_challenged(peer, [1u8; NONCE_LEN]).await;
        assert_eq!(book.pending_nonce(&peer).await, Some([1u8; NONCE_LEN]));
    }

    #[tokio::test]
    async fn sweep_stale_moves_old_awaiting_to_failed() {
        let book = PairingBook::new();
        let old_peer = some_peer();
        let fresh_peer = some_peer();
        let trusted_peer = some_peer();

        // Prepare three states: an old AwaitingResponse, a fresh one, and a
        // Trusted one. Only the old AwaitingResponse should be swept.
        {
            let mut guard = book.inner.lock().await;
            guard.insert(
                old_peer,
                PairState::AwaitingResponse {
                    nonce_sent: [0u8; NONCE_LEN],
                    started_at: Instant::now() - Duration::from_secs(10),
                },
            );
        }
        book.mark_challenged(fresh_peer, [0u8; NONCE_LEN]).await;
        book.mark_trusted(trusted_peer).await;

        let swept = book.sweep_stale(Duration::from_secs(5)).await;
        assert_eq!(swept, 1);

        assert!(matches!(
            book.state_of(&old_peer).await,
            PairState::Failed { reason: "handshake timeout" }
        ));
        assert!(matches!(
            book.state_of(&fresh_peer).await,
            PairState::AwaitingResponse { .. }
        ));
        assert!(book.is_trusted(&trusted_peer).await);
    }

    #[tokio::test]
    async fn sweep_is_idempotent_when_nothing_expired() {
        let book = PairingBook::new();
        let peer = some_peer();
        book.mark_challenged(peer, [0u8; NONCE_LEN]).await;
        assert_eq!(book.sweep_stale(Duration::from_secs(5)).await, 0);
        assert!(matches!(
            book.state_of(&peer).await,
            PairState::AwaitingResponse { .. }
        ));
    }
}
