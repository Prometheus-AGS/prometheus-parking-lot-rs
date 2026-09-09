//! Execution-budget enforcement.
//!
//! Regression cover for a defect where `PoolLimits::default_timeout` was
//! configured, validated (`config/pool.rs`) and plumbed through the builder,
//! but never read in the execution path: both `spawn_task` and the wake path
//! awaited `executor.execute(..)` unbounded. A wedged task therefore held its
//! capacity units for the life of the process, so the pool's capacity
//! guarantee did not hold.
//!
//! These drive the real `ResourcePool` on a real tokio runtime. The mailbox is
//! moved into the pool and cannot be read back (see `test_mailbox_delivery`),
//! so the observable is executor side effects — which proves the stronger
//! claim anyway: that a cancelled task's body never ran to completion.

use prometheus_parking_lot::core::{
    PoolLimits, ResourcePool, ScheduledTask, Spawn, TaskExecutor, TaskMetadata,
};
use prometheus_parking_lot::infra::mailbox::memory::InMemoryMailbox;
use prometheus_parking_lot::infra::queue::memory::InMemoryQueue;
use prometheus_parking_lot::util::clock::now_ms;
use prometheus_parking_lot::util::serde::{MailboxKey, Priority, ResourceCost, ResourceKind};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

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

#[derive(Debug, Serialize, Deserialize, Clone)]
struct SleepJob {
    sleep_ms: u64,
}

/// `started` counts bodies entered, `finished` counts bodies that ran to
/// completion, and `dropped` counts bodies whose future was destroyed before
/// completing.
///
/// `dropped` is the load-bearing signal. Asserting only `finished == 0` is not
/// enough: a task that is merely still running also has `finished == 0`, so
/// such an assertion passes against the unfixed code and proves nothing. A
/// cancelled future is *dropped*, so `dropped == 1 && finished == 0` is the
/// only combination that distinguishes "cancelled" from "still going".
#[derive(Clone)]
struct SleepExecutor {
    started: Arc<AtomicU32>,
    finished: Arc<AtomicU32>,
    dropped: Arc<AtomicU32>,
}

/// Fires on drop unless disarmed, which only the completed body does.
struct CancelWitness {
    dropped: Arc<AtomicU32>,
    armed: bool,
}

impl Drop for CancelWitness {
    fn drop(&mut self) {
        if self.armed {
            self.dropped.fetch_add(1, Ordering::SeqCst);
        }
    }
}

#[async_trait]
impl TaskExecutor<SleepJob, String> for SleepExecutor {
    async fn execute(&self, payload: SleepJob, _meta: TaskMetadata) -> String {
        self.started.fetch_add(1, Ordering::SeqCst);
        let mut witness = CancelWitness {
            dropped: Arc::clone(&self.dropped),
            armed: true,
        };
        tokio::time::sleep(Duration::from_millis(payload.sleep_ms)).await;
        // Reached only if the future was never cancelled mid-await.
        witness.armed = false;
        self.finished.fetch_add(1, Ordering::SeqCst);
        format!("slept {}ms", payload.sleep_ms)
    }
}

fn meta(id: u64, units: u32) -> TaskMetadata {
    TaskMetadata {
        id,
        priority: Priority::Normal,
        cost: ResourceCost {
            kind: ResourceKind::Cpu,
            units,
        },
        created_at_ms: now_ms(),
        idempotency_key: None,
        deadline_ms: None,
        mailbox: Some(MailboxKey {
            tenant: "test-tenant".to_string(),
            user_id: Some("user-1".to_string()),
            session_id: None,
        }),
    }
}

type Pool = ResourcePool<
    SleepJob,
    String,
    InMemoryQueue<SleepJob>,
    InMemoryMailbox<String>,
    SleepExecutor,
    TokioSpawner,
>;

fn pool(max_units: u32, default_timeout: Duration, ex: SleepExecutor) -> Pool {
    ResourcePool::new(
        PoolLimits {
            max_units,
            max_queue_depth: 64,
            default_timeout,
        },
        InMemoryQueue::new(64),
        InMemoryMailbox::new(),
        ex,
        TokioSpawner,
    )
}

