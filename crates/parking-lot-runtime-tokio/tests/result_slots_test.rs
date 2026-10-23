//! S-17: async result waiters use no blocking threads; unretrieved results
//! expire after their TTL (cr-22).
//!
//! Run with:
//!
//! ```text
//! cargo nextest run -p prometheus-parking-lot-runtime-tokio --locked -E "test(waiters_no_blocking_threads)"
//! ```
//!
//! Oracle: on a runtime built with `max_blocking_threads(1)` whose one
//! blocking thread is occupied, 10,000 concurrent `retrieve_async` waiters all
//! resolve with their task's value once the results are delivered; a result
//! never retrieved is evicted after its TTL (driven by a `TestClock`) and a
//! later retrieve reports it expired.
//!
//! Negative control: under `--cfg parking_lot_negative_control="s-17"` each
//! waiter parks a blocking-pool thread (the 0.2 design). With the blocking
//! pool exhausted no waiter runs, every wait times out and
//! `waiters_no_blocking_threads` fails.

use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::sync::watch;

use prometheus_parking_lot_core::config::WorkerPoolConfig;
use prometheus_parking_lot_core::{
    MailboxKey, Priority, ResourceCost, ResourceKind, SchedulerError, TaskError, TaskMetadata,
    TestClock, WorkerExecutor,
};
use prometheus_parking_lot_runtime_tokio::WorkerPool;

/// Concurrent waiters in the load scenario.
const WAITERS: u64 = 10_000;
/// Upper bound on any single wait; reaching it fails the test.
const WAIT: Duration = Duration::from_secs(20);
/// Bound on the load scenario once the results are released. Shorter than
/// `WAIT` on purpose: a waiter that cannot run until its own wait times out
/// (the negative control) then takes the value late and must still fail.
const SCENARIO: Duration = Duration::from_secs(10);

fn meta(id: u64) -> TaskMetadata {
    TaskMetadata {
        id,
        mailbox: None,
        priority: Priority::Normal,
        cost: ResourceCost::new(ResourceKind::Cpu, 1),
        deadline_ms: None,
        created_at_ms: 0,
        idempotency_key: None,
        sequence: 0,
    }
}

/// Returns its payload once the gate opens.
#[derive(Clone)]
struct Gated {
    open: watch::Receiver<bool>,
}

#[async_trait]
impl WorkerExecutor<u64, u64> for Gated {
    async fn execute(&self, payload: u64, _meta: TaskMetadata) -> Result<u64, TaskError> {
        let mut open = self.open.clone();
        open.wait_for(|open| *open)
            .await
            .map_err(|_| TaskError::executor("gate dropped"))?;
        Ok(payload)
    }
}

/// Returns its payload at once.
#[derive(Clone)]
struct Echo;

#[async_trait]
impl WorkerExecutor<u64, u64> for Echo {
    async fn execute(&self, payload: u64, _meta: TaskMetadata) -> Result<u64, TaskError> {
        Ok(payload)
    }
}

