//! Audit C1: the native `WorkerPool` enforces `max_units` and serves waiting
//! tasks by priority, then submission order (0.2 ran whatever a free worker
//! pulled off a FIFO channel, ignoring both).

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::Mutex;
use tokio::sync::Semaphore;

use prometheus_parking_lot_core::config::WorkerPoolConfig;
use prometheus_parking_lot_core::{
    Priority, ResourceCost, ResourceKind, SchedulerError, TaskError, TaskMetadata, WorkerExecutor,
};
use prometheus_parking_lot_runtime_tokio::WorkerPool;

const WAIT: Duration = Duration::from_secs(10);

/// Executor that waits for a gate permit and records execution order and the
/// peak units running at once.
#[derive(Clone)]
struct GatedExecutor {
    gate: Arc<Semaphore>,
    order: Arc<Mutex<Vec<&'static str>>>,
    running_units: Arc<AtomicU32>,
    peak_units: Arc<AtomicU32>,
}

impl GatedExecutor {
    fn new() -> Self {
        Self {
            gate: Arc::new(Semaphore::new(0)),
            order: Arc::new(Mutex::new(Vec::new())),
            running_units: Arc::new(AtomicU32::new(0)),
            peak_units: Arc::new(AtomicU32::new(0)),
        }
    }
}

#[async_trait]
impl WorkerExecutor<&'static str, &'static str> for GatedExecutor {
    async fn execute(
        &self,
        name: &'static str,
        meta: TaskMetadata,
    ) -> Result<&'static str, TaskError> {
        let units = meta.cost.total_units();
        let now = self.running_units.fetch_add(units, Ordering::SeqCst) + units;
        self.peak_units.fetch_max(now, Ordering::SeqCst);
        self.order.lock().push(name);
        self.gate
            .acquire()
            .await
            .expect("gate never closed")
            .forget();
        self.running_units.fetch_sub(units, Ordering::SeqCst);
        Ok(name)
    }
}

fn meta(priority: Priority, units: u32) -> TaskMetadata {
    TaskMetadata {
        id: 0,
        mailbox: None,
        priority,
        cost: ResourceCost::new(ResourceKind::GpuVram, units),
        deadline_ms: None,
        created_at_ms: 0,
        idempotency_key: None,
        sequence: 0,
    }
}

/// Four workers but two units: no more than two one-unit tasks run at once,
/// the rest wait, and all of them finish.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn worker_pool_enforces_max_units_with_idle_workers() {
    let executor = GatedExecutor::new();
    let pool = WorkerPool::new(
        WorkerPoolConfig::new()
            .with_worker_count(4)
            .with_max_units(2)
            .with_max_queue_depth(16),
        executor.clone(),
    )
    .expect("valid config");

    let keys: Vec<_> = ["a", "b", "c", "d", "e", "f"]
        .into_iter()
        .map(|name| {
            pool.submit(name, meta(Priority::Normal, 1))
                .expect("accepted")
        })
        .collect();
    let stats = pool.stats();
    assert_eq!(stats.used_units, 2);
    assert_eq!(stats.active_tasks, 2);
    assert_eq!(stats.queued_tasks, 4);

    executor.gate.add_permits(keys.len());
    for key in &keys {
        pool.retrieve_async(key, WAIT).await.expect("task ran");
    }
    assert!(
        executor.peak_units.load(Ordering::SeqCst) <= 2,
        "{} units ran at once on a 2-unit pool",
        executor.peak_units.load(Ordering::SeqCst)
    );
    assert_eq!(pool.stats().used_units, 0);
    pool.shutdown();
}

/// One worker held by a blocker; waiting tasks run highest priority first,
/// FIFO within a priority.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn worker_pool_serves_priority_then_submission_order() {
    let executor = GatedExecutor::new();
    let pool = WorkerPool::new(
        WorkerPoolConfig::new()
            .with_worker_count(1)
            .with_max_units(4)
            .with_max_queue_depth(16),
        executor.clone(),
    )
    .expect("valid config");

    let mut keys = vec![pool.submit("blocker", meta(Priority::Low, 1)).expect("ok")];
    for (name, priority) in [
        ("low", Priority::Low),
        ("normal", Priority::Normal),
        ("high-1", Priority::High),
        ("critical", Priority::Critical),
        ("high-2", Priority::High),
    ] {
        keys.push(pool.submit(name, meta(priority, 1)).expect("ok"));
    }
    executor.gate.add_permits(keys.len());
    for key in &keys {
        pool.retrieve_async(key, WAIT).await.expect("task ran");
    }
    assert_eq!(
        *executor.order.lock(),
        vec!["blocker", "critical", "high-1", "high-2", "normal", "low"]
    );
    pool.shutdown();
}

/// A task costing more than the pool's total units is rejected at once
/// instead of waiting forever.
#[test]
fn worker_pool_rejects_a_task_larger_than_the_pool() {
    let pool = WorkerPool::new(
        WorkerPoolConfig::new()
            .with_worker_count(1)
            .with_max_units(4)
            .with_max_queue_depth(16),
        GatedExecutor::new(),
    )
    .expect("valid config");
    assert!(matches!(
        pool.submit("huge", meta(Priority::High, 5)),
        Err(SchedulerError::CapacityExceeded {
            requested: 5,
            available: 4,
            ..
        })
    ));
    pool.shutdown();
}

/// Tasks still waiting at shutdown are failed with `Cancelled`, not dropped;
/// the running task finishes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn worker_pool_shutdown_cancels_waiting_tasks() {
    let executor = GatedExecutor::new();
    let pool = Arc::new(
        WorkerPool::new(
            WorkerPoolConfig::new()
                .with_worker_count(1)
                .with_max_units(1)
                .with_max_queue_depth(16),
            executor.clone(),
        )
        .expect("valid config"),
    );
    let running = pool
        .submit("running", meta(Priority::Normal, 1))
        .expect("ok");
    let waiting = pool
        .submit("waiting", meta(Priority::Normal, 1))
        .expect("ok");

    // Shut down while "running" is still blocked on the gate, so "waiting"
    // is certainly still parked when shutdown empties the queue.
    let shutdown = {
        let pool = Arc::clone(&pool);
        tokio::task::spawn_blocking(move || pool.shutdown())
    };
    match pool.retrieve_async(&waiting, WAIT).await {
        Err(SchedulerError::Task(TaskError::Cancelled { .. })) => {}
        other => panic!("expected Task(Cancelled), got {other:?}"),
    }
    executor.gate.add_permits(1);
    assert_eq!(
        pool.retrieve_async(&running, WAIT).await.expect("ran"),
        "running"
    );
    shutdown.await.expect("shutdown thread");
}
