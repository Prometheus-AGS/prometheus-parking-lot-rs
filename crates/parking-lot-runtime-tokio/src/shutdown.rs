//! Graceful shutdown for the pools (cr-29), plus the caller-pluggable
//! shutdown signal (CR-13).
//!
//! ## Policies
//!
//! Every pool has `shutdown_with(policy)`, which stops accepting work, applies
//! a [`ShutdownPolicy`] and returns a [`ShutdownReport`]. Whatever the policy,
//! no accepted task is silently dropped: each one reaches its own outcome, is
//! handed to the pool's `CancellationSink` (in-memory pools) or is handed back
//! to its durable queue (durable pool).
//!
//! | policy | in-memory pool: queued | in-memory pool: running | durable pool: leased, waiting for capacity | durable pool: running |
//! |---|---|---|---|---|
//! | `Drain { deadline }` | keeps starting as capacity frees until the deadline; the rest is then cancelled to the sink | runs to its outcome; one still running at the deadline keeps running and is reported `unsettled` | handed back at once | runs to its outcome; past the deadline it keeps running (heartbeat on) and is reported `unsettled` |
//! | `DrainThenCancel { deadline }` (default) | as `Drain` | runs until the deadline, then is cancelled to the sink | handed back at once | runs until the deadline, then is cancelled and handed back |
//! | `CancelImmediately` | cancelled to the sink at once | cancelled to the sink at once | handed back at once | cancelled and handed back at once |
//!
//! "Cancelled to the sink" means: the task's mailbox (or result slot) receives
//! `TaskError::Cancelled`, and a `CancelledTask` is offered to the pool's
//! `CancellationSink` under its `delta` bound. A durable pool's tasks that
//! were never leased stay in the durable queue; it stops leasing at once
//! under every policy. "Handed back" is `LeaseQueue::nack(Requeue)`: the task
//! runs again on the next start of a pool on that queue.
//!
//! ## Caller-pluggable signal (CR-13)
//!
//! Callers decide *when* to shut down: a [`ShutdownHandle`] wraps a
//! [`tokio_util::sync::CancellationToken`] that a Ctrl-C handler, supervisor
//! or test can fire; [`wait_for_signal`] waits for Ctrl-C or SIGTERM. The pool
//! applies the policy when the caller calls its `shutdown_with`:
//!
//! ```rust,ignore
//! use prometheus_parking_lot::core::shutdown::{ShutdownPolicy, wait_for_signal};
//!
//! wait_for_signal().await;
//! let report = pool.shutdown_with(ShutdownPolicy::default()).await;
//! assert_eq!(report.unanswered, 0);
//! ```

use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Policy controlling what happens to in-flight tasks during pool shutdown.
///
/// Choose based on the workload: AI inference tasks are expensive but can hang,
/// so the default `DrainThenCancel` policy is recommended for most inference pools.
///
/// This enum is `#[non_exhaustive]`; new policies may be added in minor versions.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum ShutdownPolicy {
    /// Wait for all in-flight tasks to finish before shutting down.
    ///
    /// **Warning:** if any task hangs indefinitely, the pool will never shut
    /// down. Only use this when you can guarantee bounded execution time.
    Drain {
        /// Maximum time to wait for in-flight tasks to complete.
        deadline: Duration,
    },

    /// Wait for in-flight tasks up to `deadline`, then cancel any stragglers.
    ///
    /// This is the **default policy**. Cancelled stragglers are captured and
    /// handed to the pool's `CancellationSink` for optional replay (durable
    /// pool: handed back to the queue).
    DrainThenCancel {
        /// Maximum time to wait before cancelling remaining tasks.
        deadline: Duration,
    },

    /// Cancel all in-flight tasks immediately without waiting.
    ///
    /// Use for tests, Ctrl-C handlers where you want immediate exit, or when
    /// the payload is safe to discard and has no side effects.
    CancelImmediately,
}

impl ShutdownPolicy {
    /// How long the pool waits for accepted work to finish before it cancels
    /// what is left (zero for [`CancelImmediately`](Self::CancelImmediately)).
    #[must_use]
    pub const fn drain_deadline(&self) -> Duration {
        match self {
            Self::Drain { deadline } | Self::DrainThenCancel { deadline } => *deadline,
            Self::CancelImmediately => Duration::ZERO,
        }
    }

    /// Whether tasks still running at the drain deadline are cancelled.
    #[must_use]
    pub const fn cancels_stragglers(&self) -> bool {
        !matches!(self, Self::Drain { .. })
    }
}

impl Default for ShutdownPolicy {
    fn default() -> Self {
        Self::DrainThenCancel {
            deadline: Duration::from_secs(30),
        }
    }
}

