//! Core scheduling abstractions and capacity accounting.

pub mod audit;
pub mod cancellation;
pub mod error;
pub mod executor;
pub mod hooks;
pub mod resource_pool;
#[cfg(feature = "tokio-runtime")]
pub mod shutdown;
pub mod worker_pool;

pub use audit::{AuditEvent, AuditSink, InMemoryAuditSink, PostgresAuditSink, build_audit_event};
pub use cancellation::{
    CancelReason, CancellationChannel, CancellationConfig, CancellationSink, CancelledTask,
    LoggingCancellationSink, RecordingCancellationSink, SinkOutcome,
};
pub use error::{AppResult, SchedulerError};
pub use executor::{TaskExecutor, TaskPayload, WorkerExecutor};
pub use hooks::{
    AuditHookAdapter, CancelledReason, Hook, HookBus, LifecycleEvent, RecordingHook,
    RejectionReason, TracingHook, emit_cancelled, emit_completed, emit_rejected, emit_submitted,
};
pub use resource_pool::{
    Mailbox, PoolLimits, ResourcePool, ScheduledTask, Spawn, TaskMetadata, TaskQueue, TaskStatus,
    WakeState, sync_wake_worker_loop,
};
#[cfg(feature = "tokio-runtime")]
pub use shutdown::{ShutdownHandle, ShutdownPolicy, wait_for_signal};
pub use worker_pool::{PoolError, PoolStats, WorkerPool};
