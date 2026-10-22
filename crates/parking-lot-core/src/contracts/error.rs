//! Unified error taxonomy for all scheduler components.
//!
//! ## Design
//!
//! One `#[non_exhaustive]` error enum covers both the `ResourcePool` /
//! `TaskQueue` / `Mailbox` surface and the `WorkerPool` surface so callers
//! have a single type to `match` on.
//!
//! `thiserror` is used throughout; `#[source]` chains preserve the underlying
//! cause for diagnostics. `anyhow` is reserved for application-layer code via
//! [`AppResult`].
//!
//! ## Queue-full errors
//!
//! [`SchedulerError::QueueFull`] carries actionable context: which pool, the
//! current depth, the configured capacity, and an optional hint for when the
//! caller might retry. Callers should propagate this to end-users rather than
//! treating it as an opaque failure.

use std::time::Duration;
use thiserror::Error;

/// All errors that can be produced by the scheduler, pool, queue, and mailbox
/// components.
///
/// This enum is `#[non_exhaustive]` so that new variants can be added in minor
/// releases without breaking `match` arms in downstream code.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SchedulerError {
    /// The queue for the named pool is full; no further tasks can be accepted
    /// until capacity is freed.
    ///
    /// The attached context lets callers surface actionable diagnostics to
    /// end-users or implement back-off without guessing at numbers.
    #[error(
        "queue full for pool '{pool}': depth {depth}/{capacity}\
        {}", retry_hint.map(|d| format!(" (retry after {d:.1?})")).unwrap_or_default()
    )]
    QueueFull {
        /// Name of the pool whose queue is full.
        pool: String,
        /// Current queue depth at the time of rejection.
        depth: usize,
        /// Configured maximum queue depth.
        capacity: usize,
        /// Optional caller hint: wait at least this long before retrying.
        retry_hint: Option<Duration>,
    },

    /// The task's resource cost would exceed the pool's available capacity.
    ///
    /// This is a transient condition; the task should be retried once other
    /// tasks complete and release capacity.
    #[error("capacity exceeded: need {requested} units, {available} available in pool '{pool}'")]
    CapacityExceeded {
        /// Name of the pool.
        pool: String,
        /// Resource units required by the task.
        requested: u32,
        /// Resource units currently available.
        available: u32,
    },

    /// The task's absolute deadline has already passed.
    #[error("deadline expired for task {task_id}")]
    DeadlineExpired {
        /// Identifier of the expired task.
        task_id: u64,
    },

    /// The pool has been shut down and is no longer accepting work.
    #[error("pool '{pool}' has been shut down")]
    PoolShutdown {
        /// Name of the pool that was shut down.
        pool: String,
    },

    /// Configuration validation failed; the provided values are invalid.
    #[error("invalid configuration: {message}")]
    InvalidConfig {
        /// Human-readable description of what is wrong.
        message: String,
    },

    /// A backend I/O or serialization error occurred.
    ///
    /// Unlike the old `Backend(String)`, this variant preserves the underlying
    /// error via `#[source]` so the full chain is visible in diagnostics.
    #[error("backend error in '{context}': {source}")]
    Backend {
        /// Short description of which operation or backend produced the error.
        context: String,
        /// The underlying cause.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },

    /// An operation timed out.
    #[error("operation timed out{}", task_id.map(|id| format!(" (task {id})")).unwrap_or_default())]
    Timeout {
        /// Optional task identifier that timed out.
        task_id: Option<u64>,
    },

    /// The requested result was not found in the mailbox or result store.
    #[error("result not found{}", key.as_deref().map(|k| format!(" for key '{k}'")).unwrap_or_default())]
    ResultNotFound {
        /// Optional key that was looked up.
        key: Option<String>,
    },

    /// An internal invariant was violated; this indicates a library bug.
    #[error("internal error: {message}")]
    Internal {
        /// Description of the invariant that was violated.
        message: String,
    },
}

impl SchedulerError {
    /// Convenience constructor for [`SchedulerError::QueueFull`] without a
    /// retry hint.
    pub fn queue_full(pool: impl Into<String>, depth: usize, capacity: usize) -> Self {
        Self::QueueFull {
            pool: pool.into(),
            depth,
            capacity,
            retry_hint: None,
        }
    }

    /// Convenience constructor for [`SchedulerError::Backend`] from any error.
    pub fn backend(
        context: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::Backend {
            context: context.into(),
            source: Box::new(source),
        }
    }

    /// Convenience constructor for [`SchedulerError::Internal`].
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
        }
    }

    /// Convenience constructor for [`SchedulerError::InvalidConfig`].
    pub fn invalid_config(message: impl Into<String>) -> Self {
        Self::InvalidConfig {
            message: message.into(),
        }
    }

    /// Returns `true` if this error represents a transient condition the caller
    /// may retry (queue full, capacity exceeded, timeout).
    #[must_use]
    pub const fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::QueueFull { .. } | Self::CapacityExceeded { .. } | Self::Timeout { .. }
        )
    }
}

/// Application-facing result type using `anyhow` for higher-level contexts.
///
/// Use this in application / binary code where rich error context chains are
/// preferred. Library code should return `Result<T, SchedulerError>` directly.
pub type AppResult<T> = Result<T, anyhow::Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_full_display_no_hint() {
        let e = SchedulerError::queue_full("llm_inference", 1000, 1000);
        let s = e.to_string();
        assert!(s.contains("llm_inference"), "{s}");
        assert!(s.contains("1000/1000"), "{s}");
        assert!(!s.contains("retry"), "{s}");
    }

    #[test]
    fn queue_full_display_with_hint() {
        let e = SchedulerError::QueueFull {
            pool: "gpu".into(),
            depth: 500,
            capacity: 512,
            retry_hint: Some(Duration::from_millis(250)),
        };
        let s = e.to_string();
        assert!(s.contains("gpu"), "{s}");
        assert!(s.contains("retry"), "{s}");
    }

    #[test]
    fn capacity_exceeded_display() {
        let e = SchedulerError::CapacityExceeded {
            pool: "gpu".into(),
            requested: 10,
            available: 4,
        };
        let s = e.to_string();
        assert!(s.contains("10"), "{s}");
        assert!(s.contains('4'), "{s}");
    }

    #[test]
    fn backend_preserves_source() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let e = SchedulerError::backend("yaque::enqueue", io_err);
        let s = e.to_string();
        assert!(s.contains("yaque"), "{s}");
        // source chain accessible
        let src = std::error::Error::source(&e);
        assert!(src.is_some());
    }

    #[test]
    fn is_retryable_classification() {
        assert!(SchedulerError::queue_full("p", 1, 1).is_retryable());
        assert!(
            SchedulerError::CapacityExceeded {
                pool: "p".into(),
                requested: 1,
                available: 0
            }
            .is_retryable()
        );
        assert!(SchedulerError::Timeout { task_id: None }.is_retryable());
        assert!(!SchedulerError::DeadlineExpired { task_id: 1 }.is_retryable());
        assert!(!SchedulerError::PoolShutdown { pool: "p".into() }.is_retryable());
    }
}
