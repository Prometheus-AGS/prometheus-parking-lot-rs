//! # Prometheus Parking Lot - core
//!
//! The runtime-free core of the `prometheus_parking_lot` scheduler: scheduling
//! contracts (task queue, mailbox, spawner, executors, task metadata, errors),
//! lifecycle hooks and audit types, the in-memory queue and mailbox, and the
//! worker pool configuration.
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

pub use contracts::{
    AppResult, Clock, InMemoryQueue, Mailbox, MailboxKey, PoolLimits, PoolStats, Priority,
    ResourceCost, ResourceKind, ScheduledTask, SchedulerError, Spawn, SubmissionSequence,
    SystemClock, TaskError, TaskExecutor, TaskId, TaskMetadata, TaskPayload, TaskQueue, TaskStatus,
    TestClock, WorkerExecutor, now_ms,
};
pub use hooks::audit::{AuditEvent, AuditSink, InMemoryAuditSink, build_audit_event};
pub use hooks::{
    AuditHookAdapter, CancelledReason, Hook, HookBus, LifecycleEvent, RecordingHook,
    RejectionReason, TracingHook, emit_cancelled, emit_completed, emit_rejected, emit_submitted,
};
pub use mailbox::InMemoryMailbox;
