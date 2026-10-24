//! Lifecycle-event emission shared by the pools (cr-31).
//!
//! Each pool owns a [`HookBus`] and emits through an [`Events`] view of it.
//! Every method builds its event inside [`HookBus::emit_with`], so a pool
//! without hooks pays one atomic load and a branch per transition and
//! allocates nothing. Which transition emits what, per pool, is in the pool's
//! module docs; the shared contract is in `prometheus_parking_lot_core::hooks`.

use std::time::{Duration, Instant};

use prometheus_parking_lot_core::hooks::{CancelledReason, RejectionReason};
use prometheus_parking_lot_core::{
    HookBus, LifecycleEvent, SchedulerError, TaskError, TaskId, TaskMetadata, now_ms,
};

use crate::cancellation::CancelReason;

/// A pool's bus and name, for emitting.
#[derive(Clone, Copy)]
pub struct Events<'a> {
    pub bus: &'a HookBus,
    pub pool: &'a str,
}

impl Events<'_> {
    /// Start timing a run, only when someone is listening.
    pub fn clock(&self) -> Option<Instant> {
        self.bus.is_active().then(Instant::now)
    }

    pub fn submitted(&self, meta: &TaskMetadata) {
        self.bus.emit_with(|| LifecycleEvent::Submitted {
            task_id: meta.id,
            idempotency_key: meta.idempotency_key.clone(),
            pool: self.pool.to_owned(),
            at_ms: now_ms(),
        });
    }

    pub fn queued(&self, task_id: TaskId, queue_depth: usize) {
        self.bus.emit_with(|| LifecycleEvent::Queued {
            task_id,
            pool: self.pool.to_owned(),
            queue_depth,
            at_ms: now_ms(),
        });
    }

    pub fn started(&self, task_id: TaskId, worker_id: usize) {
        self.bus.emit_with(|| LifecycleEvent::Started {
            task_id,
            pool: self.pool.to_owned(),
            worker_id,
            at_ms: now_ms(),
        });
    }

    /// `Completed` for a value, `Failed` for an error.
    pub fn finished<T>(
        &self,
        task_id: TaskId,
        outcome: &Result<T, TaskError>,
        started: Option<Instant>,
    ) {
        match outcome {
            Ok(_) => self.completed(task_id, started),
            Err(error) => self.failed(task_id, error),
        }
    }

    pub fn completed(&self, task_id: TaskId, started: Option<Instant>) {
        self.bus.emit_with(|| LifecycleEvent::Completed {
            task_id,
            pool: self.pool.to_owned(),
            duration_ms: started.map_or(0, |at| millis(at.elapsed())),
            at_ms: now_ms(),
        });
    }

    pub fn failed(&self, task_id: TaskId, error: &TaskError) {
        self.bus.emit_with(|| LifecycleEvent::Failed {
            task_id,
            pool: self.pool.to_owned(),
            error: error.to_string(),
            at_ms: now_ms(),
        });
    }

    pub fn rejected(&self, task_id: TaskId, error: &SchedulerError) {
        self.bus.emit_with(|| LifecycleEvent::Rejected {
            task_id,
            pool: self.pool.to_owned(),
            reason: rejection_reason(error),
            at_ms: now_ms(),
        });
    }

    pub fn expired(&self, task_id: TaskId) {
        self.bus.emit_with(|| LifecycleEvent::Expired {
            task_id,
            pool: self.pool.to_owned(),
            at_ms: now_ms(),
        });
    }

    /// The terminal event of a task cancelled at shutdown.
    ///
    /// Negative control S-21: under
    /// `--cfg parking_lot_negative_control="s-21"` it is suppressed, so the
    /// `lifecycle_events_` ordering tests must find cancelled tasks without
    /// a terminal event.
    pub fn cancelled(&self, meta: &TaskMetadata, reason: &CancelReason, attempt: u32) {
        #[cfg(parking_lot_negative_control = "s-21")]
        {
            let _ = (meta, reason, attempt);
        }
        #[cfg(not(parking_lot_negative_control = "s-21"))]
        self.bus.emit_with(|| LifecycleEvent::Cancelled {
            task_id: meta.id,
            idempotency_key: meta.idempotency_key.clone(),
            pool: self.pool.to_owned(),
            reason: cancelled_reason(reason),
            attempt,
            at_ms: now_ms(),
        });
    }

    /// The terminal event of a durable run handed back to its queue.
    ///
    /// Negative control S-21: suppressed under the same cfg as
    /// [`cancelled`](Self::cancelled), so the durable `lifecycle_events_`
    /// tests must find a requeued run without its terminal event.
    pub fn requeued(&self, task_id: TaskId, idempotency_key: Option<&str>, attempt: u32) {
        #[cfg(parking_lot_negative_control = "s-21")]
        {
            let _ = (task_id, idempotency_key, attempt);
        }
        #[cfg(not(parking_lot_negative_control = "s-21"))]
        self.bus.emit_with(|| LifecycleEvent::Requeued {
            task_id,
            idempotency_key: idempotency_key.map(str::to_owned),
            pool: self.pool.to_owned(),
            attempt,
            at_ms: now_ms(),
        });
    }
}

fn millis(elapsed: Duration) -> u64 {
    u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
}

/// The event's reason for a refused submission.
fn rejection_reason(error: &SchedulerError) -> RejectionReason {
    match error {
        SchedulerError::QueueFull { .. } => RejectionReason::QueueFull,
        SchedulerError::DeadlineExpired { .. } => RejectionReason::DeadlineExpired,
        SchedulerError::PoolShutdown { .. } => RejectionReason::PoolShutdown,
        SchedulerError::CapacityExceeded { kind, .. } => {
            RejectionReason::CapacityExceeded { kind: *kind }
        }
        SchedulerError::NotStored { .. } => RejectionReason::NotStored(error.to_string()),
        other => RejectionReason::Custom(other.to_string()),
    }
}

/// The event's reason for a shutdown cancellation.
const fn cancelled_reason(reason: &CancelReason) -> CancelledReason {
    match reason {
        CancelReason::QueueShutdown => CancelledReason::QueueShutdown,
        CancelReason::DrainTimeout => CancelledReason::DrainTimeout,
        CancelReason::DeadlineExpired => CancelledReason::DeadlineExpired,
        CancelReason::SignalCancelled => CancelledReason::SignalCancelled,
    }
}
