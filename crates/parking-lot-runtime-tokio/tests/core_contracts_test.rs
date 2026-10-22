//! cr-18 core contracts, driven through the real pools on a real tokio
//! runtime:
//!
//! - an executor error is recorded in the mailbox as `Failed` carrying the
//!   error, and the task's capacity is released (`ResourcePool`), or returned
//!   from `retrieve` as `SchedulerError::Task` (`WorkerPool`);
//! - 1,000 equal-priority tasks submitted from one thread to a one-unit pool
//!   run in submission order;
//! - deadlines are judged by the pool's `Clock`, so a `TestClock` expires
//!   tasks without sleeping.
//!
//! The mailbox is shared with the test through `SharedMailbox`, a thin
//! `Mailbox` wrapper over the in-memory mailbox, so outcomes are read back
//! from the mailbox the pool delivered to.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::sync::{Notify, Semaphore};

use prometheus_parking_lot_core::config::WorkerPoolConfig;
use prometheus_parking_lot_core::{
    InMemoryMailbox, InMemoryQueue, Mailbox, MailboxKey, PoolLimits, Priority, ResourceCost,
    ResourceKind, ScheduledTask, SchedulerError, Spawn, TaskError, TaskExecutor, TaskMetadata,
    TaskStatus, TestClock, WorkerExecutor,
};
use prometheus_parking_lot_runtime_tokio::{ResourcePool, WorkerPool};

/// Upper bound on any wait in these tests; reaching it fails the test.
const WAIT: Duration = Duration::from_secs(10);

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

/// What the scripted executor does with a job.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum Action {
    /// Return `Ok(index)`.
    Succeed,
    /// Return a permanent executor error.
    Fail,
    /// Wait for a permit on the gate, then succeed.
    Block,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Job {
    index: u64,
    action: Action,
}

/// Executor that records the order in which jobs ran.
#[derive(Clone)]
struct ScriptedExecutor {
    ran: Arc<Mutex<Vec<u64>>>,
    gate: Arc<Semaphore>,
    ran_one: Arc<Notify>,
}

impl ScriptedExecutor {
    fn new() -> Self {
        Self {
            ran: Arc::new(Mutex::new(Vec::new())),
            gate: Arc::new(Semaphore::new(0)),
            ran_one: Arc::new(Notify::new()),
        }
    }

    fn ran(&self) -> Vec<u64> {
        self.ran.lock().clone()
    }

    /// Wait until `count` jobs have run.
    async fn wait_for_runs(&self, count: usize) {
        tokio::time::timeout(WAIT, async {
            while self.ran.lock().len() < count {
                self.ran_one.notified().await;
            }
        })
        .await
        .expect("jobs did not run in time");
    }
}

#[async_trait]
impl TaskExecutor<Job, String> for ScriptedExecutor {
    async fn execute(&self, job: Job, _meta: TaskMetadata) -> Result<String, TaskError> {
        if matches!(job.action, Action::Block) {
            self.gate
                .acquire()
                .await
                .expect("gate is never closed")
                .forget();
        }
        self.ran.lock().push(job.index);
        self.ran_one.notify_one();
        match job.action {
            Action::Fail => Err(TaskError::executor(format!("job {} failed", job.index))),
            Action::Succeed | Action::Block => Ok(job.index.to_string()),
        }
    }
}

/// A mailbox the test can read while the pool owns a clone of it.
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
        self.delivered.notify_one();
        Ok(())
    }
}

impl SharedMailbox {
    /// Wait for and consume the outcome delivered for `key`.
    async fn take(&self, key: &MailboxKey) -> (TaskStatus, Option<String>) {
        tokio::time::timeout(WAIT, async {
            loop {
                let message = self.inner.lock().consume(key).expect("in-memory consume");
                if let Some(message) = message {
                    return message;
                }
                self.delivered.notified().await;
            }
        })
        .await
        .expect("no outcome delivered in time")
    }
}

type Pool =
    ResourcePool<Job, String, InMemoryQueue<Job>, SharedMailbox, ScriptedExecutor, TokioSpawner>;

fn pool(max_units: u32, executor: &ScriptedExecutor, mailbox: &SharedMailbox) -> Pool {
    ResourcePool::new(
        PoolLimits {
            max_units,
            max_queue_depth: 2_000,
            default_timeout: Duration::from_secs(30),
        },
        InMemoryQueue::new(2_000),
        mailbox.clone(),
        executor.clone(),
        TokioSpawner,
    )
}

fn key(name: &str) -> MailboxKey {
    MailboxKey {
        tenant: "cr-18".into(),
        user_id: None,
        session_id: Some(name.into()),
    }
}