/// What a pool's `shutdown_with` did, counted from the moment it began.
///
/// Every task the pool had accepted and not finished when the shutdown began
/// is in exactly one of `completed`, `cancelled` or `unsettled` (a durable
/// pool's tasks that were never leased stay in its queue and are not counted).
/// `cancelled` splits by what happened next:
/// `cancelled == recorded + requeued + dropped + unanswered`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ShutdownReport {
    /// Tasks that reached their own outcome during the shutdown: completed,
    /// failed or expired.
    pub completed: u64,
    /// Tasks cancelled: queued tasks left at the drain deadline, running
    /// stragglers, and (durable pool) leased tasks handed back for any
    /// reason.
    pub cancelled: u64,
    /// Cancelled tasks the sink answered [`SinkOutcome::Recorded`] for.
    ///
    /// [`SinkOutcome::Recorded`]: crate::SinkOutcome::Recorded
    pub recorded: u64,
    /// Tasks handed back to the durable queue (`nack(Requeue)`, or a lease
    /// left to expire): cancelled at shutdown, or (durable pool) leased tasks
    /// whose outcome could not be stored. They run again on the next start of
    /// a pool on that queue, and count in `cancelled` too. Always zero for the
    /// in-memory pools, which have no queue that outlives them.
    pub requeued: u64,
    /// Cancelled tasks the sink answered `Drop` for — including the default
    /// sink, which logs them — or `Requeue`, which an in-memory pool cannot
    /// honour (logged at warn level).
    pub dropped: u64,
    /// Cancelled tasks whose sink did not answer within
    /// `CancellationConfig::delta`, or panicked (logged at error level).
    pub unanswered: u64,
    /// Accepted tasks still not settled when `shutdown_with` returned: a task
    /// still running after a `Drain` deadline, an executor that ignored
    /// cancellation by never yielding, or a sink still being called. They are
    /// not dropped: each still reaches its outcome or its sink later.
    pub unsettled: u64,
}

/// A handle to a worker pool's shutdown mechanism.
///
/// Obtained when a pool is built without an external `CancellationToken`.
/// Clone-able so it can be shared across Ctrl-C handlers, test teardown, etc.
#[derive(Clone, Debug)]
pub struct ShutdownHandle {
    token: CancellationToken,
}

impl ShutdownHandle {
    /// Create a new handle wrapping a cancellation token.
    #[must_use]
    pub const fn new(token: CancellationToken) -> Self {
        Self { token }
    }

    /// Get the underlying `CancellationToken`.
    #[must_use]
    pub const fn token(&self) -> &CancellationToken {
        &self.token
    }

    /// Fire the shutdown signal.
    ///
    /// This only cancels the token, so whoever waits on it learns that
    /// shutdown was requested; it does not touch a pool. The policy is
    /// applied by the pool's own `shutdown_with(policy)`, which the waiter
    /// calls (see the module docs).
    pub fn shutdown_with(&self, _policy: ShutdownPolicy) {
        self.token.cancel();
    }

    /// Wait until a Ctrl-C or SIGTERM is received, then fire the shutdown
    /// signal ([`shutdown_with`](Self::shutdown_with)).
    ///
    /// This is the recommended way to wire graceful shutdown in a binary's
    /// `main()` function or integration tests.
    ///
    /// # Panics
    ///
    /// Panics if called outside of a tokio runtime context.
    pub async fn shutdown_on_signal(&self, policy: ShutdownPolicy) {
        wait_for_signal().await;
        self.shutdown_with(policy);
    }
}

/// Wait for Ctrl-C or (on Unix) SIGTERM, whichever arrives first.
///
/// On non-Unix platforms only Ctrl-C is handled.
///
/// # Panics
///
/// Panics if the SIGTERM handler cannot be installed (Unix only), or if
/// called outside a tokio runtime with signal support enabled.
pub async fn wait_for_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let ctrl_c = tokio::signal::ctrl_c();
        // SAFETY: signal() may fail only on OOM or bad kind; both are bugs.
        let mut sigterm =
            signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");

        tokio::select! {
            _ = ctrl_c => {},
            _ = sigterm.recv() => {},
        }
    }

    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutdown_policy_default_is_drain_then_cancel() {
        let p = ShutdownPolicy::default();
        assert!(matches!(p, ShutdownPolicy::DrainThenCancel { .. }));
        assert!(p.cancels_stragglers());
        assert_eq!(p.drain_deadline(), Duration::from_secs(30));
        let drain = ShutdownPolicy::Drain {
            deadline: Duration::from_secs(1),
        };
        assert!(!drain.cancels_stragglers());
        assert_eq!(
            ShutdownPolicy::CancelImmediately.drain_deadline(),
            Duration::ZERO
        );
    }

    #[tokio::test]
    async fn shutdown_handle_cancels_token() {
        let token = CancellationToken::new();
        let handle = ShutdownHandle::new(token.clone());
        assert!(!token.is_cancelled());
        handle.shutdown_with(ShutdownPolicy::CancelImmediately);
        assert!(token.is_cancelled());
    }
}
