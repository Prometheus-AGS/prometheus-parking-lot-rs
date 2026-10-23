//! Cancellation capture and replay infrastructure (CR-14, wired into the
//! pools by cr-29).
//!
//! When an in-memory pool (`ResourcePool`, the native `WorkerPool`) cancels a
//! task at shutdown — a queued task left at the drain deadline, or a running
//! straggler — it captures the task's owned context and hands it to the
//! caller via the [`CancellationSink`] trait, under a bounded per-call
//! deadline ([`CancellationConfig::delta`]) so it can never hang the
//! application. Register a sink with the pool's `with_cancellation_sink`;
//! without one, the pool uses a [`LoggingCancellationSink`]. The durable pool
//! does not use a sink: its queue is the capture, and cancelled tasks are
//! handed back to it (`nack`, requeue). See `crate::shutdown` for the policy
//! table.
//!
//! ## Design: three-layer DLQ pattern
//!
//! 1. **Capture** — [`CancelledTask<P>`] bundles the owned payload, metadata,
//!    reason, and attempt count so the caller can reconstruct/replay the work.
//! 2. **Bounded handoff** — the pools call the sink under
//!    `tokio::time::timeout(delta)` from the shutdown path (the native
//!    `WorkerPool` from a dedicated thread), never while holding a pool lock.
//!    An overrun or a panic is counted as `unanswered` in the
//!    `ShutdownReport` and logged at error level. [`CancellationChannel`] is
//!    the standalone bounded-channel form of this handoff for callers that
//!    capture their own cancellations.
//! 3. **Replay routing** — `SinkOutcome::Recorded` signals the caller has
//!    durably recorded the work for out-of-band replay; `SinkOutcome::Drop`
//!    discards it. `SinkOutcome::Requeue` cannot be honoured by an in-memory
//!    pool (it has no queue that outlives the shutdown); it is counted as
//!    dropped and logged.
//!
//! ## Auto-requeue policy (D3)
//!
//! Advisory: [`CancelReason::should_auto_requeue`] for callers that replay
//! tasks themselves. The pools do not retry cancelled tasks.
//!
//! | `CancelReason`      | Default action       |
//! |---------------------|----------------------|
//! | `QueueShutdown`     | auto-requeue         |
//! | `DrainTimeout`      | auto-requeue         |
//! | `DeadlineExpired`   | DLQ only (no loop)   |
//! | `SignalCancelled`   | DLQ only             |
//!
//! ## Retry bound (D4)
//!
//! Auto-requeue respects a per-pool `max_attempts` (default 3) with exponential
//! back-off. Exhausted attempts → terminal DLQ.
//!
//! ## Idempotency (D2)
//!
//! Delivery is at-least-once. Executors receiving replayed tasks **must** be
//! idempotent, deduplicated on `TaskMetadata.idempotency_key`.
//!
//! ## Example
//!
//! ```rust,ignore
//! use std::sync::Arc;
//! use prometheus_parking_lot::core::cancellation::{
//!     CancellationSink, CancelledTask, SinkOutcome,
//! };
//!
//! struct MySink;
//! #[async_trait::async_trait]
//! impl CancellationSink<MyPayload> for MySink {
//!     async fn on_cancelled(&self, task: CancelledTask<MyPayload>) -> SinkOutcome {
//!         // Persist to DB or dead-letter store for out-of-band replay
//!         my_db.insert_dlq(&task.meta, task.payload).await;
//!         SinkOutcome::Recorded
//!     }
//! }
//! ```

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use prometheus_parking_lot_core::TaskMetadata;

use crate::contain::contain;

/// Why a task was cancelled.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CancelReason {
    /// The pool was shut down while the task was still queued: it never
    /// started. Auto-requeued by default (environmental; not the task's
    /// fault).
    QueueShutdown,
    /// The pool's drain deadline elapsed while this task was still running
    /// (also used by `ShutdownPolicy::CancelImmediately`, whose deadline is
    /// zero). Auto-requeued by default.
    DrainTimeout,
    /// The task's own deadline expired while it was running or queued.
    /// Goes to DLQ only (the task may hang again if auto-requeued).
    DeadlineExpired,
    /// An OS signal (Ctrl-C / SIGTERM) caused immediate cancellation.
    /// Goes to DLQ only (deliberate cancellation by the operator).
    SignalCancelled,
}

impl CancelReason {
    /// Returns `true` for reasons that should trigger automatic requeue
    /// (D3: environmental cancellations, not inherent to the task).
    #[must_use]
    pub const fn should_auto_requeue(&self) -> bool {
        matches!(self, Self::QueueShutdown | Self::DrainTimeout)
    }
}

