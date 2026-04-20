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
//!
//! Concurrency note: we back the map with `std::sync::Mutex`, not
//! `tokio::sync::Mutex`, because SwarmNL's RPC handler is a sync
//! `fn(RpcData) -> RpcData` (see `library-feedback.md`) and needs to query
//! the book without being able to `.await`. Every critical section is a
//! single HashMap op and never crosses an await point, which is the case
//! tokio's own docs call out as the right time to use `std::sync::Mutex`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use swarm_nl::PeerId;

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
    pub fn is_trusted(&self, peer: &PeerId) -> bool {
        let guard = self.lock();
        matches!(guard.get(peer), Some(PairState::Trusted { .. }))
    }

    /// Record that we have sent a Challenge to `peer` carrying `nonce`.
    /// Overwrites any prior state — starting a fresh handshake resets the
    /// record from either `Failed` or a stale `AwaitingResponse`.
    pub fn mark_challenged(&self, peer: PeerId, nonce: [u8; NONCE_LEN]) {
        let mut guard = self.lock();
        guard.insert(
            peer,
            PairState::AwaitingResponse {
                nonce_sent: nonce,
                started_at: Instant::now(),
            },
        );
    }

    /// Promote `peer` to `Trusted`.
    pub fn mark_trusted(&self, peer: PeerId) {
        let mut guard = self.lock();
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
    pub fn mark_failed(&self, peer: PeerId, reason: &'static str) {
        let mut guard = self.lock();
        guard.insert(peer, PairState::Failed { reason });
    }

    /// Return the nonce we previously sent to `peer`, if we are currently
    /// awaiting their response. Used by the handshake driver to verify the
    /// MAC on an incoming `Response`.
    pub fn pending_nonce(&self, peer: &PeerId) -> Option<[u8; NONCE_LEN]> {
        let guard = self.lock();
        match guard.get(peer) {
            Some(PairState::AwaitingResponse { nonce_sent, .. }) => Some(*nonce_sent),
            _ => None,
        }
    }

    /// Snapshot a peer's current state, mostly useful for tests and
    /// UI-tab diagnostics.
    pub fn state_of(&self, peer: &PeerId) -> PairState {
        let guard = self.lock();
        guard.get(peer).cloned().unwrap_or(PairState::Unknown)
    }

    /// Move every `AwaitingResponse` entry older than `timeout` into
    /// `Failed { reason: "handshake timeout" }`. `Trusted` / `Failed` /
    /// `Unknown` are untouched. Returns the number of entries swept so
    /// callers can log or surface a metric.
    pub fn sweep_stale(&self, timeout: Duration) -> usize {
        let mut guard = self.lock();
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

    /// Return all peers currently recorded in the book with their states.
    /// Used by the UI panel and by tests.
    pub fn snapshot(&self) -> Vec<(PeerId, PairState)> {
        let guard = self.lock();
        guard.iter().map(|(k, v)| (*k, v.clone())).collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<PeerId, PairState>> {
        // A panic while holding the lock poisons it; we recover so the
        // app stays up rather than cascading a failure.
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Test helper: insert an `AwaitingResponse` entry with a chosen
    /// `started_at`, so integration tests can exercise `sweep_stale`
    /// without burning five seconds of wall-clock time.
    #[doc(hidden)]
    pub fn insert_awaiting_for_test(
        &self,
        peer: PeerId,
        nonce: [u8; NONCE_LEN],
        started_at: Instant,
    ) {
        let mut guard = self.lock();
        guard.insert(
            peer,
            PairState::AwaitingResponse {
                nonce_sent: nonce,
                started_at,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn some_peer() -> PeerId {
        PeerId::random()
    }

    #[test]
    fn default_is_unknown_and_not_trusted() {
        let book = PairingBook::new();
        let peer = some_peer();
        assert!(!book.is_trusted(&peer));
        assert!(matches!(book.state_of(&peer), PairState::Unknown));
    }

    #[test]
    fn mark_challenged_stores_nonce_and_is_not_trusted() {
        let book = PairingBook::new();
        let peer = some_peer();
        let nonce = [0x77u8; NONCE_LEN];
        book.mark_challenged(peer, nonce);

        assert!(!book.is_trusted(&peer));
        assert_eq!(book.pending_nonce(&peer), Some(nonce));
    }

    #[test]
    fn mark_trusted_flips_is_trusted() {
        let book = PairingBook::new();
        let peer = some_peer();
        book.mark_challenged(peer, [0u8; NONCE_LEN]);
        book.mark_trusted(peer);
        assert!(book.is_trusted(&peer));
        assert!(book.pending_nonce(&peer).is_none());
    }

    #[test]
    fn mark_failed_overrides_trusted() {
        let book = PairingBook::new();
        let peer = some_peer();
        book.mark_trusted(peer);
        book.mark_failed(peer, "kicked");
        assert!(!book.is_trusted(&peer));
        match book.state_of(&peer) {
            PairState::Failed { reason } => assert_eq!(reason, "kicked"),
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn mark_challenged_after_failed_restarts_handshake() {
        let book = PairingBook::new();
        let peer = some_peer();
        book.mark_failed(peer, "old run");
        book.mark_challenged(peer, [1u8; NONCE_LEN]);
        assert_eq!(book.pending_nonce(&peer), Some([1u8; NONCE_LEN]));
    }

    #[test]
    fn sweep_stale_moves_old_awaiting_to_failed() {
        let book = PairingBook::new();
        let old_peer = some_peer();
        let fresh_peer = some_peer();
        let trusted_peer = some_peer();

        {
            let mut guard = book.lock();
            guard.insert(
                old_peer,
                PairState::AwaitingResponse {
                    nonce_sent: [0u8; NONCE_LEN],
                    started_at: Instant::now() - Duration::from_secs(10),
                },
            );
        }
        book.mark_challenged(fresh_peer, [0u8; NONCE_LEN]);
        book.mark_trusted(trusted_peer);

        let swept = book.sweep_stale(Duration::from_secs(5));
        assert_eq!(swept, 1);

        assert!(matches!(
            book.state_of(&old_peer),
            PairState::Failed { reason: "handshake timeout" }
        ));
        assert!(matches!(
            book.state_of(&fresh_peer),
            PairState::AwaitingResponse { .. }
        ));
        assert!(book.is_trusted(&trusted_peer));
    }

    #[test]
    fn sweep_is_idempotent_when_nothing_expired() {
        let book = PairingBook::new();
        let peer = some_peer();
        book.mark_challenged(peer, [0u8; NONCE_LEN]);
        assert_eq!(book.sweep_stale(Duration::from_secs(5)), 0);
        assert!(matches!(
            book.state_of(&peer),
            PairState::AwaitingResponse { .. }
        ));
    }
}
