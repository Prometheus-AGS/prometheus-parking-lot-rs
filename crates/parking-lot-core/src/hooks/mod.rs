//! Lifecycle hooks for pool events.
//!
//! ## Overview
//!
//! Pools emit [`LifecycleEvent`]s at every transition of a task (cr-31). Any
//! code wanting to observe those events (metrics, logging, distributed
//! tracing, audit, alerting) implements the [`Hook`] trait and registers with
//! a pool's [`HookBus`].
//!
//! Hooks are **fire-and-observe**: the bus calls each registered hook and
//! discards the return value. Hooks cannot modify pool state.
//!
//! ## Event sequence
//!
//! Every task a pool sees emits, in this order:
//!
//! 1. [`Submitted`](LifecycleEvent::Submitted) when `submit` is called;
//! 2. [`Queued`](LifecycleEvent::Queued) if it parks waiting for capacity
//!    (emitted under the dispatch lock, so before anything can start it);
//! 3. [`Started`](LifecycleEvent::Started) when the executor is about to run
//!    it;
//! 4. exactly one terminal event ([`LifecycleEvent::is_terminal`]):
//!    `Completed`, `Failed`, `Rejected` (refused at submission; or, by a
//!    durable pool, a leased task too large for every consumer that leased
//!    it, once its attempts reach the pool's oversize attempt limit: it is
//!    then settled as `Dropped` and dead-lettered), `Cancelled` (shutdown),
//!    `Expired` (deadline passed before it started) or `Requeued` (a durable
//!    pool handed it back to its queue, including a task too large for that
//!    pool below the limit; another run follows, in this or a later process).
//!
//! `Queued` and `Started` are optional (a task rejected at submission has
//! neither; one cancelled while parked has no `Started`). A durable pool's
//! task redelivered from its queue starts at `Queued`/`Started`: its
//! `Submitted` was emitted by the process that accepted it.
//!
//! Exceptions to "exactly one terminal event":
//!
//! - A parked task removed by `ResourcePool::prune_expired`
//!   (`Dispatcher::prune_expired`) emits *no* terminal event, because the
//!   `TaskQueue` contract does not return the ids of the tasks it prunes.
//!   Changing that contract is tracked for 0.4.0 in issue #57. A parked task
//!   whose deadline passes and that the drain finds at the head is reported
//!   as `Expired` as usual.
//! - A durable pool's `submit` whose enqueue fails with an *unknown* outcome
//!   (`SchedulerError::Backend`, for example a connection lost during the
//!   commit) emits `Submitted` and no terminal event: the task may have been
//!   stored and may run, here or in another process, and that run emits its
//!   own events. An enqueue that certainly stored nothing
//!   (`SchedulerError::NotStored`, a full queue) is `Rejected`.
//! - A durable pool's task can run more than once (a redelivery after a
//!   crash or an expired lease, a hand-back), so for it the rule holds *per
//!   run*, not per task: each run ends in exactly one terminal event, every
//!   run but the last ends in `Requeued`, and only `Requeued` is followed by
//!   further events (the next run's). Runs in other processes emit on their
//!   own buses, so one bus can see a task's `Submitted` without its final
//!   terminal event, or a run without its `Submitted`.
//!
//! ## Delivery and zero cost when empty
//!
//! See [`bus`]: events are queued without blocking and delivered in order on
//! the bus's own thread, so a slow or panicking hook never stalls dispatch.
//! With no hook registered, emission is one atomic load and a branch; no
//! event is built.
//!
//! **Not on `wasm32`.** That target has no threads, so the bus calls every
//! hook inline, on the emitting call: a slow hook stalls the pool, `Queued`
//! is delivered while the pool holds its dispatch lock (a hook must not call
//! back into the pool there), and since `wasm32-unknown-unknown` builds abort
//! on panic, a panicking hook aborts the module rather than being contained.
//!
//! ## `AuditSink` backward-compatibility
//!
//! The existing [`AuditSink`] trait is kept for compatibility. Use
//! [`AuditHookAdapter`] to register any `AuditSink` implementation as a
//! `Hook`.
//!
//! ## Example
//!
//! ```rust,ignore
//! use prometheus_parking_lot::core::hooks::{Hook, HookBus, LifecycleEvent};
//!
//! struct MetricsHook;
//! impl Hook for MetricsHook {
//!     fn on_event(&self, event: &LifecycleEvent) {
//!         match event {
//!             LifecycleEvent::Completed { task_id, .. } => {
//!                 metrics::increment_counter!("tasks_completed", "id" => task_id.to_string());
//!             }
//!             LifecycleEvent::Rejected { .. } | LifecycleEvent::Cancelled { .. } => {
//!                 metrics::increment_counter!("tasks_dropped");
//!             }
//!             _ => {}
//!         }
//!     }
//! }
//!
//! let bus = HookBus::new();
//! bus.register(std::sync::Arc::new(MetricsHook));
//! ```