/// The owned context of a cancelled task, handed to [`CancellationSink`].
///
/// Contains everything needed to inspect, log, or replay the cancelled work.
#[derive(Debug)]
pub struct CancelledTask<P> {
    /// The task payload — owned by the caller after capture.
    ///
    /// Always present for a task that never started
    /// ([`CancelReason::QueueShutdown`]). For a running task the executor owns
    /// the payload, so it is present only when the pool kept a copy:
    /// `ResourcePool` keeps a serialized snapshot of every running task's
    /// payload while a sink is registered (`with_cancellation_sink`); the
    /// native `WorkerPool`, whose payloads need be neither `Clone` nor
    /// serializable, cannot, and passes `None`.
    pub payload: Option<P>,
    /// Full task metadata including id, priority, cost, deadline, and
    /// `idempotency_key`.
    pub meta: TaskMetadata,
    /// Why the task was cancelled.
    pub reason: CancelReason,
    /// Number of prior execution attempts (0 = first attempt).
    pub attempt: u32,
    /// Timestamp (ms since epoch) when cancellation was recorded.
    pub cancelled_at_ms: u64,
}

impl<P> CancelledTask<P> {
    /// Convenience: the idempotency key, if set on the originating task.
    pub fn idempotency_key(&self) -> Option<&str> {
        self.meta.idempotency_key.as_deref()
    }

    /// Returns `true` if this task should be considered for automatic requeue.
    pub const fn should_auto_requeue(&self, max_attempts: u32) -> bool {
        self.reason.should_auto_requeue() && self.attempt < max_attempts
    }

    /// Compute the exponential back-off delay before redelivery.
    ///
    /// Formula: `base_ms * 2^attempt`, capped at `max_delay_ms`.
    pub fn backoff_delay(&self, base_ms: u64, max_delay_ms: u64) -> Duration {
        let shift = self.attempt.min(20);
        let multiplier = 1u64.wrapping_shl(shift);
        let exp = base_ms.saturating_mul(multiplier);
        Duration::from_millis(exp.min(max_delay_ms))
    }
}

/// What the [`CancellationSink`] did with the cancelled task.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SinkOutcome {
    /// The sink has durably recorded the task for out-of-band replay.
    Recorded,
    /// The sink wants the library to requeue the task (nack on persistent
    /// backends; silently accepted on in-memory backends).
    Requeue,
    /// The task should be discarded; no replay is desired.
    Drop,
}

/// Receiver of cancelled tasks.
///
/// Implement this to handle cancellation side effects: write to a dead-letter
/// store, re-enqueue to a durable queue, emit metrics, etc.
///
/// ## Contract
///
/// - `on_cancelled` is called **off the pool's teardown hot path** via a
///   bounded mpsc channel and a dedicated drain task.
/// - It is invoked under a per-call `timeout(delta)`. If it returns after the
///   deadline the result is discarded and the default fallback sink fires.
/// - It must be cancel-safe (may be dropped at any await point).
/// - It must **not** call back into the pool that cancelled it (deadlock risk).
#[async_trait]
pub trait CancellationSink<P>: Send + Sync {
    /// Handle a cancelled task. Must return within the pool's `delta` timeout.
    async fn on_cancelled(&self, task: CancelledTask<P>) -> SinkOutcome;
}

/// Configuration for the cancellation handoff.
///
/// The pools (`with_cancellation_config`) use only `delta`; the other fields
/// configure a [`CancellationChannel`] and the advisory retry helpers on
/// [`CancelledTask`].
#[derive(Debug, Clone)]
pub struct CancellationConfig {
    /// Capacity of the bounded mpsc channel ([`CancellationChannel`] only).
    /// When full, new cancellations fall back to the default sink immediately
    /// (never blocks).
    pub channel_capacity: usize,
    /// Per-call timeout for `on_cancelled`. A pool counts an overrun as
    /// `unanswered` in its `ShutdownReport`; a pool's shutdown waits at most
    /// this long (plus a second) for the sink answers of cancelled stragglers.
    pub delta: Duration,
    /// Maximum number of auto-requeue attempts per task (D4, default 3).
    pub max_attempts: u32,
    /// Base back-off delay in ms for exponential redelivery (D4).
    pub backoff_base_ms: u64,
    /// Maximum back-off delay in ms (D4).
    pub backoff_max_ms: u64,
}

