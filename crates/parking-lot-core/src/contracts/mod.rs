//! Scheduling contracts: the traits and value types every backend, runtime
//! adapter and pool implementation builds against.
//!
//! Nothing in this module performs IO or depends on an async runtime.
//!
//! Two generations of storage contracts coexist until cr-49 (ADR 0003):
//!
//! - [`LeaseQueue`] and [`ResultMailbox`] (async, leasing, idempotent) are the
//!   backend contracts every durable backend implements and the conformance
//!   suite checks. [`InMemoryLeaseQueue`] and [`InMemoryResultMailbox`] are
//!   their reference implementations.
//! - [`TaskQueue`] and [`Mailbox`] (synchronous) are the in-process contracts
//!   the in-memory `ResourcePool` parks tasks and delivers results through. Its
//!   durable mode, `DurableResourcePool` (cr-49), uses [`LeaseQueue`] and
//!   [`ResultMailbox`] instead.

pub mod clock;
#[cfg(any(test, feature = "conformance"))]
pub mod conformance;
pub mod error;
pub mod executor;
pub mod lease_queue;
pub mod mailbox;
pub mod memory_lease_queue;
pub mod memory_queue;
pub mod memory_result_mailbox;
pub mod queue;
pub mod result_mailbox;
pub mod spawn;
pub mod task;
pub mod types;
pub mod worker_pool;

pub use clock::{Clock, SystemClock, TestClock, now_ms};
pub use error::{AppResult, SchedulerError, TaskError};
pub use executor::{TaskExecutor, TaskPayload, WorkerExecutor};
pub use lease_queue::{
    Enqueued, LeaseQueue, LeaseToken, LeasedTask, NackAction, QueueDepth, QueueEntryId,
};
pub use mailbox::Mailbox;
pub use memory_lease_queue::{DeadLetter, InMemoryLeaseQueue};
pub use memory_queue::InMemoryQueue;
pub use memory_result_mailbox::InMemoryResultMailbox;
pub use queue::TaskQueue;
pub use result_mailbox::{
    Delivery, DeliveryOutcome, MailboxEntry, NoopNotifier, Notifier, ResultMailbox, StorageMailbox,
};
pub use spawn::Spawn;
pub use task::{PoolLimits, ScheduledTask, SubmissionSequence, TaskMetadata, TaskStatus};
pub use types::{MailboxKey, Priority, ResourceCost, ResourceKind, TaskId};
pub use worker_pool::PoolStats;