/// Block (outside any runtime) until the pool has finished `n` tasks.
fn wait_completed<E: WorkerExecutor<u64, u64>>(pool: &WorkerPool<u64, u64, E>, n: u64) {
    let started = Instant::now();
    while pool.stats().completed_tasks < n {
        assert!(started.elapsed() < WAIT, "tasks did not finish in {WAIT:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

const fn is_expired(error: &SchedulerError) -> bool {
    matches!(error, SchedulerError::ResultExpired { key: Some(_) })
}

#[test]
fn waiters_no_blocking_threads() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(1)
        .enable_all()
        .build()
        .expect("runtime");

    // Occupy the runtime's only blocking thread for the whole scenario.
    let (occupied_tx, occupied_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let occupant = runtime.spawn_blocking(move || {
        occupied_tx.send(()).expect("test thread waits");
        let _ = release_rx.recv();
    });
    occupied_rx
        .recv_timeout(WAIT)
        .expect("the blocking thread is occupied");

    let (gate, open) = watch::channel(false);
    let pool = Arc::new(
        WorkerPool::new(
            WorkerPoolConfig::new()
                .with_worker_count(4)
                .with_max_units(4)
                .with_max_queue_depth(usize::try_from(WAITERS).expect("fits")),
            Gated { open },
        )
        .expect("pool"),
    );
    let keys: Vec<MailboxKey> = (0..WAITERS)
        .map(|i| pool.submit(i, meta(i)).expect("accepted"))
        .collect();

    let outcome = runtime.block_on(async {
        let waiters: Vec<_> = keys
            .into_iter()
            .map(|key| {
                let pool = Arc::clone(&pool);
                tokio::spawn(async move { pool.retrieve_async(&key, WAIT).await })
            })
            .collect();

        // Every waiter is now parked on its slot: no result exists yet.
        tokio::time::sleep(Duration::from_millis(200)).await;
        let finished_early = waiters.iter().filter(|w| w.is_finished()).count();
        assert_eq!(finished_early, 0, "a waiter resolved before any result");

        gate.send_replace(true);
        tokio::time::timeout(SCENARIO, async {
            let mut values = Vec::with_capacity(waiters.len());
            for waiter in waiters {
                values.push(waiter.await.expect("waiter task"));
            }
            values
        })
        .await
    });

    let _ = release_tx.send(());
    runtime.block_on(occupant).expect("occupant");
    runtime.shutdown_timeout(WAIT);
    pool.shutdown();

    let values = outcome.expect("all 10,000 waiters resolved within the scenario bound");
    for (i, value) in (0..WAITERS).zip(values) {
        assert_eq!(value.expect("waiter got its result"), i);
    }
}

#[test]
fn waiters_no_blocking_threads_unretrieved_result_expires_after_ttl() {
    let clock = TestClock::new(0);
    let pool = WorkerPool::new(
        WorkerPoolConfig::new()
            .with_worker_count(2)
            .with_max_units(2)
            .with_max_queue_depth(8),
        Echo,
    )
    .expect("pool")
    .with_clock(clock.clone())
    .with_result_ttl(Duration::from_secs(30));

    let kept = pool.submit(1, meta(1)).expect("accepted");
    let dropped = pool.submit(2, meta(2)).expect("accepted");
    wait_completed(&pool, 2);

    // Inside the TTL the result is still there.
    clock.advance(Duration::from_secs(29));
    assert_eq!(pool.retrieve(&kept, WAIT).expect("within TTL"), 1);

    // Past the TTL the unretrieved result is evicted and reported expired,
    // once; after that the key is simply unknown.
    clock.advance(Duration::from_secs(2));
    let expired = pool.retrieve(&dropped, WAIT).expect_err("evicted");
    assert!(
        is_expired(&expired),
        "expected an expired result, got {expired:?}"
    );
    let gone = pool.retrieve(&dropped, WAIT).expect_err("forgotten");
    assert!(
        matches!(gone, SchedulerError::ResultNotFound { .. }),
        "expected a plain ResultNotFound, got {gone:?}"
    );
    pool.shutdown();
}

#[test]
fn waiters_no_blocking_threads_default_ttl_is_the_task_timeout() {
    let clock = TestClock::new(0);
    let pool = WorkerPool::new(
        WorkerPoolConfig::new()
            .with_worker_count(1)
            .with_max_units(1)
            .with_max_queue_depth(8)
            .with_timeout_ms(5_000),
        Echo,
    )
    .expect("pool")
    .with_clock(clock.clone());

    let early = pool.submit(1, meta(1)).expect("accepted");
    let late = pool.submit(2, meta(2)).expect("accepted");
    wait_completed(&pool, 2);

    clock.advance(Duration::from_millis(4_999));
    assert_eq!(pool.retrieve(&early, WAIT).expect("within TTL"), 1);
    clock.advance(Duration::from_millis(1));
    let expired = pool
        .retrieve(&late, WAIT)
        .expect_err("evicted at the task timeout");
    assert!(
        is_expired(&expired),
        "expected an expired result, got {expired:?}"
    );
    pool.shutdown();
}

#[tokio::test]
async fn waiters_no_blocking_threads_timed_out_wait_keeps_the_result() {
    let (gate, open) = watch::channel(false);
    let pool = WorkerPool::new(
        WorkerPoolConfig::new()
            .with_worker_count(1)
            .with_max_units(1)
            .with_max_queue_depth(8),
        Gated { open },
    )
    .expect("pool");

    let key = pool.submit_async(7, meta(7)).await.expect("accepted");
    let early = pool
        .retrieve_async(&key, Duration::from_millis(50))
        .await
        .expect_err("not finished yet");
    assert!(
        matches!(early, SchedulerError::Timeout { .. }),
        "expected Timeout, got {early:?}"
    );

    // The caller gave up, but the result is not lost: a later call gets it.
    gate.send_replace(true);
    assert_eq!(pool.retrieve_async(&key, WAIT).await.expect("ran"), 7);
    let taken = pool
        .retrieve_async(&key, Duration::from_millis(10))
        .await
        .expect_err("taken once");
    assert!(
        matches!(taken, SchedulerError::ResultNotFound { .. }),
        "expected ResultNotFound, got {taken:?}"
    );
    pool.shutdown();
}

#[tokio::test]
#[should_panic(expected = "Cannot block the current thread from within a runtime")]
async fn waiters_no_blocking_threads_blocking_retrieve_refuses_an_async_thread() {
    let pool = WorkerPool::new(
        WorkerPoolConfig::new()
            .with_worker_count(1)
            .with_max_units(1)
            .with_max_queue_depth(8),
        Echo,
    )
    .expect("pool");
    let key = pool.submit(1, meta(1)).expect("accepted");
    // Parking this thread would stall the runtime driving the test.
    let _ = pool.retrieve(&key, WAIT);
}

#[tokio::test]
async fn waiters_no_blocking_threads_blocking_retrieve_runs_on_spawn_blocking() {
    let pool = Arc::new(
        WorkerPool::new(
            WorkerPoolConfig::new()
                .with_worker_count(1)
                .with_max_units(1)
                .with_max_queue_depth(8),
            Echo,
        )
        .expect("pool"),
    );
    let key = pool.submit(3, meta(3)).expect("accepted");
    let blocking = Arc::clone(&pool);
    let value = tokio::task::spawn_blocking(move || blocking.retrieve(&key, WAIT))
        .await
        .expect("blocking task")
        .expect("ran");
    assert_eq!(value, 3);
    tokio::task::spawn_blocking(move || pool.shutdown())
        .await
        .expect("shutdown");
}
