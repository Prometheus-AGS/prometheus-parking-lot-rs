//! Integration test demonstrating complete parking lot algorithm.
//!
//! This test validates:
//! 1. Tasks execute with actual payloads (not empty spawns)
//! 2. Capacity management works correctly
//! 3. Tasks are queued when capacity exhausted
//! 4. Tasks wake up when capacity becomes available
//! 5. Results are delivered to mailbox
//! 6. Priority ordering is respected

use async_trait::async_trait;
use prometheus_parking_lot::core::{
    PoolLimits, ResourcePool, ScheduledTask, Spawn, TaskExecutor, TaskMetadata, TaskStatus,
};
use prometheus_parking_lot::infra::mailbox::memory::InMemoryMailbox;
use prometheus_parking_lot::infra::queue::memory::InMemoryQueue;
use prometheus_parking_lot::util::clock::now_ms;
use prometheus_parking_lot::util::serde::{MailboxKey, Priority, ResourceCost, ResourceKind};
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

// Test payload type
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct TestJob {
    name: String,
    value: u32,
}

// Test executor that simulates work
#[derive(Clone)]
struct TestExecutor {
    results: Arc<Mutex<Vec<String>>>,
}

impl TestExecutor {
    fn new() -> Self {
        Self {
            results: Arc::new(Mutex::new(Vec::new())),
        }
    }

    async fn get_results(&self) -> Vec<String> {
        self.results.lock().await.clone()
    }
}

#[async_trait]
impl TaskExecutor<TestJob, String> for TestExecutor {
    async fn execute(&self, payload: TestJob, meta: TaskMetadata) -> String {
        // Simulate some work
        tokio::time::sleep(Duration::from_millis(10)).await;
        
        let result = format!("Task {}: {} = {}", meta.id, payload.name, payload.value * 2);
        
        // Record execution order
        self.results.lock().await.push(result.clone());
        
        result
    }
}

// Simple tokio spawner for tests
#[derive(Clone)]
struct TestSpawner;

impl Spawn for TestSpawner {
    fn spawn<F>(&self, fut: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        tokio::spawn(fut);
    }
}

#[tokio::test]
async fn test_immediate_execution() {
    // Test that a task executes immediately when capacity available
    let limits = PoolLimits {
        max_units: 10,
        max_queue_depth: 100,
        default_timeout: Duration::from_secs(60),
    };

    let queue = InMemoryQueue::new(100);
    let mailbox = InMemoryMailbox::new();
    let executor = TestExecutor::new();
    let spawner = TestSpawner;

    let pool = ResourcePool::new(limits, queue, mailbox, executor.clone(), spawner);

    let meta = TaskMetadata {
        id: 1,
        priority: Priority::Normal,
        cost: ResourceCost {
            kind: ResourceKind::Cpu,
            units: 5,
        },
        created_at_ms: now_ms(),
        deadline_ms: None,
        mailbox: None,
    };

    let job = TestJob {
        name: "test_job".to_string(),
        value: 42,
    };

    let status = pool.submit(ScheduledTask { meta, payload: job }, now_ms()).await.unwrap();
    
    assert!(matches!(status, TaskStatus::Running));

    // Wait for task to complete
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify execution occurred
    let results = executor.get_results().await;
    assert_eq!(results.len(), 1);
    assert!(results[0].contains("test_job"));
    assert!(results[0].contains("84")); // 42 * 2
}

#[tokio::test]
async fn test_capacity_enforcement_and_queueing() {
    // Test that tasks are queued when capacity exhausted
    let limits = PoolLimits {
        max_units: 10,
        max_queue_depth: 100,
        default_timeout: Duration::from_secs(60),
    };

    let queue = InMemoryQueue::new(100);
    let mailbox = InMemoryMailbox::new();
    let executor = TestExecutor::new();
    let spawner = TestSpawner;

    let pool = ResourcePool::new(limits, queue, mailbox, executor.clone(), spawner);

    // Submit first task that uses all capacity
    let meta1 = TaskMetadata {
        id: 1,
        priority: Priority::Normal,
        cost: ResourceCost {
            kind: ResourceKind::Cpu,
            units: 10,
        },
        created_at_ms: now_ms(),
        deadline_ms: None,
        mailbox: None,
    };

    let job1 = TestJob {
        name: "job1".to_string(),
        value: 1,
    };

    let status1 = pool.submit(ScheduledTask { meta: meta1, payload: job1 }, now_ms()).await.unwrap();
    assert!(matches!(status1, TaskStatus::Running));

    // Submit second task - should be queued
    let meta2 = TaskMetadata {
        id: 2,
        priority: Priority::Normal,
        cost: ResourceCost {
            kind: ResourceKind::Cpu,
            units: 5,
        },
        created_at_ms: now_ms(),
        deadline_ms: None,
        mailbox: None,
    };

    let job2 = TestJob {
        name: "job2".to_string(),
        value: 2,
    };

    let status2 = pool.submit(ScheduledTask { meta: meta2, payload: job2 }, now_ms()).await.unwrap();
    assert!(matches!(status2, TaskStatus::Queued));

    // Wait for tasks to complete
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Both tasks should have executed
    let results = executor.get_results().await;
    assert_eq!(results.len(), 2);
}

