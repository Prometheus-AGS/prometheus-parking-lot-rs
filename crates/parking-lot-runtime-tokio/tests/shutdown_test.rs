//! S-9 (cr-29): no accepted task is lost when an in-memory pool shuts down.
//!
//! Each test fills a pool with two long tasks, one short task running beside
//! them and a backlog of short tasks parked behind, then calls
//! `shutdown_with` under one [`ShutdownPolicy`]. The oracle: every accepted
//! task ends up either completed (its outcome is `Completed`) or captured by
//! the cancellation sink (its outcome is `Cancelled` and the sink holds it),
//! never both and never neither; and the `ShutdownReport` agrees with what the
//! sink holds. Under `Drain` the long tasks outlive the deadline: they are
//! reported unsettled and must still complete afterwards.
//!
//! ```text
//! cargo nextest run --workspace --all-features --locked -E "test(shutdown_)"
//! ```
//!
//! Negative control: under `--cfg parking_lot_negative_control="s-9"` the
//! native `WorkerPool` drops a task a worker receives after the shutdown flag
//! (the 0.2 worker loop). The short tasks woken during the drain are received
//! that way, so `shutdown_worker_pool_drain_*` and
//! `shutdown_worker_pool_drain_then_cancel_*` must fail: those tasks have
//! neither an outcome nor a capture.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

use prometheus_parking_lot_core::config::WorkerPoolConfig;
use prometheus_parking_lot_core::{
    InMemoryMailbox, InMemoryQueue, Mailbox, MailboxKey, PoolLimits, Priority, ResourceCost,
    ResourceKind, ScheduledTask, SchedulerError, Spawn, TaskError, TaskExecutor, TaskMetadata,
    TaskStatus, WorkerExecutor,
};
use prometheus_parking_lot_runtime_tokio::{
    CancelReason, CancellationConfig, CancellationSink, CancelledTask, RecordingCancellationSink,
    ResourcePool, ShutdownPolicy, ShutdownReport, SinkOutcome, WorkerPool,
};

/// Upper bound on any wait for an outcome; reaching it fails the test.
const WAIT: Duration = Duration::from_secs(10);
/// How long a short task runs.
const SHORT_MS: u64 = 100;
/// Short tasks submitted after the long ones: one runs, the rest park.
const SHORTS: u64 = 10;
/// Long tasks: two of the pool's three units.
const LONGS: u64 = 2;
/// The drain deadline: a few short tasks complete within it.
const DRAIN: Duration = Duration::from_millis(350);

/// A job: sleep `ms`, then return `done:<id>`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
struct Job {
    id: u64,
    ms: u64,
}

/// Runs jobs and counts the executions that started.
#[derive(Clone, Default)]
struct Sleeper {
    started: Arc<AtomicUsize>,
}

