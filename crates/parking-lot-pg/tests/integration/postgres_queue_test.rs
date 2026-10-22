//! Integration tests for PostgresQueue implementation.

#![cfg(feature = "postgres")]

use prometheus_parking_lot::core::{ScheduledTask, TaskMetadata, TaskQueue};
use prometheus_parking_lot::infra::queue::postgres::PostgresQueue;
use prometheus_parking_lot::util::clock::now_ms;
use prometheus_parking_lot::util::serde::{MailboxKey, Priority, ResourceCost, ResourceKind};
use testcontainers::clients::Cli;

use super::{PostgresTestFixture, TestPayload};

fn make_task(id: u64, priority: Priority) -> ScheduledTask<TestPayload> {
    ScheduledTask {
        meta: TaskMetadata {
            id,
            mailbox: Some(MailboxKey {
                tenant: "test-tenant".into(),
                user_id: Some(format!("user-{}", id)),
                session_id: None,
            }),
            priority,
            cost: ResourceCost::new(ResourceKind::Cpu, 1),
            deadline_ms: None,
            created_at_ms: now_ms(),
        },
        payload: TestPayload::new(id, format!("test-payload-{}", id)),
    }
}

#[tokio::test]
async fn test_postgres_queue_enqueue_dequeue() {
    let docker = Cli::default();
    let fixture = PostgresTestFixture::new(&docker)
        .await
        .expect("Failed to create test fixture");

    let mut queue = PostgresQueue::new(fixture.db.clone(), "test_queue".into(), 100);

    // Enqueue tasks
    let task1 = make_task(1, Priority::Normal);
    let task2 = make_task(2, Priority::Normal);

    queue.enqueue(task1.clone()).expect("Failed to enqueue task1");
    queue.enqueue(task2.clone()).expect("Failed to enqueue task2");

    assert_eq!(queue.len(), 2);

    // Dequeue tasks (should maintain FIFO within same priority)
    let dequeued1 = queue.dequeue().expect("Failed to dequeue").expect("Queue is empty");
    assert_eq!(dequeued1.meta.id, 1);
    assert_eq!(dequeued1.payload, task1.payload);

    let dequeued2 = queue.dequeue().expect("Failed to dequeue").expect("Queue is empty");
    assert_eq!(dequeued2.meta.id, 2);

    assert_eq!(queue.len(), 0);
    assert!(queue.dequeue().expect("Failed to dequeue").is_none());
}

#[tokio::test]
async fn test_postgres_queue_priority_ordering() {
    let docker = Cli::default();
    let fixture = PostgresTestFixture::new(&docker)
        .await
        .expect("Failed to create test fixture");

    let mut queue = PostgresQueue::new(fixture.db.clone(), "priority_queue".into(), 100);

    // Enqueue with different priorities
    queue.enqueue(make_task(1, Priority::Low)).expect("enqueue failed");
    queue.enqueue(make_task(2, Priority::Critical)).expect("enqueue failed");
    queue.enqueue(make_task(3, Priority::Normal)).expect("enqueue failed");
    queue.enqueue(make_task(4, Priority::High)).expect("enqueue failed");

    // Should dequeue in priority order: Critical > High > Normal > Low
    assert_eq!(queue.dequeue().unwrap().unwrap().meta.id, 2); // Critical
    assert_eq!(queue.dequeue().unwrap().unwrap().meta.id, 4); // High
    assert_eq!(queue.dequeue().unwrap().unwrap().meta.id, 3); // Normal
    assert_eq!(queue.dequeue().unwrap().unwrap().meta.id, 1); // Low
}

#[tokio::test]
async fn test_postgres_queue_full() {
    let docker = Cli::default();
    let fixture = PostgresTestFixture::new(&docker)
        .await
        .expect("Failed to create test fixture");

    let mut queue = PostgresQueue::new(fixture.db.clone(), "small_queue".into(), 2);

    queue.enqueue(make_task(1, Priority::Normal)).expect("enqueue failed");
    queue.enqueue(make_task(2, Priority::Normal)).expect("enqueue failed");

    // Third task should fail due to max depth
    let result = queue.enqueue(make_task(3, Priority::Normal));
    assert!(result.is_err());
    assert!(matches!(result, Err(prometheus_parking_lot::core::SchedulerError::QueueFull(_))));
}

