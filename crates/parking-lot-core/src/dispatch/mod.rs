//! Dispatch: pure scheduling helpers shared by the pool implementations.
//!
//! The priority dispatcher of cr-19 is implemented here. Today the module holds
//! the counters and task envelope that both worker pools already shared.
//!
//! These items were `pub(crate)` inside the single crate and are public now only
//! so the native (`prometheus-parking-lot-runtime-tokio`) and WASM
//! (`prometheus-parking-lot-wasm`) worker pools can share them. They are hidden
//! from the documentation and are not a stable API; the priority dispatcher of
//! cr-19 replaces them.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::contracts::task::TaskMetadata;
use crate::contracts::types::MailboxKey;
use crate::contracts::worker_pool::PoolStats;

/// Internal counters for pool statistics (thread-safe).
#[doc(hidden)]
#[derive(Debug)]
pub struct PoolCounters {
    /// Currently executing tasks.
    pub active_tasks: AtomicU64,
    /// Tasks waiting in the queue.
    pub queued_tasks: AtomicU64,
    /// Resource units currently in use.
    pub used_units: std::sync::atomic::AtomicU32,
    /// Total tasks completed successfully.
    pub completed_tasks: AtomicU64,
    /// Total tasks that failed.
    pub failed_tasks: AtomicU64,
    /// Total tasks submitted.
    pub submitted_tasks: AtomicU64,
}

impl Default for PoolCounters {
    fn default() -> Self {
        Self {
            active_tasks: AtomicU64::new(0),
            queued_tasks: AtomicU64::new(0),
            used_units: std::sync::atomic::AtomicU32::new(0),
            completed_tasks: AtomicU64::new(0),
            failed_tasks: AtomicU64::new(0),
            submitted_tasks: AtomicU64::new(0),
        }
    }
}

impl PoolCounters {
    /// Get a snapshot of current statistics.
    #[must_use]
    pub fn snapshot(&self, worker_count: usize, total_units: u32) -> PoolStats {
        PoolStats {
            worker_count,
            active_tasks: self.active_tasks.load(Ordering::Relaxed),
            queued_tasks: self.queued_tasks.load(Ordering::Relaxed),
            used_units: self.used_units.load(Ordering::Relaxed),
            total_units,
            completed_tasks: self.completed_tasks.load(Ordering::Relaxed),
            failed_tasks: self.failed_tasks.load(Ordering::Relaxed),
            submitted_tasks: self.submitted_tasks.load(Ordering::Relaxed),
        }
    }
}

/// A task submitted to the worker pool, containing payload and metadata.
#[doc(hidden)]
#[derive(Debug)]
pub struct WorkerTask<P> {
    /// The task payload to execute.
    pub payload: P,
    /// Task metadata including ID, priority, cost, etc.
    pub meta: TaskMetadata,
    /// Mailbox key for result storage.
    pub mailbox_key: MailboxKey,
}

/// Generate a unique mailbox key for a task.
#[doc(hidden)]
#[must_use]
pub fn generate_mailbox_key(task_id: u64) -> MailboxKey {
    MailboxKey {
        tenant: "worker_pool".into(),
        user_id: None,
        session_id: Some(task_id.to_string()),
    }
}

/// Get the key string for a mailbox key (used for internal storage).
#[doc(hidden)]
#[must_use]
pub fn mailbox_key_to_string(key: &MailboxKey) -> String {
    format!(
        "{}:{}",
        key.tenant,
        key.session_id.as_deref().unwrap_or("unknown")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_counters_snapshot() {
        let counters = PoolCounters::default();
        counters.submitted_tasks.fetch_add(10, Ordering::Relaxed);
        counters.completed_tasks.fetch_add(5, Ordering::Relaxed);
        counters.used_units.fetch_add(100, Ordering::Relaxed);

        let stats = counters.snapshot(4, 1000);
        assert_eq!(stats.worker_count, 4);
        assert_eq!(stats.submitted_tasks, 10);
        assert_eq!(stats.completed_tasks, 5);
        assert_eq!(stats.used_units, 100);
        assert_eq!(stats.total_units, 1000);
    }
}
