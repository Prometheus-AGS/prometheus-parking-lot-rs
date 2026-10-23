//! The WASM pool on a real JS event loop (`wasm-bindgen-test`, Node.js).
//!
//! Smoke, capacity, priority order, budgets and shutdown, through the public
//! API only. Run with
//! `cargo test --target wasm32-unknown-unknown -p prometheus-parking-lot-wasm`
//! (needs `wasm-bindgen-test-runner` matching the `wasm-bindgen` in Cargo.lock).
//!
//! Executors wait with `prometheus_parking_lot_wasm::sleep`, the `Send` JS
//! timer; a raw `gloo_timers` future cannot be awaited inside a
//! `WorkerExecutor`.
#![cfg(target_arch = "wasm32")]
#![expect(
    clippy::future_not_send,
    reason = "single-threaded wasm: the pool's retrieve future holds a JS timer handle"
)]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::Mutex;
use prometheus_parking_lot_core::config::WorkerPoolConfig;
use prometheus_parking_lot_core::{
    Priority, ResourceCost, ResourceKind, SchedulerError, TaskError, TaskMetadata, TestClock,
    WorkerExecutor,
};
use prometheus_parking_lot_wasm::{WorkerPool, sleep};
use wasm_bindgen_test::wasm_bindgen_test;

const LONG: Duration = Duration::from_secs(10);

fn meta(id: u64, priority: Priority, units: u32) -> TaskMetadata {
    TaskMetadata {
        id,
        mailbox: None,
        priority,
        cost: ResourceCost::new(ResourceKind::Cpu, units),
        deadline_ms: None,
        created_at_ms: 0,
        idempotency_key: None,
        sequence: 0,
    }
}

fn config(workers: usize, units: u32, depth: usize, timeout_ms: u64) -> WorkerPoolConfig {
    WorkerPoolConfig::new()
        .with_worker_count(workers)
        .with_max_units(units)
        .with_max_queue_depth(depth)
        .with_timeout_ms(timeout_ms)
}

/// Sleeps `payload` milliseconds, tracking the units held while it does.
#[derive(Clone, Default)]
struct Tracking {
    held: Arc<AtomicU32>,
    peak: Arc<AtomicU32>,
    started: Arc<Mutex<Vec<u64>>>,
}

#[async_trait]
impl WorkerExecutor<u64, u64> for Tracking {
    async fn execute(&self, payload: u64, meta: TaskMetadata) -> Result<u64, TaskError> {
        let units = meta.cost.total_units();
        let now = self.held.fetch_add(units, Ordering::SeqCst) + units;
        self.peak.fetch_max(now, Ordering::SeqCst);
        self.started.lock().push(meta.id);
        sleep(Duration::from_millis(payload)).await;
        self.held.fetch_sub(units, Ordering::SeqCst);
        Ok(meta.id)
    }
}

#[wasm_bindgen_test]
async fn smoke_submit_and_retrieve() {
    let pool = WorkerPool::new(config(2, 10, 8, 5_000), Tracking::default()).expect("pool");
    let key = pool
        .submit_async(5, meta(7, Priority::Normal, 1))
        .await
        .expect("submit");
    let result = pool.retrieve_async(&key, LONG).await.expect("result");
    assert_eq!(result, 7);
    let stats = pool.stats();
    assert_eq!(stats.submitted_tasks, 1);
    assert_eq!(stats.completed_tasks, 1);
    assert_eq!(stats.used_units, 0);
}

/// cr-23: per-kind limits from the configuration reach admission. 100 total
/// units but 2 VRAM units: two 2-unit VRAM tasks never run together, and a
/// 3-unit VRAM task is rejected.
#[wasm_bindgen_test]
async fn resource_limits_admit_by_every_dimension() {
    let executor = Tracking::default();
    let pool = WorkerPool::new(
        config(4, 100, 8, 5_000).with_resource_limit(ResourceKind::GpuVram, 2),
        executor.clone(),
    )
    .expect("pool");
    let vram = |id: u64, units: u32| TaskMetadata {
        cost: ResourceCost::new(ResourceKind::GpuVram, units),
        ..meta(id, Priority::Normal, 0)
    };
    assert!(matches!(
        pool.submit(5, vram(1, 3)),
        Err(SchedulerError::CapacityExceeded {
            requested: 3,
            available: 2,
            ..
        })
    ));
    let keys: Vec<_> = (2..5)
        .map(|id| pool.submit(5, vram(id, 2)).expect("submit"))
        .collect();
    for key in &keys {
        pool.retrieve_async(key, LONG).await.expect("result");
    }
    assert_eq!(
        executor.peak.load(Ordering::SeqCst),
        2,
        "VRAM tasks overlapped"
    );
}

