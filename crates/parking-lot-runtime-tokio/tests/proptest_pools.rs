//! Property tests for the capacity and queue-depth invariants of the real
//! pools (S-1 "Property test" scenario), driven through their public APIs:
//!
//! - the units of the tasks executing at once never exceed `max_units`
//!   (observed by the executor itself, independently of the semaphore);
//! - parked tasks never exceed `max_queue_depth`, and `QueueFull` is returned
//!   only when the queue was at its limit;
//! - a task costing more than the pool's total capacity is rejected with
//!   `CapacityExceeded`, and every other accepted task runs exactly once (no
//!   task is silently dropped).
//!
//! Case count: 10,000 per pool by default (about 1 s for `ResourcePool` and
//! 3 s for the native `WorkerPool` on an M-series laptop, dev profile),
//! overridable with `PROPTEST_CASES`.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use proptest::prelude::*;
use proptest::test_runner::Config;

use prometheus_parking_lot_core::config::WorkerPoolConfig;
use prometheus_parking_lot_core::{
    InMemoryMailbox, InMemoryQueue, PoolLimits, Priority, ResourceCost, ResourceKind,
    ScheduledTask, SchedulerError, Spawn, TaskError, TaskExecutor, TaskMetadata, TaskStatus,
    WorkerExecutor,
};
use prometheus_parking_lot_runtime_tokio::{ResourcePool, WorkerPool};

/// Upper bound on any wait in these tests.
const WAIT: Duration = Duration::from_secs(20);

fn cases(default: u32) -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

/// One generated task: its cost, priority, and how long it "works" (in
/// cooperative yields) — plus how many yields the submitter lets pass before
/// the next submission.
#[derive(Debug, Clone, Copy)]
struct Step {
    cost: u32,
    priority: Priority,
    work: u8,
    pause: u8,
}

fn priority() -> impl Strategy<Value = Priority> {
    prop_oneof![
        Just(Priority::Low),
        Just(Priority::Normal),
        Just(Priority::High),
        Just(Priority::Critical),
    ]
}

/// A pool shape plus the steps to run against it. Costs range up to one past
/// `max_units` so oversized tasks are generated too.
fn scenario(min_depth: usize) -> impl Strategy<Value = (u32, usize, Vec<Step>)> {
    (1_u32..=5, min_depth..=5).prop_flat_map(|(max_units, depth)| {
        let step = (0..=max_units + 1, priority(), 0_u8..4, 0_u8..3).prop_map(
            |(cost, priority, work, pause)| Step {
                cost,
                priority,
                work,
                pause,
            },
        );
        (
            Just(max_units),
            Just(depth),
            prop::collection::vec(step, 1..24),
        )
    })
}

fn meta(id: u64, step: Step) -> TaskMetadata {
    TaskMetadata {
        id,
        mailbox: None,
        priority: step.priority,
        cost: ResourceCost::new(ResourceKind::GpuVram, step.cost),
        deadline_ms: None,
        created_at_ms: 0,
        idempotency_key: None,
        sequence: 0,
    }
}

/// Executor that measures the units executing at once.
#[derive(Clone, Default)]
struct Meter {
    running_units: Arc<AtomicU32>,
    peak_units: Arc<AtomicU32>,
    running_tasks: Arc<AtomicU32>,
    peak_tasks: Arc<AtomicU32>,
    finished: Arc<AtomicUsize>,
}

impl Meter {
    async fn work(&self, units: u32, yields: u8) {
        let now = self.running_units.fetch_add(units, Ordering::SeqCst) + units;
        self.peak_units.fetch_max(now, Ordering::SeqCst);
        let tasks = self.running_tasks.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak_tasks.fetch_max(tasks, Ordering::SeqCst);
        for _ in 0..yields {
            tokio::task::yield_now().await;
            std::thread::yield_now();
        }
        self.running_tasks.fetch_sub(1, Ordering::SeqCst);
        self.running_units.fetch_sub(units, Ordering::SeqCst);
        self.finished.fetch_add(1, Ordering::SeqCst);
    }
}

#[async_trait]
impl TaskExecutor<u8, u8> for Meter {
    async fn execute(&self, work: u8, meta: TaskMetadata) -> Result<u8, TaskError> {
        self.work(meta.cost.total_units(), work).await;
        Ok(work)
    }
}