impl Sleeper {
    async fn run(&self, job: Job) -> Result<String, TaskError> {
        self.started.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(job.ms)).await;
        Ok(format!("done:{}", job.id))
    }

    /// Wait (at most `WAIT`) until `count` executions have started.
    async fn wait_started(&self, count: usize) {
        let deadline = Instant::now() + WAIT;
        while self.started.load(Ordering::SeqCst) < count {
            assert!(
                Instant::now() < deadline,
                "{count} executions never started"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
}

#[async_trait]
impl TaskExecutor<Job, String> for Sleeper {
    async fn execute(&self, job: Job, _meta: TaskMetadata) -> Result<String, TaskError> {
        self.run(job).await
    }
}

#[async_trait]
impl WorkerExecutor<Job, String> for Sleeper {
    async fn execute(&self, job: Job, _meta: TaskMetadata) -> Result<String, TaskError> {
        self.run(job).await
    }
}

#[derive(Clone)]
struct TokioSpawner;

impl Spawn for TokioSpawner {
    fn spawn<F>(&self, fut: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        tokio::spawn(fut);
    }
}

/// A mailbox the test reads while the pool owns a clone of it.
#[derive(Clone, Default)]
struct SharedMailbox {
    inner: Arc<Mutex<InMemoryMailbox<String>>>,
    delivered: Arc<Notify>,
}

impl Mailbox<String> for SharedMailbox {
    fn deliver(
        &mut self,
        key: &MailboxKey,
        status: TaskStatus,
        payload: Option<String>,
    ) -> Result<(), SchedulerError> {
        self.inner.lock().deliver(key, status, payload)?;
        self.delivered.notify_waiters();
        Ok(())
    }
}

impl SharedMailbox {
    /// The outcome delivered for `key`, waiting at most `WAIT`; `None` if
    /// none arrives.
    async fn take(&self, key: &MailboxKey) -> Option<(TaskStatus, Option<String>)> {
        tokio::time::timeout(WAIT, async {
            loop {
                let notified = self.delivered.notified();
                if let Some(message) = self.inner.lock().consume(key).expect("consume") {
                    return message;
                }
                notified.await;
            }
        })
        .await
        .ok()
    }
}

fn key(id: u64) -> MailboxKey {
    MailboxKey {
        tenant: "cr-29".into(),
        user_id: None,
        session_id: Some(id.to_string()),
    }
}

fn meta(id: u64) -> TaskMetadata {
    TaskMetadata {
        id,
        mailbox: Some(key(id)),
        priority: Priority::Normal,
        cost: ResourceCost::new(ResourceKind::Cpu, 1),
        deadline_ms: None,
        created_at_ms: 0,
        idempotency_key: None,
        sequence: 0,
    }
}

/// The jobs of one scenario: `LONGS` long ones (ids 0..), then `SHORTS`
/// short ones.
fn jobs(long_ms: u64) -> Vec<Job> {
    (0..LONGS)
        .map(|id| Job { id, ms: long_ms })
        .chain((LONGS..LONGS + SHORTS).map(|id| Job { id, ms: SHORT_MS }))
        .collect()
}

/// Where every accepted task ended up.
#[derive(Debug, Default)]
struct Accounting {
    completed: BTreeSet<u64>,
    cancelled: BTreeSet<u64>,
    missing: BTreeSet<u64>,
    /// Sink captures by task id, with whether they carried the payload.
    captured: Vec<(u64, CancelReason, bool)>,
}

impl Accounting {
    /// The S-9 oracle.
    fn assert_nothing_lost(&self, accepted: u64, report: &ShutdownReport) {
        assert!(
            self.missing.is_empty(),
            "tasks with neither an outcome nor a capture (lost): {:?}; {self:?}",
            self.missing
        );
        let captured: BTreeSet<u64> = self.captured.iter().map(|(id, ..)| *id).collect();
        assert_eq!(
            captured.len(),
            self.captured.len(),
            "a task was captured twice: {self:?}"
        );
        assert_eq!(
            captured, self.cancelled,
            "the sink must hold exactly the tasks whose outcome is Cancelled"
        );
        assert!(
            self.completed.is_disjoint(&self.cancelled),
            "a task both completed and was cancelled: {self:?}"
        );
        assert_eq!(
            u64::try_from(self.completed.len() + self.cancelled.len()).expect("small"),
            accepted,
            "every accepted task completed or was captured: {self:?}"
        );
        assert_eq!(
            report.cancelled,
            u64::try_from(captured.len()).expect("small"),
            "{report:?}"
        );
        assert_eq!(report.recorded, report.cancelled, "{report:?}");
        assert_eq!(
            (report.requeued, report.dropped, report.unanswered),
            (0, 0, 0),
            "{report:?}"
        );
    }

    fn captures_with(&self, reason: &CancelReason) -> Vec<(u64, bool)> {
        self.captured
            .iter()
            .filter(|(_, captured_reason, _)| captured_reason == reason)
            .map(|(id, _, has_payload)| (*id, *has_payload))
            .collect()
    }
}

async fn sink_captures(sink: &RecordingCancellationSink<Job>) -> Vec<(u64, CancelReason, bool)> {
    sink.drain()
        .await
        .into_iter()
        .map(|task| {
            if let Some(job) = task.payload {
                assert_eq!(job.id, task.meta.id, "captured payload belongs to the task");
            }
            (task.meta.id, task.reason, task.payload.is_some())
        })
        .collect()
}

// ── ResourcePool ────────────────────────────────────────────────────────────

type Pool = ResourcePool<Job, String, InMemoryQueue<Job>, SharedMailbox, Sleeper, TokioSpawner>;

fn resource_pool(mailbox: &SharedMailbox, sink: &Arc<RecordingCancellationSink<Job>>) -> Pool {
    ResourcePool::new(
        PoolLimits {
            max_units: 3,
            max_queue_depth: 64,
            default_timeout: Duration::from_secs(120),
        },
        InMemoryQueue::new(64),
        mailbox.clone(),
        Sleeper::default(),
        TokioSpawner,
    )
    .with_cancellation_sink(Arc::clone(sink) as Arc<dyn CancellationSink<Job>>)
}

/// Fill a `ResourcePool`, shut it down under `policy`, then collect every
/// task's outcome.
async fn resource_pool_scenario(
    policy: ShutdownPolicy,
    long_ms: u64,
) -> (Accounting, ShutdownReport, Duration) {
    let mailbox = SharedMailbox::default();
    let sink = Arc::new(RecordingCancellationSink::new());
    let pool = resource_pool(&mailbox, &sink);
    let jobs = jobs(long_ms);
    for job in &jobs {
        pool.submit(ScheduledTask {
            meta: meta(job.id),
            payload: *job,
        })
        .await
        .expect("accepted");
    }
    assert_eq!(pool.running_tasks(), 3);

    let started = Instant::now();
    let report = pool.shutdown_with(policy).await;
    let took = started.elapsed();
    assert!(matches!(
        pool.submit(ScheduledTask {
            meta: meta(99),
            payload: Job { id: 99, ms: 0 }
        })
        .await,
        Err(SchedulerError::PoolShutdown { .. })
    ));

    let mut accounting = Accounting::default();
    for job in &jobs {
        match mailbox.take(&key(job.id)).await {
            Some((TaskStatus::Completed, Some(value))) => {
                assert_eq!(value, format!("done:{}", job.id));
                accounting.completed.insert(job.id);
            }
            Some((TaskStatus::Failed(TaskError::Cancelled { .. }), None)) => {
                accounting.cancelled.insert(job.id);
            }
            Some(other) => panic!("task {} ended {other:?}", job.id),
            None => {
                accounting.missing.insert(job.id);
            }
        }
    }
    accounting.captured = sink_captures(&sink).await;
    (accounting, report, took)
}

/// `Drain`: the backlog keeps starting until the deadline and the rest is
/// captured with its payload; the running tasks (the two long ones and the
/// short one on the third unit) are not cancelled — they are reported
/// unsettled and complete afterwards.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shutdown_resource_pool_drain_accounts_for_every_task() {
    let (accounting, report, _) =
        resource_pool_scenario(ShutdownPolicy::Drain { deadline: DRAIN }, 1_500).await;
    accounting.assert_nothing_lost(LONGS + SHORTS, &report);
    assert_eq!(report.unsettled, LONGS + 1, "{report:?}");
    for id in 0..LONGS {
        assert!(
            accounting.completed.contains(&id),
            "long task {id} must complete"
        );
    }
    assert!(
        report.completed >= 1,
        "a short task completed during the drain: {report:?}"
    );
    let queued = accounting.captures_with(&CancelReason::QueueShutdown);
    assert!(!queued.is_empty(), "{accounting:?}");
    assert!(queued.iter().all(|(_, has_payload)| *has_payload));
    assert!(
        accounting
            .captures_with(&CancelReason::DrainTimeout)
            .is_empty()
    );
}

/// `DrainThenCancel`: as `Drain` until the deadline, then the running tasks
/// (the long ones and one short one) are cancelled and reach the sink with
/// their payload, restored from the pool's snapshot.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shutdown_resource_pool_drain_then_cancel_accounts_for_every_task() {
    let (accounting, report, took) =
        resource_pool_scenario(ShutdownPolicy::DrainThenCancel { deadline: DRAIN }, 60_000).await;
    accounting.assert_nothing_lost(LONGS + SHORTS, &report);
    assert_eq!(report.unsettled, 0, "{report:?}");
    assert!(took < Duration::from_secs(5), "took {took:?}");
    assert!(report.completed >= 1, "{report:?}");
    assert_stragglers(&accounting, true);
}

