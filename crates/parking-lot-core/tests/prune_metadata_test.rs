//! OA-10: removed tasks retain their metadata/payload through real queues.
#![cfg(not(parking_lot_loom))]

use std::collections::BTreeMap;

use prometheus_parking_lot_core::dispatch::{
    Dispatcher, ParkedQueue, PriorityQueue, TaskQueueAdapter,
};
use prometheus_parking_lot_core::semaphore::UnitSemaphore;
use prometheus_parking_lot_core::{
    InMemoryQueue, MailboxKey, Priority, ResourceCost, ResourceKind, ScheduledTask, SchedulerError,
    TaskMetadata, TaskQueue,
};

/// Intentionally has no Clone implementation.
#[derive(Debug, serde::Serialize)]
struct Payload(String);

fn task(id: u64, deadline: Option<u128>, priority: Priority) -> ScheduledTask<Payload> {
    ScheduledTask {
        meta: TaskMetadata {
            id,
            mailbox: Some(MailboxKey {
                tenant: format!("tenant-{id}"),
                user_id: Some(format!("user-{id}")),
                session_id: Some(format!("session-{id}")),
            }),
            priority,
            cost: ResourceCost::new(ResourceKind::Cpu, 1),
            deadline_ms: deadline,
            created_at_ms: 7,
            idempotency_key: Some(format!("original-{id}")),
            sequence: id,
        },
        payload: Payload(format!("payload-{id}")),
    }
}

fn exact_prune<Q: ParkedQueue<ScheduledTask<Payload>>>(mut queue: Q) {
    let tasks = [
        task(1, Some(99), Priority::High),
        task(2, Some(100), Priority::Normal),
        task(3, Some(99), Priority::Low),
        task(4, Some(101), Priority::Normal),
    ];
    let expected: BTreeMap<_, _> = tasks
        .iter()
        .map(|task| (task.meta.id, serde_json::to_value(task).expect("snapshot")))
        .collect();
    for task in tasks {
        queue.push(task).expect("park");
    }
    assert_eq!(queue.head().expect("head").expect("present").sequence, 1);
    let mut removed = queue.prune_expired(100).expect("prune");
    removed.sort_by_key(|task| task.meta.id);
    assert_eq!(
        removed.iter().map(|task| task.meta.id).collect::<Vec<_>>(),
        vec![1, 3]
    );
    for task in removed {
        assert_eq!(
            serde_json::to_value(&task).expect("removed"),
            expected[&task.meta.id]
        );
    }
    assert_eq!(queue.len(), 2);
    assert!(queue.prune_expired(100).expect("repeat").is_empty());
    let removed = queue.prune_expired(101).expect("next deadline");
    assert_eq!(removed.len(), 1);
    assert_eq!(
        serde_json::to_value(&removed[0]).expect("removed"),
        expected[&2]
    );
    assert_eq!(
        queue
            .head()
            .expect("head")
            .expect("equality is live")
            .sequence,
        4
    );
    let survivor = queue.pop_head().expect("survivor");
    assert_eq!(
        serde_json::to_value(survivor).expect("survivor"),
        expected[&4]
    );
    assert!(queue.prune_expired(u128::MAX).expect("empty").is_empty());
}

#[test]
fn prune_metadata_priority_queue_returns_original_tasks_without_clone() {
    exact_prune(PriorityQueue::new());
}

#[test]
fn prune_metadata_adapter_returns_front_and_backend_tasks_without_clone() {
    exact_prune(TaskQueueAdapter::new(InMemoryQueue::new(8)));
}

/// A legal failing collaborator: first prune returns an error without
/// removing any task; the next call uses the real in-memory backend.
struct FailOnce<P> {
    backend: InMemoryQueue<P>,
    fail: bool,
}

impl<P> TaskQueue<P> for FailOnce<P> {
    fn enqueue(&mut self, task: ScheduledTask<P>) -> Result<(), SchedulerError> {
        self.backend.enqueue(task)
    }

    fn dequeue(&mut self) -> Result<Option<ScheduledTask<P>>, SchedulerError> {
        self.backend.dequeue()
    }

