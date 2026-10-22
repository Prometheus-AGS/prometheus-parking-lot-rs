//! Unified error taxonomy for all scheduler components.
//!
//! ## Design
//!
//! Two `#[non_exhaustive]` enums form one family:
//!
//! - [`TaskError`]: how one task's *execution* failed — the executor returned
//!   an error, panicked, ran past its budget, was cancelled, or its deadline
//!   passed before it started. Executors return it; pools record it in the
//!   mailbox as [`TaskStatus::Failed`](crate::contracts::TaskStatus) and hand
//!   it back from result retrieval. It is plain data (`Clone`, `Serialize`)
//!   because it is stored and shipped between processes.
//! - [`SchedulerError`]: everything else — admission (queue full, capacity,
//!   deadline at submit, shutdown), configuration, backend IO, result lookup —
//!   plus [`SchedulerError::Task`], which carries a [`TaskError`] out of an API
//!   that returns a task's result.
//!
//! Both the `ResourcePool` and the `WorkerPool` report through
//! `SchedulerError`; the 0.2 `PoolError` enum is gone (see the mapping in
//! `CHANGELOG.md`, 0.3.0).
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

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// How the execution of one task failed.
///
/// Executors return [`TaskError::Executor`] (through [`TaskError::executor`] or
/// [`TaskError::transient`]); the scheduler produces the other variants.
#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum TaskError {
    /// The executor ran and returned an error.
    #[error("executor error: {message}")]
    Executor {
        /// The executor's description of the failure.
        message: String,
        /// The executor's classification: `true` when running the same task
        /// again may succeed (a transient fault), `false` when it will not.
        retryable: bool,
    },

    /// The executor panicked while running the task.
    #[error("executor panicked: {message}")]
    ExecutorPanicked {
        /// The panic payload, when it was a string.
        message: String,
    },

    /// The task ran past its execution budget and was cancelled.
    #[error("execution exceeded its {budget:?} budget")]
    Timeout {
        /// The budget the task was given.
        budget: Duration,
    },

    /// The task was cancelled before it finished (for example at shutdown).
    #[error("task cancelled: {reason}")]
    Cancelled {
        /// Why the task was cancelled.
        reason: String,
    },

    /// The task's deadline passed before it could start; it never ran.
    #[error("deadline expired before the task started")]
    DeadlineExpired,
}

impl TaskError {
    /// A permanent executor failure: running the task again will not help.
    pub fn executor(message: impl fmt::Display) -> Self {
        Self::Executor {
            message: message.to_string(),
            retryable: false,
        }
    }

    /// A transient executor failure: running the task again may succeed.
    pub fn transient(message: impl fmt::Display) -> Self {
        Self::Executor {
            message: message.to_string(),
            retryable: true,
        }
    }

    /// Returns `true` only when the executor marked the failure transient.
    ///
    /// Scheduler-originated failures (panic, timeout, cancellation, expired
    /// deadline) are not retried automatically; a durable dispatcher decides
    /// redelivery for those by policy.
    #[must_use]
    pub const fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Executor {
                retryable: true,
                ..
            }
        )
    }
}

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

    /// The task's resource cost exceeds the pool's total capacity, so the
    /// task can never be admitted; `available` is the pool's total.
    ///
    /// This is not transient: the pool rejects the task at submission instead
    /// of parking a task that could never start and would block the tasks
    /// behind it.
    #[error("capacity exceeded: need {requested} units, pool '{pool}' has {available}")]
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

    /// The caller no longer holds the lease named by a
    /// [`LeaseToken`](crate::contracts::LeaseToken): the lease expired (and the
    /// task may already be redelivered to another consumer), or the task was
    /// already acked, nacked or dead-lettered.
    ///
    /// Not retryable: the consumer must stop working on the task. Whoever holds
    /// the current lease owns its outcome.
    #[error("lease on queue entry {entry} is no longer held")]
    LeaseLost {
        /// The queue entry the stale token named.
        entry: u64,
    },

    /// A task's execution failed; returned by APIs that hand back a task's
    /// result (for example `WorkerPool::retrieve`).
    #[error(transparent)]
    Task(#[from] TaskError),

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
    /// may retry: a full queue, a timed-out wait, or a task failure its
    /// executor marked transient.
    #[must_use]
    pub const fn is_retryable(&self) -> bool {
        match self {
            Self::QueueFull { .. } | Self::Timeout { .. } => true,
            Self::Task(task) => task.is_retryable(),
            _ => false,
        }
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
            !SchedulerError::CapacityExceeded {
                pool: "p".into(),
                requested: 2,
                available: 1
            }
            .is_retryable()
        );
        assert!(SchedulerError::Timeout { task_id: None }.is_retryable());
        assert!(SchedulerError::Task(TaskError::transient("busy")).is_retryable());
        assert!(!SchedulerError::Task(TaskError::executor("bad input")).is_retryable());
        assert!(!SchedulerError::DeadlineExpired { task_id: 1 }.is_retryable());
        assert!(!SchedulerError::PoolShutdown { pool: "p".into() }.is_retryable());
    }

    #[test]
    fn task_error_round_trips_through_serde() {
        let errors = [
            TaskError::executor("model not found"),
            TaskError::transient("gpu busy"),
            TaskError::ExecutorPanicked {
                message: "index out of bounds".into(),
            },
            TaskError::Timeout {
                budget: Duration::from_secs(60),
            },
            TaskError::Cancelled {
                reason: "shutdown".into(),
            },
            TaskError::DeadlineExpired,
        ];
        for err in errors {
            let json = serde_json::to_string(&err).expect("serialize");
            let back: TaskError = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, err, "{json}");
        }
    }

    #[test]
    fn task_variant_is_transparent() {
        let e = SchedulerError::from(TaskError::executor("bad input"));
        assert_eq!(e.to_string(), "executor error: bad input");
    }
}