#[wasm_bindgen_test]
async fn capacity_is_never_exceeded() {
    // 16 worker slots but 3 units: the 0.2 pool (audit C1) ran all of them.
    let executor = Tracking::default();
    let pool = WorkerPool::new(config(16, 3, 64, 5_000), executor.clone()).expect("pool");
    let mut keys = Vec::new();
    for id in 0..12 {
        // Mixed costs 1 and 2.
        let units = 1 + u32::try_from(id % 2).expect("small");
        keys.push(
            pool.submit(5, meta(id, Priority::Normal, units))
                .expect("submit"),
        );
        assert!(pool.stats().used_units <= 3, "units over max at submit");
    }
    for key in &keys {
        pool.retrieve_async(key, LONG).await.expect("result");
    }
    let peak = executor.peak.load(Ordering::SeqCst);
    assert!(peak <= 3, "peak held units {peak} exceeded max_units 3");
    assert!(
        peak >= 2,
        "tasks never overlapped (peak {peak}); the test proves nothing"
    );
    let stats = pool.stats();
    assert_eq!(stats.completed_tasks, 12);
    assert_eq!(stats.used_units, 0);
    assert_eq!(stats.active_tasks, 0);
}

#[wasm_bindgen_test]
async fn oversized_task_is_rejected() {
    let pool = WorkerPool::new(config(2, 3, 8, 5_000), Tracking::default()).expect("pool");
    let result = pool.submit(1, meta(1, Priority::Normal, 4));
    assert!(
        matches!(
            result,
            Err(SchedulerError::CapacityExceeded {
                requested: 4,
                available: 3,
                ..
            })
        ),
        "{result:?}"
    );
}

#[wasm_bindgen_test]
async fn priority_then_fifo_order() {
    let executor = Tracking::default();
    // One unit: strictly one task at a time, so start order is dispatch order.
    let pool = WorkerPool::new(config(4, 1, 16, 5_000), executor.clone()).expect("pool");
    let mut keys = Vec::new();
    // id 0 starts at once and holds the unit while the rest park.
    for (id, priority) in [
        (0, Priority::Low),
        (1, Priority::Low),
        (2, Priority::Normal),
        (3, Priority::High),
        (4, Priority::High),
        (5, Priority::Low),
    ] {
        keys.push(pool.submit(10, meta(id, priority, 1)).expect("submit"));
    }
    for key in &keys {
        pool.retrieve_async(key, LONG).await.expect("result");
    }
    assert_eq!(*executor.started.lock(), vec![0, 3, 4, 2, 1, 5]);
}

/// Sleeps an hour; records in `cancelled` if it is dropped before finishing.
#[derive(Clone, Default)]
struct Wedged {
    cancelled: Arc<AtomicBool>,
}

struct CancelWitness {
    armed: bool,
    cancelled: Arc<AtomicBool>,
}

impl Drop for CancelWitness {
    fn drop(&mut self) {
        if self.armed {
            self.cancelled.store(true, Ordering::SeqCst);
        }
    }
}

#[async_trait]
impl WorkerExecutor<u64, u64> for Wedged {
    async fn execute(&self, payload: u64, meta: TaskMetadata) -> Result<u64, TaskError> {
        let mut witness = CancelWitness {
            armed: true,
            cancelled: Arc::clone(&self.cancelled),
        };
        sleep(Duration::from_millis(payload)).await;
        witness.armed = false;
        Ok(meta.id)
    }
}

#[wasm_bindgen_test]
async fn budget_cancels_a_wedged_executor() {
    let executor = Wedged::default();
    let pool = WorkerPool::new(config(2, 1, 8, 50), executor.clone()).expect("pool");

    // Within budget: completes, and the witness does not fire.
    let ok = pool
        .submit(5, meta(1, Priority::Normal, 1))
        .expect("submit");
    assert_eq!(pool.retrieve_async(&ok, LONG).await.expect("in budget"), 1);
    assert!(!executor.cancelled.load(Ordering::SeqCst));

    // An hour of sleep against a 50 ms budget: cancelled, Timeout delivered.
    let wedged = pool
        .submit(3_600_000, meta(2, Priority::Normal, 1))
        .expect("submit");
    let result = pool.retrieve_async(&wedged, LONG).await;
    assert!(
        matches!(
            result,
            Err(SchedulerError::Task(TaskError::Timeout { budget })) if budget == Duration::from_millis(50)
        ),
        "{result:?}"
    );
    assert!(
        executor.cancelled.load(Ordering::SeqCst),
        "executor future was not dropped"
    );

    // The unit came back: the next task runs.
    assert_eq!(pool.stats().used_units, 0);
    let next = pool
        .submit(5, meta(3, Priority::Normal, 1))
        .expect("submit");
    assert_eq!(pool.retrieve_async(&next, LONG).await.expect("next"), 3);
    assert_eq!(pool.stats().failed_tasks, 1);
}

