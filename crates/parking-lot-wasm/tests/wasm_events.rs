//! S-21 on the WASM pool (cr-53): every task's lifecycle events arrive in
//! causal order, with the same taxonomy as the native pools.
//!
//! The oracle is the one in
//! `crates/parking-lot-runtime-tokio/tests/lifecycle_events_test.rs`
//! (`check_sequence` and `violations`), replicated here because that file is
//! a native test target and cannot be shared with a wasm32 one: per task id,
//! over the events in delivery order, `Submitted` first; then at most one
//! `Queued`; then at most one `Started`; then exactly one terminal event, and
//! nothing after it. On this target hooks run inline, so there is nothing to
//! flush: an event is delivered before the emitting call returns.
//!
//! Negative control: under `--cfg parking_lot_negative_control="s-21"` the
//! pool suppresses the `Cancelled` event of a task cancelled at shutdown, so
//! `lifecycle_events_cancelled_at_shutdown` must fail ("no terminal event").
//! It does not prove the other paths; the exact-sequence assertions do.
#![cfg(target_arch = "wasm32")]
#![expect(
    clippy::future_not_send,
    reason = "single-threaded wasm: the pool's retrieve future holds a JS timer handle"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::Mutex;
use prometheus_parking_lot_core::config::WorkerPoolConfig;
use prometheus_parking_lot_core::hooks::{CancelledReason, RejectionReason};
use prometheus_parking_lot_core::{
    Hook, LifecycleEvent, Priority, ResourceCost, ResourceKind, SchedulerError, TaskError,
    TaskMetadata, TestClock, WorkerExecutor,
};
use prometheus_parking_lot_wasm::{WorkerPool, sleep};
use wasm_bindgen_test::wasm_bindgen_test;

const LONG: Duration = Duration::from_secs(10);

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

    /// The kinds of `task_id`'s events, in delivery order.
    fn kinds(&self, task_id: u64) -> Vec<&'static str> {
        self.snapshot()
            .iter()
            .filter(|event| event.task_id() == task_id)
            .map(LifecycleEvent::kind)
            .collect()
    }

    fn of_kind(&self, task_id: u64, kind: &str) -> Option<LifecycleEvent> {
        self.snapshot()
            .into_iter()
            .find(|event| event.task_id() == task_id && event.kind() == kind)
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
    // 0 nothing, 1 submitted, 2 queued, 3 started, 4 terminal.
    let mut stage = 0;
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
            (_, _) => 4,
        };
    }
    if stage == 4 {
        Ok(())
    } else {
        Err("no terminal event".into())
    }
}

fn assert_ordered(log: &Log, ids: impl IntoIterator<Item = u64>) {
    let ids: BTreeSet<u64> = ids.into_iter().collect();
    let found = violations(&log.snapshot(), &ids);
    assert!(
        found.is_empty(),
        "{} of {} tasks broke the lifecycle order:\n{}",
        found.len(),
        ids.len(),
        found.join("\n")
    );
}

// ---------------------------------------------------------------------------
// Executor and builders
// ---------------------------------------------------------------------------

/// What a job does when it runs.
#[derive(Debug, Clone, Copy)]
enum Act {
    Succeed,
    Fail,
}

#[derive(Clone, Copy)]
struct Runner;

#[async_trait]
impl WorkerExecutor<(u64, Act), u64> for Runner {
    async fn execute(&self, payload: (u64, Act), meta: TaskMetadata) -> Result<u64, TaskError> {
        let (ms, act) = payload;
        sleep(Duration::from_millis(ms)).await;
        match act {
            Act::Succeed => Ok(meta.id),
            Act::Fail => Err(TaskError::Executor {
                message: "failed on purpose".into(),
                retryable: false,
            }),
        }
    }
}