#[tokio::test]
async fn test_postgres_queue_prune_expired() {
    let docker = Cli::default();
    let fixture = PostgresTestFixture::new(&docker)
        .await
        .expect("Failed to create test fixture");

    let mut queue = PostgresQueue::new(fixture.db.clone(), "expiry_queue".into(), 100);

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

    // Task with expired deadline
    let mut task4 = make_task(4, Priority::Normal);
    task4.meta.deadline_ms = Some(now - 500);
    queue.enqueue(task4).expect("enqueue failed");

    assert_eq!(queue.len(), 4);

    // Prune expired tasks
    let pruned = queue.prune_expired(now).expect("prune failed");
    assert_eq!(pruned, 2); // Tasks 2 and 4 should be removed

    assert_eq!(queue.len(), 2);

    // Remaining tasks should be 1 and 3
    let remaining_ids: Vec<u64> = vec![
        queue.dequeue().unwrap().unwrap().meta.id,
        queue.dequeue().unwrap().unwrap().meta.id,
    ];
    assert!(remaining_ids.contains(&1));
    assert!(remaining_ids.contains(&3));
}

#[tokio::test]
async fn test_postgres_queue_fifo_within_priority() {
    let docker = Cli::default();
    let fixture = PostgresTestFixture::new(&docker)
        .await
        .expect("Failed to create test fixture");

    let mut queue = PostgresQueue::new(fixture.db.clone(), "fifo_queue".into(), 100);

    // Enqueue multiple tasks with same priority at different times
    for i in 1..=5 {
        let task = make_task(i, Priority::High);
        queue.enqueue(task).expect("enqueue failed");
        // Small delay to ensure different timestamps
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    // Should dequeue in FIFO order (1, 2, 3, 4, 5)
    for expected_id in 1..=5 {
        let task = queue.dequeue().unwrap().unwrap();
        assert_eq!(task.meta.id, expected_id);
    }
}

#[tokio::test]
async fn test_postgres_queue_concurrent_dequeue() {
    let docker = Cli::default();
    let fixture = PostgresTestFixture::new(&docker)
        .await
        .expect("Failed to create test fixture");

    let db = fixture.db.clone();
    let mut queue = PostgresQueue::new(db.clone(), "concurrent_queue".into(), 100);

    // Enqueue 10 tasks
    for i in 1..=10 {
        queue.enqueue(make_task(i, Priority::Normal)).expect("enqueue failed");
    }

    // Spawn concurrent workers to dequeue
    let mut handles = vec![];
    for worker_id in 0..3 {
        let db_clone = db.clone();
        let handle = tokio::spawn(async move {
            let mut worker_queue = PostgresQueue::new(db_clone, "concurrent_queue".into(), 100);
            let mut dequeued = vec![];
            
            while let Some(task) = worker_queue.dequeue().expect("dequeue failed") {
                dequeued.push(task.meta.id);
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            
            (worker_id, dequeued)
        });
        handles.push(handle);
    }

    // Wait for all workers
    let results = futures::future::join_all(handles).await;

    // Collect all dequeued task IDs
    let mut all_ids = vec![];
    for result in results {
        let (_, ids) = result.expect("worker panicked");
        all_ids.extend(ids);
    }

    // Each task should be dequeued exactly once
    all_ids.sort();
    assert_eq!(all_ids.len(), 10);
    assert_eq!(all_ids, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
}

#[tokio::test]
async fn test_postgres_queue_persistence() {
    let docker = Cli::default();
    let fixture = PostgresTestFixture::new(&docker)
        .await
        .expect("Failed to create test fixture");

    let db = fixture.db.clone();

    // Enqueue tasks with first queue instance
    {
        let mut queue = PostgresQueue::new(db.clone(), "persist_queue".into(), 100);
        queue.enqueue(make_task(1, Priority::High)).expect("enqueue failed");
        queue.enqueue(make_task(2, Priority::Normal)).expect("enqueue failed");
        queue.enqueue(make_task(3, Priority::Critical)).expect("enqueue failed");
    }
    // Drop queue

    // Create new queue instance with same DB connection
    let mut queue2 = PostgresQueue::new(db.clone(), "persist_queue".into(), 100);

    // Tasks should still be there, in priority order
    assert_eq!(queue2.len(), 3);
    assert_eq!(queue2.dequeue().unwrap().unwrap().meta.id, 3); // Critical
    assert_eq!(queue2.dequeue().unwrap().unwrap().meta.id, 1); // High
    assert_eq!(queue2.dequeue().unwrap().unwrap().meta.id, 2); // Normal
}
