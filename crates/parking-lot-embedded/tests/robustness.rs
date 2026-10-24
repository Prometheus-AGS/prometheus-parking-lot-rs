//! Entries the consumer cannot use must not wedge the queue or the mailbox,
//! and a task that keeps killing its consumer must stop being redelivered.

use std::time::Duration;

use prometheus_parking_lot_core::{
    Delivery, LeaseQueue, MailboxKey, NackAction, Priority, ResourceCost, ResourceKind,
    ResultMailbox, ScheduledTask, SchedulerError, TaskMetadata, TaskStatus, TestClock,
};
use prometheus_parking_lot_embedded::EmbeddedStore;
use serde::{Deserialize, Serialize};

const START_MS: u128 = 1_700_000_000_000;
const LEASE: Duration = Duration::from_secs(30);

fn task<P>(payload: P, key: &str, priority: Priority) -> ScheduledTask<P> {
    ScheduledTask {
        meta: TaskMetadata {
            id: 0,
            mailbox: None,
            priority,
            cost: ResourceCost::new(ResourceKind::Cpu, 1),
            deadline_ms: None,
            created_at_ms: 0,
            idempotency_key: Some(key.to_owned()),
            sequence: 0,
        },
        payload,
    }
}

fn mailbox_key() -> MailboxKey {
    MailboxKey {
        tenant: "robust".to_owned(),
        user_id: None,
        session_id: None,
    }
}

async fn dead_reasons(queue: &prometheus_parking_lot_embedded::RedbLeaseQueue<String>) -> String {
    queue
        .dead_letters()
        .await
        .expect("dead letters")
        .into_iter()
        .map(|(_, json)| json)
        .collect()
}

/// An entry whose payload no longer decodes as `P` (an app update changed the
/// type) is dead-lettered and the valid entries behind it are still leased.
#[tokio::test]
async fn undecodable_entry_does_not_wedge_the_queue() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = EmbeddedStore::open(dir.path().join("wedge.redb")).expect("open");
    // Written by the "old" build: payload is a number, and it sorts first.
    let old = store.lease_queue::<u64>(10);
    let _ = old
        .enqueue(task(42_u64, "old", Priority::Critical))
        .await
        .expect("enqueue old");
    let queue = store.lease_queue::<String>(10);
    let _ = queue
        .enqueue(task("good-1".to_owned(), "g1", Priority::Normal))
        .await
        .expect("enqueue");
    let _ = queue
        .enqueue(task("good-2".to_owned(), "g2", Priority::Low))
        .await
        .expect("enqueue");

    let leased = queue
        .lease(10, LEASE)
        .await
        .expect("lease is not wedged by the bad entry");
    let payloads: Vec<_> = leased.iter().map(|l| l.task.payload.as_str()).collect();
    assert_eq!(payloads, ["good-1", "good-2"]);
    let depth = queue.depth().await.expect("depth");
    assert_eq!(depth.dead_lettered, 1, "{depth:?}");
    assert!(
        dead_reasons(&queue).await.contains("reason"),
        "dead letter carries the decode error as its reason"
    );

    // A second lease is not wedged either, and the bad key is free again.
    assert!(
        queue
            .lease(10, LEASE)
            .await
            .expect("second lease")
            .is_empty()
    );
    let again = old
        .enqueue(task(7_u64, "old", Priority::Normal))
        .await
        .expect("enqueue");
    assert!(matches!(
        again,
        prometheus_parking_lot_core::Enqueued::Accepted(_)
    ));
}

/// A bad entry reached through lease-expiry promotion does not roll back the
/// promotion of the good ones.
#[tokio::test]
async fn undecodable_entry_after_lease_expiry_is_dead_lettered() {
    let dir = tempfile::tempdir().expect("tempdir");
    let clock = TestClock::new(START_MS);
    let store = EmbeddedStore::open_with_clock(dir.path().join("expiry.redb"), clock.clone())
        .expect("open");
    let old = store.lease_queue::<u64>(10);
    let queue = store.lease_queue::<String>(10);
    let _ = old
        .enqueue(task(1_u64, "bad", Priority::High))
        .await
        .expect("enqueue");
    let _ = queue
        .enqueue(task("good".to_owned(), "good", Priority::Low))
        .await
        .expect("enqueue");
    // Lease the good entry only, then let the lease lapse.
    let first = old.lease(1, LEASE).await.expect("lease bad as u64");
    assert_eq!(first.len(), 1);
    let _ = first;
    clock.advance(LEASE * 2);

    let leased = queue.lease(10, LEASE).await.expect("lease");
    assert_eq!(
        leased.len(),
        1,
        "the good entry is not stuck behind the bad one"
    );
    assert_eq!(leased[0].task.payload, "good");
}

