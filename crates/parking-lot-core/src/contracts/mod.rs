//! Scheduling contracts: the traits and value types every backend, runtime
//! adapter and pool implementation builds against.
//!
//! Nothing in this module performs IO or depends on an async runtime.

pub mod clock;
pub mod error;
pub mod executor;
pub mod mailbox;
pub mod memory_queue;
pub mod queue;
pub mod spawn;
pub mod task;
pub mod types;
pub mod worker_pool;

pub use clock::now_ms;
pub use error::{AppResult, SchedulerError};
pub use executor::{TaskExecutor, TaskPayload, WorkerExecutor};
pub use mailbox::Mailbox;
pub use memory_queue::InMemoryQueue;
pub use queue::TaskQueue;
pub use spawn::Spawn;
pub use task::{PoolLimits, ScheduledTask, TaskMetadata, TaskStatus};
pub use types::{MailboxKey, Priority, ResourceCost, ResourceKind, TaskId};
pub use worker_pool::{PoolError, PoolStats};
