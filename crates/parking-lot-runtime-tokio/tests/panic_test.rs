//! S-3: executor panic containment in `ResourcePool` and the native
//! `WorkerPool`.
//!
//! Oracle: of 100 tasks on a one-slot pool, the one that panics is delivered
//! as `TaskError::ExecutorPanicked`, the other 99 complete, capacity returns to
//! zero, the failure is counted once and the pool keeps serving. A panic that
//! gets past the executor containment (a panic payload whose destructor
//! panics) is caught by the pool's backstop: the task is still reported, the
//! parked work behind it still runs, and a `WorkerPool` worker that died is
//! replaced.
//!
//! Run with:
//!
//! ```text
//! cargo nextest run -p prometheus-parking-lot-runtime-tokio --locked -E "test(panic_)"
//! ```
//!
//! Negative control: under `--cfg parking_lot_negative_control="s-3"` panics
//! are not caught, and every `panic_*` test fails (it waits, bounded by
//! `WAIT`, for an outcome or a worker that never comes).

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::sync::{Barrier, Notify};

use prometheus_parking_lot_core::config::WorkerPoolConfig;
use prometheus_parking_lot_core::{
    InMemoryMailbox, InMemoryQueue, Mailbox, MailboxKey, PoolLimits, Priority, ResourceCost,
    ResourceKind, ScheduledTask, SchedulerError, Spawn, TaskError, TaskExecutor, TaskMetadata,
    TaskStatus, WorkerExecutor,
};
use prometheus_parking_lot_runtime_tokio::{ResourcePool, WorkerPool};

/// Upper bound on any wait in these tests; reaching it fails the test.
const WAIT: Duration = Duration::from_secs(10);
/// Tasks per run in the 1-in-100 scenarios.
const TASKS: u64 = 100;
/// The task id that panics in the 1-in-100 scenarios.
const PANICKING: u64 = 50;
/// Panic message of `Job::Panic`.
const BOOM: &str = "executor boom";
/// Panic message of `BadPayload`'s destructor.
const PAYLOAD_DROP: &str = "panic payload destructor panicked";

/// What the executor does with a job.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
enum Job {
    /// Yield once, then return the number.
    Echo(u64),
    /// Panic with `BOOM`.
    Panic,
    /// Panic with a `BadPayload`, whose destructor panics again.
    PanicWithBadPayload,
    /// Wait until two rendezvous jobs run at once, then return 0.
    Rendezvous,
    /// Wait until the test opens the executor's gate, then return the number.
    Gated(u64),
}

/// A panic payload whose destructor panics: the pool's executor containment
/// catches the first panic, and dropping this payload then panics outside it.
struct BadPayload;

impl Drop for BadPayload {
    fn drop(&mut self) {
        panic!("{PAYLOAD_DROP}");
    }
}

#[derive(Clone)]
struct PanicExecutor {
    rendezvous: Arc<Barrier>,
    gate: Arc<Notify>,
}

impl PanicExecutor {
    fn new() -> Self {
        Self {
            rendezvous: Arc::new(Barrier::new(2)),
            gate: Arc::new(Notify::new()),
        }
    }

    async fn run(&self, job: Job) -> Result<u64, TaskError> {
        match job {
            Job::Echo(value) => {
                tokio::task::yield_now().await;
                Ok(value)
            }
            Job::Panic => panic!("{BOOM}"),
            Job::PanicWithBadPayload => std::panic::panic_any(BadPayload),
            Job::Rendezvous => {
                self.rendezvous.wait().await;
                Ok(0)
            }
            Job::Gated(value) => {
                self.gate.notified().await;
                Ok(value)
            }
        }
    }
}

#[async_trait]
impl TaskExecutor<Job, u64> for PanicExecutor {
    async fn execute(&self, job: Job, _meta: TaskMetadata) -> Result<u64, TaskError> {
        self.run(job).await
    }
}