/// The tasks running at the deadline — both long ones and one short one —
/// were cancelled, with or without their payload.
fn assert_stragglers(accounting: &Accounting, with_payload: bool) {
    let stragglers = accounting.captures_with(&CancelReason::DrainTimeout);
    assert_eq!(stragglers.len(), 3, "{accounting:?}");
    for id in 0..LONGS {
        assert!(
            stragglers.contains(&(id, with_payload)),
            "long task {id} is cancelled (payload kept: {with_payload}): {accounting:?}"
        );
    }
    assert!(
        stragglers
            .iter()
            .all(|(_, has_payload)| *has_payload == with_payload)
    );
}

/// `CancelImmediately`: everything running is cancelled and everything
/// queued captured, at once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shutdown_resource_pool_cancel_immediately_accounts_for_every_task() {
    let (accounting, report, took) =
        resource_pool_scenario(ShutdownPolicy::CancelImmediately, 60_000).await;
    accounting.assert_nothing_lost(LONGS + SHORTS, &report);
    assert_eq!(report.unsettled, 0, "{report:?}");
    assert!(took < Duration::from_secs(2), "took {took:?}");
    let mut stragglers = accounting.captures_with(&CancelReason::DrainTimeout);
    stragglers.sort_unstable();
    assert_eq!(
        stragglers.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        vec![0, 1, 2],
        "the three running tasks are cancelled: {accounting:?}"
    );
}