#[wasm_bindgen_test]
async fn budget_follows_the_task_deadline() {
    let executor = Wedged::default();
    // Default timeout an hour: only the deadline can stop the task.
    let pool = WorkerPool::new(config(1, 1, 8, 3_600_000), executor.clone()).expect("pool");
    let mut task = meta(1, Priority::Normal, 1);
    let now = u128::from(js_now());
    task.deadline_ms = Some(now + 40);
    let key = pool.submit(3_600_000, task).expect("submit");
    let result = pool.retrieve_async(&key, LONG).await;
    assert!(
        matches!(
            result,
            Err(SchedulerError::Task(TaskError::Timeout { budget })) if budget <= Duration::from_millis(40)
        ),
        "{result:?}"
    );
    assert!(executor.cancelled.load(Ordering::SeqCst));
}

#[wasm_bindgen_test]
async fn long_budget_does_not_fire_at_once() {
    // setTimeout fires a delay above i32::MAX ms immediately; the pool must
    // clamp, not wrap, or every long budget would cancel its task at once.
    let executor = Wedged::default();
    let pool = WorkerPool::new(config(1, 1, 8, u64::MAX / 2), executor.clone()).expect("pool");
    let key = pool
        .submit(3_600_000, meta(1, Priority::Normal, 1))
        .expect("submit");
    let result = pool.retrieve_async(&key, Duration::from_millis(100)).await;
    assert!(
        matches!(result, Err(SchedulerError::Timeout { .. })),
        "{result:?}"
    );
    assert!(
        !executor.cancelled.load(Ordering::SeqCst),
        "task was cancelled early"
    );
}

#[wasm_bindgen_test]
async fn a_deadline_that_passed_while_parked_expires_the_task() {
    let clock = TestClock::new(1_000);
    let executor = Tracking::default();
    let pool = WorkerPool::new(config(1, 1, 8, 5_000), executor.clone())
        .expect("pool")
        .with_clock(clock.clone());
    let running = pool
        .submit(20, meta(1, Priority::Normal, 1))
        .expect("submit");
    let mut late = meta(2, Priority::Normal, 1);
    late.deadline_ms = Some(1_010);
    let parked = pool.submit(1, late).expect("submit");
    clock.advance(Duration::from_secs(1));
    pool.retrieve_async(&running, LONG).await.expect("first");
    let result = pool.retrieve_async(&parked, LONG).await;
    assert!(
        matches!(
            result,
            Err(SchedulerError::Task(TaskError::DeadlineExpired))
        ),
        "{result:?}"
    );
    assert_eq!(*executor.started.lock(), vec![1], "the expired task ran");
}

#[wasm_bindgen_test]
async fn retrieve_times_out_and_discards_the_slot() {
    let pool = WorkerPool::new(config(1, 1, 8, 60_000), Wedged::default()).expect("pool");
    let key = pool
        .submit(3_600_000, meta(1, Priority::Normal, 1))
        .expect("submit");
    let first = pool.retrieve_async(&key, Duration::from_millis(30)).await;
    assert!(
        matches!(first, Err(SchedulerError::Timeout { .. })),
        "{first:?}"
    );
    let second = pool.retrieve_async(&key, Duration::from_millis(30)).await;
    assert!(
        matches!(second, Err(SchedulerError::ResultNotFound { .. })),
        "{second:?}"
    );
}

