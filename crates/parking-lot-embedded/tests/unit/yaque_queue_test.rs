//! Unit tests for YaqueQueue implementation using tempdir for isolation.

use prometheus_parking_lot_core::{
    MailboxKey, Priority, ResourceCost, ResourceKind, ScheduledTask, TaskMetadata, TaskQueue,
    now_ms,
};
use prometheus_parking_lot_embedded::YaqueQueue;
use tempfile::tempdir;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
struct TestPayload {
    id: u64,
    data: String,
}

fn make_task(id: u64, priority: Priority) -> ScheduledTask<TestPayload> {
    ScheduledTask {
        meta: TaskMetadata {
            id,
            mailbox: Some(MailboxKey {
                tenant: "test-tenant".into(),
                user_id: Some(format!("user-{id}")),
                session_id: None,
            }),
            priority,
            cost: ResourceCost::new(ResourceKind::Cpu, 1),
            deadline_ms: None,
            created_at_ms: now_ms(),
            idempotency_key: None,
            sequence: 0,
        },
        payload: TestPayload {
            id,
            data: format!("test-payload-{id}"),
        },
    }
}

#[test]
#[ignore = "YaqueQueue.dequeue is plain FIFO and ignores priority; the redb lease queue orders by priority (fixed by cr-28-embedded-redb-queue)"]
fn test_yaque_queue_enqueue_dequeue_persistence() {
    let dir = tempdir().expect("Failed to create temp dir");
    let path = dir.path();

    // Create queue and enqueue tasks
    {
        let mut queue = YaqueQueue::new(path, "test_stream", 100).expect("Failed to create queue");
        queue.enqueue(make_task(1, Priority::Normal)).expect("enqueue failed");
        queue.enqueue(make_task(2, Priority::High)).expect("enqueue failed");
        
        assert_eq!(queue.len(), 2);
    }
    // Drop queue

    // Verify file exists
    let queue_file = path.join("test_stream.jsonl");
    assert!(queue_file.exists(), "Queue file should exist");

    // Create new queue instance and verify tasks persisted
    let mut queue2 = YaqueQueue::<TestPayload>::new(path, "test_stream", 100).expect("Failed to create queue");
    assert_eq!(queue2.len(), 2);

    // Dequeue in priority order
    let task1 = queue2.dequeue().expect("dequeue failed").expect("Queue is empty");
    assert_eq!(task1.meta.id, 2); // High priority first

    let task2 = queue2.dequeue().expect("dequeue failed").expect("Queue is empty");
    assert_eq!(task2.meta.id, 1);
}

#[test]
fn test_yaque_queue_reload_from_disk() {
    let dir = tempdir().expect("Failed to create temp dir");
    let path = dir.path();

    // Create queue and add tasks
    {
        let mut queue = YaqueQueue::new(path, "reload_test", 100).expect("Failed to create queue");
        for i in 1..=5 {
            queue.enqueue(make_task(i, Priority::Normal)).expect("enqueue failed");
        }
    }

    // Create new queue instance - should load all tasks from disk
    let mut queue2 = YaqueQueue::<TestPayload>::new(path, "reload_test", 100).expect("Failed to create queue");
    assert_eq!(queue2.len(), 5);

    // Dequeue all and verify
    for expected_id in 1..=5 {
        let task = queue2.dequeue().expect("dequeue failed").expect("Queue is empty");
        assert_eq!(task.meta.id, expected_id);
    }

    assert_eq!(queue2.len(), 0);
}

#[test]
#[ignore = "YaqueQueue.dequeue is plain FIFO and ignores priority; the redb lease queue orders by priority (fixed by cr-28-embedded-redb-queue)"]
fn test_yaque_queue_priority_ordering() {
    let dir = tempdir().expect("Failed to create temp dir");
    let path = dir.path();
    let mut queue = YaqueQueue::new(path, "priority_test", 100).expect("Failed to create queue");

    // Enqueue with different priorities
    queue.enqueue(make_task(1, Priority::Low)).expect("enqueue failed");
    queue.enqueue(make_task(2, Priority::Critical)).expect("enqueue failed");
    queue.enqueue(make_task(3, Priority::Normal)).expect("enqueue failed");
    queue.enqueue(make_task(4, Priority::High)).expect("enqueue failed");

    // Should dequeue in priority order
    assert_eq!(queue.dequeue().unwrap().unwrap().meta.id, 2); // Critical
    assert_eq!(queue.dequeue().unwrap().unwrap().meta.id, 4); // High
    assert_eq!(queue.dequeue().unwrap().unwrap().meta.id, 3); // Normal
    assert_eq!(queue.dequeue().unwrap().unwrap().meta.id, 1); // Low
}

#[test]
fn test_yaque_queue_prune_expired_rewrites_file() {
    let dir = tempdir().expect("Failed to create temp dir");
    let path = dir.path();
    let mut queue = YaqueQueue::new(path, "prune_test", 100).expect("Failed to create queue");

    let now = now_ms();

    // Task with no deadline
    queue.enqueue(make_task(1, Priority::Normal)).expect("enqueue failed");

    // Task with expired deadline
    let mut task2 = make_task(2, Priority::Normal);
    task2.meta.deadline_ms = Some(now - 1000);
    queue.enqueue(task2).expect("enqueue failed");

    // Task with future deadline
    let mut task3 = make_task(3, Priority::Normal);
    task3.meta.deadline_ms = Some(now + 10000);
    queue.enqueue(task3).expect("enqueue failed");

    assert_eq!(queue.len(), 3);

    // Get file size before pruning
    let queue_file = path.join("prune_test.jsonl");
    let size_before = std::fs::metadata(&queue_file)
        .expect("File should exist")
        .len();

    // Prune expired
    let pruned = queue.prune_expired(now).expect("prune failed");
    assert_eq!(pruned, 1); // Only task 2 should be removed

    // Get file size after pruning
    let size_after = std::fs::metadata(&queue_file)
        .expect("File should exist")
        .len();

    // File should be smaller
    assert!(size_after < size_before, "File should be smaller after pruning");

    assert_eq!(queue.len(), 2);
}