#[async_trait]
impl WorkerExecutor<Job, u64> for PanicExecutor {
    async fn execute(&self, job: Job, _meta: TaskMetadata) -> Result<u64, TaskError> {
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

/// A mailbox the test reads while the pool owns a clone of it. Delivering
/// for the key in `panic_on` panics (once).
#[derive(Clone, Default)]
struct SharedMailbox {
    inner: Arc<Mutex<InMemoryMailbox<u64>>>,
    delivered: Arc<Notify>,
    panic_on: Arc<Mutex<Option<MailboxKey>>>,
}

impl Mailbox<u64> for SharedMailbox {
    fn deliver(
        &mut self,
        key: &MailboxKey,
        status: TaskStatus,
        payload: Option<u64>,
    ) -> Result<(), SchedulerError> {
        let panics = {
            let mut panic_on = self.panic_on.lock();
            panic_on.as_ref() == Some(key) && panic_on.take().is_some()
        };
        assert!(!panics, "mailbox panicked delivering {key:?}");
        self.inner.lock().deliver(key, status, payload)?;
        self.delivered.notify_waiters();
        Ok(())
    }
}

impl SharedMailbox {
    /// Wait, until `deadline`, for the outcome delivered for `key`.
    async fn take(&self, key: &MailboxKey, deadline: Instant) -> (TaskStatus, Option<u64>) {
        let wait = async {
            loop {
                let notified = self.delivered.notified();
                let message = self.inner.lock().consume(key).expect("consume");
                if let Some(message) = message {
                    return message;
                }
                notified.await;
            }
        };
        tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), wait)
            .await
            .unwrap_or_else(|_| panic!("no outcome delivered for {key:?} within {WAIT:?}"))
    }
}

type Pool = ResourcePool<Job, u64, InMemoryQueue<Job>, SharedMailbox, PanicExecutor, TokioSpawner>;

/// A one-unit `ResourcePool`: one task runs at a time, the rest park.
fn resource_pool(mailbox: &SharedMailbox) -> Pool {
    resource_pool_with(mailbox, PanicExecutor::new())
}

fn resource_pool_with(mailbox: &SharedMailbox, executor: PanicExecutor) -> Pool {
    ResourcePool::new(
        PoolLimits {
            max_units: 1,
            max_queue_depth: 128,
            default_timeout: WAIT,
        },
        InMemoryQueue::new(128),
        mailbox.clone(),
        executor,
        TokioSpawner,
    )
}

/// A native `WorkerPool` of `workers` workers and as many units.
fn worker_pool(workers: usize) -> WorkerPool<Job, u64, PanicExecutor> {
    let budget_ms = u64::try_from(WAIT.as_millis()).expect("budget fits u64");
    WorkerPool::new(
        WorkerPoolConfig::new()
            .with_worker_count(workers)
            .with_max_units(u32::try_from(workers).expect("small"))
            .with_max_queue_depth(128)
            .with_timeout_ms(budget_ms),
        PanicExecutor::new(),
    )
    .expect("valid config")
}

fn key(id: u64) -> MailboxKey {
    MailboxKey {
        tenant: "cr-21-panic".into(),
        user_id: None,
        session_id: Some(id.to_string()),
    }
}

fn meta(id: u64) -> TaskMetadata {
    TaskMetadata {
        id,
        mailbox: Some(key(id)),
        priority: Priority::Normal,
        cost: ResourceCost::new(ResourceKind::GpuVram, 1),
        deadline_ms: None,
        created_at_ms: 0,
        idempotency_key: None,
        sequence: 0,
    }
}

fn task(id: u64, job: Job) -> ScheduledTask<Job> {
    ScheduledTask {
        meta: meta(id),
        payload: job,
    }
}

/// The job for task `id` in the 1-in-100 scenarios.
const fn job_for(id: u64) -> Job {
    if id == PANICKING {
        Job::Panic
    } else {
        Job::Echo(id)
    }
}

fn assert_panicked_status(status: &TaskStatus, expected: &str) {
    match status {
        TaskStatus::Failed(TaskError::ExecutorPanicked { message }) => {
            assert_eq!(message, expected, "wrong panic message");
        }
        other => panic!("expected Failed(ExecutorPanicked), got {other:?}"),
    }
}

