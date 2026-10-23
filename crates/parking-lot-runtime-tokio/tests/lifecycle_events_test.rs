//! S-21 (cr-31): every task's lifecycle events arrive in causal order.
//!
//! A recording hook is registered on the real pools (`ResourcePool`, the
//! native `WorkerPool`, `DurableResourcePool` on the in-memory lease queue and
//! result mailbox). The oracle, per task id, over the events in delivery
//! order: `Submitted` first; then one or more *runs*, each at most one
//! `Queued`, then at most one `Started`, then exactly one terminal event.
//! Only a run that ends in `Requeued` (a durable pool handed the task back to
//! its queue) may be followed by another run; nothing follows any other
//! terminal event. The in-memory pools never requeue, so for them this is
//! "exactly one terminal event per task"; for the durable pool it is "exactly
//! one terminal event per run". It is checked over 1,000 concurrently
//! submitted tasks (completions, failures, panics, expiries and rejections
//! mixed), over shutdown cancellation under each cr-29 policy, over rejection
//! at submit and over the durable pool's requeue paths (shutdown, and a
//! delivery that keeps failing).
//!
//! ```text
//! cargo nextest run -p prometheus-parking-lot-runtime-tokio --locked -E "test(lifecycle_events_)"
//! ```
//!
//! Negative control: under `--cfg parking_lot_negative_control="s-21"` the
//! pools suppress the terminal event of a task cancelled at shutdown and of a
//! durable run handed back to its queue, so the
//! `lifecycle_events_*_cancelled_*` and `lifecycle_events_durable_*_requeued_*`
//! tests must fail.

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use prometheus_parking_lot_core::config::WorkerPoolConfig;
use prometheus_parking_lot_core::hooks::{CancelledReason, RejectionReason};
use prometheus_parking_lot_core::{
    Delivery, DeliveryOutcome, Enqueued, Hook, HookBus, InMemoryLeaseQueue, InMemoryMailbox,
    InMemoryQueue, InMemoryResultMailbox, LeaseQueue, LeaseToken, LeasedTask, LifecycleEvent,
    MailboxEntry, MailboxKey, NackAction, PoolLimits, Priority, QueueDepth, ResourceCost,
    ResourceKind, ResultMailbox, ScheduledTask, SchedulerError, TaskError, TaskExecutor,
    TaskMetadata, WorkerExecutor, now_ms,
};
use prometheus_parking_lot_runtime_tokio::{
    DurableOptions, DurableResourcePool, ResourcePool, ShutdownPolicy, TokioSpawner, WorkerPool,
};

/// Upper bound on any wait; reaching it fails the test.
const WAIT: Duration = Duration::from_secs(30);

// ---------------------------------------------------------------------------
// Recording hook and the ordering oracle
// ---------------------------------------------------------------------------

/// Keeps every event, in delivery order.
#[derive(Default)]
struct Log {
    events: Mutex<Vec<LifecycleEvent>>,
}

impl Hook for Log {
    fn on_event(&self, event: &LifecycleEvent) {
        self.events.lock().push(event.clone());
    }
}

impl Log {
    fn snapshot(&self) -> Vec<LifecycleEvent> {
        self.events.lock().clone()
    }

    /// Events of `task_id`, in delivery order.
    fn of(&self, task_id: u64) -> Vec<LifecycleEvent> {
        self.snapshot()
            .into_iter()
            .filter(|event| event.task_id() == task_id)
            .collect()
    }
}

/// The S-21 oracle: one entry per task that breaks the order (empty = pass).
fn violations(events: &[LifecycleEvent], ids: &BTreeSet<u64>) -> Vec<String> {
    let mut by_task: BTreeMap<u64, Vec<&'static str>> = BTreeMap::new();
    for event in events {
        by_task
            .entry(event.task_id())
            .or_default()
            .push(event.kind());
    }
    let mut found = Vec::new();
    for id in ids {
        let kinds = by_task.remove(id).unwrap_or_default();
        if let Err(problem) = check_sequence(&kinds) {
            found.push(format!("task {id}: {problem} in {kinds:?}"));
        }
    }
    for (id, kinds) in by_task {
        found.push(format!(
            "task {id}: events for a task never submitted: {kinds:?}"
        ));
    }
    found
}