use crate::contracts::ResourceKind;
use crate::contracts::clock::now_ms;

pub mod audit;
pub mod bus;

pub use bus::{DEFAULT_HOOK_QUEUE_CAPACITY, HookBus};

/// Reason a task was rejected before it could start.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RejectionReason {
    /// The queue has reached its maximum depth.
    QueueFull,
    /// The task's deadline had already passed.
    DeadlineExpired,
    /// The pool has been shut down.
    PoolShutdown,
    /// The task needs more units than the pool's limit in some dimension, so
    /// it could never start: the pool's total units (`kind` is `None`), or
    /// the limit of the resource kind `kind` (cr-23). The dimension is the
    /// one `SchedulerError::CapacityExceeded` names.
    CapacityExceeded {
        /// The limited kind the task exceeded, or `None` for the pool's
        /// total.
        kind: Option<ResourceKind>,
    },
    /// A custom reason provided by a policy implementation.
    Custom(String),
    /// The durable queue's backend failed before it stored anything
    /// (`SchedulerError::NotStored`: the task could not be serialised, no
    /// connection or transaction could be opened, a statement failed before
    /// the commit). Holds the error's text. Nothing was accepted, so
    /// resubmitting cannot duplicate the task.
    NotStored(String),
}

/// Reason a running task was cancelled before completion.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CancelledReason {
    /// The pool's drain deadline elapsed during shutdown.
    DrainTimeout,
    /// The task's per-task deadline expired while it was running.
    DeadlineExpired,
    /// An OS signal (Ctrl-C / SIGTERM) triggered immediate cancellation.
    SignalCancelled,
    /// The pool was shut down while this task was in the queue.
    QueueShutdown,
}

/// Events emitted by pools throughout the task lifecycle.
///
/// This enum is `#[non_exhaustive]` — match with a `_ => {}` arm.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum LifecycleEvent {
    /// A task was submitted to the pool.
    Submitted {
        /// Internal task ID.
        task_id: u64,
        /// Caller's idempotency key, if set.
        idempotency_key: Option<String>,
        /// Pool name.
        pool: String,
        /// Timestamp (ms since epoch).
        at_ms: u128,
    },

    /// A task was parked in the queue (capacity not immediately available).
    Queued {
        /// Internal task ID.
        task_id: u64,
        /// Pool name.
        pool: String,
        /// Queue depth at enqueueing.
        queue_depth: usize,
        /// Timestamp.
        at_ms: u128,
    },

    /// A task started executing on a worker.
    Started {
        /// Internal task ID.
        task_id: u64,
        /// Pool name.
        pool: String,
        /// Worker index (the native `WorkerPool`'s worker thread; `0` for
        /// pools that run tasks on an async runtime).
        worker_id: usize,
        /// Timestamp.
        at_ms: u128,
    },

    /// A task completed successfully.
    Completed {
        /// Internal task ID.
        task_id: u64,
        /// Pool name.
        pool: String,
        /// Wall-clock execution time in milliseconds.
        duration_ms: u64,
        /// Timestamp.
        at_ms: u128,
    },

    /// A task failed with an error.
    Failed {
        /// Internal task ID.
        task_id: u64,
        /// Pool name.
        pool: String,
        /// Human-readable error description.
        error: String,
        /// Timestamp.
        at_ms: u128,
    },

    /// A task was rejected before it could be queued or started.
    Rejected {
        /// Internal task ID.
        task_id: u64,
        /// Pool name.
        pool: String,
        /// Why the task was rejected.
        reason: RejectionReason,
        /// Timestamp.
        at_ms: u128,
    },

    /// A running task was cancelled (e.g. during shutdown).
    ///
    /// Cancelled tasks are handed to the pool's `CancellationSink` for
    /// potential replay; this event is for observability only.
    Cancelled {
        /// Internal task ID.
        task_id: u64,
        /// Caller's idempotency key, if set (needed for replay matching).
        idempotency_key: Option<String>,
        /// Pool name.
        pool: String,
        /// Why the task was cancelled.
        reason: CancelledReason,
        /// Number of prior attempts (0 = first attempt).
        attempt: u32,
        /// Timestamp.
        at_ms: u128,
    },

    /// A queued task was evicted by an explicit eviction. Not emitted by the
    /// built-in pools, which report a passed deadline as
    /// [`Expired`](Self::Expired).
    Evicted {
        /// Internal task ID.
        task_id: u64,
        /// Pool name.
        pool: String,
        /// Timestamp.
        at_ms: u128,
    },

    /// A task's deadline passed before it could start; it never ran (its
    /// outcome is `TaskStatus::Expired`).
    Expired {
        /// Internal task ID.
        task_id: u64,
        /// Pool name.
        pool: String,
        /// Timestamp.
        at_ms: u128,
    },

    /// A durable pool handed a task back to its queue without an outcome
    /// (shutdown, a cost too large for this pool that another consumer may
    /// fit, a delivery that kept failing, or a panic outside the executor):
    /// it runs again on a later lease. Terminal for this run.
    Requeued {
        /// Internal task ID.
        task_id: u64,
        /// The idempotency key the task's outcome is delivered under (the
        /// caller's key, or one derived from the queue entry).
        idempotency_key: Option<String>,
        /// Pool name.
        pool: String,
        /// Number of prior attempts, as the queue reported at lease time.
        attempt: u32,
        /// Timestamp.
        at_ms: u128,
    },
}

