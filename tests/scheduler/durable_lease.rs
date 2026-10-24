//! Lease extension: a task that runs for more than three lease durations keeps
//! its lease through the heartbeat and is never leased by a second dispatcher.
//!
//! The test waits on what the pools report (their lifecycle events, the
//! drain of `shutdown_with`), not on timers: it proceeds as soon as the task
//! has started, watches the queue for exactly as long as the task runs, and
//! checks the queue once the drain has resolved the lease.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use prometheus_parking_lot::core::{LeaseQueue, ResultMailbox, ShutdownPolicy, TaskStatus};
use prometheus_parking_lot::infra::EmbeddedStore;
use prometheus_parking_lot::{DurableOptions, DurableResourcePool, HookBus, LifecycleEvent};

use crate::support::{Behaviour, Executor, event_log, key, limits, spawner, task};

const LEASE: Duration = Duration::from_secs(1);
/// Longer than three leases.
const RUN_FOR: Duration = Duration::from_millis(3_300);

fn options(name: &str, bus: &HookBus) -> DurableOptions {
    DurableOptions::new(LEASE)
        .with_name(name)
        .with_poll_interval(Duration::from_millis(50))
        .with_hooks(bus.clone())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn durable_lease_extension_keeps_a_long_task_from_redelivery() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = EmbeddedStore::open(dir.path().join("lease.redb")).expect("open store");
    let (bus, mut log) = event_log();
    // Both pools share the executor, so `runs` counts executions anywhere.
    let executor = Executor::new(Behaviour::SleepThenSucceed(RUN_FOR));
    let first = DurableResourcePool::start(
        limits(4),
        store.lease_queue::<String>(100),
        store.result_mailbox::<String>(),
        executor.clone(),
        spawner(),
        options("first", &bus),
    )
    .expect("start the first dispatcher");
    let second = DurableResourcePool::start(
        limits(4),
        store.lease_queue::<String>(100),
        store.result_mailbox::<String>(),
        executor.clone(),
        spawner(),
        options("second", &bus),
    )
    .expect("start the second dispatcher");

    first
        .submit(task("long", "long-task"))
        .await
        .expect("submit");

    // One of the pools leased the task and is running it.
    let started = log
        .wait_for(|event| matches!(event, LifecycleEvent::Started { .. }))
        .await;
    assert!(started.is_some(), "no pool started the task");

    // A third consumer polls the raw queue until the task's run ends: the
    // task is leased the whole time, so it must never be handed out.
    let stolen = Arc::new(AtomicU32::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let thief = tokio::spawn({
        let raw = store.lease_queue::<String>(100);
        let stolen = Arc::clone(&stolen);
        let stop = Arc::clone(&stop);
        async move {
            while !stop.load(Ordering::SeqCst) {
                let batch = raw.lease(1, LEASE).await.expect("lease");
                if let Some(leased) = batch.into_iter().next() {
                    stolen.fetch_add(1, Ordering::SeqCst);
                    let _ = raw.ack(leased.token).await;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    });
    let ended = log.wait_for(LifecycleEvent::is_terminal).await;
    stop.store(true, Ordering::SeqCst);
    thief.await.expect("the thief task");
    assert!(
        matches!(ended, Some(LifecycleEvent::Completed { task_id: 1, .. })),
        "{ended:?}; events: {:?}",
        log.kinds_of(1)
    );
    assert_eq!(
        stolen.load(Ordering::SeqCst),
        0,
        "a leased task was handed out"
    );
    assert_eq!(
        log.kinds_of(1),
        ["submitted", "started", "completed"],
        "one run, never redelivered"
    );

    // `Completed` is emitted once the result is stored.
    let mailbox = store.result_mailbox::<String>();
    let entry = mailbox
        .consume(&key("long"))
        .await
        .expect("consume")
        .expect("the long task delivered its result");
    assert_eq!(entry.status, TaskStatus::Completed);
    assert_eq!(executor.runs(), 1, "the long task ran more than once");
    assert!(
        mailbox
            .consume(&key("long"))
            .await
            .expect("consume")
            .is_none(),
        "exactly one result"
    );

    // The drain returns once every lease the pools held is resolved.
    let drain = ShutdownPolicy::Drain {
        deadline: Duration::from_secs(60),
    };
    let _ = first.shutdown_with(drain.clone()).await;
    let _ = second.shutdown_with(drain).await;
    let depth = store
        .lease_queue::<String>(100)
        .depth()
        .await
        .expect("depth");
    assert_eq!(depth.live(), 0, "{depth:?}");
}
