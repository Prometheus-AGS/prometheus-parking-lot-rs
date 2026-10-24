//! Lifecycle-event emission for the WASM pool (cr-53).
//!
//! The same taxonomy and order as the native pools
//! (`prometheus-parking-lot-runtime-tokio`, cr-31): `Submitted`, then `Queued`
//! if the task parks, then `Started`, then exactly one terminal event. Events
//! go through the core [`HookBus`], which on `wasm32` delivers inline on the
//! emitting call (there are no threads). Timestamps come from the pool's
//! [`Clock`], not `std::time`, which panics on this target.

use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;

use prometheus_parking_lot_core::hooks::{CancelledReason, RejectionReason};
use prometheus_parking_lot_core::{
    Clock, HookBus, LifecycleEvent, SchedulerError, TaskError, TaskId, TaskMetadata,
};

/// A pool's bus, name and clock, for emitting.
#[derive(Clone, Copy)]
pub struct Events<'a> {
    pub bus: &'a HookBus,
    pub pool: &'a str,
    pub clock: &'a RwLock<Arc<dyn Clock>>,
}

impl Events<'_> {
    fn at_ms(&self) -> u128 {
        self.clock.read().now_ms()
    }

    /// Start timing a run, only when someone is listening.
    pub fn timer(&self) -> Option<Duration> {
        self.bus.is_active().then(|| self.clock.read().monotonic())
    }

    pub fn submitted(&self, meta: &TaskMetadata) {
        self.bus.emit_with(|| LifecycleEvent::Submitted {
            task_id: meta.id,
            idempotency_key: meta.idempotency_key.clone(),
            pool: self.pool.to_owned(),
            at_ms: self.at_ms(),
        });
    }

    pub fn queued(&self, task_id: TaskId, queue_depth: usize) {
        self.bus.emit_with(|| LifecycleEvent::Queued {
            task_id,
            pool: self.pool.to_owned(),
            queue_depth,
            at_ms: self.at_ms(),
        });
    }

    /// The WASM pool has no worker threads; `worker_id` is always 0.
    pub fn started(&self, task_id: TaskId) {
        self.bus.emit_with(|| LifecycleEvent::Started {
            task_id,
            pool: self.pool.to_owned(),
            worker_id: 0,
            at_ms: self.at_ms(),
        });
    }

    /// `Completed` for a value, `Failed` for an error.
    pub fn finished<T>(
        &self,
        task_id: TaskId,
        outcome: &Result<T, TaskError>,
        started: Option<Duration>,
    ) {
        match outcome {
            Ok(_) => self.completed(task_id, started),
            Err(error) => self.failed(task_id, error),
        }
    }

    fn completed(&self, task_id: TaskId, started: Option<Duration>) {
        self.bus.emit_with(|| LifecycleEvent::Completed {
            task_id,
            pool: self.pool.to_owned(),
            duration_ms: started.map_or(0, |at| {
                let elapsed = self.clock.read().monotonic().saturating_sub(at);
                u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
            }),
            at_ms: self.at_ms(),
        });
    }

    fn failed(&self, task_id: TaskId, error: &TaskError) {
        // OA-13 browser control: cancellation still settles the real result,
        // but dropping its terminal event must break the event-order oracle.
        #[cfg(parking_lot_negative_control = "oa-13-budget-event")]
        if matches!(error, TaskError::Timeout { .. }) {
            return;
        }
        self.bus.emit_with(|| LifecycleEvent::Failed {
            task_id,
            pool: self.pool.to_owned(),
            error: error.to_string(),
            at_ms: self.at_ms(),
        });
    }

    pub fn rejected(&self, task_id: TaskId, error: &SchedulerError) {
        self.bus.emit_with(|| LifecycleEvent::Rejected {
            task_id,
            pool: self.pool.to_owned(),
            reason: rejection_reason(error),
            at_ms: self.at_ms(),
        });
    }

    pub fn expired(&self, task_id: TaskId) {
        self.bus.emit_with(|| LifecycleEvent::Expired {
            task_id,
            pool: self.pool.to_owned(),
            at_ms: self.at_ms(),
        });
    }

    /// The terminal event of a task cancelled at shutdown.
    ///
    /// Negative control S-21: under
    /// `--cfg parking_lot_negative_control="s-21"` it is suppressed, so the
    /// ordering tests must find cancelled tasks without a terminal event.
    pub fn cancelled(&self, meta: &TaskMetadata) {
        #[cfg(parking_lot_negative_control = "s-21")]
        {
            let _ = meta;
        }
        #[cfg(not(parking_lot_negative_control = "s-21"))]
        self.bus.emit_with(|| LifecycleEvent::Cancelled {
            task_id: meta.id,
            idempotency_key: meta.idempotency_key.clone(),
            pool: self.pool.to_owned(),
            reason: CancelledReason::QueueShutdown,
            attempt: 0,
            at_ms: self.at_ms(),
        });
    }
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
        other => RejectionReason::Custom(other.to_string()),
    }
}