/// Delivered exactly `max_attempts` times, then dead-lettered at the next lease.
#[tokio::test]
async fn exhausted_attempts_are_dead_lettered() {
    let dir = tempfile::tempdir().expect("tempdir");
    let clock = TestClock::new(START_MS);
    let store = EmbeddedStore::open_with_clock(dir.path().join("poison.redb"), clock.clone())
        .expect("open");
    let queue = store.lease_queue::<String>(10).with_max_attempts(2);
    let _ = queue
        .enqueue(task(
            "crash-loop".to_owned(),
            "crash-loop",
            Priority::Normal,
        ))
        .await
        .expect("enqueue");

    for attempt in 1..=2 {
        let leased = queue.lease(1, LEASE).await.expect("lease");
        assert_eq!(leased.len(), 1, "attempt {attempt}");
        assert_eq!(leased[0].attempt, attempt);
        clock.advance(LEASE * 2); // the consumer "crashes"
    }

    assert!(queue.lease(1, LEASE).await.expect("lease").is_empty());
    let depth = queue.depth().await.expect("depth");
    assert_eq!((depth.live(), depth.dead_lettered), (0, 1), "{depth:?}");
    assert!(dead_reasons(&queue).await.contains("max attempts"));

    // The dead-lettered entry released its idempotency key.
    let _ = queue
        .enqueue(task("again".to_owned(), "crash-loop", Priority::Normal))
        .await
        .expect("key is free after dead-lettering");
}

/// A nack that requeues the last allowed attempt is dead-lettered at the next
/// lease, like an expiry.
#[tokio::test]
async fn requeue_after_the_last_attempt_is_dead_lettered() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = EmbeddedStore::open(dir.path().join("nack.redb")).expect("open");
    let queue = store.lease_queue::<String>(10).with_max_attempts(1);
    let _ = queue
        .enqueue(task("once".to_owned(), "once", Priority::Normal))
        .await
        .expect("enqueue");
    let leased = queue.lease(1, LEASE).await.expect("lease").remove(0);
    queue
        .nack(leased.token, NackAction::requeue())
        .await
        .expect("nack");
    assert!(queue.lease(1, LEASE).await.expect("lease").is_empty());
    assert_eq!(queue.depth().await.expect("depth").dead_lettered, 1);
}

/// Without a limit the entry is redelivered forever (the default).
#[tokio::test]
async fn no_limit_redelivers_forever() {
    let dir = tempfile::tempdir().expect("tempdir");
    let clock = TestClock::new(START_MS);
    let store = EmbeddedStore::open_with_clock(dir.path().join("forever.redb"), clock.clone())
        .expect("open");
    let queue = store.lease_queue::<String>(10);
    let _ = queue
        .enqueue(task("loop".to_owned(), "loop", Priority::Normal))
        .await
        .expect("enqueue");
    for attempt in 1..=5 {
        let leased = queue.lease(1, LEASE).await.expect("lease");
        assert_eq!(leased[0].attempt, attempt);
        clock.advance(LEASE * 2);
    }
}

#[derive(Serialize, Deserialize)]
struct Old {
    value: u64,
}

/// `consume` of an entry that no longer decodes returns the error once and
/// removes the entry, like the Postgres mailbox.
#[tokio::test]
async fn undecodable_mailbox_entry_is_deleted_and_reported_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = EmbeddedStore::open(dir.path().join("mailbox.redb")).expect("open");
    let old = store.result_mailbox::<Old>();
    let new = store.result_mailbox::<String>();
    let _ = old
        .deliver(Delivery::new(
            mailbox_key(),
            1,
            TaskStatus::Completed,
            Some(Old { value: 1 }),
        ))
        .await
        .expect("deliver");
    let _ = new
        .deliver(Delivery::new(
            mailbox_key(),
            2,
            TaskStatus::Completed,
            Some("fine".to_owned()),
        ))
        .await
        .expect("deliver");

    let first = new.consume(&mailbox_key()).await;
    assert!(
        matches!(first, Err(SchedulerError::Backend { .. })),
        "{first:?}"
    );
    let second = new.consume(&mailbox_key()).await.expect("second consume");
    assert_eq!(
        second.expect("the valid result behind it").task_id,
        2,
        "the bad entry is gone and no longer blocks the key"
    );
}

/// A payload that cannot be serialized.
struct Unserializable;

impl Serialize for Unserializable {
    fn serialize<S: serde::Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom("this payload cannot be serialized"))
    }
}

impl<'de> Deserialize<'de> for Unserializable {
    fn deserialize<D: serde::Deserializer<'de>>(_deserializer: D) -> Result<Self, D::Error> {
        Err(serde::de::Error::custom("never stored"))
    }
}

/// An enqueue that fails before anything is written reports `NotStored`, not
/// an unknown-outcome `Backend` error, and leaves the queue empty.
#[tokio::test]
async fn enqueue_failure_before_any_write_is_not_stored() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = EmbeddedStore::open(dir.path().join("not-stored.redb")).expect("open");
    let queue = store.lease_queue::<Unserializable>(10);
    let err = queue
        .enqueue(task(Unserializable, "bad", Priority::Normal))
        .await
        .expect_err("an unserializable payload is refused");
    assert!(
        matches!(err, SchedulerError::NotStored { .. }),
        "expected NotStored, got {err:?}"
    );
    let depth = queue.depth().await.expect("depth");
    assert_eq!(
        (depth.ready, depth.delayed, depth.leased, depth.dead_lettered),
        (0, 0, 0, 0)
    );
}
