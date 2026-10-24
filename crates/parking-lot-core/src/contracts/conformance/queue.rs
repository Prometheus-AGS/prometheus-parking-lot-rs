//! [`LeaseQueue`] conformance cases. Each takes a [`QueueHarness`], builds its
//! own queue and panics on the first violated rule.

use std::time::Duration;

use super::QueueHarness;
use crate::contracts::error::SchedulerError;
use crate::contracts::lease_queue::{Enqueued, LeaseQueue, LeaseToken, LeasedTask, NackAction};
use crate::contracts::task::{ScheduledTask, TaskMetadata};
use crate::contracts::types::{MailboxKey, Priority, ResourceCost, ResourceKind};

/// Depth used when a case does not test the limit.
const ROOMY: usize = 100;

/// A task whose payload is `payload` at `priority`.
fn task(priority: Priority, payload: &str) -> ScheduledTask<String> {
    ScheduledTask {
        meta: TaskMetadata {
            id: 0,
            mailbox: None,
            priority,
            cost: ResourceCost::new(ResourceKind::Cpu, 1),
            deadline_ms: None,
            created_at_ms: 0,
            idempotency_key: None,
            sequence: 0,
        },
        payload: payload.to_owned(),
    }
}

/// A normal-priority task with an idempotency key.
fn keyed(payload: &str, key: &str) -> ScheduledTask<String> {
    let mut task = task(Priority::Normal, payload);
    task.meta.idempotency_key = Some(key.to_owned());
    task
}

async fn enqueue<Q: LeaseQueue<String>>(queue: &Q, task: ScheduledTask<String>) -> Enqueued {
    queue.enqueue(task).await.expect("enqueue")
}

async fn lease<Q: LeaseQueue<String>>(
    queue: &Q,
    max: usize,
    lease_for: Duration,
) -> Vec<LeasedTask<String>> {
    queue.lease(max, lease_for).await.expect("lease")
}

/// Lease exactly one task and return it.
async fn lease_one<Q: LeaseQueue<String>>(queue: &Q, lease_for: Duration) -> LeasedTask<String> {
    let mut leased = lease(queue, 1, lease_for).await;
    assert_eq!(leased.len(), 1, "expected one leasable task");
    leased.remove(0)
}

fn payloads(leased: &[LeasedTask<String>]) -> Vec<&str> {
    leased.iter().map(|t| t.task.payload.as_str()).collect()
}

fn assert_lease_lost(result: Result<(), SchedulerError>, token: LeaseToken, what: &str) {
    match result {
        Err(SchedulerError::LeaseLost { entry }) => assert_eq!(entry, token.entry().0, "{what}"),
        other => panic!("{what}: expected LeaseLost, got {other:?}"),
    }
}

/// Enqueue returns `Accepted`; the leased task carries its metadata and
/// payload unchanged, attempt 1 and a token for its entry.
pub async fn enqueue_and_lease_round_trip<H: QueueHarness>(h: &H) {
    let queue = h.queue(ROOMY).await;
    let mut original = task(Priority::High, "payload-1");
    original.meta.id = 41;
    original.meta.cost = ResourceCost::new(ResourceKind::GpuVram, 8).with(ResourceKind::Cpu, 2);
    original.meta.deadline_ms = Some(9_000_000_000_000);
    original.meta.created_at_ms = 1_700_000_000_000;
    original.meta.idempotency_key = Some("idem-41".into());
    original.meta.sequence = 7;
    original.meta.mailbox = Some(MailboxKey {
        tenant: "tenant".into(),
        user_id: Some("user".into()),
        session_id: Some("session".into()),
    });

    let Enqueued::Accepted(entry) = enqueue(&queue, original.clone()).await else {
        panic!("first enqueue must be Accepted");
    };
    let leased = lease_one(&queue, h.lease_duration()).await;

    assert_eq!(leased.token.entry(), entry);
    assert_eq!(leased.attempt, 1);
    assert_eq!(leased.task.payload, original.payload);
    let (got, want) = (&leased.task.meta, &original.meta);
    assert_eq!(got.id, want.id);
    assert_eq!(got.priority, want.priority);
    assert_eq!(got.cost, want.cost);
    assert_eq!(got.deadline_ms, want.deadline_ms);
    assert_eq!(got.created_at_ms, want.created_at_ms);
    assert_eq!(got.idempotency_key, want.idempotency_key);
    assert_eq!(got.mailbox, want.mailbox);
    assert_eq!(
        got.sequence, want.sequence,
        "sequence must survive the round trip"
    );
    queue.ack(leased.token).await.expect("ack");
}

