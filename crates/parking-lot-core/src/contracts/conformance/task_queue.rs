//! Synchronous [`TaskQueue`] conformance for per-task expiry removal.
//!
//! These cases accept a real, empty queue with room for at least four tasks.
//! They do not emit lifecycle events themselves: the pool owns those events
//! and must exercise its separate production lifecycle fixture.

use crate::contracts::{
    MailboxKey, Priority, ResourceCost, ResourceKind, ScheduledTask, TaskMetadata, TaskQueue,
};

fn task(id: u64, deadline_ms: Option<u128>, priority: Priority) -> ScheduledTask<String> {
    ScheduledTask {
        meta: TaskMetadata {
            id,
            mailbox: Some(MailboxKey {
                tenant: format!("tenant-{id}"),
                user_id: Some(format!("user-{id}")),
                session_id: Some(format!("session-{id}")),
            }),
            priority,
            cost: ResourceCost::new(ResourceKind::Cpu, 2),
            deadline_ms,
            created_at_ms: 7,
            idempotency_key: Some(format!("caller-{id}")),
            sequence: id,
        },
        payload: format!("payload-{id}"),
    }
}

fn same_task(actual: &ScheduledTask<String>, expected: &ScheduledTask<String>) {
    assert_eq!(actual.meta.id, expected.meta.id);
    assert_eq!(actual.meta.mailbox, expected.meta.mailbox);
    assert_eq!(actual.meta.priority, expected.meta.priority);
    assert_eq!(actual.meta.cost, expected.meta.cost);
    assert_eq!(actual.meta.deadline_ms, expected.meta.deadline_ms);
    assert_eq!(actual.meta.created_at_ms, expected.meta.created_at_ms);
    assert_eq!(actual.meta.idempotency_key, expected.meta.idempotency_key);
    assert_eq!(actual.meta.sequence, expected.meta.sequence);
    assert_eq!(actual.payload, expected.payload);
}

/// Pruning returns all and only removed tasks with their exact original metadata/payload, once.
///
/// Equality at a deadline is live, and surviving work keeps its normal priority
/// ordering and remains available to run.
pub fn prune_returns_all_removed_tasks<Q: TaskQueue<String>>(queue: &mut Q) {
    assert!(queue.is_empty(), "conformance requires an empty queue");
    let expired = task(1, Some(99), Priority::High);
    let at_deadline = task(2, Some(100), Priority::Low);
    let without_deadline = task(3, None, Priority::Normal);
    let future_head = task(4, Some(101), Priority::Critical);
    for task in [&expired, &at_deadline, &without_deadline, &future_head] {
        queue.enqueue(task.clone()).expect("enqueue fixture task");
    }
    let removed = queue.prune_expired(100).expect("prune past deadline");
    assert_eq!(removed.len(), 1, "prune lost removed-task metadata");
    same_task(&removed[0], &expired);
    assert_eq!(queue.len(), 3);
    assert!(queue.prune_expired(100).expect("repeat prune").is_empty());

    let removed = queue.prune_expired(101).expect("prune next deadline");
    assert_eq!(removed.len(), 1, "deadline equality must remain live");
    same_task(&removed[0], &at_deadline);
    assert_eq!(queue.len(), 2);
    same_task(
        &queue.dequeue().expect("dequeue").expect("live head"),
        &future_head,
    );
    same_task(
        &queue.dequeue().expect("dequeue").expect("no deadline"),
        &without_deadline,
    );
    assert!(queue.dequeue().expect("empty dequeue").is_none());
    assert!(
        queue
            .prune_expired(u128::MAX)
            .expect("empty prune")
            .is_empty()
    );
}