#[wasm_bindgen_test]
async fn queue_depth_is_bounded_and_shutdown_fails_parked_tasks() {
    let pool = WorkerPool::new(config(1, 1, 1, 5_000), Tracking::default()).expect("pool");
    let running = pool
        .submit(30, meta(1, Priority::Normal, 1))
        .expect("submit");
    let parked = pool
        .submit(1, meta(2, Priority::Normal, 1))
        .expect("submit");
    let full = pool.submit(1, meta(3, Priority::Normal, 1));
    assert!(
        matches!(full, Err(SchedulerError::QueueFull { .. })),
        "{full:?}"
    );

    // Let the admitted task start: one admitted but not yet polled when the
    // pool shuts down is cancelled too (documented on `shutdown`).
    sleep(Duration::from_millis(5)).await;
    pool.shutdown();
    let refused = pool.submit(1, meta(4, Priority::Normal, 1));
    assert!(
        matches!(refused, Err(SchedulerError::PoolShutdown { .. })),
        "{refused:?}"
    );

    // The parked task is failed, not dropped; the running one finishes.
    let cancelled = pool.retrieve_async(&parked, LONG).await;
    assert!(
        matches!(
            cancelled,
            Err(SchedulerError::Task(TaskError::Cancelled { .. }))
        ),
        "{cancelled:?}"
    );
    assert_eq!(
        pool.retrieve_async(&running, LONG).await.expect("running"),
        1
    );
}

fn js_now() -> u64 {
    // The pool's default clock reads Date.now(); so does the test.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "Date.now() is a finite, positive millisecond count"
    )]
    {
        js_sys::Date::now() as u64
    }
}

// ---------------------------------------------------------------------------
// Skip-ahead (cr-20) and vector admission through the dispatcher (cr-53)
// ---------------------------------------------------------------------------

/// Run the skip-ahead scenario and return the order tasks started in.
///
/// 4 units. Task 0 (2 units) runs long and leaves 2 free. Task 1 (3 units)
/// is the head and cannot fit. Tasks 2, 3 and 4 (1 unit, short) can.
async fn skip_scenario(max_head_skips: u32) -> (Vec<u64>, u64, u64) {
    let executor = Tracking::default();
    let pool = WorkerPool::new(
        config(8, 4, 16, 5_000).with_max_head_skips(max_head_skips),
        executor.clone(),
    )
    .expect("pool");
    let mut keys = Vec::new();
    keys.push(pool.submit(60, meta(0, Priority::Normal, 2)).expect("0"));
    keys.push(pool.submit(5, meta(1, Priority::Normal, 3)).expect("1"));
    for id in 2..=4 {
        keys.push(
            pool.submit(5, meta(id, Priority::Normal, 1))
                .expect("small"),
        );
    }
    for key in &keys {
        pool.retrieve_async(key, LONG).await.expect("result");
    }
    let stats = pool.skip_ahead_stats();
    let started = executor.started.lock().clone();
    (started, stats.skipped, stats.reservations)
}

/// Small tasks start in place of a head that does not fit, at most
/// `max_head_skips` times; then capacity is reserved for the head, so the
/// next small task waits behind it.
#[wasm_bindgen_test]
async fn skip_ahead_is_bounded_by_the_starvation_guard() {
    let (started, skipped, reservations) = skip_scenario(2).await;
    assert_eq!(
        started,
        vec![0, 2, 3, 1, 4],
        "two small tasks skip the head, the head starts before the third"
    );
    assert_eq!((skipped, reservations), (2, 1));
}

/// `max_head_skips: 0` is strict head-of-line: nothing starts in place of a
/// head that does not fit.
#[wasm_bindgen_test]
async fn skip_ahead_off_is_strict_head_of_line() {
    let (started, skipped, reservations) = skip_scenario(0).await;
    assert_eq!(started, vec![0, 1, 2, 3, 4]);
    assert_eq!(skipped, 0);
    assert_eq!(reservations, 0);
}

/// A full dimension parks its next task without blocking another dimension:
/// the CPU task starts in place of the parked VRAM head.
#[wasm_bindgen_test]
async fn a_full_dimension_does_not_block_another() {
    let executor = Tracking::default();
    let pool = WorkerPool::new(
        config(4, 100, 8, 5_000).with_resource_limit(ResourceKind::GpuVram, 2),
        executor.clone(),
    )
    .expect("pool");
    let vram = |id: u64| TaskMetadata {
        cost: ResourceCost::new(ResourceKind::GpuVram, 2),
        ..meta(id, Priority::Normal, 0)
    };
    let keys = [
        pool.submit(40, vram(0)).expect("v0"),
        pool.submit(5, vram(1)).expect("v1"),
        pool.submit(5, meta(2, Priority::Normal, 1)).expect("cpu"),
    ];
    for key in &keys {
        pool.retrieve_async(key, LONG).await.expect("result");
    }
    assert_eq!(*executor.started.lock(), vec![0, 2, 1]);
}
