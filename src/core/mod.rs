//! Core scheduling abstractions and capacity accounting.
//!
//! Since the workspace split (ADR 0001) this module re-exports the
//! runtime-free contracts from `prometheus-parking-lot-core` and, behind the
//! default `tokio-runtime` feature, the pools and runtime pieces from
//! `prometheus-parking-lot-runtime-tokio`, under the paths they had before.

/// Audit sink contract and implementations.
pub mod audit {
    pub use prometheus_parking_lot_core::hooks::audit::*;
    #[cfg(feature = "postgres")]
    pub use prometheus_parking_lot_pg::audit::PostgresAuditSink;
}

pub use prometheus_parking_lot_core::contracts::{clock, error, executor};
pub use prometheus_parking_lot_core::hooks;
#[cfg(feature = "tokio-runtime")]
pub use prometheus_parking_lot_runtime_tokio::cancellation;
#[cfg(feature = "tokio-runtime")]
pub use prometheus_parking_lot_runtime_tokio::shutdown;

/// Scheduling contracts (queue, mailbox, spawner, task types) and the async
/// `ResourcePool`.
pub mod resource_pool {
    pub use prometheus_parking_lot_core::contracts::{
        Mailbox, PoolLimits, ScheduledTask, Spawn, SubmissionSequence, TaskMetadata, TaskQueue,
        TaskStatus,
    };
    #[cfg(feature = "tokio-runtime")]
    pub use prometheus_parking_lot_runtime_tokio::resource_pool::ResourcePool;
}

/// Async backend contracts (ADR 0003): the leasing task queue, the idempotent
/// result mailbox, notifiers and the storage-plus-notifier composition that
/// durable backends implement.
///
/// [`BackgroundNotifier`] runs a [`Notifier`] off the delivery path with
/// bounded retries ([`RetryPolicy`], waiting through a [`Sleeper`]); it is the
/// [`DetachedNotifier`] a [`StorageMailbox`] takes (cr-34). For example,
/// `StorageMailbox::new(storage, BackgroundNotifier::new(notifier, spawner, sleeper))`
/// with `infra::events::IggyNotifier` (feature `iggy`).
pub mod backend {
    pub use prometheus_parking_lot_core::contracts::{
        Delivery, DeliveryOutcome, Enqueued, LeaseQueue, LeaseToken, LeasedTask, MailboxEntry,
        NackAction, NoopNotifier, Notifier, QueueDepth, QueueEntryId, ResultMailbox,
        StorageMailbox,
    };
    pub use prometheus_parking_lot_core::mailbox::{
        BackgroundNotifier, DetachedNotifier, NotifyStats, NotifyStatsSnapshot, RetryPolicy,
        Sleeper,
    };
}

/// `WorkerPool`: dedicated worker threads (native) or async tasks (WASM).
pub mod worker_pool {
    pub use prometheus_parking_lot_core::contracts::PoolStats;
    #[cfg(all(feature = "tokio-runtime", not(target_arch = "wasm32")))]
    pub use prometheus_parking_lot_runtime_tokio::worker_pool::WorkerPool;
    #[cfg(target_arch = "wasm32")]
    pub use prometheus_parking_lot_wasm::WorkerPool;
}

#[cfg(feature = "postgres")]
pub use audit::PostgresAuditSink;
pub use audit::{AuditEvent, AuditSink, InMemoryAuditSink, build_audit_event};
pub use backend::{
    BackgroundNotifier, Delivery, DeliveryOutcome, DetachedNotifier, Enqueued, LeaseQueue,
    LeaseToken, LeasedTask, MailboxEntry, NackAction, NoopNotifier, Notifier, NotifyStats,
    NotifyStatsSnapshot, QueueDepth, QueueEntryId, ResultMailbox, RetryPolicy, Sleeper,
    StorageMailbox,
};
#[cfg(feature = "tokio-runtime")]
pub use cancellation::{
    CancelReason, CancellationChannel, CancellationConfig, CancellationSink, CancelledTask,
    LoggingCancellationSink, RecordingCancellationSink, SinkOutcome,
};
pub use clock::{Clock, SystemClock, TestClock};
pub use error::{AppResult, SchedulerError, TaskError};
pub use executor::{TaskExecutor, TaskPayload, WorkerExecutor};
pub use hooks::{
    AuditHookAdapter, CancelledReason, Hook, HookBus, LifecycleEvent, RecordingHook,
    RejectionReason, TracingHook, emit_cancelled, emit_completed, emit_rejected, emit_submitted,
};
#[cfg(feature = "tokio-runtime")]
pub use resource_pool::ResourcePool;
pub use resource_pool::{
    Mailbox, PoolLimits, ScheduledTask, Spawn, SubmissionSequence, TaskMetadata, TaskQueue,
    TaskStatus,
};
#[cfg(feature = "tokio-runtime")]
pub use shutdown::{ShutdownHandle, ShutdownPolicy, ShutdownReport, wait_for_signal};
pub use worker_pool::PoolStats;
#[cfg(any(
    all(feature = "tokio-runtime", not(target_arch = "wasm32")),
    target_arch = "wasm32"
))]
pub use worker_pool::WorkerPool;
