//! Worker pool contract types shared by every `WorkerPool` implementation.
//!
//! The native pool (`prometheus-parking-lot-runtime-tokio`) and the WASM pool
//! (`prometheus-parking-lot-wasm`) both report errors through [`PoolError`] and
//! statistics through [`PoolStats`].

use std::fmt;

/// Errors that can occur when using a `WorkerPool`.
#[derive(Debug)]
#[non_exhaustive]
pub enum PoolError {
    /// The task queue is full; no more tasks can be accepted.
    QueueFull,

    /// Insufficient resource capacity to run the task.
    InsufficientCapacity {
        /// Units requested by the task.
        requested: u32,
        /// Units currently available.
        available: u32,
    },

    /// The operation timed out.
    Timeout,

    /// The requested result was not found in the mailbox.
    ResultNotFound,

    /// The pool has been shut down.
    PoolShutdown,

    /// Configuration validation failed.
    InvalidConfig(String),

    /// Internal error (worker thread panic, channel closed, etc.).
    Internal(String),
}

impl fmt::Display for PoolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::QueueFull => write!(f, "task queue is full"),
            Self::InsufficientCapacity {
                requested,
                available,
            } => {
                write!(
                    f,
                    "insufficient capacity: requested {requested}, available {available}"
                )
            }
            Self::Timeout => write!(f, "operation timed out"),
            Self::ResultNotFound => write!(f, "result not found in mailbox"),
            Self::PoolShutdown => write!(f, "pool has been shut down"),
            Self::InvalidConfig(msg) => write!(f, "invalid configuration: {msg}"),
            Self::Internal(msg) => write!(f, "internal error: {msg}"),
        }
    }
}

impl std::error::Error for PoolError {}

/// Statistics about pool utilization and performance.
#[derive(Debug, Clone, Default)]
pub struct PoolStats {
    /// Number of worker threads/tasks.
    pub worker_count: usize,

    /// Currently executing tasks.
    pub active_tasks: u64,

    /// Tasks waiting in the queue.
    pub queued_tasks: u64,

    /// Resource units currently in use.
    pub used_units: u32,

    /// Total resource units available.
    pub total_units: u32,

    /// Total tasks completed successfully.
    pub completed_tasks: u64,

    /// Total tasks that failed.
    pub failed_tasks: u64,

    /// Total tasks submitted.
    pub submitted_tasks: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_error_display() {
        let err = PoolError::QueueFull;
        assert_eq!(format!("{err}"), "task queue is full");

        let err = PoolError::InsufficientCapacity {
            requested: 100,
            available: 50,
        };
        assert_eq!(
            format!("{err}"),
            "insufficient capacity: requested 100, available 50"
        );

        let err = PoolError::Timeout;
        assert_eq!(format!("{err}"), "operation timed out");
    }

    #[test]
    fn test_pool_stats_default() {
        let stats = PoolStats::default();
        assert_eq!(stats.worker_count, 0);
        assert_eq!(stats.active_tasks, 0);
        assert_eq!(stats.completed_tasks, 0);
    }
}