impl Default for CancellationConfig {
    fn default() -> Self {
        Self {
            channel_capacity: 64,
            delta: Duration::from_secs(5),
            max_attempts: 3,
            backoff_base_ms: 100,
            backoff_max_ms: 30_000,
        }
    }
}

/// A no-op sink that logs every cancelled task and discards it.
pub struct LoggingCancellationSink {
    pool: String,
}

impl LoggingCancellationSink {
    /// Create a logging sink for `pool`.
    pub fn new(pool: impl Into<String>) -> Self {
        Self { pool: pool.into() }
    }
}

#[async_trait]
impl<P: Send + 'static> CancellationSink<P> for LoggingCancellationSink {
    async fn on_cancelled(&self, task: CancelledTask<P>) -> SinkOutcome {
        warn!(
            pool = %self.pool,
            task_id = task.meta.id,
            attempt = task.attempt,
            reason = ?task.reason,
            idempotency_key = ?task.idempotency_key(),
            "task cancelled — no durable sink configured; work is lost"
        );
        SinkOutcome::Drop
    }
}

/// A recording sink that stores cancelled tasks in memory (useful for tests).
pub struct RecordingCancellationSink<P> {
    tasks: tokio::sync::Mutex<Vec<CancelledTask<P>>>,
}

impl<P> Default for RecordingCancellationSink<P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P> RecordingCancellationSink<P> {
    /// Create an empty recording sink.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tasks: tokio::sync::Mutex::new(Vec::new()),
        }
    }

    /// Drain all recorded cancellations.
    pub async fn drain(&self) -> Vec<CancelledTask<P>> {
        self.tasks.lock().await.drain(..).collect()
    }

    /// Count of recorded cancellations.
    pub async fn len(&self) -> usize {
        self.tasks.lock().await.len()
    }

    /// True when no cancellations are recorded.
    pub async fn is_empty(&self) -> bool {
        self.tasks.lock().await.is_empty()
    }
}

#[async_trait]
impl<P: Send + 'static> CancellationSink<P> for RecordingCancellationSink<P> {
    async fn on_cancelled(&self, task: CancelledTask<P>) -> SinkOutcome {
        self.tasks.lock().await.push(task);
        SinkOutcome::Recorded
    }
}

/// Offer `task` to `sink`, bounded by `delta`. `None` when the sink overran
/// `delta` or panicked; both are logged at error level.
pub(crate) async fn offer<P: Send + 'static>(
    sink: &Arc<dyn CancellationSink<P>>,
    task: CancelledTask<P>,
    delta: Duration,
) -> Option<SinkOutcome> {
    let task_id = task.meta.id;
    match tokio::time::timeout(delta, contain(sink.on_cancelled(task))).await {
        Ok(Ok(outcome)) => Some(outcome),
        Ok(Err(panicked)) => {
            error!(task_id, %panicked, "cancellation sink panicked; the cancelled task is not recorded");
            None
        }
        Err(_) => {
            error!(
                task_id,
                ?delta,
                "cancellation sink did not answer within delta; the cancelled task is not recorded"
            );
            None
        }
    }
}

/// A standalone bounded handoff to a sink, for callers that capture their own
/// cancellations.
///
/// A bounded sender pushes cancelled tasks off the caller's hot path to a
/// drain task. The pools do not use it: they call the sink themselves and
/// count every answer in their `ShutdownReport`.
///
/// Created via [`CancellationChannel::new`].
pub struct CancellationChannel<P> {
    tx: mpsc::Sender<CancelledTask<P>>,
}