fn executor() -> SleepExecutor {
    SleepExecutor {
        started: Arc::new(AtomicU32::new(0)),
        finished: Arc::new(AtomicU32::new(0)),
        dropped: Arc::new(AtomicU32::new(0)),
    }
}

/// The defect itself: work outliving its budget must be cancelled, not awaited.
#[tokio::test]
async fn work_exceeding_the_budget_is_cancelled() {
    let ex = executor();
    let p = pool(4, Duration::from_millis(100), ex.clone());

    p.submit(
        ScheduledTask {
            meta: meta(1, 1),
            payload: SleepJob { sleep_ms: 5_000 },
        },
        now_ms(),
    )
    .await
    .expect("submit");

    // Far past the 100ms budget, far short of the 5s the task asked for.
    tokio::time::sleep(Duration::from_millis(700)).await;

    assert_eq!(
        ex.started.load(Ordering::SeqCst),
        1,
        "the task must have started"
    );
    assert_eq!(
        ex.finished.load(Ordering::SeqCst),
        0,
        "the body must NOT have run to completion — it was over budget"
    );
    assert_eq!(
        ex.dropped.load(Ordering::SeqCst),
        1,
        "the body must have been CANCELLED (future dropped), not merely still running"
    );
}

/// The real-world symptom: before the fix a wedged task held its units forever,
/// so nothing queued behind it could ever start.
#[tokio::test]
async fn a_wedged_task_releases_capacity_for_the_next_task() {
    let ex = executor();
    // One unit total: the wedged task must free it or the next task starves.
    let p = pool(1, Duration::from_millis(150), ex.clone());

    p.submit(
        ScheduledTask {
            meta: meta(1, 1),
            payload: SleepJob { sleep_ms: 30_000 },
        },
        now_ms(),
    )
    .await
    .expect("submit wedged");

    tokio::time::sleep(Duration::from_millis(400)).await;

    p.submit(
        ScheduledTask {
            meta: meta(2, 1),
            payload: SleepJob { sleep_ms: 10 },
        },
        now_ms(),
    )
    .await
    .expect("submit fast");

    tokio::time::sleep(Duration::from_millis(400)).await;

    assert_eq!(
        ex.started.load(Ordering::SeqCst),
        2,
        "both tasks must have started"
    );
    assert_eq!(
        ex.finished.load(Ordering::SeqCst),
        1,
        "exactly the fast task completed; the wedged one was cancelled and freed its unit"
    );
}

/// The fix must not truncate legitimate work.
#[tokio::test]
async fn work_within_the_budget_completes_normally() {
    let ex = executor();
    let p = pool(4, Duration::from_secs(5), ex.clone());

    p.submit(
        ScheduledTask {
            meta: meta(1, 1),
            payload: SleepJob { sleep_ms: 20 },
        },
        now_ms(),
    )
    .await
    .expect("submit");

    tokio::time::sleep(Duration::from_millis(400)).await;

    assert_eq!(
        ex.finished.load(Ordering::SeqCst),
        1,
        "in-budget work must complete untouched"
    );
    assert_eq!(
        ex.dropped.load(Ordering::SeqCst),
        0,
        "in-budget work must never be cancelled"
    );
}

/// A per-task `deadline_ms` nearer than the pool default must win, so a caller
/// can bound one task more tightly than the pool.
#[tokio::test]
async fn a_nearer_per_task_deadline_overrides_the_pool_default() {
    let ex = executor();
    // Generous pool budget; the per-task deadline is the only thing that can cut this.
    let p = pool(4, Duration::from_secs(30), ex.clone());

    let mut m = meta(1, 1);
    m.deadline_ms = Some(now_ms() + 120);

    p.submit(
        ScheduledTask {
            meta: m,
            payload: SleepJob { sleep_ms: 5_000 },
        },
        now_ms(),
    )
    .await
    .expect("submit");

    tokio::time::sleep(Duration::from_millis(700)).await;

    assert_eq!(ex.started.load(Ordering::SeqCst), 1);
    assert_eq!(
        ex.finished.load(Ordering::SeqCst),
        0,
        "the 120ms per-task deadline must bound the task, not the 30s pool default"
    );
    assert_eq!(
        ex.dropped.load(Ordering::SeqCst),
        1,
        "the per-task deadline must actually cancel the body, not just elapse"
    );
}