/// A leased task is invisible to other lease calls while its lease holds.
pub async fn leased_task_is_invisible<H: QueueHarness>(h: &H) {
    let queue = h.queue(ROOMY).await;
    // A lease far longer than the time the case lets pass: on a backend that
    // reads the real clock, a slow machine must not be able to expire it
    // between two of the calls below.
    let lease_for = h.lease_duration() * 10;
    let _ = enqueue(&queue, task(Priority::Normal, "only")).await;
    let leased = lease_one(&queue, lease_for).await;

    h.advance(h.lease_duration() / 2).await;
    assert!(
        lease(&queue, 10, lease_for).await.is_empty(),
        "leased task was handed out twice"
    );
    let depth = queue.depth().await.expect("depth");
    assert_eq!((depth.ready, depth.leased), (0, 1), "{depth:?}");
    queue.ack(leased.token).await.expect("ack within lease");
}

/// After the lease expires the task is redelivered with a higher attempt and
/// a new token; the old token is fenced off and the new one works.
pub async fn lease_expiry_redelivers<H: QueueHarness>(h: &H) {
    let queue = h.queue(ROOMY).await;
    let lease_for = h.lease_duration();
    let _ = enqueue(&queue, task(Priority::Normal, "crashy")).await;
    let first = lease_one(&queue, lease_for).await;

    h.advance(lease_for + lease_for / 2).await;
    let second = lease_one(&queue, lease_for).await;
    assert_eq!(second.task.payload, "crashy");
    assert_eq!(second.token.entry(), first.token.entry());
    assert_ne!(second.token, first.token, "a redelivery needs a new token");
    assert_eq!(second.attempt, 2);

    assert_lease_lost(
        queue.ack(first.token).await,
        first.token,
        "ack with the expired token",
    );
    assert_lease_lost(
        queue
            .nack(first.token, NackAction::dead_letter("stale"))
            .await,
        first.token,
        "nack with the expired token",
    );
    queue
        .ack(second.token)
        .await
        .expect("current lease holder acks");
}

/// Once a lease has expired its token is useless even before the task is
/// redelivered: no ack, extend or nack revives it.
pub async fn expired_lease_cannot_be_used<H: QueueHarness>(h: &H) {
    let queue = h.queue(ROOMY).await;
    let lease_for = h.lease_duration();
    let _ = enqueue(&queue, task(Priority::Normal, "late")).await;
    let leased = lease_one(&queue, lease_for).await;

    h.advance(lease_for + lease_for / 2).await;
    assert_lease_lost(
        queue.extend(leased.token, lease_for).await,
        leased.token,
        "extend after expiry",
    );
    assert_lease_lost(
        queue.ack(leased.token).await,
        leased.token,
        "ack after expiry",
    );
    assert_lease_lost(
        queue.nack(leased.token, NackAction::requeue()).await,
        leased.token,
        "nack after expiry",
    );
    let depth = queue.depth().await.expect("depth");
    assert_eq!(
        (depth.ready, depth.leased),
        (1, 0),
        "an expired lease is ready: {depth:?}"
    );
    assert_eq!(lease_one(&queue, lease_for).await.attempt, 2);
}

/// Ack removes the task for good; a second ack is `LeaseLost`.
pub async fn ack_removes_task<H: QueueHarness>(h: &H) {
    let queue = h.queue(ROOMY).await;
    let lease_for = h.lease_duration();
    let _ = enqueue(&queue, task(Priority::Normal, "done")).await;
    let leased = lease_one(&queue, lease_for).await;
    queue.ack(leased.token).await.expect("ack");

    h.advance(lease_for * 2).await;
    assert!(
        lease(&queue, 10, lease_for).await.is_empty(),
        "acked task came back"
    );
    assert_eq!(queue.depth().await.expect("depth").live(), 0);
    assert_lease_lost(queue.ack(leased.token).await, leased.token, "second ack");
}

