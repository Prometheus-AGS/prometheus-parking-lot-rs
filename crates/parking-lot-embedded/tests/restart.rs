//! Behaviour across a clean close and reopen, and under concurrent leasing.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use prometheus_parking_lot_core::{
    Delivery, DeliveryOutcome, Enqueued, LeaseQueue, MailboxKey, NackAction, Priority,
    ResourceCost, ResourceKind, ResultMailbox, ScheduledTask, SchedulerError, TaskMetadata,
    TaskStatus, TestClock,
};
use prometheus_parking_lot_embedded::EmbeddedStore;

const START_MS: u128 = 1_700_000_000_000;
const LEASE: Duration = Duration::from_secs(30);

fn task(name: &str, priority: Priority) -> ScheduledTask<String> {
    ScheduledTask {
        meta: TaskMetadata {
            id: 0,
            mailbox: None,
            priority,
            cost: ResourceCost::new(ResourceKind::Cpu, 1),
            deadline_ms: None,
            created_at_ms: 0,
            idempotency_key: Some(name.to_owned()),
            sequence: 0,
        },
        payload: name.to_owned(),
    }
}

fn mailbox_key() -> MailboxKey {
    MailboxKey {
        tenant: "restart".to_owned(),
        user_id: None,
        session_id: None,
    }
}

/// Queue order, attempt counts, idempotency keys, dead letters and the mailbox
/// (including a consumed result's remembered key) all survive close + reopen.
#[tokio::test]
async fn state_survives_close_and_reopen() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("restart.redb");
    let clock = TestClock::new(START_MS);

    {
        let store = EmbeddedStore::open_with_clock(&path, clock.clone()).expect("open");
        let queue = store.lease_queue::<String>(10);
        let mailbox = store.result_mailbox::<String>();
        for (name, priority) in [
            ("low", Priority::Low),
            ("critical", Priority::Critical),
            ("normal", Priority::Normal),
            ("poison", Priority::Normal),
        ] {
            let _ = queue.enqueue(task(name, priority)).await.expect("enqueue");
        }
        // Lease the critical task once and let the lease lapse (attempt 1 used).
        let first = queue.lease(1, LEASE).await.expect("lease").remove(0);
        assert_eq!(first.task.payload, "critical");
        // Dead-letter one task; the other two stay leased and will lapse.
        let rest = queue.lease(10, LEASE).await.expect("lease");
        let poison = rest
            .into_iter()
            .find(|l| l.task.payload == "poison")
            .expect("poison task leased");
        queue
            .nack(poison.token, NackAction::dead_letter("poison"))
            .await
            .expect("dead-letter");
        let delivery = Delivery::new(
            mailbox_key(),
            1,
            TaskStatus::Completed,
            Some("r".to_owned()),
        )
        .with_idempotency_key("done");
        assert_eq!(
            mailbox.deliver(delivery).await.expect("deliver"),
            DeliveryOutcome::Stored
        );
        assert!(
            mailbox
                .consume(&mailbox_key())
                .await
                .expect("consume")
                .is_some()
        );
    }

    clock.advance(LEASE * 2);
    let store = EmbeddedStore::open_with_clock(&path, clock).expect("reopen");
    let queue = store.lease_queue::<String>(10);
    let mailbox = store.result_mailbox::<String>();

    let depth = queue.depth().await.expect("depth");
    assert_eq!((depth.ready, depth.leased, depth.dead_lettered), (3, 0, 1));
    let leased = queue.lease(10, LEASE).await.expect("lease");
    let order: Vec<_> = leased.iter().map(|l| l.task.payload.as_str()).collect();
    assert_eq!(
        order,
        ["critical", "normal", "low"],
        "priority then FIFO after reopen"
    );
    assert!(
        leased.iter().all(|l| l.attempt >= 2),
        "every task was leased before the restart"
    );
    assert!(matches!(
        queue
            .enqueue(task("critical", Priority::Low))
            .await
            .expect("enqueue"),
        Enqueued::Duplicate(_)
    ));
    let letters = queue_dead_letters(&queue).await;
    assert!(letters.contains("\"reason\":\"poison\""), "{letters}");

    let again = Delivery::new(
        mailbox_key(),
        1,
        TaskStatus::Completed,
        Some("r2".to_owned()),
    )
    .with_idempotency_key("done");
    assert_eq!(
        mailbox.deliver(again).await.expect("deliver"),
        DeliveryOutcome::Duplicate {
            status: TaskStatus::Completed
        },
        "a consumed result's key is remembered across restarts"
    );
}

async fn queue_dead_letters(
    queue: &prometheus_parking_lot_embedded::RedbLeaseQueue<String>,
) -> String {
    queue
        .dead_letters()
        .await
        .expect("dead letters")
        .into_iter()
        .map(|(_, json)| json)
        .collect()
}

/// Entries are handed out once even when many tasks lease at the same time.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_leases_never_hand_out_an_entry_twice() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = EmbeddedStore::open(dir.path().join("concurrent.redb")).expect("open");
    let queue = Arc::new(store.lease_queue::<String>(1_000));
    for i in 0..60 {
        let _ = queue
            .enqueue(task(&format!("t{i}"), Priority::Normal))
            .await
            .expect("enqueue");
    }

    let mut workers = Vec::new();
    for _ in 0..6 {
        let queue = Arc::clone(&queue);
        workers.push(tokio::spawn(async move {
            let mut mine = Vec::new();
            loop {
                let batch = queue
                    .lease(3, Duration::from_secs(300))
                    .await
                    .expect("lease");
                if batch.is_empty() {
                    return mine;
                }
                for leased in batch {
                    let token = leased.token;
                    mine.push(leased.task.payload.clone());
                    queue.ack(token).await.expect("ack");
                }
            }
        }));
    }
    let mut all = Vec::new();
    for worker in workers {
        all.extend(worker.await.expect("worker"));
    }
    let unique: BTreeSet<_> = all.iter().cloned().collect();
    assert_eq!(all.len(), 60, "every task was processed");
    assert_eq!(unique.len(), 60, "no task was handed to two workers");
    assert_eq!(queue.depth().await.expect("depth").live(), 0);
}

/// A second handle on a file that is already open is refused, not corrupted.
#[tokio::test]
async fn second_open_of_the_same_file_is_an_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("locked.redb");
    let _first = EmbeddedStore::open(&path).expect("open");
    let second = EmbeddedStore::open(&path);
    assert!(
        matches!(second, Err(SchedulerError::Backend { .. })),
        "{second:?}"
    );
}