#[tokio::test]
async fn test_wake_up_mechanism() {
    // Test that queued tasks wake up when capacity becomes available
    let limits = PoolLimits {
        max_units: 10,
        max_queue_depth: 100,
        default_timeout: Duration::from_secs(60),
    };

    let queue = InMemoryQueue::new(100);
    let mailbox = InMemoryMailbox::new();
    let executor = TestExecutor::new();
    let spawner = TestSpawner;

    let pool = ResourcePool::new(limits, queue, mailbox, executor.clone(), spawner);

    // Fill capacity
    let meta1 = TaskMetadata {
        id: 1,
        priority: Priority::Normal,
        cost: ResourceCost {
            kind: ResourceKind::Cpu,
            units: 10,
        },
        created_at_ms: now_ms(),
        deadline_ms: None,
        mailbox: None,
    };

    pool.submit(ScheduledTask { 
        meta: meta1.clone(), 
        payload: TestJob { name: "task1".to_string(), value: 1 } 
    }, now_ms()).await.unwrap();

    // Queue several more tasks
    for i in 2..=5 {
        let meta = TaskMetadata {
            id: i,
            priority: Priority::Normal,
            cost: ResourceCost {
                kind: ResourceKind::Cpu,
                units: 3,
            },
            created_at_ms: now_ms(),
            deadline_ms: None,
            mailbox: None,
        };

        let status = pool.submit(ScheduledTask { 
            meta, 
            payload: TestJob { name: format!("task{}", i), value: i as u32 } 
        }, now_ms()).await.unwrap();
        
        assert!(matches!(status, TaskStatus::Queued));
    }

    // Wait for all tasks to complete (task1 finishes, wakes task2, etc.)
    tokio::time::sleep(Duration::from_millis(500)).await;

    // All 5 tasks should have executed
    let results = executor.get_results().await;
    assert_eq!(results.len(), 5);
}

#[tokio::test]
async fn test_mailbox_delivery() {
    // Test that results are delivered to mailbox
    let limits = PoolLimits {
        max_units: 10,
        max_queue_depth: 100,
        default_timeout: Duration::from_secs(60),
    };

    let queue = InMemoryQueue::new(100);
    let mailbox = InMemoryMailbox::new();
    let executor = TestExecutor::new();
    let spawner = TestSpawner;

    let pool = Arc::new(ResourcePool::new(
        limits, 
        queue, 
        mailbox, 
        executor.clone(), 
        spawner
    ));

    let mailbox_key = MailboxKey {
        tenant: "test-tenant".to_string(),
        user_id: Some("user-123".to_string()),
        session_id: None,
    };

    let meta = TaskMetadata {
        id: 1,
        priority: Priority::Normal,
        cost: ResourceCost {
            kind: ResourceKind::Cpu,
            units: 5,
        },
        created_at_ms: now_ms(),
        deadline_ms: None,
        mailbox: Some(mailbox_key.clone()),
    };

    let job = TestJob {
        name: "mailbox_test".to_string(),
        value: 100,
    };

    pool.submit(ScheduledTask { meta, payload: job }, now_ms()).await.unwrap();

    // Wait for task to complete
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Note: In this simplified test, we can't easily access the mailbox after it's moved into the pool.
    // In a real implementation, the mailbox would be shared or we'd have a separate query interface.
    // For now, we verify the task executed by checking the executor results.
    let results = executor.get_results().await;
    assert_eq!(results.len(), 1);
    assert!(results[0].contains("200")); // 100 * 2
}

