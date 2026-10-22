//! Lifecycle hooks for pool events.
//!
//! ## Overview
//!
//! Pools emit [`LifecycleEvent`]s at key points in task processing. Any code
//! wanting to observe those events (metrics, logging, distributed tracing,
//! audit, alerting) implements the [`Hook`] trait and registers with a pool's
//! [`HookBus`].
//!
//! Hooks are **fire-and-observe**: the bus calls each registered hook and
//! discards the return value. Hooks must not modify pool state and should
//! complete quickly; heavy I/O should be offloaded to background tasks inside
//! the hook implementation.
//!
//! ## Zero-cost when empty
//!
//! `HookBus::emit` is a no-op when no hooks are registered. The check is a
//! single slice-empty test — no allocation, no virtual dispatch.
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
//! let mut bus = HookBus::new();
//! bus.register(std::sync::Arc::new(MetricsHook));
//! ```

use std::sync::Arc;

use crate::contracts::clock::now_ms;

pub mod audit;

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
    /// A custom reason provided by a policy implementation.
    Custom(String),
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

    /// A task was placed in the queue (capacity not immediately available).
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
        /// Worker index.
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

    /// A queued task was evicted (deadline expired or explicit eviction).
    Evicted {
        /// Internal task ID.
        task_id: u64,
        /// Pool name.
        pool: String,
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
        }
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
            | Self::Evicted { task_id, .. } => *task_id,
        }
    }
}

/// A lifecycle observer. Implement this to react to pool events.
///
/// Implementations must be `Send + Sync` because the bus may be shared across
/// threads. `on_event` is called synchronously; offload I/O to background tasks
/// if needed, and return quickly.
pub trait Hook: Send + Sync {
    /// Called synchronously for each emitted [`LifecycleEvent`].
    fn on_event(&self, event: &LifecycleEvent);
}

/// A registry of [`Hook`] observers for a single pool.
///
/// Hooks are stored as `Arc<dyn Hook>` so they can be cheaply shared and
/// registered on multiple buses. Emission is a no-op when empty.
#[derive(Default)]
pub struct HookBus {
    hooks: Vec<Arc<dyn Hook>>,
}

impl HookBus {
    /// Create an empty bus.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a hook. All registered hooks receive every subsequent event.
    pub fn register(&mut self, hook: Arc<dyn Hook>) {
        self.hooks.push(hook);
    }

    /// Emit an event to all registered hooks.
    ///
    /// No-op (zero overhead) when no hooks are registered.
    pub fn emit(&self, event: &LifecycleEvent) {
        for hook in &self.hooks {
            hook.on_event(event);
        }
    }

    /// Returns `true` if at least one hook is registered.
    #[must_use]
    pub fn is_active(&self) -> bool {
        !self.hooks.is_empty()
    }
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
    if bus.is_active() {
        bus.emit(&LifecycleEvent::Submitted {
            task_id,
            idempotency_key,
            pool: pool.into(),
            at_ms: now_ms(),
        });
    }
}

/// Convenience: emit a `Rejected` event.
pub fn emit_rejected(
    bus: &HookBus,
    task_id: u64,
    pool: impl Into<String>,
    reason: RejectionReason,
) {
    if bus.is_active() {
        bus.emit(&LifecycleEvent::Rejected {
            task_id,
            pool: pool.into(),
            reason,
            at_ms: now_ms(),
        });
    }
}

/// Convenience: emit a `Completed` event.
pub fn emit_completed(bus: &HookBus, task_id: u64, pool: impl Into<String>, duration_ms: u64) {
    if bus.is_active() {
        bus.emit(&LifecycleEvent::Completed {
            task_id,
            pool: pool.into(),
            duration_ms,
            at_ms: now_ms(),
        });
    }
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
    if bus.is_active() {
        bus.emit(&LifecycleEvent::Cancelled {
            task_id,
            idempotency_key,
            pool: pool.into(),
            reason,
            attempt,
            at_ms: now_ms(),
        });
    }
}

#[cfg(test)]
mod tests {
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
        let mut bus = HookBus::new();
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
        let mut bus = HookBus::new();
        bus.register(Arc::clone(&h1) as Arc<dyn Hook>);
        bus.register(Arc::clone(&h2) as Arc<dyn Hook>);

        bus.emit(&LifecycleEvent::Evicted {
            task_id: 3,
            pool: "p".into(),
            at_ms: 0,
        });
        assert_eq!(h1.len(), 1);
        assert_eq!(h2.len(), 1);
    }
}