fn assert_panicked_error(result: Result<u64, SchedulerError>, expected: &str) {
    match result {
        Err(SchedulerError::Task(TaskError::ExecutorPanicked { message })) => {
            assert_eq!(message, expected, "wrong panic message");
        }
        other => panic!("expected Task(ExecutorPanicked), got {other:?}"),
    }
}

/// Time left until `deadline` (zero once it passed).
fn left(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}

// ── ResourcePool ────────────────────────────────────────────────────────────

/// One panic among 100 tasks on a one-unit pool: the 99 others complete (they
/// were all parked behind running tasks, so each depends on the previous
/// task's units coming back), the panic is delivered as `ExecutorPanicked`,
/// nothing stays held or parked, and the pool still runs a full-capacity task.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn panic_resource_pool_one_in_100_is_contained() {
    let mailbox = SharedMailbox::default();
    let pool = resource_pool(&mailbox);

    for id in 0..TASKS {
        pool.submit(task(id, job_for(id))).await.expect("accepted");
    }

    let deadline = Instant::now() + WAIT;
    let mut completed = 0_u64;
    for id in 0..TASKS {
        let (status, payload) = mailbox.take(&key(id), deadline).await;
        if id == PANICKING {
            assert_panicked_status(&status, BOOM);
            assert_eq!(payload, None);
        } else {
            assert_eq!(status, TaskStatus::Completed, "task {id}");
            assert_eq!(payload, Some(id));
            completed += 1;
        }
    }
    assert_eq!(completed, TASKS - 1);
    assert_eq!(pool.active_units(), 0, "capacity leaked");
    assert_eq!(pool.running_tasks(), 0);
    assert_eq!(pool.queued_tasks(), 0);
    assert_eq!(pool.completed_tasks(), TASKS - 1);
    assert_eq!(pool.failed_tasks(), 1);

    // The pool keeps serving, at its full capacity.
    pool.submit(task(TASKS, Job::Echo(7)))
        .await
        .expect("accepted");
    let (status, payload) = mailbox.take(&key(TASKS), Instant::now() + WAIT).await;
    assert_eq!(status, TaskStatus::Completed);
    assert_eq!(payload, Some(7));
    assert_eq!(pool.active_units(), 0);
    assert_eq!(pool.completed_tasks(), TASKS);
    assert_eq!(pool.failed_tasks(), 1);
}

/// A panic that escapes the executor containment (its payload's destructor
/// panics) still reports the task, releases its unit and starts the task
/// parked behind it (`Dispatcher::poke`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn panic_resource_pool_escaped_panic_wakes_parked_task() {
    let mailbox = SharedMailbox::default();
    let pool = resource_pool(&mailbox);

    assert_eq!(
        pool.submit(task(1, Job::PanicWithBadPayload))
            .await
            .expect("accepted"),
        TaskStatus::Running
    );
    pool.submit(task(2, Job::Echo(2))).await.expect("accepted");

    let deadline = Instant::now() + WAIT;
    let (status, payload) = mailbox.take(&key(1), deadline).await;
    assert_panicked_status(&status, PAYLOAD_DROP);
    assert_eq!(payload, None);
    let (status, payload) = mailbox.take(&key(2), deadline).await;
    assert_eq!(status, TaskStatus::Completed);
    assert_eq!(payload, Some(2));
    assert_eq!(pool.active_units(), 0, "capacity leaked");
    assert_eq!(pool.queued_tasks(), 0);
    assert_eq!(pool.completed_tasks(), 1);
    assert_eq!(pool.failed_tasks(), 1);
}