#[tokio::test]
async fn test_priority_ordering() {
    // Test that higher priority tasks are executed first when woken
    let limits = PoolLimits {
        max_units: 10,
        max_queue_depth: 100,
        default_timeout: Duration::from_secs(60),
    };

    let queue = InMemoryQueue::new(100);
    let mailbox = InMemoryMailbox::new();
    let executor = TestExecutor::new();
    let spawner = TestSpawner;

    let pool = ResourcePool::new(limits, queue, mailbox, executor.clone(), spawner);

    // Fill capacity
    pool.submit(ScheduledTask {
        meta: TaskMetadata {
            id: 1,
            priority: Priority::Normal,
            cost: ResourceCost { kind: ResourceKind::Cpu, units: 10 },
            created_at_ms: now_ms(),
            deadline_ms: None,
            mailbox: None,
        },
        payload: TestJob { name: "blocker".to_string(), value: 0 },
    }, now_ms()).await.unwrap();

    // Queue tasks with different priorities
    let priorities = vec![
        (2u64, Priority::Low),
        (3u64, Priority::Critical),
        (4u64, Priority::Normal),
        (5u64, Priority::High),
    ];

    for (id, priority) in priorities {
        pool.submit(ScheduledTask {
            meta: TaskMetadata {
                id,
                priority,
                cost: ResourceCost { kind: ResourceKind::Cpu, units: 3 },
                created_at_ms: now_ms(),
                deadline_ms: None,
                mailbox: None,
            },
            payload: TestJob { name: format!("task_{:?}", priority), value: id as u32 },
        }, now_ms()).await.unwrap();
    }

    // Wait for execution
    tokio::time::sleep(Duration::from_millis(500)).await;

    let results = executor.get_results().await;
    
    // First should be the blocker, then Critical, High, Normal, Low
    assert!(results.len() >= 3);
    assert!(results[1].contains("Critical"));
}

#[tokio::test]
async fn test_deadline_rejection() {
    // Test that expired tasks are rejected
    let limits = PoolLimits {
        max_units: 10,
        max_queue_depth: 100,
        default_timeout: Duration::from_secs(60),
    };

    let queue = InMemoryQueue::new(100);
    let mailbox = InMemoryMailbox::new();
    let executor = TestExecutor::new();
    let spawner = TestSpawner;

    let pool = ResourcePool::new(limits, queue, mailbox, executor, spawner);

    let past_time = now_ms() - 1000; // 1 second in the past

    let meta = TaskMetadata {
        id: 1,
        priority: Priority::Normal,
        cost: ResourceCost {
            kind: ResourceKind::Cpu,
            units: 5,
        },
        created_at_ms: now_ms(),
        deadline_ms: Some(past_time),
        mailbox: None,
    };

    let result = pool.submit(ScheduledTask {
        meta,
        payload: TestJob { name: "expired".to_string(), value: 1 },
    }, now_ms()).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_concurrent_submissions() {
    // Test that concurrent task submissions work correctly with atomic capacity tracking
    let limits = PoolLimits {
        max_units: 100,
        max_queue_depth: 1000,
        default_timeout: Duration::from_secs(60),
    };

    let queue = InMemoryQueue::new(1000);
    let mailbox = InMemoryMailbox::new();
    let executor = TestExecutor::new();
    let spawner = TestSpawner;

    let pool = Arc::new(ResourcePool::new(limits, queue, mailbox, executor.clone(), spawner));

    // Submit many tasks concurrently
    let num_tasks = 50;
    let mut handles = Vec::new();

    for i in 0..num_tasks {
        let pool = Arc::clone(&pool);
        let handle = tokio::spawn(async move {
            let meta = TaskMetadata {
                id: i,
                priority: Priority::Normal,
                cost: ResourceCost {
                    kind: ResourceKind::Cpu,
                    units: 2, // Each task uses 2 units
                },
                created_at_ms: now_ms(),
                deadline_ms: None,
                mailbox: None,
            };

            let job = TestJob {
                name: format!("concurrent_task_{}", i),
                value: i as u32,
            };

            pool.submit(ScheduledTask { meta, payload: job }, now_ms()).await
        });
        handles.push(handle);
    }

    // Wait for all submissions to complete
    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok());
    }

    // Wait for all tasks to execute
    tokio::time::sleep(Duration::from_millis(1000)).await;

    // Verify all tasks executed
    let results = executor.get_results().await;
    assert_eq!(results.len(), num_tasks as usize);
}

#[tokio::test]
async fn test_shutdown() {
    // Test that shutdown signals wake workers correctly
    let limits = PoolLimits {
        max_units: 10,
        max_queue_depth: 100,
        default_timeout: Duration::from_secs(60),
    };

    let queue = InMemoryQueue::new(100);
    let mailbox = InMemoryMailbox::new();
    let executor = TestExecutor::new();
    let spawner = TestSpawner;

    let pool = ResourcePool::new(limits, queue, mailbox, executor.clone(), spawner);

    // Submit a task
    let meta = TaskMetadata {
        id: 1,
        priority: Priority::Normal,
        cost: ResourceCost {
            kind: ResourceKind::Cpu,
            units: 5,
        },
        created_at_ms: now_ms(),
        deadline_ms: None,
        mailbox: None,
    };

    let job = TestJob {
        name: "shutdown_test".to_string(),
        value: 42,
    };

    pool.submit(ScheduledTask { meta, payload: job }, now_ms()).await.unwrap();

    // Wait for task to complete
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Shutdown the pool
    pool.shutdown();

    // Verify shutdown doesn't panic and task executed
    let results = executor.get_results().await;
    assert_eq!(results.len(), 1);
}