impl LifecycleEvent {
    /// Convenience: event kind as a static string (useful for metrics labels).
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Submitted { .. } => "submitted",
            Self::Queued { .. } => "queued",
            Self::Started { .. } => "started",
            Self::Completed { .. } => "completed",
            Self::Failed { .. } => "failed",
            Self::Rejected { .. } => "rejected",
            Self::Cancelled { .. } => "cancelled",
            Self::Evicted { .. } => "evicted",
            Self::Expired { .. } => "expired",
            Self::Requeued { .. } => "requeued",
        }
    }

    /// Whether this event ends the task's run: `Completed`, `Failed`,
    /// `Rejected`, `Cancelled`, `Expired`, `Evicted` or `Requeued`.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        !matches!(
            self,
            Self::Submitted { .. } | Self::Queued { .. } | Self::Started { .. }
        )
    }

    /// Task ID of the event, for correlation.
    #[must_use]
    pub const fn task_id(&self) -> u64 {
        match self {
            Self::Submitted { task_id, .. }
            | Self::Queued { task_id, .. }
            | Self::Started { task_id, .. }
            | Self::Completed { task_id, .. }
            | Self::Failed { task_id, .. }
            | Self::Rejected { task_id, .. }
            | Self::Cancelled { task_id, .. }
            | Self::Evicted { task_id, .. }
            | Self::Expired { task_id, .. }
            | Self::Requeued { task_id, .. } => *task_id,
        }
    }

    /// Name of the pool that emitted the event.
    #[must_use]
    pub fn pool(&self) -> &str {
        match self {
            Self::Submitted { pool, .. }
            | Self::Queued { pool, .. }
            | Self::Started { pool, .. }
            | Self::Completed { pool, .. }
            | Self::Failed { pool, .. }
            | Self::Rejected { pool, .. }
            | Self::Cancelled { pool, .. }
            | Self::Evicted { pool, .. }
            | Self::Expired { pool, .. }
            | Self::Requeued { pool, .. } => pool,
        }
    }
}

