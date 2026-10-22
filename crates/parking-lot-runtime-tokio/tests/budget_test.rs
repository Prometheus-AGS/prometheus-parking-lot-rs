//! S-4: bounded execution in `ResourcePool` and the native `WorkerPool`.
//!
//! A never-returning executor is cancelled at its budget (the task's deadline
//! or the pool's default timeout, whichever is nearer), its units are released
//! so the task parked behind it runs, and the timeout is delivered. A *cancel
//! witness* — a guard whose `Drop` records a cancellation only if the executor
//! future is dropped before it finished — proves the executor was actually
//! cancelled, and that an in-budget task is not.
//!
//! Run with:
//!
//! ```text
//! cargo nextest run -p prometheus-parking-lot-runtime-tokio --locked -E "test(budget_)"
//! ```
//!
//! Negative control: under `--cfg parking_lot_negative_control="s-4"` the
//! pools do not apply the budget, and every `budget_*_wedged_*` and
//! `budget_*_deadline_*` test fails (it waits for a timeout outcome that never
//! comes, bounded by `WAIT`).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

use prometheus_parking_lot_core::config::WorkerPoolConfig;
use prometheus_parking_lot_core::{
    InMemoryMailbox, InMemoryQueue, Mailbox, MailboxKey, PoolLimits, Priority, ResourceCost,
    ResourceKind, ScheduledTask, SchedulerError, Spawn, TaskError, TaskExecutor, TaskMetadata,
    TaskStatus, TestClock, WorkerExecutor,
};
use prometheus_parking_lot_runtime_tokio::{ResourcePool, WorkerPool};

/// Upper bound on any wait in these tests; reaching it fails the test.
const WAIT: Duration = Duration::from_secs(10);
/// The pools' default execution budget in the wedged-executor tests.
const BUDGET: Duration = Duration::from_millis(200);
/// Fixed wall time for the `TestClock`.
const WALL_MS: u128 = 1_000_000;

/// What the executor does with a job.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
enum Job {
    /// Never return.
    Wedge,
    /// Return at once.
    Quick,
}

/// Sets `cancelled` when dropped while still armed: the executor future was
/// dropped before it finished.
struct CancelWitness {
    cancelled: Arc<AtomicBool>,
    armed: bool,
}

impl Drop for CancelWitness {
    fn drop(&mut self) {
        if self.armed {
            self.cancelled.store(true, Ordering::SeqCst);
        }
    }
}

#[derive(Clone, Default)]
struct WitnessExecutor {
    /// One flag per job kind: did a job of this kind get cancelled?
    wedge_cancelled: Arc<AtomicBool>,
    quick_cancelled: Arc<AtomicBool>,
    quick_runs: Arc<AtomicUsize>,
}

impl WitnessExecutor {
    async fn run(&self, job: Job) -> Result<String, TaskError> {
        let mut witness = CancelWitness {
            cancelled: Arc::clone(match job {
                Job::Wedge => &self.wedge_cancelled,
                Job::Quick => &self.quick_cancelled,
            }),
            armed: true,
        };
        if job == Job::Wedge {
            std::future::pending::<()>().await;
        }
        self.quick_runs.fetch_add(1, Ordering::SeqCst);
        witness.armed = false;
        Ok(format!("{job:?}"))
    }
}

#[async_trait]
impl TaskExecutor<Job, String> for WitnessExecutor {
    async fn execute(&self, job: Job, _meta: TaskMetadata) -> Result<String, TaskError> {
        self.run(job).await
    }
}

