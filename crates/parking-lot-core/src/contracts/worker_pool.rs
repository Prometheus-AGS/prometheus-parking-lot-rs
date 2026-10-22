//! Worker pool contract types shared by every `WorkerPool` implementation.
//!
//! The native pool (`prometheus-parking-lot-runtime-tokio`) and the WASM pool
//! (`prometheus-parking-lot-wasm`) report statistics through [`PoolStats`] and
//! errors through [`SchedulerError`](crate::contracts::SchedulerError) (the 0.2
//! `PoolError` enum was folded into it in 0.3.0).

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

    /// The 0.2 `PoolError` variants now surface as these `SchedulerError`
    /// variants (the 0.3.0 mapping); their messages stay actionable.
    #[test]
    fn test_pool_error_display() {
        use crate::contracts::{SchedulerError, TaskError};

        let err = SchedulerError::queue_full("worker_pool", 10, 10);
        assert_eq!(
            err.to_string(),
            "queue full for pool 'worker_pool': depth 10/10"
        );

        let err = SchedulerError::Timeout { task_id: None };
        assert_eq!(err.to_string(), "operation timed out");

        let err = SchedulerError::from(TaskError::executor("bad prompt"));
        assert_eq!(err.to_string(), "executor error: bad prompt");
    }

    #[test]
    fn test_pool_stats_default() {
        let stats = PoolStats::default();
        assert_eq!(stats.worker_count, 0);
        assert_eq!(stats.active_tasks, 0);
        assert_eq!(stats.completed_tasks, 0);
    }
}
