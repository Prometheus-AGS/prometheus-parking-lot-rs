//! Worker pool implementation with dedicated worker threads (native) or async tasks (WASM).
//!
//! This module provides a `WorkerPool` that manages CPU/GPU-bound work separately from
//! the main async runtime. On native platforms, it spawns dedicated OS threads; on WASM,
//! it uses async tasks with cooperative scheduling.
//!
//! # Key Features
//!
//! - **Unified API**: Same programming model for native and WASM
//! - **Dual API**: Blocking (`submit`/`retrieve`) and async (`submit_async`/`retrieve_async`)
//! - **No serialization requirement**: Result types can include channels, handles, etc.
//! - **Resource-aware**: Capacity-based admission control with queue overflow
//!
//! # Example
//!
//! ```rust,ignore
//! use prometheus_parking_lot::core::{WorkerPool, WorkerExecutor, TaskMetadata};
//! use prometheus_parking_lot::config::WorkerPoolConfig;
//!
//! // Create a pool
//! let pool = WorkerPool::new(
//!     WorkerPoolConfig::new()
//!         .with_worker_count(4)
//!         .with_max_units(1000),
//!     my_executor,
//! )?;
//!
//! // Submit work (async API works everywhere)
//! let key = pool.submit_async(job, meta).await?;
//! let result = pool.retrieve_async(&key, Duration::from_secs(60)).await?;
//! ```

// Platform-specific implementations
#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod wasm;

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::core::TaskMetadata;
use crate::util::serde::MailboxKey;

/// Errors that can occur when using a `WorkerPool`.
#[derive(Debug)]
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
            Self::InsufficientCapacity { requested, available } => {
                write!(f, "insufficient capacity: requested {requested}, available {available}")
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

/// Internal counters for pool statistics (thread-safe).
#[derive(Debug)]
pub(crate) struct PoolCounters {
    pub active_tasks: AtomicU64,
    pub queued_tasks: AtomicU64,
    pub used_units: std::sync::atomic::AtomicU32,
    pub completed_tasks: AtomicU64,
    pub failed_tasks: AtomicU64,
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
#[derive(Debug)]
pub(crate) struct WorkerTask<P> {
    /// The task payload to execute.
    pub payload: P,
    /// Task metadata including ID, priority, cost, etc.
    pub meta: TaskMetadata,
    /// Mailbox key for result storage.
    pub mailbox_key: MailboxKey,
}

/// Generate a unique mailbox key for a task.
pub(crate) fn generate_mailbox_key(task_id: u64) -> MailboxKey {
    MailboxKey {
        tenant: "worker_pool".into(),
        user_id: None,
        session_id: Some(task_id.to_string()),
    }
}

/// Get the key string for a mailbox key (used for internal storage).
pub(crate) fn mailbox_key_to_string(key: &MailboxKey) -> String {
    format!("{}:{}", key.tenant, key.session_id.as_deref().unwrap_or("unknown"))
}

// Re-export the platform-specific WorkerPool implementation
#[cfg(not(target_arch = "wasm32"))]
pub use native::WorkerPool;

#[cfg(target_arch = "wasm32")]
pub use wasm::WorkerPool;

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_pool_error_display() {
        let err = PoolError::QueueFull;
        assert_eq!(format!("{}", err), "task queue is full");
        
        let err = PoolError::InsufficientCapacity { requested: 100, available: 50 };
        assert_eq!(format!("{}", err), "insufficient capacity: requested 100, available 50");
        
        let err = PoolError::Timeout;
        assert_eq!(format!("{}", err), "operation timed out");
    }
    
    #[test]
    fn test_pool_stats_default() {
        let stats = PoolStats::default();
        assert_eq!(stats.worker_count, 0);
        assert_eq!(stats.active_tasks, 0);
        assert_eq!(stats.completed_tasks, 0);
    }
    
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
