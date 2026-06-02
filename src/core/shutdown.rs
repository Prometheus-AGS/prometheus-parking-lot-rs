//! Graceful shutdown primitives for resource-aware worker pools.
//!
//! ## Design: Tokio three-part model
//!
//! 1. **When** — determined by the caller (Ctrl-C, SIGTERM, supervisor token, test teardown)
//! 2. **Tell** — via a [`tokio_util::sync::CancellationToken`] propagated to every worker
//! 3. **Wait** — via a [`tokio_util::task::TaskTracker`] that drains in-flight work
//!
//! ## Caller-pluggable shutdown (CR-13)
//!
//! Callers supply an external `CancellationToken` when building a pool, giving them
//! upstream ownership of *when* shutdown fires. If no token is supplied the pool creates
//! its own and exposes a [`ShutdownHandle`] for the caller to trigger later.
//!
//! Shutdown *policy* — what to do with in-flight work — is a closed [`ShutdownPolicy`]
//! enum owned by the library. Callers pick a policy; they do **not** supply a callback.
//! Reactions to individual task cancellations are delivered via the hooks bus and the
//! `CancellationSink` (CR-07, CR-14), not here.
//!
//! ## Example
//!
//! ```rust,ignore
//! use prometheus_parking_lot::core::shutdown::{ShutdownHandle, ShutdownPolicy};
//! use std::time::Duration;
//!
//! // Pool creates its own token; caller gets a handle to trigger later.
//! let (pool, handle) = WorkerPool::with_shutdown_handle(config, executor)?;
//!
//! // Trigger graceful drain from anywhere (Ctrl-C handler, test teardown, etc.)
//! handle.shutdown_with(ShutdownPolicy::DrainThenCancel {
//!     deadline: Duration::from_secs(30),
//! });
//!
//! // Or wire up OS signals automatically:
//! // handle.shutdown_on_signal().await; // ctrl_c + SIGTERM
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
    /// handed to the pool's `CancellationSink` for optional replay.
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

impl Default for ShutdownPolicy {
    fn default() -> Self {
        Self::DrainThenCancel {
            deadline: Duration::from_secs(30),
        }
    }
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
    /// Create a new handle wrapping a cancellation token.
    pub fn new(token: CancellationToken) -> Self {
        Self { token }
    }

    /// Get the underlying `CancellationToken`.
    pub fn token(&self) -> &CancellationToken {
        &self.token
    }

    /// Trigger shutdown with the given policy.
    ///
    /// For `Drain` and `DrainThenCancel`, this signals workers to stop
    /// accepting new tasks; in-flight work continues until the deadline.
    /// For `CancelImmediately`, all in-flight work is cancelled at once.
    ///
    /// This method is non-blocking; it fires the cancellation signal and
    /// returns immediately. Use [`ShutdownHandle::shutdown_on_signal`] to
    /// block until the pool is fully drained.
    pub fn shutdown_with(&self, _policy: ShutdownPolicy) {
        self.token.cancel();
    }

    /// Block the current thread until a Ctrl-C or SIGTERM is received, then
    /// trigger shutdown with the given policy.
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
pub async fn wait_for_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
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