/// A user `Mailbox` that panics while a completion delivers its outcome does
/// not lose the task that completion woke: the woken task is started before
/// the mailbox is called. (Not a `panic_` test: it does not depend on the
/// executor containment, so the S-3 control does not apply to it.)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resource_pool_woken_task_survives_a_mailbox_that_panics() {
    let mailbox = SharedMailbox::default();
    *mailbox.panic_on.lock() = Some(key(1));
    let executor = PanicExecutor::new();
    let pool = resource_pool_with(&mailbox, executor.clone());

    // 1 holds the only unit until the gate opens; 2 parks behind it.
    assert_eq!(
        pool.submit(task(1, Job::Gated(1))).await.expect("accepted"),
        TaskStatus::Running
    );
    pool.submit(task(2, Job::Echo(2))).await.expect("accepted");
    assert_eq!(pool.queued_tasks(), 1);
    executor.gate.notify_one();

    // 1's completion wakes 2, then panics in the mailbox: 2 must still run.
    let (status, payload) = mailbox.take(&key(2), Instant::now() + WAIT).await;
    assert_eq!(status, TaskStatus::Completed);
    assert_eq!(payload, Some(2));
    assert_eq!(pool.queued_tasks(), 0);
    let idle = Instant::now() + WAIT;
    while pool.active_units() != 0 {
        assert!(Instant::now() < idle, "capacity leaked");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

// ── native WorkerPool ───────────────────────────────────────────────────────

/// One panic among 100 tasks on a one-worker, one-unit pool: the single
/// worker survives the panic and runs the other 99, the panic is returned as
/// `ExecutorPanicked`, counters and capacity are right, and the pool keeps
/// serving.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn panic_worker_pool_one_in_100_is_contained() {
    let pool = worker_pool(1);

    let mut keys = Vec::new();
    for id in 0..TASKS {
        keys.push(pool.submit(job_for(id), meta(id)).expect("accepted"));
    }

    let deadline = Instant::now() + WAIT;
    for (id, key) in (0..TASKS).zip(&keys) {
        let result = pool.retrieve_async(key, left(deadline)).await;
        if id == PANICKING {
            assert_panicked_error(result, BOOM);
        } else {
            assert_eq!(result.expect("task completed"), id);
        }
    }
    let stats = pool.stats();
    assert_eq!(stats.used_units, 0, "capacity leaked");
    assert_eq!(stats.active_tasks, 0);
    assert_eq!(stats.queued_tasks, 0);
    assert_eq!(stats.submitted_tasks, TASKS);
    assert_eq!(stats.completed_tasks, TASKS - 1);
    assert_eq!(stats.failed_tasks, 1);
    assert_eq!(stats.worker_count, 1);

    // The pool keeps serving, at its full capacity.
    let again = pool.submit(Job::Echo(7), meta(TASKS)).expect("accepted");
    assert_eq!(pool.retrieve_async(&again, WAIT).await.expect("ran"), 7);
    assert_eq!(pool.stats().completed_tasks, TASKS);
    pool.shutdown();
}

/// A panic that escapes the executor containment kills its worker thread.
/// The task is still reported as panicked, the worker is replaced, and the
/// pool again runs two tasks at once: the two rendezvous tasks each wait for
/// the other, so with one worker left they would both time out.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn panic_worker_pool_dead_worker_is_replaced() {
    let pool = worker_pool(2);

    let bad = pool
        .submit(Job::PanicWithBadPayload, meta(1))
        .expect("accepted");
    assert_panicked_error(pool.retrieve_async(&bad, WAIT).await, PAYLOAD_DROP);

    let first = pool.submit(Job::Rendezvous, meta(2)).expect("accepted");
    let second = pool.submit(Job::Rendezvous, meta(3)).expect("accepted");
    let deadline = Instant::now() + WAIT;
    assert_eq!(
        pool.retrieve_async(&first, left(deadline))
            .await
            .expect("first rendezvous ran"),
        0
    );
    assert_eq!(
        pool.retrieve_async(&second, left(deadline))
            .await
            .expect("second rendezvous ran"),
        0
    );

    let stats = pool.stats();
    assert_eq!(stats.worker_count, 2, "the dead worker was not replaced");
    assert_eq!(stats.used_units, 0, "capacity leaked");
    assert_eq!(stats.active_tasks, 0);
    assert_eq!(stats.completed_tasks, 2);
    assert_eq!(stats.failed_tasks, 1);
    pool.shutdown();
}