/// A lifecycle observer. Implement this to react to pool events.
///
/// `on_event` runs on the [`HookBus`]'s delivery thread, never on a pool's
/// scheduling path, one event at a time in emission order. It may block (a
/// slow hook makes the bus drop events once its queue is full, never stalls
/// the pool) and a panic is contained and counted.
///
/// On `wasm32` none of that holds: there is no delivery thread, so
/// `on_event` runs inline on the pool's scheduling path (for `Queued`, under
/// its dispatch lock, so it must not call back into the pool), a slow hook
/// stalls the pool, and a panic aborts the module. See the [`bus`] docs.
pub trait Hook: Send + Sync {
    /// Called for each emitted [`LifecycleEvent`], in emission order.
    fn on_event(&self, event: &LifecycleEvent);
}

// --- AuditSink backward-compatibility bridge ---

use crate::hooks::audit::{AuditSink, build_audit_event};

/// Wraps a boxed [`AuditSink`] as a [`Hook`] for backward compatibility.
///
/// Any pool still using `AuditSink` can be migrated to the hooks API by wrapping:
///
/// ```rust,ignore
/// let adapter = AuditHookAdapter::new(Box::new(my_audit_sink));
/// bus.register(Arc::new(adapter));
/// ```
pub struct AuditHookAdapter {
    sink: parking_lot::Mutex<Box<dyn AuditSink>>,
    pool: String,
}

impl AuditHookAdapter {
    /// Create an adapter wrapping an `AuditSink`.
    pub fn new(sink: Box<dyn AuditSink>, pool: impl Into<String>) -> Self {
        Self {
            sink: parking_lot::Mutex::new(sink),
            pool: pool.into(),
        }
    }
}

impl Hook for AuditHookAdapter {
    fn on_event(&self, event: &LifecycleEvent) {
        let action = event.kind();
        let event_id = format!("{}-{}", action, event.task_id());
        let audit = build_audit_event(
            event_id,
            event.task_id().to_string(),
            &self.pool,
            "system",
            action,
            None,
        );
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.sink.lock().record(audit);
        }));
    }
}

// --- Built-in hooks ---

/// A hook that logs lifecycle events at DEBUG/INFO/WARN level using `tracing`.
pub struct TracingHook {
    pool: String,
}

impl TracingHook {
    /// Create a tracing hook for the named pool.
    pub fn new(pool: impl Into<String>) -> Self {
        Self { pool: pool.into() }
    }
}

impl Hook for TracingHook {
    fn on_event(&self, event: &LifecycleEvent) {
        match event {
            LifecycleEvent::Submitted { task_id, .. } => {
                tracing::debug!(pool = %self.pool, task_id, "task submitted");
            }
            LifecycleEvent::Started {
                task_id, worker_id, ..
            } => {
                tracing::debug!(pool = %self.pool, task_id, worker_id, "task started");
            }
            LifecycleEvent::Completed {
                task_id,
                duration_ms,
                ..
            } => {
                tracing::info!(pool = %self.pool, task_id, duration_ms, "task completed");
            }
            LifecycleEvent::Failed { task_id, error, .. } => {
                tracing::warn!(pool = %self.pool, task_id, %error, "task failed");
            }
            LifecycleEvent::Rejected {
                task_id, reason, ..
            } => {
                tracing::warn!(pool = %self.pool, task_id, ?reason, "task rejected");
            }
            LifecycleEvent::Cancelled {
                task_id,
                reason,
                attempt,
                ..
            } => {
                tracing::warn!(pool = %self.pool, task_id, ?reason, attempt, "task cancelled");
            }
            LifecycleEvent::Expired { task_id, .. } => {
                tracing::warn!(pool = %self.pool, task_id, "task expired before it started");
            }
            LifecycleEvent::Requeued {
                task_id, attempt, ..
            } => {
                tracing::info!(pool = %self.pool, task_id, attempt, "task requeued");
            }
            _ => {}
        }
    }
}

/// A hook that records events into an in-memory `VecDeque` — useful for tests.
pub struct RecordingHook {
    events: parking_lot::Mutex<std::collections::VecDeque<LifecycleEvent>>,
    max_events: usize,
}

impl RecordingHook {
    /// Create a recording hook with a bounded buffer.
    #[must_use]
    pub fn new(max_events: usize) -> Self {
        Self {
            events: parking_lot::Mutex::new(std::collections::VecDeque::with_capacity(max_events)),
            max_events,
        }
    }

    /// Drain all recorded events.
    pub fn drain(&self) -> Vec<LifecycleEvent> {
        self.events.lock().drain(..).collect()
    }