/// Nack-requeue makes the task ready at once, as a new attempt; the old
/// token is fenced off.
pub async fn nack_requeue_redelivers<H: QueueHarness>(h: &H) {
    let queue = h.queue(ROOMY).await;
    let lease_for = h.lease_duration();
    let _ = enqueue(&queue, task(Priority::Normal, "retry")).await;
    let first = lease_one(&queue, lease_for).await;
    queue
        .nack(first.token, NackAction::requeue())
        .await
        .expect("nack");

    let second = lease_one(&queue, lease_for).await;
    assert_eq!(second.task.payload, "retry");
    assert_eq!(second.attempt, 2);
    assert_lease_lost(
        queue.ack(first.token).await,
        first.token,
        "ack with the nacked token",
    );
    queue.ack(second.token).await.expect("ack");
}

/// Nack-requeue with a delay hides the task until the delay has elapsed.
pub async fn nack_requeue_with_delay<H: QueueHarness>(h: &H) {
    let queue = h.queue(ROOMY).await;
    let lease_for = h.lease_duration();
    let _ = enqueue(&queue, task(Priority::Normal, "backoff")).await;
    let first = lease_one(&queue, lease_for).await;
    queue
        .nack(first.token, NackAction::requeue_after(lease_for))
        .await
        .expect("nack");

    assert!(
        lease(&queue, 10, lease_for).await.is_empty(),
        "delayed task leased early"
    );
    let depth = queue.depth().await.expect("depth");
    assert_eq!(
        (depth.ready, depth.delayed, depth.leased),
        (0, 1, 0),
        "{depth:?}"
    );

    h.advance(lease_for + lease_for / 2).await;
    let second = lease_one(&queue, lease_for).await;
    assert_eq!(second.attempt, 2);
    queue.ack(second.token).await.expect("ack");
}

/// A nacked token is resolved: while the delay runs, `ack`, `extend` and
/// `nack` with it are `LeaseLost` and leave the requeued task alone, which is
/// redelivered once the delay has elapsed.
pub async fn nacked_token_is_fenced_during_delay<H: QueueHarness>(h: &H) {
    let queue = h.queue(ROOMY).await;
    let lease_for = h.lease_duration();
    let _ = enqueue(&queue, task(Priority::Normal, "backoff")).await;
    let first = lease_one(&queue, lease_for).await;
    queue
        .nack(first.token, NackAction::requeue_after(lease_for))
        .await
        .expect("nack");

    assert_lease_lost(
        queue.ack(first.token).await,
        first.token,
        "ack with the nacked token during the delay",
    );
    // An extend that took effect would hide the task well past the delay.
    assert_lease_lost(
        queue.extend(first.token, lease_for * 10).await,
        first.token,
        "extend with the nacked token during the delay",
    );
    assert_lease_lost(
        queue.nack(first.token, NackAction::requeue()).await,
        first.token,
        "nack with the nacked token during the delay",
    );

    h.advance(lease_for + lease_for / 2).await;
    let second = lease_one(&queue, lease_for).await;
    assert_eq!(second.task.payload, "backoff");
    assert_eq!(second.attempt, 2);
    assert_lease_lost(
        queue.ack(first.token).await,
        first.token,
        "ack with the nacked token after redelivery",
    );
    queue.ack(second.token).await.expect("ack");
}

/// Dead-lettering removes the task from the live set for good, counts it,
/// frees its idempotency key and fences the token.
pub async fn nack_dead_letter<H: QueueHarness>(h: &H) {
    let queue = h.queue(ROOMY).await;
    let lease_for = h.lease_duration();
    let entry = enqueue(&queue, keyed("poison", "idem-poison"))
        .await
        .entry();
    let leased = lease_one(&queue, lease_for).await;
    queue
        .nack(leased.token, NackAction::dead_letter("always fails"))
        .await
        .expect("dead-letter");

    h.advance(lease_for * 2).await;
    assert!(
        lease(&queue, 10, lease_for).await.is_empty(),
        "dead letter was leased"
    );
    let depth = queue.depth().await.expect("depth");
    assert_eq!((depth.live(), depth.dead_lettered), (0, 1), "{depth:?}");
    assert_lease_lost(
        queue.ack(leased.token).await,
        leased.token,
        "ack after dead-letter",
    );

    let again = enqueue(&queue, keyed("poison", "idem-poison")).await;
    assert!(
        matches!(again, Enqueued::Accepted(id) if id != entry),
        "a dead-lettered key no longer deduplicates: {again:?}"
    );
}