/// One-unit task; every task shares `created_at_ms`, so only the pool's
/// submission sequence can order equal priorities.
fn task(index: u64, action: Action, mailbox: Option<MailboxKey>) -> ScheduledTask<Job> {
    ScheduledTask {
        meta: TaskMetadata {
            id: index,
            mailbox,
            priority: Priority::Normal,
            cost: ResourceCost::new(ResourceKind::GpuVram, 1),
            deadline_ms: None,
            created_at_ms: 1_000,
            idempotency_key: None,
            sequence: 0,
        },
        payload: Job { index, action },
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn executor_error_is_recorded_as_failed_and_capacity_is_released() {
    let executor = ScriptedExecutor::new();
    let mailbox = SharedMailbox::default();
    let pool = pool(1, &executor, &mailbox);

    let status = pool
        .submit(task(1, Action::Fail, Some(key("failing"))))
        .await
        .expect("submit failing task");
    assert_eq!(status, TaskStatus::Running);

    let (status, payload) = mailbox.take(&key("failing")).await;
    assert_eq!(
        status,
        TaskStatus::Failed(TaskError::Executor {
            message: "job 1 failed".into(),
            retryable: false,
        })
    );
    assert_eq!(payload, None);
    // Capacity is released before the outcome is delivered.
    assert_eq!(pool.active_units(), 0);

    // The released unit is usable: the next task starts immediately and
    // completes with its value.
    let status = pool
        .submit(task(2, Action::Succeed, Some(key("next"))))
        .await
        .expect("submit next task");
    assert_eq!(status, TaskStatus::Running);
    let (status, payload) = mailbox.take(&key("next")).await;
    assert_eq!(status, TaskStatus::Completed);
    assert_eq!(payload.as_deref(), Some("2"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn equal_priority_tasks_run_in_submission_order_on_a_one_unit_pool() {
    const TASKS: u64 = 1_000;
    let executor = ScriptedExecutor::new();
    let mailbox = SharedMailbox::default();
    let pool = pool(1, &executor, &mailbox);

    // Submitted from this one task while earlier tasks complete on the
    // runtime's other threads, so submissions race completions throughout.
    for index in 0..TASKS {
        pool.submit(task(index, Action::Succeed, None))
            .await
            .expect("submit");
    }

    executor
        .wait_for_runs(usize::try_from(TASKS).expect("fits"))
        .await;
    let expected: Vec<u64> = (0..TASKS).collect();
    assert_eq!(executor.ran(), expected);
    assert_eq!(pool.active_units(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deadlines_are_judged_by_the_pool_clock() {
    // Wall time 1,000,000 ms: 1970 by the system clock, so every deadline
    // below is long past for the system clock and only the test clock can
    // accept them.
    let clock = TestClock::new(1_000_000);
    let executor = ScriptedExecutor::new();
    let mailbox = SharedMailbox::default();
    let pool = pool(1, &executor, &mailbox).with_clock(clock.clone());

    let with_deadline = |index, action, key: Option<MailboxKey>, deadline_ms| {
        let mut task = task(index, action, key);
        task.meta.deadline_ms = Some(deadline_ms);
        task
    };

    // Occupy the only unit, then park three tasks behind it.
    pool.submit(task(1, Action::Block, Some(key("blocker"))))
        .await
        .expect("submit blocker");
    for parked in [
        with_deadline(2, Action::Succeed, None, 1_001_000),
        with_deadline(3, Action::Succeed, Some(key("survivor")), 1_010_000),
        with_deadline(4, Action::Succeed, Some(key("expires-at-head")), 1_002_500),
    ] {
        assert_eq!(
            pool.submit(parked).await.expect("submit parked"),
            TaskStatus::Queued
        );
    }

    // A deadline already behind the test clock is rejected at submission.
    let err = pool
        .submit(with_deadline(5, Action::Succeed, None, 999_999))
        .await
        .expect_err("expired at submit");
    assert!(
        matches!(err, SchedulerError::DeadlineExpired { task_id: 5 }),
        "{err:?}"
    );

    // Two seconds pass on the test clock only: task 2 is now expired and
    // pruning removes it; tasks 3 and 4 are still in time.
    clock.advance(Duration::from_secs(2));
    assert_eq!(pool.prune_expired().await.expect("prune"), 1);

    // One more second: task 4 expires while still parked.
    clock.advance(Duration::from_secs(1));

    // Release the blocker. The dispatcher starts task 3, then finds task 4
    // expired when it reaches the head of the queue: it never runs, and its
    // mailbox records `Expired`.
    executor.gate.add_permits(1);
    assert_eq!(mailbox.take(&key("blocker")).await.0, TaskStatus::Completed);
    assert_eq!(
        mailbox.take(&key("survivor")).await.0,
        TaskStatus::Completed
    );
    assert_eq!(
        mailbox.take(&key("expires-at-head")).await,
        (TaskStatus::Expired, None)
    );
    assert_eq!(executor.ran(), vec![1, 3]);
    assert_eq!(pool.active_units(), 0);
}

/// Worker-pool executor that fails every job.
#[derive(Clone)]
struct FailingWorker;

#[async_trait]
impl WorkerExecutor<u32, u32> for FailingWorker {
    async fn execute(&self, job: u32, _meta: TaskMetadata) -> Result<u32, TaskError> {
        Err(TaskError::transient(format!("job {job} hit a busy device")))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn worker_pool_returns_executor_errors_from_retrieve() {
    let pool = WorkerPool::new(
        WorkerPoolConfig::new()
            .with_worker_count(1)
            .with_max_units(10)
            .with_max_queue_depth(10),
        FailingWorker,
    )
    .expect("pool");

    let key = pool
        .submit_async(7, task(7, Action::Succeed, None).meta)
        .await
        .expect("submit");
    let err = pool
        .retrieve_async(&key, WAIT)
        .await
        .expect_err("executor error");

    assert!(err.is_retryable(), "{err:?}");
    assert!(
        matches!(
            &err,
            SchedulerError::Task(TaskError::Executor { message, retryable: true })
                if message == "job 7 hit a busy device"
        ),
        "{err:?}"
    );
    let stats = pool.stats();
    assert_eq!(stats.failed_tasks, 1);
    assert_eq!(stats.completed_tasks, 0);
    assert_eq!(stats.used_units, 0);
    pool.shutdown();
}