    /// Count of events recorded so far.
    pub fn len(&self) -> usize {
        self.events.lock().len()
    }

    /// True if no events have been recorded.
    pub fn is_empty(&self) -> bool {
        self.events.lock().is_empty()
    }
}

impl Hook for RecordingHook {
    fn on_event(&self, event: &LifecycleEvent) {
        let mut guard = self.events.lock();
        if guard.len() >= self.max_events {
            guard.pop_front();
        }
        guard.push_back(event.clone());
    }
}

/// Convenience: emit a `Submitted` event from pool metadata.
pub fn emit_submitted(
    bus: &HookBus,
    task_id: u64,
    idempotency_key: Option<String>,
    pool: impl Into<String>,
) {
    bus.emit_with(|| LifecycleEvent::Submitted {
        task_id,
        idempotency_key,
        pool: pool.into(),
        at_ms: now_ms(),
    });
}

/// Convenience: emit a `Rejected` event.
pub fn emit_rejected(
    bus: &HookBus,
    task_id: u64,
    pool: impl Into<String>,
    reason: RejectionReason,
) {
    bus.emit_with(|| LifecycleEvent::Rejected {
        task_id,
        pool: pool.into(),
        reason,
        at_ms: now_ms(),
    });
}

/// Convenience: emit a `Completed` event.
pub fn emit_completed(bus: &HookBus, task_id: u64, pool: impl Into<String>, duration_ms: u64) {
    bus.emit_with(|| LifecycleEvent::Completed {
        task_id,
        pool: pool.into(),
        duration_ms,
        at_ms: now_ms(),
    });
}

/// Convenience: emit a `Cancelled` event.
pub fn emit_cancelled(
    bus: &HookBus,
    task_id: u64,
    idempotency_key: Option<String>,
    pool: impl Into<String>,
    reason: CancelledReason,
    attempt: u32,
) {
    bus.emit_with(|| LifecycleEvent::Cancelled {
        task_id,
        idempotency_key,
        pool: pool.into(),
        reason,
        attempt,
        at_ms: now_ms(),
    });
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn empty_bus_emit_is_noop() {
        let bus = HookBus::new();
        // Must not panic
        bus.emit(&LifecycleEvent::Submitted {
            task_id: 1,
            idempotency_key: None,
            pool: "test".into(),
            at_ms: 0,
        });
        assert!(!bus.is_active());
    }

    #[test]
    fn recording_hook_captures_events() {
        let hook = Arc::new(RecordingHook::new(100));
        let bus = HookBus::new();
        bus.register(Arc::clone(&hook) as Arc<dyn Hook>);
        assert!(bus.is_active());

        bus.emit(&LifecycleEvent::Submitted {
            task_id: 42,
            idempotency_key: Some("key-1".into()),
            pool: "pool".into(),
            at_ms: 0,
        });
        bus.emit(&LifecycleEvent::Completed {
            task_id: 42,
            pool: "pool".into(),
            duration_ms: 50,
            at_ms: 1,
        });
        assert!(bus.flush(std::time::Duration::from_secs(5)));

        let events = hook.drain();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind(), "submitted");
        assert_eq!(events[1].kind(), "completed");
        assert_eq!(events[0].task_id(), 42);
    }

    #[test]
    fn lifecycle_event_kind_and_task_id() {
        let e = LifecycleEvent::Rejected {
            task_id: 7,
            pool: "p".into(),
            reason: RejectionReason::QueueFull,
            at_ms: 0,
        };
        assert_eq!(e.kind(), "rejected");
        assert_eq!(e.task_id(), 7);
    }

    #[test]
    fn multiple_hooks_all_called() {
        let h1 = Arc::new(RecordingHook::new(10));
        let h2 = Arc::new(RecordingHook::new(10));
        let bus = HookBus::new();
        bus.register(Arc::clone(&h1) as Arc<dyn Hook>);
        bus.register(Arc::clone(&h2) as Arc<dyn Hook>);

        bus.emit(&LifecycleEvent::Evicted {
            task_id: 3,
            pool: "p".into(),
            at_ms: 0,
        });
        assert!(bus.flush(std::time::Duration::from_secs(5)));
        assert_eq!(h1.len(), 1);
        assert_eq!(h2.len(), 1);
    }
}