/// Extending a lease keeps the task leased past its original expiry; the
/// new expiry counts from the extend.
pub async fn extend_keeps_lease<H: QueueHarness>(h: &H) {
    let queue = h.queue(ROOMY).await;
    let lease_for = h.lease_duration();
    let _ = enqueue(&queue, task(Priority::Normal, "long")).await;
    let leased = lease_one(&queue, lease_for).await;

    // Extend early (a quarter of the lease) and by far more than the case
    // waits afterwards, so a slow machine cannot expire either lease between
    // calls on a backend that reads the real clock.
    h.advance(lease_for / 4).await;
    queue
        .extend(leased.token, lease_for * 10)
        .await
        .expect("extend");
    // Past the original expiry (1.25 x), far inside the extended one.
    h.advance(lease_for).await;
    assert!(
        lease(&queue, 10, lease_for).await.is_empty(),
        "extended lease was not honoured"
    );
    queue
        .ack(leased.token)
        .await
        .expect("ack under the extended lease");
}

/// Leases hand out the highest priority first and enqueue order within a
/// priority, across separate lease calls.
pub async fn priority_then_fifo<H: QueueHarness>(h: &H) {
    let queue = h.queue(ROOMY).await;
    let lease_for = h.lease_duration();
    for (priority, payload) in [
        (Priority::Low, "low-1"),
        (Priority::High, "high-1"),
        (Priority::Normal, "normal-1"),
        (Priority::Critical, "critical-1"),
        (Priority::High, "high-2"),
        (Priority::Normal, "normal-2"),
        (Priority::High, "high-3"),
    ] {
        let _ = enqueue(&queue, task(priority, payload)).await;
    }

    let first = lease(&queue, 3, lease_for).await;
    assert_eq!(payloads(&first), ["critical-1", "high-1", "high-2"]);
    let rest = lease(&queue, 10, lease_for).await;
    assert_eq!(payloads(&rest), ["high-3", "normal-1", "normal-2", "low-1"]);
}

/// A requeued task keeps its place ahead of later tasks of its priority.
pub async fn requeued_task_keeps_its_place<H: QueueHarness>(h: &H) {
    let queue = h.queue(ROOMY).await;
    let lease_for = h.lease_duration();
    for payload in ["first", "second", "third"] {
        let _ = enqueue(&queue, task(Priority::Normal, payload)).await;
    }
    let head = lease_one(&queue, lease_for).await;
    assert_eq!(head.task.payload, "first");
    queue
        .nack(head.token, NackAction::requeue())
        .await
        .expect("nack");

    let all = lease(&queue, 10, lease_for).await;
    assert_eq!(payloads(&all), ["first", "second", "third"]);
}

/// `max == 0` leases nothing; a zero lease duration is rejected for lease
/// and extend.
pub async fn lease_arguments<H: QueueHarness>(h: &H) {
    let queue = h.queue(ROOMY).await;
    let lease_for = h.lease_duration();
    let _ = enqueue(&queue, task(Priority::Normal, "a")).await;

    assert!(lease(&queue, 0, lease_for).await.is_empty());
    assert!(matches!(
        queue.lease(1, Duration::ZERO).await,
        Err(SchedulerError::InvalidConfig { .. })
    ));
    let leased = lease_one(&queue, lease_for).await;
    assert!(matches!(
        queue.extend(leased.token, Duration::ZERO).await,
        Err(SchedulerError::InvalidConfig { .. })
    ));
    queue.ack(leased.token).await.expect("ack");
}