#[async_trait]
impl WorkerExecutor<Job, String> for WitnessExecutor {
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
    /// Wait (at most `WAIT`) for the outcome delivered for `key`.
    async fn take(&self, key: &MailboxKey) -> (TaskStatus, Option<String>) {
        tokio::time::timeout(WAIT, async {
            loop {
                let notified = self.delivered.notified();
                let message = self.inner.lock().consume(key).expect("consume");
                if let Some(message) = message {
                    return message;
                }
                notified.await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("no outcome delivered for {key:?} within {WAIT:?}"))
    }
}

type Pool =
    ResourcePool<Job, String, InMemoryQueue<Job>, SharedMailbox, WitnessExecutor, TokioSpawner>;

fn resource_pool(executor: &WitnessExecutor, mailbox: &SharedMailbox, budget: Duration) -> Pool {
    ResourcePool::new(
        PoolLimits {
            max_units: 1,
            max_queue_depth: 8,
            default_timeout: budget,
        },
        InMemoryQueue::new(8),
        mailbox.clone(),
        executor.clone(),
        TokioSpawner,
    )
    .with_clock(TestClock::new(WALL_MS))
}

fn key(name: &str) -> MailboxKey {
    MailboxKey {
        tenant: "cr-19-budget".into(),
        user_id: None,
        session_id: Some(name.into()),
    }
}

fn meta(id: u64, mailbox: Option<MailboxKey>, deadline_ms: Option<u128>) -> TaskMetadata {
    TaskMetadata {
        id,
        mailbox,
        priority: Priority::Normal,
        cost: ResourceCost::new(ResourceKind::GpuVram, 1),
        deadline_ms,
        created_at_ms: WALL_MS,
        idempotency_key: None,
        sequence: 0,
    }
}

fn assert_timed_out(status: &TaskStatus, expected_budget: Duration) {
    match status {
        TaskStatus::Failed(TaskError::Timeout { budget }) => {
            assert_eq!(*budget, expected_budget, "timed out with the wrong budget");
        }
        other => panic!("expected Failed(Timeout), got {other:?}"),
    }
}

// ── ResourcePool ────────────────────────────────────────────────────────────

/// The wedged task holds the only unit; the quick task parks behind it. At
/// the budget the wedged executor is cancelled, the timeout is delivered, the
/// unit is released and the quick task runs.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn budget_resource_pool_wedged_executor_is_cancelled() {
    let executor = WitnessExecutor::default();
    let mailbox = SharedMailbox::default();
    let pool = resource_pool(&executor, &mailbox, BUDGET);
    let started = Instant::now();

    let wedged = ScheduledTask {
        meta: meta(1, Some(key("wedged")), None),
        payload: Job::Wedge,
    };
    assert_eq!(
        pool.submit(wedged).await.expect("accepted"),
        TaskStatus::Running
    );
    let quick = ScheduledTask {
        meta: meta(2, Some(key("quick")), None),
        payload: Job::Quick,
    };
    assert_eq!(
        pool.submit(quick).await.expect("accepted"),
        TaskStatus::Queued
    );

    let (status, payload) = mailbox.take(&key("wedged")).await;
    assert_timed_out(&status, BUDGET);
    assert!(payload.is_none());
    assert!(started.elapsed() >= BUDGET, "cancelled before its budget");
    assert!(
        executor.wedge_cancelled.load(Ordering::SeqCst),
        "the wedged executor future was not dropped"
    );

    let (status, payload) = mailbox.take(&key("quick")).await;
    assert_eq!(status, TaskStatus::Completed);
    assert_eq!(payload.as_deref(), Some("Quick"));
    assert!(
        !executor.quick_cancelled.load(Ordering::SeqCst),
        "the witness fired for an in-budget task"
    );
    assert_eq!(pool.active_units(), 0);
}

/// A deadline nearer than the default timeout is the budget; the deadline is
/// read through the pool's `TestClock`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn budget_resource_pool_deadline_nearer_than_default_is_the_budget() {
    let executor = WitnessExecutor::default();
    let mailbox = SharedMailbox::default();
    let pool = resource_pool(&executor, &mailbox, Duration::from_secs(3_600));
    let deadline_budget = Duration::from_millis(150);

    let wedged = ScheduledTask {
        meta: meta(1, Some(key("deadline")), Some(WALL_MS + 150)),
        payload: Job::Wedge,
    };
    pool.submit(wedged).await.expect("accepted");
    let (status, _) = mailbox.take(&key("deadline")).await;
    assert_timed_out(&status, deadline_budget);
    assert!(executor.wedge_cancelled.load(Ordering::SeqCst));
    assert_eq!(pool.active_units(), 0);
}