fn meta(id: u64, units: u32) -> TaskMetadata {
    TaskMetadata {
        id,
        mailbox: None,
        priority: Priority::Normal,
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

fn pool_with_log(config: WorkerPoolConfig) -> (WorkerPool<(u64, Act), u64, Runner>, Arc<Log>) {
    let log = Arc::new(Log::default());
    let pool = WorkerPool::new(config, Runner)
        .expect("pool")
        .with_hook(log.clone());
    (pool, log)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Completed, failed and timed-out tasks, one of which parked first, in exact
/// sequence, with the payload fields the native pools carry.
#[wasm_bindgen_test]
async fn lifecycle_events_completed_failed_and_timed_out() {
    // One unit: task 1 runs, 2 and 3 park behind it (Queued), 3 times out.
    let (pool, log) = pool_with_log(config(1, 1, 8, 40));
    let ok = pool.submit((5, Act::Succeed), meta(1, 1)).expect("submit");
    let failing = pool.submit((5, Act::Fail), meta(2, 1)).expect("submit");
    let wedged = pool
        .submit((3_600_000, Act::Succeed), meta(3, 1))
        .expect("submit");

    assert_eq!(pool.retrieve_async(&ok, LONG).await.expect("ok"), 1);
    assert!(matches!(
        pool.retrieve_async(&failing, LONG).await,
        Err(SchedulerError::Task(TaskError::Executor { .. }))
    ));
    assert!(matches!(
        pool.retrieve_async(&wedged, LONG).await,
        Err(SchedulerError::Task(TaskError::Timeout { .. }))
    ));

    assert_eq!(
        log.kinds(1),
        ["submitted", "started", "completed"],
        "a task that starts at once is never queued"
    );
    assert_eq!(log.kinds(2), ["submitted", "queued", "started", "failed"]);
    assert_eq!(log.kinds(3), ["submitted", "queued", "started", "failed"]);
    assert_eq!(log.snapshot().len(), 3 + 4 + 4, "an unexpected extra event");

    let Some(LifecycleEvent::Queued {
        queue_depth, pool, ..
    }) = log.of_kind(2, "queued")
    else {
        panic!("task 2 has no Queued event");
    };
    assert_eq!((queue_depth, pool.as_str()), (1, "worker_pool"));
    let Some(LifecycleEvent::Failed { error, .. }) = log.of_kind(3, "failed") else {
        panic!("task 3 has no Failed event");
    };
    assert!(
        error.contains("budget") || error.contains("imeout"),
        "{error}"
    );
    assert_ordered(&log, [1, 2, 3]);
}

/// Many tasks, mixed outcomes, a small queue: the oracle holds for all of
/// them, rejected ones included.
#[wasm_bindgen_test]
async fn lifecycle_events_hold_under_a_mixed_load() {
    let (pool, log) = pool_with_log(config(4, 3, 6, 5_000));
    let mut keys = Vec::new();
    let mut ids = Vec::new();
    for id in 0..40u64 {
        let act = if id % 5 == 0 { Act::Fail } else { Act::Succeed };
        let units = 1 + u32::try_from(id % 3).expect("small");
        ids.push(id);
        if let Ok(key) = pool.submit((2, act), meta(id, units)) {
            keys.push(key);
        }
        if id % 7 == 0 {
            sleep(Duration::from_millis(3)).await;
        }
    }
    for key in &keys {
        let _ = pool.retrieve_async(key, LONG).await;
    }
    let rejected = ids.len() - keys.len();
    assert!(rejected > 0, "the queue never filled: nothing was rejected");
    assert_ordered(&log, ids);
    let terminal = log.snapshot().iter().filter(|e| e.is_terminal()).count();
    assert_eq!(terminal, 40, "exactly one terminal event per task");
}

#[wasm_bindgen_test]
async fn lifecycle_events_rejected_at_submit() {
    // Depth 1, 2 units: task 1 runs, 2 parks, 3 finds the queue full.
    let clock = TestClock::new(1_000);
    let log = Arc::new(Log::default());
    let pool = WorkerPool::new(config(1, 2, 1, 5_000), Runner)
        .expect("pool")
        .with_clock(clock)
        .with_hook(log.clone());
    let a = pool.submit((20, Act::Succeed), meta(1, 2)).expect("a");
    let b = pool.submit((1, Act::Succeed), meta(2, 2)).expect("b");
    let full = pool.submit((1, Act::Succeed), meta(3, 2));
    assert!(matches!(full, Err(SchedulerError::QueueFull { .. })));
    let oversized = pool.submit((1, Act::Succeed), meta(4, 3));
    assert!(matches!(
        oversized,
        Err(SchedulerError::CapacityExceeded { .. })
    ));
    let mut late = meta(5, 1);
    late.deadline_ms = Some(500);
    let expired = pool.submit((1, Act::Succeed), late);
    assert!(matches!(
        expired,
        Err(SchedulerError::DeadlineExpired { .. })
    ));
    pool.retrieve_async(&a, LONG).await.expect("a");
    pool.retrieve_async(&b, LONG).await.expect("b");
    pool.shutdown();
    let refused = pool.submit((1, Act::Succeed), meta(6, 1));
    assert!(matches!(refused, Err(SchedulerError::PoolShutdown { .. })));

    for (id, expected) in [
        (3, RejectionReason::QueueFull),
        (4, RejectionReason::CapacityExceeded),
        (5, RejectionReason::DeadlineExpired),
        (6, RejectionReason::PoolShutdown),
    ] {
        assert_eq!(log.kinds(id), ["submitted", "rejected"], "task {id}");
        let Some(LifecycleEvent::Rejected { reason, .. }) = log.of_kind(id, "rejected") else {
            panic!("task {id} has no Rejected event");
        };
        assert_eq!(reason, expected, "task {id}");
    }
    assert_ordered(&log, 1..=6);
}

/// A task whose deadline passes while it is parked gets `Expired` and never
/// `Started`.
#[wasm_bindgen_test]
async fn lifecycle_events_expired_while_parked() {
    let clock = TestClock::new(1_000);
    let log = Arc::new(Log::default());
    let pool = WorkerPool::new(config(1, 1, 8, 5_000), Runner)
        .expect("pool")
        .with_clock(clock.clone())
        .with_hook(log.clone());
    let running = pool.submit((20, Act::Succeed), meta(1, 1)).expect("run");
    let mut late = meta(2, 1);
    late.deadline_ms = Some(1_010);
    let parked = pool.submit((1, Act::Succeed), late).expect("park");
    clock.advance(Duration::from_secs(1));
    pool.retrieve_async(&running, LONG).await.expect("first");
    assert!(matches!(
        pool.retrieve_async(&parked, LONG).await,
        Err(SchedulerError::Task(TaskError::DeadlineExpired))
    ));
    assert_eq!(log.kinds(2), ["submitted", "queued", "expired"]);
    assert_ordered(&log, [1, 2]);
}

/// Shutdown: the running task finishes, parked tasks get `Cancelled` with
/// `QueueShutdown` (the negative control suppresses exactly this event).
#[wasm_bindgen_test]
async fn lifecycle_events_cancelled_at_shutdown() {
    let (pool, log) = pool_with_log(config(1, 1, 8, 5_000));
    let running = pool.submit((30, Act::Succeed), meta(1, 1)).expect("run");
    let parked: Vec<_> = (2..=4)
        .map(|id| pool.submit((1, Act::Succeed), meta(id, 1)).expect("park"))
        .collect();
    // Let the first task start before shutting down.
    sleep(Duration::from_millis(5)).await;
    pool.shutdown();
    pool.retrieve_async(&running, LONG).await.expect("running");
    for key in &parked {
        assert!(matches!(
            pool.retrieve_async(key, LONG).await,
            Err(SchedulerError::Task(TaskError::Cancelled { .. }))
        ));
    }
    assert_eq!(log.kinds(1), ["submitted", "started", "completed"]);
    for id in 2..=4 {
        assert_eq!(log.kinds(id), ["submitted", "queued", "cancelled"], "{id}");
        let Some(LifecycleEvent::Cancelled {
            reason, attempt, ..
        }) = log.of_kind(id, "cancelled")
        else {
            panic!("task {id} has no Cancelled event");
        };
        assert_eq!((reason, attempt), (CancelledReason::QueueShutdown, 0));
    }
    assert_ordered(&log, 1..=4);
}

/// With no hook registered the bus is inactive and the pool behaves as before.
#[wasm_bindgen_test]
async fn lifecycle_events_cost_nothing_without_a_hook() {
    let pool = WorkerPool::new(config(1, 1, 8, 5_000), Runner).expect("pool");
    assert!(!pool.hooks().is_active());
    let key = pool.submit((1, Act::Succeed), meta(1, 1)).expect("submit");
    assert_eq!(pool.retrieve_async(&key, LONG).await.expect("result"), 1);
    // A hook registered later sees later tasks only.
    let log = Arc::new(Log::default());
    pool.hooks().register(log.clone());
    let key = pool.submit((1, Act::Succeed), meta(2, 1)).expect("submit");
    pool.retrieve_async(&key, LONG).await.expect("result");
    assert_eq!(log.kinds(1), Vec::<&str>::new());
    assert_eq!(log.kinds(2), ["submitted", "started", "completed"]);
}