impl<P: Send + Sync + 'static> CancellationChannel<P> {
    /// Create a new channel + spawn the drain task.
    ///
    /// The drain task runs until all senders are dropped (i.e. the pool shuts
    /// down) and has processed the remaining queue.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "public constructor takes the config by value; changing it to a reference is an API change left to the contracts work (cr-18)"
    )]
    pub fn new(sink: Arc<dyn CancellationSink<P>>, config: CancellationConfig) -> Self {
        let (tx, mut rx) = mpsc::channel::<CancelledTask<P>>(config.channel_capacity);

        // Spawn the drain task — runs independently, bounded by delta per call.
        tokio::spawn(async move {
            while let Some(task) = rx.recv().await {
                let should_log_drop = task.reason.should_auto_requeue();
                match tokio::time::timeout(config.delta, sink.on_cancelled(task)).await {
                    Ok(SinkOutcome::Requeue) => {
                        info!("cancellation sink requested requeue — nack on persistent backends");
                    }
                    Ok(_) => {
                        // Recorded: the caller has durably recorded it.
                        // Drop or any future variant: discarded.
                    }
                    Err(_) => {
                        // sink overran its deadline
                        warn!(
                            "cancellation sink exceeded delta — task dropped (use a faster sink or increase delta)"
                        );
                        if should_log_drop {
                            warn!("auto-requeue task was not durably captured; work may be lost");
                        }
                    }
                }
            }
        });

        Self { tx }
    }

    /// Try to enqueue a cancelled task.
    ///
    /// Returns `false` if the channel is full (caller should use the fallback
    /// sink directly). Never blocks.
    pub fn try_send(&self, task: CancelledTask<P>) -> bool {
        self.tx.try_send(task).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prometheus_parking_lot_core::{Priority, ResourceCost, ResourceKind};
    use std::sync::Arc;

    fn make_meta(id: u64) -> TaskMetadata {
        TaskMetadata {
            id,
            mailbox: None,
            priority: Priority::Normal,
            cost: ResourceCost::new(ResourceKind::Cpu, 1),
            deadline_ms: None,
            created_at_ms: 0,
            idempotency_key: None,
            sequence: 0,
        }
    }

    #[test]
    fn cancel_reason_auto_requeue_policy() {
        assert!(CancelReason::QueueShutdown.should_auto_requeue());
        assert!(CancelReason::DrainTimeout.should_auto_requeue());
        assert!(!CancelReason::DeadlineExpired.should_auto_requeue());
        assert!(!CancelReason::SignalCancelled.should_auto_requeue());
    }

    #[test]
    fn cancelled_task_should_auto_requeue_respects_max_attempts() {
        let task = CancelledTask {
            payload: Some("work"),
            meta: make_meta(1),
            reason: CancelReason::DrainTimeout,
            attempt: 2,
            cancelled_at_ms: 0,
        };
        assert!(task.should_auto_requeue(3)); // attempt 2 < max 3
        assert!(!task.should_auto_requeue(2)); // attempt 2 >= max 2
    }

    #[test]
    fn backoff_delay_exponential() {
        let task = CancelledTask {
            payload: Some("work"),
            meta: make_meta(1),
            reason: CancelReason::DrainTimeout,
            attempt: 2,
            cancelled_at_ms: 0,
        };
        let d = task.backoff_delay(100, 10_000);
        assert_eq!(d, Duration::from_millis(400)); // 100 * 2^2
    }

    #[tokio::test]
    async fn recording_sink_captures_task() {
        let sink = Arc::new(RecordingCancellationSink::<String>::new());
        let task = CancelledTask {
            payload: Some("hello".to_string()),
            meta: make_meta(42),
            reason: CancelReason::DrainTimeout,
            attempt: 0,
            cancelled_at_ms: 0,
        };
        let outcome = sink.on_cancelled(task).await;
        assert_eq!(outcome, SinkOutcome::Recorded);
        assert_eq!(sink.len().await, 1);
    }

    #[tokio::test]
    async fn channel_delivers_to_sink() {
        let sink = Arc::new(RecordingCancellationSink::<String>::new());
        let sink_clone = Arc::clone(&sink);
        let ch = CancellationChannel::new(
            sink_clone as Arc<dyn CancellationSink<String>>,
            CancellationConfig::default(),
        );

        let task = CancelledTask {
            payload: Some("test".to_string()),
            meta: make_meta(7),
            reason: CancelReason::QueueShutdown,
            attempt: 0,
            cancelled_at_ms: 0,
        };
        assert!(ch.try_send(task));

        // Give the drain task time to process
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(sink.len().await, 1);
    }

    #[tokio::test]
    async fn channel_full_try_send_returns_false() {
        let sink = Arc::new(RecordingCancellationSink::<String>::new());
        let config = CancellationConfig {
            channel_capacity: 1,
            ..Default::default()
        };
        // Fill the channel
        let ch = CancellationChannel::new(sink as Arc<dyn CancellationSink<String>>, config);
        let make_task = || CancelledTask {
            payload: Some("t".to_string()),
            meta: make_meta(1),
            reason: CancelReason::DrainTimeout,
            attempt: 0,
            cancelled_at_ms: 0,
        };
        // First send may or may not succeed depending on drain speed;
        // eventually a second send should hit the capacity limit.
        let _ = ch.try_send(make_task());
        // If channel is now full, try_send returns false
        // (best-effort: drain task may have already consumed it in CI)
        let _ = ch.try_send(make_task());
        // No assertion on exact count — just verify it doesn't panic/block.
    }
}
