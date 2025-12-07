//! Core scheduling abstractions and capacity accounting.

pub mod error;
pub mod resource_pool;
pub mod audit;
pub mod executor;

pub use error::{AppResult, SchedulerError};
pub use resource_pool::{
    Mailbox, PoolLimits, ResourcePool, ScheduledTask, Spawn, TaskMetadata, TaskQueue, TaskStatus,
    WakeState, sync_wake_worker_loop,
};
pub use audit::{AuditEvent, AuditSink, InMemoryAuditSink, PostgresAuditSink, build_audit_event};
pub use executor::{TaskExecutor, TaskPayload};