/// A sink that never answers within `delta` cannot hold the shutdown: the
/// capture is counted as unanswered, and the task's mailbox still says
/// Cancelled.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_resource_pool_counts_a_sink_that_does_not_answer() {
    struct Silent;
    #[async_trait]
    impl CancellationSink<Job> for Silent {
        async fn on_cancelled(&self, _task: CancelledTask<Job>) -> SinkOutcome {
            std::future::pending().await
        }
    }

    let mailbox = SharedMailbox::default();
    let pool = ResourcePool::new(
        PoolLimits {
            max_units: 1,
            max_queue_depth: 8,
            default_timeout: Duration::from_secs(120),
        },
        InMemoryQueue::new(8),
        mailbox.clone(),
        Sleeper::default(),
        TokioSpawner,
    )
    .with_cancellation_sink(Arc::new(Silent))
    .with_cancellation_config(CancellationConfig {
        delta: Duration::from_millis(50),
        ..CancellationConfig::default()
    });
    for id in 0..2 {
        pool.submit(ScheduledTask {
            meta: meta(id),
            payload: Job { id, ms: 60_000 },
        })
        .await
        .expect("accepted");
    }
    let report = pool.shutdown_with(ShutdownPolicy::CancelImmediately).await;
    assert_eq!(
        (report.cancelled, report.unanswered, report.unsettled),
        (2, 2, 0),
        "{report:?}"
    );
    for id in 0..2 {
        assert!(matches!(
            mailbox.take(&key(id)).await,
            Some((TaskStatus::Failed(TaskError::Cancelled { .. }), None))
        ));
    }
}

/// An idle pool shuts down at once under the default policy.
#[tokio::test]
async fn shutdown_resource_pool_idle_returns_at_once() {
    let mailbox = SharedMailbox::default();
    let sink = Arc::new(RecordingCancellationSink::new());
    let pool = resource_pool(&mailbox, &sink);
    let started = Instant::now();
    let report = pool.shutdown_with(ShutdownPolicy::default()).await;
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(report, ShutdownReport::default());
}

// ── WorkerPool ──────────────────────────────────────────────────────────────

/// Fill a native `WorkerPool`, shut it down under `policy`, then collect every
/// task's outcome.
async fn worker_pool_scenario(
    policy: ShutdownPolicy,
    long_ms: u64,
) -> (Accounting, ShutdownReport, Duration) {
    let sink = Arc::new(RecordingCancellationSink::new());
    let executor = Sleeper::default();
    let pool = WorkerPool::new(
        WorkerPoolConfig::new()
            .with_worker_count(3)
            .with_max_units(3)
            .with_max_queue_depth(64)
            .with_timeout_ms(120_000),
        executor.clone(),
    )
    .expect("pool")
    .with_cancellation_sink(Arc::clone(&sink) as Arc<dyn CancellationSink<Job>>);
    let jobs = jobs(long_ms);
    let mut keys = Vec::new();
    for job in &jobs {
        keys.push((job.id, pool.submit(*job, meta(job.id)).expect("accepted")));
    }
    // The three admitted tasks are running on the workers before the
    // shutdown starts, so the only tasks a worker receives after the flag
    // are the ones woken during the drain.
    executor.wait_started(3).await;

    let started = Instant::now();
    let report = pool.shutdown_with_async(policy).await;
    let took = started.elapsed();
    assert!(matches!(
        pool.submit(Job { id: 99, ms: 0 }, meta(99)),
        Err(SchedulerError::PoolShutdown { .. })
    ));

    let mut accounting = Accounting::default();
    for (id, key) in keys {
        match pool.retrieve_async(&key, WAIT).await {
            Ok(value) => {
                assert_eq!(value, format!("done:{id}"));
                accounting.completed.insert(id);
            }
            Err(SchedulerError::Task(TaskError::Cancelled { .. })) => {
                accounting.cancelled.insert(id);
            }
            Err(SchedulerError::Timeout { .. }) => {
                accounting.missing.insert(id);
            }
            Err(other) => panic!("task {id} ended {other:?}"),
        }
    }
    accounting.captured = sink_captures(&sink).await;
    (accounting, report, took)
}

