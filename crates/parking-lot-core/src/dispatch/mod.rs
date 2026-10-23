//! Dispatch: the priority dispatcher shared by every pool, plus the counters
//! and task envelope the worker pools share.
//!
//! [`Dispatcher`] owns the parked queue and a unit-weighted
//! [`UnitSemaphore`](crate::semaphore::UnitSemaphore): submission does the
//! depth check and the enqueue atomically, and dispatch peeks at the head and
//! pops it only when it fits; when it does not, a smaller parked item that
//! fits may skip ahead of it, a bounded number of times (cr-20). Every pool is
//! built on it: `ResourcePool`, the native `WorkerPool` and
//! `DurableResourcePool` (`prometheus-parking-lot-runtime-tokio`) and the WASM
//! `WorkerPool`.
//!
//! `PoolCounters`, `WorkerTask` and the mailbox-key helpers were
//! `pub(crate)` inside the single crate and are public only so the native and
//! WASM worker pools can share them. They are hidden from the documentation and
//! are not a stable API.

mod dispatcher;
mod parked;

pub use dispatcher::{
    Admission, DEFAULT_MAX_HEAD_SKIPS, Dispatched, Dispatcher, Grant, SkipAheadStats,
};
pub use parked::{
    ADAPTER_LOOKAHEAD, Parkable, ParkedQueue, PriorityQueue, TaskQueueAdapter, Ticket,
};

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