#[async_trait]
impl WorkerExecutor<u8, u8> for Meter {
    async fn execute(&self, work: u8, meta: TaskMetadata) -> Result<u8, TaskError> {
        self.work(meta.cost.total_units(), work).await;
        Ok(work)
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

/// Run `steps` against a `ResourcePool` on a current-thread runtime, so the
/// pool state read just before a submission is the state the submission
/// sees, and check every invariant.
fn check_resource_pool(max_units: u32, depth: usize, steps: &[Step]) -> Result<(), TestCaseError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let meter = Meter::default();
        let pool = ResourcePool::<u8, u8, _, _, _, _>::new(
            PoolLimits {
                max_units,
                max_queue_depth: depth,
                default_timeout: WAIT,
            },
            InMemoryQueue::new(depth),
            InMemoryMailbox::new(),
            meter.clone(),
            TokioSpawner,
        );

        let mut accepted = 0_usize;
        for (index, step) in steps.iter().enumerate() {
            let parked_before = pool.queued_tasks();
            let units_before = pool.active_units();
            let id = u64::try_from(index).expect("index fits u64");
            let outcome = pool
                .submit(ScheduledTask {
                    meta: meta(id, *step),
                    payload: step.work,
                })
                .await;

            if step.cost > max_units {
                prop_assert!(
                    matches!(outcome, Err(SchedulerError::CapacityExceeded { .. })),
                    "oversized task: {outcome:?}"
                );
            } else {
                let fits_now = parked_before == 0 && units_before + step.cost <= max_units;
                match outcome {
                    Ok(TaskStatus::Running) => accepted += 1,
                    Ok(TaskStatus::Queued) => {
                        prop_assert!(!fits_now, "a task that fit at once was parked");
                        accepted += 1;
                    }
                    Err(SchedulerError::QueueFull { .. }) => {
                        prop_assert!(!fits_now, "a task that fit at once was rejected");
                        prop_assert!(
                            parked_before >= depth,
                            "QueueFull with {parked_before} of {depth} parked"
                        );
                    }
                    other => prop_assert!(false, "unexpected outcome {other:?}"),
                }
            }
            prop_assert!(pool.queued_tasks() <= depth);
            prop_assert!(pool.active_units() <= max_units);
            for _ in 0..step.pause {
                tokio::task::yield_now().await;
            }
        }

        let drained = tokio::time::timeout(WAIT, async {
            while meter.finished.load(Ordering::SeqCst) < accepted {
                tokio::task::yield_now().await;
            }
        })
        .await;
        prop_assert!(
            drained.is_ok(),
            "only {} of {accepted} accepted tasks ran ({} still parked)",
            meter.finished.load(Ordering::SeqCst),
            pool.queued_tasks()
        );
        prop_assert_eq!(meter.finished.load(Ordering::SeqCst), accepted);
        prop_assert!(
            meter.peak_units.load(Ordering::SeqCst) <= max_units,
            "{} units executed at once, max_units {max_units}",
            meter.peak_units.load(Ordering::SeqCst)
        );
        prop_assert_eq!(pool.queued_tasks(), 0);
        // The last task's permit is returned right after its executor
        // returned; let that completion finish.
        tokio::task::yield_now().await;
        prop_assert_eq!(pool.active_units(), 0);
        Ok(())
    })
}

/// Run `steps` against a native `WorkerPool` (real worker threads) and check
/// every invariant. Workers run concurrently with submission, so the parked
/// count read before a submission is an upper bound on what it sees.
fn check_worker_pool(
    workers: usize,
    max_units: u32,
    depth: usize,
    steps: &[Step],
) -> Result<(), TestCaseError> {
    let meter = Meter::default();
    let pool = WorkerPool::new(
        WorkerPoolConfig::new()
            .with_worker_count(workers)
            .with_max_units(max_units)
            .with_max_queue_depth(depth)
            .with_timeout_ms(20_000),
        meter.clone(),
    )
    .expect("valid config");

    let mut keys = Vec::new();
    for (index, step) in steps.iter().enumerate() {
        let parked_before = pool.stats().queued_tasks;
        let id = u64::try_from(index).expect("index fits u64");
        let outcome = pool.submit(step.work, meta(id, *step));
        if step.cost > max_units {
            prop_assert!(
                matches!(outcome, Err(SchedulerError::CapacityExceeded { .. })),
                "oversized task: {outcome:?}"
            );
        } else {
            match outcome {
                Ok(key) => keys.push(key),
                Err(SchedulerError::QueueFull { .. }) => prop_assert!(
                    parked_before >= depth as u64,
                    "QueueFull with at most {parked_before} of {depth} parked"
                ),
                Err(other) => prop_assert!(false, "unexpected error {other}"),
            }
        }
        let stats = pool.stats();
        prop_assert!(stats.queued_tasks <= depth as u64);
        prop_assert!(stats.used_units <= max_units);
        if step.pause > 0 {
            std::thread::yield_now();
        }
    }

    for key in &keys {
        let value = pool.retrieve(key, WAIT);
        prop_assert!(value.is_ok(), "accepted task lost: {value:?}");
    }
    prop_assert_eq!(meter.finished.load(Ordering::SeqCst), keys.len());
    prop_assert!(
        meter.peak_units.load(Ordering::SeqCst) <= max_units,
        "{} units executed at once, max_units {max_units}",
        meter.peak_units.load(Ordering::SeqCst)
    );
    let workers_u32 = u32::try_from(workers).expect("small");
    prop_assert!(meter.peak_tasks.load(Ordering::SeqCst) <= workers_u32);
    pool.shutdown();
    Ok(())
}

proptest! {
    #![proptest_config(Config::with_cases(cases(10_000)))]

    #[test]
    fn resource_pool_capacity_and_depth_invariants((max_units, depth, steps) in scenario(0)) {
        check_resource_pool(max_units, depth, &steps)?;
    }
}

proptest! {
    #![proptest_config(Config::with_cases(cases(10_000)))]

    #[test]
    fn worker_pool_capacity_and_depth_invariants(
        workers in 1_usize..=3,
        (max_units, depth, steps) in scenario(1),
    ) {
        check_worker_pool(workers, max_units, depth, &steps)?;
    }
}
