//! # Prometheus Parking Lot - core
//!
//! The runtime-free core of the `prometheus_parking_lot` scheduler: scheduling
//! contracts (the async lease queue and result mailbox backends implement, the
//! in-process queue and mailbox, spawner, executors, task metadata, errors),
//! lifecycle hooks and audit types, in-memory reference backends, and the
//! worker pool configuration. With the `conformance` feature it also publishes
//! the backend conformance suite (`conformance`, `lease_queue_conformance!`,
//! `result_mailbox_conformance!`; ADR 0003).
//!
//! This crate performs no IO and depends on no async runtime, database or
//! network crate, so it builds for every target the scheduler supports. Runtime
//! adapters and storage backends live in sibling crates; most applications use
//! the `prometheus_parking_lot` facade, which re-exports everything.
//!
//! See `docs/adr/0001-workspace-split.md` for the crate map and dependency rules.

pub mod config;
pub mod contracts;
pub mod dispatch;
pub mod hooks;
pub mod mailbox;
pub mod semaphore;

#[cfg(any(test, feature = "conformance"))]
pub use contracts::conformance;
pub use contracts::{
    AppResult, Clock, DeadLetter, Delivery, DeliveryOutcome, Enqueued, InMemoryLeaseQueue,
    InMemoryQueue, InMemoryResultMailbox, LeaseQueue, LeaseToken, LeasedTask, LegacyDeliveryStatus,
    LegacyIdempotencyOwner, Mailbox, MailboxEntry, MailboxKey, NackAction, NoopNotifier, Notifier,
    PoolLimits, PoolStats, Priority, QueueDepth, QueueEntryId, ResourceCost, ResourceKind,
    ResultMailbox, ScheduledTask, SchedulerError, Spawn, StorageMailbox, SubmissionSequence,
    SystemClock, TaskError, TaskExecutor, TaskId, TaskMetadata, TaskPayload, TaskQueue, TaskStatus,
    TestClock, WorkerExecutor, now_ms,
};
pub use hooks::audit::{AuditEvent, AuditSink, InMemoryAuditSink, build_audit_event};
pub use hooks::{
    AuditHookAdapter, CancelledReason, Hook, HookBus, LifecycleEvent, RecordingHook,
    RejectionReason, TracingHook, emit_cancelled, emit_completed, emit_rejected, emit_submitted,
};
pub use mailbox::InMemoryMailbox;
