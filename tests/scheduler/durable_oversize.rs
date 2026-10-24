//! A leased task too large for the pool that leased it (W1): handed back for
//! another consumer, but only up to the oversize attempt limit, after which
//! its client gets a terminal `Dropped` outcome instead of waiting forever.

use std::sync::Arc;
use std::time::Duration;

use prometheus_parking_lot::core::{LeaseQueue, ResultMailbox, ShutdownPolicy, TaskStatus};
use prometheus_parking_lot::infra::queue::InMemoryLeaseQueue;
use prometheus_parking_lot::util::{ResourceCost, ResourceKind};
use prometheus_parking_lot::{
    DurableOptions, DurableResourcePool, LifecycleEvent, RejectionReason,
};

use crate::durable_dispatch::{SharedMailbox, SharedQueue};
use crate::support::{Behaviour, EventLog, Executor, event_log, key, limits, spawner, task};

const LEASE: Duration = Duration::from_millis(200);
/// `DurableOptions`' default oversize attempt limit.
const DEFAULT_LIMIT: u32 = 5;

/// Enqueue, straight into the queue (as a larger pool's `submit` would), a
/// task needing two units, delivered under `key("big")`; its id is 1.
async fn enqueue_two_unit_task(queue: &Arc<InMemoryLeaseQueue<String>>) {
    let mut big = task("big", "big");
    big.meta.cost = ResourceCost::new(ResourceKind::Cpu, 2);
    let _ = queue.enqueue(big).await.expect("enqueue");
}

fn shared(queue: &Arc<InMemoryLeaseQueue<String>>) -> SharedQueue {
    SharedQueue {
        inner: Arc::clone(queue),
        lose_acks: false,
    }
}

const fn is_requeued(event: &LifecycleEvent) -> bool {
    matches!(event, LifecycleEvent::Requeued { .. })
}

/// The attempts of the recorded `Requeued` events, in order.
fn requeued_attempts(log: &EventLog) -> Vec<u32> {
    log.seen
        .iter()
        .filter_map(|event| match event {
            LifecycleEvent::Requeued { attempt, .. } => Some(*attempt),
            _ => None,
        })
        .collect()
}

/// The only consumer of the queue has one unit; the task needs two. With the
/// default options it is handed back `limit - 1` times, then settled: its
/// mailbox key receives `Dropped`, the run ends in `Rejected`
/// (`CapacityExceeded`) and the entry is dead-lettered. Before the bound
/// existed it cycled forever and the client never got a result.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn durable_task_too_large_for_every_consumer_gets_a_terminal_outcome() {
    let queue = Arc::new(InMemoryLeaseQueue::<String>::new(100));
    let mailbox = SharedMailbox::default();
    enqueue_two_unit_task(&queue).await;
    let (bus, mut log) = event_log();
    let runs = Executor::new(Behaviour::Succeed);
    let small = DurableResourcePool::start(
        limits(1),
        shared(&queue),
        mailbox.clone(),
        runs.clone(),
        spawner(),
        DurableOptions::new(LEASE)
            .with_name("small")
            .with_poll_interval(Duration::from_millis(20))
            .with_hooks(bus),
    )
    .expect("start the small pool");

    let rejected = log
        .wait_for(|event| matches!(event, LifecycleEvent::Rejected { .. }))
        .await
        .unwrap_or_else(|| {
            panic!(
                "no terminal outcome; requeued at attempts {:?}",
                requeued_attempts(&log)
            )
        });
    let LifecycleEvent::Rejected {
        task_id, reason, ..
    } = rejected
    else {
        unreachable!("matched above");
    };
    assert_eq!(task_id, 1);
    assert_eq!(reason, RejectionReason::CapacityExceeded { kind: None });
    let below_limit: Vec<u32> = (1..DEFAULT_LIMIT).collect();
    assert_eq!(
        requeued_attempts(&log),
        below_limit,
        "handed back once per attempt below the limit"
    );

    // The outcome is stored before the terminal event is emitted.
    let entry = mailbox
        .inner
        .consume(&key("big"))
        .await
        .expect("consume")
        .expect("the client receives a terminal outcome");
    assert!(
        matches!(&entry.status, TaskStatus::Dropped(reason) if reason.contains("capacity exceeded")),
        "{:?}",
        entry.status
    );

    // The drain returns once the lease is resolved (dead-lettered).
    let report = small
        .shutdown_with(ShutdownPolicy::Drain {
            deadline: Duration::from_secs(30),
        })
        .await;
    assert_eq!(report.requeued, 0, "{report:?}");
    let depth = queue.depth().await.expect("depth");
    assert_eq!((depth.live(), depth.dead_lettered), (0, 1), "{depth:?}");
    assert_eq!(runs.runs(), 0, "the task never ran");
}

/// A consumer that can run the task still gets it before the bound: the
/// small pool hands it back once, then a large pool on the same queue leases
/// it at attempt 2 and runs it, although the small pool's limit is 2. The
/// limit only applies to consumers that cannot run the task.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn durable_task_too_large_here_still_runs_on_a_consumer_that_fits_it() {
    let queue = Arc::new(InMemoryLeaseQueue::<String>::new(100));
    let mailbox = SharedMailbox::default();
    enqueue_two_unit_task(&queue).await;
    let (bus, mut log) = event_log();

    let small_runs = Executor::new(Behaviour::Succeed);
    // The small pool leases at once when it starts, then polls only every
    // 30 s: after its one hand-back, the large pool is the one that leases.
    let small = DurableResourcePool::start(
        limits(1),
        shared(&queue),
        mailbox.clone(),
        small_runs.clone(),
        spawner(),
        DurableOptions::new(LEASE)
            .with_name("small")
            .with_poll_interval(Duration::from_secs(30))
            .with_oversize_attempt_limit(2)
            .with_hooks(bus.clone()),
    )
    .expect("start the small pool");
    let handed_back = log.wait_for(is_requeued).await;
    assert!(
        matches!(handed_back, Some(LifecycleEvent::Requeued { attempt: 1, ref pool, .. }) if pool == "small"),
        "{handed_back:?}"
    );

    let large_runs = Executor::new(Behaviour::Succeed);
    let large = DurableResourcePool::start(
        limits(4),
        shared(&queue),
        mailbox.clone(),
        large_runs.clone(),
        spawner(),
        DurableOptions::new(LEASE)
            .with_name("large")
            .with_poll_interval(Duration::from_millis(20))
            .with_oversize_attempt_limit(2)
            .with_hooks(bus),
    )
    .expect("start the large pool");
    let completed = log
        .wait_for(|event| {
            matches!(
                event,
                LifecycleEvent::Completed { .. } | LifecycleEvent::Rejected { .. }
            )
        })
        .await;
    assert!(
        matches!(completed, Some(LifecycleEvent::Completed { task_id: 1, ref pool, .. }) if pool == "large"),
        "{completed:?}; events: {:?}",
        log.kinds_of(1)
    );

    let entry = mailbox
        .inner
        .consume(&key("big"))
        .await
        .expect("consume")
        .expect("the large pool stored the result");
    assert_eq!(entry.status, TaskStatus::Completed);
    assert_eq!((small_runs.runs(), large_runs.runs()), (0, 1));

    let drain = ShutdownPolicy::Drain {
        deadline: Duration::from_secs(30),
    };
    let _ = large.shutdown_with(drain.clone()).await;
    let _ = small.shutdown_with(drain).await;
    let depth = queue.depth().await.expect("depth");
    assert_eq!((depth.live(), depth.dead_lettered), (0, 0), "{depth:?}");
}