/// Ready, delayed and leased entries count toward the maximum depth; acked
/// and dead-lettered ones do not.
pub async fn max_depth_counts_live_entries<H: QueueHarness>(h: &H) {
    let queue = h.queue(2).await;
    let lease_for = h.lease_duration();
    assert_eq!(queue.max_depth(), 2);
    let _ = enqueue(&queue, task(Priority::Normal, "a")).await;
    let _ = enqueue(&queue, task(Priority::Normal, "b")).await;
    let full = queue.enqueue(task(Priority::Normal, "c")).await;
    assert!(
        matches!(full, Err(SchedulerError::QueueFull { .. })),
        "{full:?}"
    );

    let a = lease_one(&queue, lease_for).await;
    assert!(
        matches!(
            queue.enqueue(task(Priority::Normal, "c")).await,
            Err(SchedulerError::QueueFull { .. })
        ),
        "a leased entry still counts toward the depth"
    );
    queue
        .nack(a.token, NackAction::dead_letter("make room"))
        .await
        .expect("dead-letter");
    let _ = enqueue(&queue, task(Priority::Normal, "c")).await;

    let b = lease_one(&queue, lease_for).await;
    queue.ack(b.token).await.expect("ack");
    let _ = enqueue(&queue, task(Priority::Normal, "d")).await;
    assert_eq!(queue.depth().await.expect("depth").live(), 2);
}

/// Enqueueing a key that an entry still in the queue has (ready or leased)
/// is a `Duplicate` of that entry; after the entry is acked the key is free.
pub async fn idempotent_enqueue<H: QueueHarness>(h: &H) {
    let queue = h.queue(ROOMY).await;
    let lease_for = h.lease_duration();
    let Enqueued::Accepted(entry) = enqueue(&queue, keyed("job", "idem-1")).await else {
        panic!("first enqueue must be Accepted");
    };
    assert_eq!(
        enqueue(&queue, keyed("job", "idem-1")).await,
        Enqueued::Duplicate(entry)
    );
    let _ = enqueue(&queue, keyed("other", "idem-2")).await;
    assert_eq!(queue.depth().await.expect("depth").live(), 2);

    let leased = lease_one(&queue, lease_for).await;
    assert_eq!(leased.token.entry(), entry);
    assert_eq!(
        enqueue(&queue, keyed("job", "idem-1")).await,
        Enqueued::Duplicate(entry),
        "a leased entry still deduplicates"
    );
    queue.ack(leased.token).await.expect("ack");
    assert!(
        matches!(enqueue(&queue, keyed("job", "idem-1")).await, Enqueued::Accepted(id) if id != entry),
        "an acked key no longer deduplicates"
    );
}

/// [`LeaseQueue::identity`] is non-empty, the same on every call and while
/// the queue is used, and differs between two distinct queues (whose entry
/// ids repeat: both start empty).
pub async fn identity_is_stable_and_distinct<H: QueueHarness>(h: &H) {
    let first = h.queue(ROOMY).await;
    let second = h.queue(ROOMY).await;
    let identity = first.identity().to_owned();
    assert!(!identity.trim().is_empty(), "empty queue identity");
    let _ = enqueue(&first, task(Priority::Normal, "job")).await;
    let leased = lease_one(&first, h.lease_duration()).await;
    first.ack(leased.token).await.expect("ack");
    assert_eq!(first.identity(), identity, "the identity changed with use");
    assert_ne!(
        second.identity(),
        identity,
        "two distinct queues share an identity, so their entry ids collide"
    );
}

/// Run every queue case in sequence, each on a fresh queue.
pub async fn run_all<H: QueueHarness>(h: &H) {
    enqueue_and_lease_round_trip(h).await;
    leased_task_is_invisible(h).await;
    lease_expiry_redelivers(h).await;
    expired_lease_cannot_be_used(h).await;
    ack_removes_task(h).await;
    nack_requeue_redelivers(h).await;
    nack_requeue_with_delay(h).await;
    nacked_token_is_fenced_during_delay(h).await;
    nack_dead_letter(h).await;
    extend_keeps_lease(h).await;
    priority_then_fifo(h).await;
    requeued_task_keeps_its_place(h).await;
    lease_arguments(h).await;
    max_depth_counts_live_entries(h).await;
    idempotent_enqueue(h).await;
    identity_is_stable_and_distinct(h).await;
}
