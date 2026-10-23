//! Lease extension: a task that runs for more than three lease durations keeps
//! its lease through the heartbeat and is never leased by a second dispatcher.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use prometheus_parking_lot::core::{LeaseQueue, ResultMailbox, TaskStatus};
use prometheus_parking_lot::infra::EmbeddedStore;
use prometheus_parking_lot::{DurableOptions, DurableResourcePool};

use crate::support::{Behaviour, Executor, eventually, key, limits, spawner, task};

const LEASE: Duration = Duration::from_secs(1);
/// Longer than three leases.
const RUN_FOR: Duration = Duration::from_millis(3_300);

fn options(name: &str) -> DurableOptions {
    DurableOptions::new(LEASE)
        .with_name(name)
        .with_poll_interval(Duration::from_millis(50))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn durable_lease_extension_keeps_a_long_task_from_redelivery() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = EmbeddedStore::open(dir.path().join("lease.redb")).expect("open store");
    // Both pools share the executor, so `runs` counts executions anywhere.
    let executor = Executor::new(Behaviour::SleepThenSucceed(RUN_FOR));
    let first = DurableResourcePool::start(
        limits(4),
        store.lease_queue::<String>(100),
        store.result_mailbox::<String>(),
        executor.clone(),
        spawner(),
        options("first"),
    )
    .expect("start the first dispatcher");
    let second = DurableResourcePool::start(
        limits(4),
        store.lease_queue::<String>(100),
        store.result_mailbox::<String>(),
        executor.clone(),
        spawner(),
        options("second"),
    )
    .expect("start the second dispatcher");

    first
        .submit(task("long", "long-task"))
        .await
        .expect("submit");

    // Wait until one of the pools holds the task.
    let raw = store.lease_queue::<String>(100);
    let raw_ref = &raw;
    // Generous: under heavy machine load the first lease can take seconds.
    let held = eventually(Duration::from_secs(60), || async move {
        let depth = raw_ref.depth().await.expect("depth");
        (depth.leased == 1).then_some(())
    })
    .await;
    assert!(held.is_some(), "no pool leased the task");

    // A third consumer polls the raw queue for the whole run: the task is
    // leased the entire time, so it must never be handed out.
    let stolen = Arc::new(AtomicU32::new(0));
    let poll_until = tokio::time::Instant::now() + RUN_FOR;
    while tokio::time::Instant::now() < poll_until {
        let batch = raw.lease(1, LEASE).await.expect("lease");
        if let Some(leased) = batch.into_iter().next() {
            stolen.fetch_add(1, Ordering::SeqCst);
            let _ = raw.ack(leased.token).await;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        stolen.load(Ordering::SeqCst),
        0,
        "a leased task was handed out"
    );

    let mailbox = store.result_mailbox::<String>();
    let mailbox_ref = &mailbox;
    let entry = eventually(Duration::from_secs(60), || async move {
        mailbox_ref.consume(&key("long")).await.expect("consume")
    })
    .await
    .expect("the long task delivers its result");
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
    let depth = store
        .lease_queue::<String>(100)
        .depth()
        .await
        .expect("depth");
    assert_eq!(depth.live(), 0, "{depth:?}");
    drop((first, second));
}