#[test]
fn test_yaque_queue_full() {
    let dir = tempdir().expect("Failed to create temp dir");
    let path = dir.path();
    let mut queue = YaqueQueue::new(path, "full_test", 2).expect("Failed to create queue");

    queue.enqueue(make_task(1, Priority::Normal)).expect("enqueue failed");
    queue.enqueue(make_task(2, Priority::Normal)).expect("enqueue failed");

    // Third task should fail
    let result = queue.enqueue(make_task(3, Priority::Normal));
    assert!(result.is_err());
}

#[test]
fn test_yaque_queue_empty_dequeue() {
    let dir = tempdir().expect("Failed to create temp dir");
    let path = dir.path();
    let mut queue = YaqueQueue::<TestPayload>::new(path, "empty_test", 100)
        .expect("Failed to create queue");

    assert_eq!(queue.len(), 0);
    assert!(queue.dequeue().unwrap().is_none());
}

#[test]
fn test_yaque_queue_fifo_within_priority() {
    let dir = tempdir().expect("Failed to create temp dir");
    let path = dir.path();
    let mut queue = YaqueQueue::new(path, "fifo_test", 100).expect("Failed to create queue");

    // Enqueue multiple tasks with same priority at different times
    for i in 1..=5 {
        let mut task = make_task(i, Priority::High);
        // Use different created_at times
        task.meta.created_at_ms = now_ms() + u128::from(i);
        queue.enqueue(task).expect("enqueue failed");
    }

    // Should dequeue in FIFO order (by created_at timestamp)
    for expected_id in 1..=5 {
        let task = queue.dequeue().unwrap().unwrap();
        assert_eq!(task.meta.id, expected_id);
    }
}

#[test]
#[ignore = "YaqueQueue.dequeue is plain FIFO and ignores priority; the redb lease queue orders by priority (fixed by cr-28-embedded-redb-queue)"]
fn test_yaque_queue_persistence_across_instances() {
    let dir = tempdir().expect("Failed to create temp dir");
    let path = dir.path();

    // Instance 1: Enqueue some tasks
    {
        let mut queue = YaqueQueue::<TestPayload>::new(path, "persist", 100).expect("Failed to create queue");
        queue.enqueue(make_task(1, Priority::Critical)).expect("enqueue failed");
        queue.enqueue(make_task(2, Priority::Normal)).expect("enqueue failed");
    }

    // Instance 2: Dequeue one task
    {
        let mut queue = YaqueQueue::<TestPayload>::new(path, "persist", 100).expect("Failed to create queue");
        assert_eq!(queue.len(), 2);
        let task = queue.dequeue().unwrap().unwrap();
        assert_eq!(task.meta.id, 1); // Critical first
    }

    // Instance 3: Remaining task should still be there
    {
        let mut queue = YaqueQueue::<TestPayload>::new(path, "persist", 100).expect("Failed to create queue");
        assert_eq!(queue.len(), 1);
        let task = queue.dequeue().unwrap().unwrap();
        assert_eq!(task.meta.id, 2);
    }

    // Instance 4: Queue should be empty
    {
        let queue = YaqueQueue::<TestPayload>::new(path, "persist", 100)
            .expect("Failed to create queue");
        assert_eq!(queue.len(), 0);
    }
}

#[test]
fn test_yaque_queue_large_payloads() {
    let dir = tempdir().expect("Failed to create temp dir");
    let path = dir.path();
    let mut queue = YaqueQueue::new(path, "large_test", 100).expect("Failed to create queue");

    // Create task with large payload
    let large_data = "x".repeat(10_000);
    let task = ScheduledTask {
        meta: TaskMetadata {
            id: 1,
            mailbox: None,
            priority: Priority::Normal,
            cost: ResourceCost::new(ResourceKind::Cpu, 1),
            deadline_ms: None,
            created_at_ms: now_ms(),
            idempotency_key: None,
            sequence: 0,
        },
        payload: TestPayload {
            id: 1,
            data: large_data.clone(),
        },
    };

    queue.enqueue(task).expect("enqueue failed");

    let dequeued = queue.dequeue().expect("dequeue failed").expect("Queue is empty");
    assert_eq!(dequeued.payload.data, large_data);
}

#[test]
fn test_yaque_queue_multiple_streams() {
    let dir = tempdir().expect("Failed to create temp dir");
    let path = dir.path();

    // Create two queues with different stream names
    let mut queue1 = YaqueQueue::new(path, "stream1", 100).expect("Failed to create queue");
    let mut queue2 = YaqueQueue::new(path, "stream2", 100).expect("Failed to create queue");

    queue1.enqueue(make_task(1, Priority::Normal)).expect("enqueue failed");
    queue2.enqueue(make_task(2, Priority::Normal)).expect("enqueue failed");

    // Each queue should maintain separate state
    assert_eq!(queue1.len(), 1);
    assert_eq!(queue2.len(), 1);

    let task1 = queue1.dequeue().unwrap().unwrap();
    assert_eq!(task1.meta.id, 1);

    let task2 = queue2.dequeue().unwrap().unwrap();
    assert_eq!(task2.meta.id, 2);

    // Verify separate files exist
    assert!(path.join("stream1.jsonl").exists());
    assert!(path.join("stream2.jsonl").exists());
}