fn check_sequence(kinds: &[&str]) -> Result<(), String> {
    // 0 nothing, 1 submitted (or a run handed back: a new run may follow),
    // 2 queued, 3 started, 4 final terminal.
    let mut stage = 0;
    let mut handed_back = false;
    for kind in kinds {
        stage = match (*kind, stage) {
            (_, 4) => return Err(format!("`{kind}` after the terminal event")),
            ("submitted", 0) => 1,
            ("queued", 1) => 2,
            ("started", 1 | 2) => 3,
            ("submitted" | "queued" | "started", _) => {
                return Err(format!("`{kind}` out of order"));
            }
            (_, 0) => return Err(format!("terminal `{kind}` before `submitted`")),
            // The run ended and the task went back to its queue.
            ("requeued", _) => 1,
            (_, _) => 4,
        };
        handed_back = *kind == "requeued";
    }
    if stage == 4 || handed_back {
        Ok(())
    } else {
        Err("no terminal event".into())
    }
}

/// Poll (flushing the bus) until every id in `ids` has a terminal event, or
/// `WAIT` passes; then flush once more so a late duplicate would show.
async fn settle(bus: &HookBus, log: &Log, ids: &BTreeSet<u64>) -> Vec<LifecycleEvent> {
    let deadline = Instant::now() + WAIT;
    loop {
        flush(bus).await;
        let events = log.snapshot();
        let terminal: BTreeSet<u64> = events
            .iter()
            .filter(|event| event.is_terminal())
            .map(LifecycleEvent::task_id)
            .collect();
        if ids.is_subset(&terminal) || Instant::now() >= deadline {
            tokio::time::sleep(Duration::from_millis(50)).await;
            flush(bus).await;
            return log.snapshot();
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn flush(bus: &HookBus) {
    let bus = bus.clone();
    let flushed = tokio::task::spawn_blocking(move || bus.flush(WAIT))
        .await
        .expect("flush thread");
    assert!(flushed, "hook bus did not flush within {WAIT:?}");
}

fn assert_ordered(bus: &HookBus, events: &[LifecycleEvent], ids: &BTreeSet<u64>) {
    assert_eq!(bus.dropped_events(), 0, "the hook queue overflowed");
    let found = violations(events, ids);
    assert!(
        found.is_empty(),
        "{} of {} tasks broke the lifecycle order:\n{}",
        found.len(),
        ids.len(),
        found.join("\n")
    );
}

/// Wait until `log` holds at least `count` events matching `pred`.
async fn wait_for(log: &Log, count: usize, pred: impl Fn(&LifecycleEvent) -> bool) {
    let deadline = Instant::now() + WAIT;
    while log.snapshot().iter().filter(|event| pred(event)).count() < count {
        assert!(Instant::now() < deadline, "expected events never arrived");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

// ---------------------------------------------------------------------------
// Payload, executor and task builders
// ---------------------------------------------------------------------------

/// What a job does when it runs.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
enum Act {
    Succeed,
    Fail,
    Panic,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
struct Job {
    ms: u64,
    act: Act,
}

#[derive(Clone, Copy)]
struct Runner;

impl Runner {
    async fn run(job: Job) -> Result<String, TaskError> {
        tokio::time::sleep(Duration::from_millis(job.ms)).await;
        match job.act {
            Act::Succeed => Ok("done".into()),
            Act::Fail => Err(TaskError::Executor {
                message: "failed on purpose".into(),
                retryable: false,
            }),
            Act::Panic => panic!("executor panics on purpose"),
        }
    }
}

#[async_trait]
impl TaskExecutor<Job, String> for Runner {
    async fn execute(&self, job: Job, _meta: TaskMetadata) -> Result<String, TaskError> {
        Self::run(job).await
    }
}

#[async_trait]
impl WorkerExecutor<Job, String> for Runner {
    async fn execute(&self, job: Job, _meta: TaskMetadata) -> Result<String, TaskError> {
        Self::run(job).await
    }
}

fn key(id: u64) -> MailboxKey {
    MailboxKey {
        tenant: "cr-31".into(),
        user_id: None,
        session_id: Some(id.to_string()),
    }
}

fn meta(id: u64, units: u32) -> TaskMetadata {
    TaskMetadata {
        id,
        mailbox: Some(key(id)),
        priority: Priority::Normal,
        cost: ResourceCost::new(ResourceKind::Cpu, units),
        deadline_ms: None,
        created_at_ms: 0,
        idempotency_key: Some(format!("job-{id}")),
        sequence: 0,
    }
}

fn task(id: u64, units: u32, job: Job) -> ScheduledTask<Job> {
    ScheduledTask {
        meta: meta(id, units),
        payload: job,
    }
}

const fn sleep_job(ms: u64) -> Job {
    Job {
        ms,
        act: Act::Succeed,
    }
}

type Pool =
    ResourcePool<Job, String, InMemoryQueue<Job>, InMemoryMailbox<String>, Runner, TokioSpawner>;

fn resource_pool(units: u32, depth: usize, log: &Arc<Log>) -> Pool {
    ResourcePool::new(
        PoolLimits {
            max_units: units,
            max_queue_depth: depth,
            default_timeout: Duration::from_secs(10),
        },
        InMemoryQueue::new(depth),
        InMemoryMailbox::new(),
        Runner,
        TokioSpawner::new(tokio::runtime::Handle::current()),
    )
    .with_hook(Arc::clone(log) as Arc<dyn Hook>)
}

fn worker_pool(units: u32, depth: usize, log: &Arc<Log>) -> WorkerPool<Job, String, Runner> {
    WorkerPool::new(
        WorkerPoolConfig::new()
            .with_worker_count(units as usize)
            .with_max_units(units)
            .with_max_queue_depth(depth)
            .with_timeout_ms(10_000),
        Runner,
    )
    .expect("worker pool")
    .with_hook(Arc::clone(log) as Arc<dyn Hook>)
}

// ---------------------------------------------------------------------------
// 1,000 tasks through the resource pool
// ---------------------------------------------------------------------------

/// The job for task `id` of the mixed workload: mostly short successes, some
/// failures and panics, a few with a deadline that may pass while parked and
/// a few too large for the pool.
fn mixed(id: u64) -> ScheduledTask<Job> {
    let act = match id {
        id if id % 97 == 0 => Act::Panic,
        id if id % 10 == 3 => Act::Fail,
        _ => Act::Succeed,
    };
    let units = if id % 250 == 7 {
        9
    } else {
        1 + u32::try_from(id % 3).expect("small")
    };
    let mut task = task(id, units, Job { ms: id % 4, act });
    if id % 50 == 11 {
        task.meta.deadline_ms = Some(now_ms() + 2);
    }
    task
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_events_resource_pool_order_holds_for_1000_tasks() {
    const TASKS: u64 = 1_000;
    const SUBMITTERS: u64 = 4;
    let log = Arc::new(Log::default());
    let pool = Arc::new(resource_pool(4, 2_000, &log));

    let handles: Vec<_> = (0..SUBMITTERS)
        .map(|lane| {
            let pool = Arc::clone(&pool);
            tokio::spawn(async move {
                let mut accepted = 0_u64;
                for id in (lane..TASKS).step_by(usize::try_from(SUBMITTERS).expect("small")) {
                    if pool.submit(mixed(id)).await.is_ok() {
                        accepted += 1;
                    }
                }
                accepted
            })
        })
        .collect();
    let mut accepted = 0;
    for handle in handles {
        accepted += handle.await.expect("submitter");
    }

    let deadline = Instant::now() + WAIT;
    while pool.completed_tasks() + pool.failed_tasks() < accepted {
        assert!(Instant::now() < deadline, "accepted tasks never finished");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let ids: BTreeSet<u64> = (0..TASKS).collect();
    let events = settle(pool.hooks(), &log, &ids).await;
    assert_ordered(pool.hooks(), &events, &ids);
    let count = |kind: &str| events.iter().filter(|event| event.kind() == kind).count();
    assert_eq!(count("submitted"), 1_000);
    assert!(count("queued") > 0, "the workload never parked a task");
    assert!(count("failed") > 0 && count("completed") > 0);
    assert!(count("rejected") >= 4, "tasks over capacity are rejected");
    assert_eq!(pool.hooks().hook_panics(), 0);
}

// ---------------------------------------------------------------------------
// Cancellation under each cr-29 policy
// ---------------------------------------------------------------------------

/// Two long tasks and one short one fill the pool's three units; ten short
/// tasks park behind them.
const LONGS: u64 = 2;
const SHORTS: u64 = 10;
const DRAIN: Duration = Duration::from_millis(350);

fn workload(long_ms: u64) -> Vec<(u64, Job)> {
    (0..LONGS)
        .map(|id| (id, sleep_job(long_ms)))
        .chain((LONGS..LONGS + SHORTS).map(|id| (id, sleep_job(100))))
        .collect()
}

const fn policies() -> [(ShutdownPolicy, u64); 3] {
    [
        // Long tasks outlive the deadline and complete afterwards.
        (ShutdownPolicy::Drain { deadline: DRAIN }, 1_500),
        (ShutdownPolicy::DrainThenCancel { deadline: DRAIN }, 30_000),
        (ShutdownPolicy::CancelImmediately, 30_000),
    ]
}

/// Checks shared by both in-memory pools after a shutdown under `policy`.
fn assert_cancellation(policy: &ShutdownPolicy, events: &[LifecycleEvent]) {
    let cancelled: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            LifecycleEvent::Cancelled {
                task_id, reason, ..
            } => Some((*task_id, reason.clone())),
            _ => None,
        })
        .collect();
    assert!(
        cancelled
            .iter()
            .any(|(_, reason)| *reason == CancelledReason::QueueShutdown),
        "{policy:?}: no parked task was cancelled: {cancelled:?}"
    );
    let long_cancelled = cancelled
        .iter()
        .filter(|(id, reason)| *id < LONGS && *reason == CancelledReason::DrainTimeout)
        .count();
    if policy.cancels_stragglers() {
        assert_eq!(
            long_cancelled, 2,
            "{policy:?}: running tasks not cancelled: {cancelled:?}"
        );
    } else {
        assert_eq!(
            long_cancelled, 0,
            "{policy:?}: Drain cancelled a running task"
        );
        let completed: BTreeSet<u64> = events
            .iter()
            .filter(|event| event.kind() == "completed")
            .map(LifecycleEvent::task_id)
            .collect();
        assert!(
            (0..LONGS).all(|id| completed.contains(&id)),
            "{policy:?}: the long tasks must complete after the deadline"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_events_resource_pool_cancelled_under_every_policy() {
    for (policy, long_ms) in policies() {
        let log = Arc::new(Log::default());
        let pool = resource_pool(3, 100, &log);
        let jobs = workload(long_ms);
        for (id, job) in &jobs {
            pool.submit(task(*id, 1, *job)).await.expect("accepted");
        }
        wait_for(&log, 3, |event| event.kind() == "started").await;
        let report = pool.shutdown_with(policy.clone()).await;
        assert!(report.cancelled > 0, "{policy:?}: {report:?}");

        let ids: BTreeSet<u64> = jobs.iter().map(|(id, _)| *id).collect();
        let events = settle(pool.hooks(), &log, &ids).await;
        assert_ordered(pool.hooks(), &events, &ids);
        assert_cancellation(&policy, &events);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_events_worker_pool_cancelled_under_every_policy() {
    for (policy, long_ms) in policies() {
        let log = Arc::new(Log::default());
        let pool = worker_pool(3, 100, &log);
        let jobs = workload(long_ms);
        for (id, job) in &jobs {
            pool.submit(*job, meta(*id, 1)).expect("accepted");
        }
        wait_for(&log, 3, |event| event.kind() == "started").await;
        let report = pool.shutdown_with_async(policy.clone()).await;
        assert!(report.cancelled > 0, "{policy:?}: {report:?}");

        let ids: BTreeSet<u64> = jobs.iter().map(|(id, _)| *id).collect();
        let events = settle(pool.hooks(), &log, &ids).await;
        assert_ordered(pool.hooks(), &events, &ids);
        assert_cancellation(&policy, &events);
    }
}

// ---------------------------------------------------------------------------
// Rejection at submit
// ---------------------------------------------------------------------------

fn rejection(log: &Log, task_id: u64) -> Option<RejectionReason> {
    log.of(task_id).into_iter().find_map(|event| match event {
        LifecycleEvent::Rejected { reason, .. } => Some(reason),
        _ => None,
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lifecycle_events_rejected_at_submit_in_both_pools() {
    let expired = |id| {
        let mut task = task(id, 1, sleep_job(0));
        task.meta.deadline_ms = Some(1);
        task
    };

    // ResourcePool: one unit, one parked slot.
    let log = Arc::new(Log::default());
    let pool = resource_pool(1, 1, &log);
    pool.submit(task(1, 1, sleep_job(500))).await.expect("runs");
    pool.submit(task(2, 1, sleep_job(0))).await.expect("parks");
    let full = pool.submit(task(3, 1, sleep_job(0))).await;
    assert!(matches!(full, Err(SchedulerError::QueueFull { .. })));
    assert!(pool.submit(task(4, 2, sleep_job(0))).await.is_err());
    assert!(pool.submit(expired(5)).await.is_err());
    pool.shutdown();
    assert!(pool.submit(task(6, 1, sleep_job(0))).await.is_err());
    let ids: BTreeSet<u64> = (1..=6).collect();
    let events = settle(pool.hooks(), &log, &ids).await;
    assert_ordered(pool.hooks(), &events, &ids);
    assert_eq!(rejection(&log, 3), Some(RejectionReason::QueueFull));
    assert_eq!(
        rejection(&log, 4),
        Some(RejectionReason::CapacityExceeded { kind: None })
    );
    assert_eq!(rejection(&log, 5), Some(RejectionReason::DeadlineExpired));
    assert_eq!(rejection(&log, 6), Some(RejectionReason::PoolShutdown));

    // WorkerPool: the same refusals.
    let log = Arc::new(Log::default());
    let pool = worker_pool(1, 1, &log);
    pool.submit(sleep_job(500), meta(1, 1)).expect("runs");
    pool.submit(sleep_job(0), meta(2, 1)).expect("parks");
    let full = pool.submit(sleep_job(0), meta(3, 1));
    assert!(matches!(full, Err(SchedulerError::QueueFull { .. })));
    assert!(pool.submit(sleep_job(0), meta(4, 2)).is_err());
    let mut late = meta(5, 1);
    late.deadline_ms = Some(1);
    assert!(pool.submit(sleep_job(0), late).is_err());
    let report = pool
        .shutdown_with_async(ShutdownPolicy::Drain {
            deadline: Duration::from_secs(5),
        })
        .await;
    assert_eq!(report.cancelled, 0, "{report:?}");
    assert!(pool.submit(sleep_job(0), meta(6, 1)).is_err());
    let events = settle(pool.hooks(), &log, &ids).await;
    assert_ordered(pool.hooks(), &events, &ids);
    assert_eq!(rejection(&log, 3), Some(RejectionReason::QueueFull));
    assert_eq!(
        rejection(&log, 4),
        Some(RejectionReason::CapacityExceeded { kind: None })
    );
    assert_eq!(rejection(&log, 5), Some(RejectionReason::DeadlineExpired));
    assert_eq!(rejection(&log, 6), Some(RejectionReason::PoolShutdown));
}

/// cr-23: a rejection for a per-kind limit names the kind, in both pools.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lifecycle_events_rejected_capacity_names_the_exceeded_kind() {
    let vram = |id| {
        let mut task = task(id, 1, sleep_job(0));
        task.meta.cost = ResourceCost::new(ResourceKind::GpuVram, 3);
        task
    };
    let wanted = Some(RejectionReason::CapacityExceeded {
        kind: Some(ResourceKind::GpuVram),
    });

    let log = Arc::new(Log::default());
    let pool = resource_pool(4, 1, &log).with_resource_limit(ResourceKind::GpuVram, 2);
    let refused = pool.submit(vram(1)).await;
    assert!(
        matches!(
            refused,
            Err(SchedulerError::CapacityExceeded {
                kind: Some(ResourceKind::GpuVram),
                ..
            })
        ),
        "{refused:?}"
    );
    let ids: BTreeSet<u64> = BTreeSet::from([1]);
    let events = settle(pool.hooks(), &log, &ids).await;
    assert_ordered(pool.hooks(), &events, &ids);
    assert_eq!(rejection(&log, 1), wanted);

    let log = Arc::new(Log::default());
    let pool = WorkerPool::new(
        WorkerPoolConfig::new()
            .with_worker_count(1)
            .with_max_units(4)
            .with_max_queue_depth(1)
            .with_timeout_ms(10_000)
            .with_resource_limit(ResourceKind::GpuVram, 2),
        Runner,
    )
    .expect("worker pool")
    .with_hook(Arc::clone(&log) as Arc<dyn Hook>);
    let mut meta = meta(1, 1);
    meta.cost = ResourceCost::new(ResourceKind::GpuVram, 3);
    assert!(pool.submit(sleep_job(0), meta).is_err());
    let events = settle(pool.hooks(), &log, &ids).await;
    assert_ordered(pool.hooks(), &events, &ids);
    assert_eq!(rejection(&log, 1), wanted);
}

// ---------------------------------------------------------------------------
// Durable pool: completed, and requeued at shutdown
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_events_durable_pool_requeued_at_shutdown() {
    let log = Arc::new(Log::default());
    let bus = HookBus::new();
    bus.register(Arc::clone(&log) as Arc<dyn Hook>);
    let pool = DurableResourcePool::start(
        PoolLimits {
            max_units: 2,
            max_queue_depth: 100,
            default_timeout: Duration::from_secs(60),
        },
        InMemoryLeaseQueue::<Job>::new(100),
        InMemoryResultMailbox::<String>::new(),
        Runner,
        TokioSpawner::new(tokio::runtime::Handle::current()),
        DurableOptions::new(Duration::from_secs(30))
            .with_name("s21")
            .with_poll_interval(Duration::from_millis(20))
            .with_hooks(bus.clone()),
    )
    .expect("start");
    assert_eq!(pool.hooks(), &bus);

    // 1 completes; 2 runs long and is cancelled at the deadline; 3 needs both
    // units, is leased and waits for capacity until the shutdown hands it
    // back.
    pool.submit(task(1, 1, sleep_job(0)))
        .await
        .expect("accepted");
    wait_for(&log, 1, |event| event.kind() == "completed").await;
    pool.submit(task(2, 1, sleep_job(30_000)))
        .await
        .expect("accepted");
    wait_for(&log, 1, |event| {
        event.kind() == "started" && event.task_id() == 2
    })
    .await;
    pool.submit(task(3, 2, sleep_job(0)))
        .await
        .expect("accepted");
    wait_for(&log, 1, |event| event.kind() == "queued").await;

    let report = pool
        .shutdown_with(ShutdownPolicy::DrainThenCancel {
            deadline: Duration::from_millis(100),
        })
        .await;
    assert_eq!(report.requeued, 2, "{report:?}");

    let ids: BTreeSet<u64> = (1..=3).collect();
    let events = settle(&bus, &log, &ids).await;
    assert_ordered(&bus, &events, &ids);
    let kinds = |id| -> Vec<&'static str> { log.of(id).iter().map(LifecycleEvent::kind).collect() };
    assert_eq!(kinds(1), ["submitted", "started", "completed"]);
    assert_eq!(kinds(2), ["submitted", "started", "requeued"]);
    assert_eq!(kinds(3), ["submitted", "queued", "requeued"]);
}

// ---------------------------------------------------------------------------
// The oracle itself
// ---------------------------------------------------------------------------

/// A result mailbox whose first `fail` deliveries fail.
struct FlakyMailbox {
    inner: InMemoryResultMailbox<String>,
    fail: Mutex<u32>,
}

impl ResultMailbox<String> for FlakyMailbox {
    async fn deliver(&self, delivery: Delivery<String>) -> Result<DeliveryOutcome, SchedulerError> {
        let failing = {
            let mut left = self.fail.lock();
            left.checked_sub(1).map(|rest| *left = rest).is_some()
        };
        if failing {
            return Err(SchedulerError::internal("injected delivery failure"));
        }
        self.inner.deliver(delivery).await
    }

    fn consume(
        &self,
        key: &MailboxKey,
    ) -> impl Future<Output = Result<Option<MailboxEntry<String>>, SchedulerError>> + Send {
        self.inner.consume(key)
    }

    fn peek(
        &self,
        key: &MailboxKey,
    ) -> impl Future<Output = Result<Option<MailboxEntry<String>>, SchedulerError>> + Send {
        self.inner.peek(key)
    }

    fn evict_expired(&self) -> impl Future<Output = Result<usize, SchedulerError>> + Send {
        self.inner.evict_expired()
    }
}

fn durable_options(bus: &HookBus) -> DurableOptions {
    DurableOptions::new(Duration::from_secs(30))
        .with_name("s21")
        .with_poll_interval(Duration::from_millis(20))
        .with_hooks(bus.clone())
}

/// A durable run whose delivery keeps failing is handed back (`Requeued`
/// ends that run) and the next run completes: one terminal event per run.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_events_durable_delivery_failure_requeued_then_completed() {
    let log = Arc::new(Log::default());
    let bus = HookBus::new();
    bus.register(Arc::clone(&log) as Arc<dyn Hook>);
    let pool = DurableResourcePool::start(
        PoolLimits {
            max_units: 2,
            max_queue_depth: 100,
            default_timeout: Duration::from_secs(60),
        },
        InMemoryLeaseQueue::<Job>::new(100),
        // Five failures: every delivery attempt of the first run fails.
        FlakyMailbox {
            inner: InMemoryResultMailbox::new(),
            fail: Mutex::new(5),
        },
        Runner,
        TokioSpawner::new(tokio::runtime::Handle::current()),
        durable_options(&bus),
    )
    .expect("start");
    pool.submit(task(1, 1, sleep_job(0)))
        .await
        .expect("accepted");
    wait_for(&log, 1, |event| event.kind() == "completed").await;

    let ids: BTreeSet<u64> = BTreeSet::from([1]);
    let events = settle(&bus, &log, &ids).await;
    assert_ordered(&bus, &events, &ids);
    let kinds: Vec<&str> = log.of(1).iter().map(LifecycleEvent::kind).collect();
    assert_eq!(
        kinds,
        ["submitted", "started", "requeued", "started", "completed"]
    );
    let result = pool.mailbox().consume(&key(1)).await.expect("consume");
    assert!(result.is_some(), "the second run's result is stored");
}

/// A queue whose `enqueue` fails with `error`, the outcome of which the pool
/// cannot know.
struct FailingEnqueue {
    inner: InMemoryLeaseQueue<Job>,
    error: fn() -> SchedulerError,
}

impl LeaseQueue<Job> for FailingEnqueue {
    fn enqueue(
        &self,
        _task: ScheduledTask<Job>,
    ) -> impl Future<Output = Result<Enqueued, SchedulerError>> + Send {
        std::future::ready(Err((self.error)()))
    }

    fn lease(
        &self,
        max: usize,
        lease_for: Duration,
    ) -> impl Future<Output = Result<Vec<LeasedTask<Job>>, SchedulerError>> + Send {
        self.inner.lease(max, lease_for)
    }

    fn ack(&self, token: LeaseToken) -> impl Future<Output = Result<(), SchedulerError>> + Send {
        self.inner.ack(token)
    }

    fn nack(
        &self,
        token: LeaseToken,
        action: NackAction,
    ) -> impl Future<Output = Result<(), SchedulerError>> + Send {
        self.inner.nack(token, action)
    }

    fn extend(
        &self,
        token: LeaseToken,
        lease_for: Duration,
    ) -> impl Future<Output = Result<(), SchedulerError>> + Send {
        self.inner.extend(token, lease_for)
    }

    fn depth(&self) -> impl Future<Output = Result<QueueDepth, SchedulerError>> + Send {
        self.inner.depth()
    }

    fn max_depth(&self) -> usize {
        self.inner.max_depth()
    }

    fn identity(&self) -> &str {
        self.inner.identity()
    }
}

/// An enqueue that fails with an unknown outcome (a backend error) emits no
/// `Rejected`: the task may have been stored and may still run. A definite
/// refusal (a full queue) is `Rejected`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lifecycle_events_durable_unknown_enqueue_outcome_is_not_rejected() {
    type Case = (u64, fn() -> SchedulerError, &'static [&'static str]);
    let cases: [Case; 2] = [
        (
            1,
            || SchedulerError::internal("connection lost mid-commit"),
            &["submitted"],
        ),
        (
            2,
            || SchedulerError::queue_full("q", 1, 1),
            &["submitted", "rejected"],
        ),
    ];
    for (id, error, expected) in cases {
        let log = Arc::new(Log::default());
        let bus = HookBus::new();
        bus.register(Arc::clone(&log) as Arc<dyn Hook>);
        let pool = DurableResourcePool::start(
            PoolLimits {
                max_units: 2,
                max_queue_depth: 100,
                default_timeout: Duration::from_secs(60),
            },
            FailingEnqueue {
                inner: InMemoryLeaseQueue::new(100),
                error,
            },
            InMemoryResultMailbox::<String>::new(),
            Runner,
            TokioSpawner::new(tokio::runtime::Handle::current()),
            durable_options(&bus),
        )
        .expect("start");
        assert!(pool.submit(task(id, 1, sleep_job(0))).await.is_err());
        flush(&bus).await;
        let kinds: Vec<&str> = log.of(id).iter().map(LifecycleEvent::kind).collect();
        assert_eq!(kinds, expected, "task {id}");
    }
}

#[test]
fn lifecycle_events_oracle_rejects_bad_sequences() {
    assert!(check_sequence(&["submitted", "queued", "started", "completed"]).is_ok());
    assert!(check_sequence(&["submitted", "rejected"]).is_ok());
    assert!(check_sequence(&["submitted", "queued", "cancelled"]).is_ok());
    assert!(check_sequence(&["submitted", "started"]).is_err());
    assert!(check_sequence(&["started", "submitted", "completed"]).is_err());
    assert!(check_sequence(&["submitted", "completed", "failed"]).is_err());
    assert!(check_sequence(&["submitted", "started", "queued", "completed"]).is_err());
    // Durable runs: `Requeued` ends a run, and another may follow.
    assert!(check_sequence(&["submitted", "started", "requeued", "started", "completed"]).is_ok());
    assert!(check_sequence(&["submitted", "queued", "requeued"]).is_ok());
    assert!(check_sequence(&["submitted", "requeued", "queued", "started", "failed"]).is_ok());
    assert!(check_sequence(&["submitted", "started", "requeued", "started"]).is_err());
    assert!(check_sequence(&["submitted", "started", "started", "completed"]).is_err());
    assert!(check_sequence(&["submitted", "requeued", "submitted", "completed"]).is_err());
    assert!(check_sequence(&["submitted", "completed", "started", "completed"]).is_err());
}