    fn prune_expired(&mut self, now_ms: u128) -> Result<Vec<ScheduledTask<P>>, SchedulerError> {
        if std::mem::take(&mut self.fail) {
            return Err(SchedulerError::backend(
                "injected prune failure",
                std::fmt::Error,
            ));
        }
        self.backend.prune_expired(now_ms)
    }

    fn max_depth(&self) -> usize {
        self.backend.max_depth()
    }

    fn len(&self) -> usize {
        self.backend.len()
    }
}

#[test]
fn prune_metadata_backend_error_preserves_front_until_successful_retry() {
    let mut queue = TaskQueueAdapter::new(FailOnce {
        backend: InMemoryQueue::new(8),
        fail: true,
    });
    queue
        .push(task(1, Some(99), Priority::High))
        .expect("front task");
    queue
        .push(task(2, Some(99), Priority::Low))
        .expect("backend task");
    queue
        .push(task(3, None, Priority::Normal))
        .expect("survivor");
    // Pull task 1 into the adapter's front before backend pruning fails.
    assert_eq!(queue.head().expect("head").expect("task").sequence, 1);
    assert!(queue.prune_expired(100).is_err());
    assert_eq!(
        queue.len(),
        3,
        "error must not consume buffered task metadata"
    );
    assert_eq!(
        queue
            .head()
            .expect("head")
            .expect("front retained")
            .sequence,
        1
    );
    let mut removed = queue.prune_expired(100).expect("retry");
    removed.sort_by_key(|task| task.meta.id);
    assert_eq!(
        removed.iter().map(|task| task.meta.id).collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert!(queue.prune_expired(100).expect("repeat").is_empty());
    assert_eq!(queue.head().expect("head").expect("survivor").sequence, 3);
    assert_eq!(queue.pop_head().expect("survivor").meta.id, 3);
}

#[test]
fn prune_metadata_dispatcher_preserves_count_and_full_expired_items_once() {
    let dispatcher = Dispatcher::in_memory("oa-10", UnitSemaphore::new(1), 8);
    let mut running = dispatcher
        .submit(task(0, None, Priority::Normal), 99)
        .expect("blocker");
    let blocker = running.dispatched.started.pop().expect("started blocker");
    for task in [
        task(1, Some(100), Priority::Normal),
        task(2, Some(100), Priority::Low),
        task(3, None, Priority::High),
    ] {
        let admission = dispatcher.submit(task, 99).expect("park");
        assert!(!admission.started);
        assert!(admission.dispatched.is_empty());
    }
    let (count, batch) = dispatcher.prune_expired(100).expect("equality");
    assert_eq!(count, 0);
    assert!(batch.is_empty());
    let (count, mut batch) = dispatcher.prune_expired(101).expect("prune");
    assert_eq!(count, 2);
    assert!(batch.started.is_empty());
    assert!(batch.rejected.is_empty());
    batch.expired.sort_by_key(|task| task.meta.id);
    assert_eq!(batch.expired.len(), 2);
    for (actual, expected) in batch.expired.iter().zip([
        task(1, Some(100), Priority::Normal),
        task(2, Some(100), Priority::Low),
    ]) {
        // submit stamps sequences 2 and 3, while every other field is retained.
        let mut expected = expected;
        expected.meta.sequence = actual.meta.id + 1;
        assert_eq!(
            serde_json::to_value(actual).expect("actual"),
            serde_json::to_value(expected).expect("expected")
        );
    }
    let (count, batch) = dispatcher.prune_expired(101).expect("repeat");
    assert_eq!(count, 0);
    assert!(batch.is_empty());
    let mut next = dispatcher.complete(blocker.permit, 101);
    let survivor = next.started.pop().expect("live follower");
    assert_eq!(survivor.item.meta.id, 3);
    assert!(dispatcher.complete(survivor.permit, 101).is_empty());
    assert_eq!((dispatcher.units_in_use(), dispatcher.parked()), (0, 0));
}

#[cfg(feature = "conformance")]
#[test]
fn prune_metadata_in_memory_backend_passes_shared_conformance() {
    prometheus_parking_lot_core::conformance::task_queue::prune_returns_all_removed_tasks(
        &mut InMemoryQueue::new(8),
    );
}
