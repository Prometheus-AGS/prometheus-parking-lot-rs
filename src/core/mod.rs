//! Core scheduling abstractions and capacity accounting.

pub mod error;
pub mod resource_pool;
pub mod audit;
pub mod executor;
pub mod hooks;
pub mod cancellation;
pub mod worker_pool;
#[cfg(feature = "tokio-runtime")]
pub mod shutdown;

pub use error::{AppResult, SchedulerError};
pub use resource_pool::{
    Mailbox, PoolLimits, ResourcePool, ScheduledTask, Spawn, TaskMetadata, TaskQueue, TaskStatus,
    WakeState, sync_wake_worker_loop,
};
pub use audit::{AuditEvent, AuditSink, InMemoryAuditSink, PostgresAuditSink, build_audit_event};
pub use cancellation::{
    CancelReason, CancellationChannel, CancellationConfig, CancellationSink, CancelledTask,
    LoggingCancellationSink, RecordingCancellationSink, SinkOutcome,
};
pub use hooks::{
    AuditHookAdapter, CancelledReason, Hook, HookBus, LifecycleEvent, RecordingHook,
    RejectionReason, TracingHook,
    emit_cancelled, emit_completed, emit_rejected, emit_submitted,
};
pub use executor::{TaskExecutor, TaskPayload, WorkerExecutor};
pub use worker_pool::{PoolError, PoolStats, WorkerPool};
#[cfg(feature = "tokio-runtime")]
pub use shutdown::{ShutdownHandle, ShutdownPolicy, wait_for_signal};