/// Control for the witness: a task that finishes within its budget is not
/// cancelled.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn budget_resource_pool_in_budget_task_is_not_cancelled() {
    let executor = WitnessExecutor::default();
    let mailbox = SharedMailbox::default();
    let pool = resource_pool(&executor, &mailbox, BUDGET);
    let quick = ScheduledTask {
        meta: meta(1, Some(key("quick")), None),
        payload: Job::Quick,
    };
    pool.submit(quick).await.expect("accepted");
    let (status, _) = mailbox.take(&key("quick")).await;
    assert_eq!(status, TaskStatus::Completed);
    assert!(!executor.quick_cancelled.load(Ordering::SeqCst));
    assert_eq!(executor.quick_runs.load(Ordering::SeqCst), 1);
}

// ── native WorkerPool ───────────────────────────────────────────────────────

fn worker_pool(
    executor: &WitnessExecutor,
    budget: Duration,
) -> WorkerPool<Job, String, WitnessExecutor> {
    let budget_ms = u64::try_from(budget.as_millis()).expect("budget fits u64");
    WorkerPool::new(
        WorkerPoolConfig::new()
            .with_worker_count(2)
            .with_max_units(1)
            .with_max_queue_depth(8)
            .with_timeout_ms(budget_ms),
        executor.clone(),
    )
    .expect("valid config")
    .with_clock(TestClock::new(WALL_MS))
}

/// The wedged task holds the only unit (two workers, so only `max_units`
/// keeps the quick task waiting). At the budget the executor is cancelled,
/// `retrieve` returns the timeout, the unit is released and the quick task
/// runs.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn budget_worker_pool_wedged_executor_is_cancelled() {
    let executor = WitnessExecutor::default();
    let pool = worker_pool(&executor, BUDGET);
    let started = Instant::now();

    let wedged = pool
        .submit_async(Job::Wedge, meta(1, None, None))
        .await
        .expect("accepted");
    let quick = pool
        .submit_async(Job::Quick, meta(2, None, None))
        .await
        .expect("accepted");
    assert_eq!(
        pool.stats().queued_tasks,
        1,
        "max_units must hold the quick task"
    );

    match pool.retrieve_async(&wedged, WAIT).await {
        Err(SchedulerError::Task(TaskError::Timeout { budget })) => assert_eq!(budget, BUDGET),
        other => panic!("expected Task(Timeout), got {other:?}"),
    }
    assert!(started.elapsed() >= BUDGET, "cancelled before its budget");
    assert!(
        executor.wedge_cancelled.load(Ordering::SeqCst),
        "the wedged executor future was not dropped"
    );

    let value = pool
        .retrieve_async(&quick, WAIT)
        .await
        .expect("quick task ran");
    assert_eq!(value, "Quick");
    assert!(!executor.quick_cancelled.load(Ordering::SeqCst));
    let stats = pool.stats();
    assert_eq!(stats.used_units, 0);
    assert_eq!(stats.failed_tasks, 1);
    assert_eq!(stats.completed_tasks, 1);
    pool.shutdown();
}

/// A deadline nearer than the default timeout is the budget (read through
/// the pool's `TestClock`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn budget_worker_pool_deadline_nearer_than_default_is_the_budget() {
    let executor = WitnessExecutor::default();
    let pool = worker_pool(&executor, Duration::from_secs(3_600));
    let key = pool
        .submit_async(Job::Wedge, meta(1, None, Some(WALL_MS + 150)))
        .await
        .expect("accepted");
    match pool.retrieve_async(&key, WAIT).await {
        Err(SchedulerError::Task(TaskError::Timeout { budget })) => {
            assert_eq!(budget, Duration::from_millis(150));
        }
        other => panic!("expected Task(Timeout), got {other:?}"),
    }
    assert!(executor.wedge_cancelled.load(Ordering::SeqCst));
    assert_eq!(pool.stats().used_units, 0);
    pool.shutdown();
}

/// Control for the witness: an in-budget task is not cancelled.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn budget_worker_pool_in_budget_task_is_not_cancelled() {
    let executor = WitnessExecutor::default();
    let pool = worker_pool(&executor, BUDGET);
    let key = pool
        .submit_async(Job::Quick, meta(1, None, None))
        .await
        .expect("accepted");
    assert_eq!(pool.retrieve_async(&key, WAIT).await.expect("ran"), "Quick");
    assert!(!executor.quick_cancelled.load(Ordering::SeqCst));
    pool.shutdown();
}