/// `Drain`: short tasks woken during the drain are received by workers after
/// the shutdown flag and must run (S-9's dropped-task path); the backlog left
/// at the deadline is captured with its payload; the long tasks complete.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_worker_pool_drain_accounts_for_every_task() {
    let (accounting, report, _) =
        worker_pool_scenario(ShutdownPolicy::Drain { deadline: DRAIN }, 1_500).await;
    accounting.assert_nothing_lost(LONGS + SHORTS, &report);
    for id in 0..LONGS {
        assert!(
            accounting.completed.contains(&id),
            "long task {id} must complete"
        );
    }
    assert!(
        accounting.completed.len() > usize::try_from(LONGS + 1).expect("small"),
        "short tasks woken during the drain must run: {accounting:?}"
    );
    let queued = accounting.captures_with(&CancelReason::QueueShutdown);
    assert!(!queued.is_empty(), "{accounting:?}");
    assert!(queued.iter().all(|(_, has_payload)| *has_payload));
}

/// `DrainThenCancel`: as `Drain` until the deadline, then the long tasks are
/// cancelled; a `WorkerPool` cannot keep their payload.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_worker_pool_drain_then_cancel_accounts_for_every_task() {
    let (accounting, report, took) =
        worker_pool_scenario(ShutdownPolicy::DrainThenCancel { deadline: DRAIN }, 60_000).await;
    accounting.assert_nothing_lost(LONGS + SHORTS, &report);
    assert_eq!(report.unsettled, 0, "{report:?}");
    assert!(took < Duration::from_secs(5), "took {took:?}");
    assert!(
        accounting.completed.len() >= 2,
        "short tasks completed during the drain: {accounting:?}"
    );
    assert_stragglers(&accounting, false);
}

/// `CancelImmediately`: running tasks cancelled, queued ones captured, and
/// the pool's threads gone, at once. No task is received during a drain, so
/// the S-9 negative control does not affect this one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_worker_pool_cancel_immediately_accounts_for_every_task() {
    let (accounting, report, took) =
        worker_pool_scenario(ShutdownPolicy::CancelImmediately, 60_000).await;
    accounting.assert_nothing_lost(LONGS + SHORTS, &report);
    assert_eq!(report.unsettled, 0, "{report:?}");
    assert!(took < Duration::from_secs(3), "took {took:?}");
    assert!(accounting.completed.is_empty(), "{accounting:?}");
    assert_stragglers(&accounting, false);
}

/// The blocking variant works from a plain thread and leaves no worker.
#[test]
fn shutdown_worker_pool_blocking_variant_from_a_plain_thread() {
    let sink = Arc::new(RecordingCancellationSink::new());
    let executor = Sleeper::default();
    let pool = WorkerPool::new(
        WorkerPoolConfig::new()
            .with_worker_count(1)
            .with_max_units(1)
            .with_max_queue_depth(8),
        executor.clone(),
    )
    .expect("pool")
    .with_cancellation_sink(Arc::clone(&sink) as Arc<dyn CancellationSink<Job>>);
    let running = pool
        .submit(Job { id: 0, ms: 60_000 }, meta(0))
        .expect("accepted");
    let queued = pool
        .submit(Job { id: 1, ms: 0 }, meta(1))
        .expect("accepted");
    let deadline = Instant::now() + WAIT;
    while executor.started.load(Ordering::SeqCst) < 1 {
        assert!(Instant::now() < deadline, "the running task never started");
        std::thread::sleep(Duration::from_millis(5));
    }
    let report = pool.shutdown_with(ShutdownPolicy::CancelImmediately);
    assert_eq!(
        (report.cancelled, report.recorded, report.unsettled),
        (2, 2, 0),
        "{report:?}"
    );
    assert_eq!(pool.stats().worker_count, 0);
    for key in [running, queued] {
        assert!(matches!(
            pool.retrieve(&key, WAIT),
            Err(SchedulerError::Task(TaskError::Cancelled { .. }))
        ));
    }
}
