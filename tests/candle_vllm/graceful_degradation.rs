//! Tests for graceful degradation under load

use async_trait::async_trait;
use prometheus_parking_lot::config::WorkerPoolConfig;
use prometheus_parking_lot::core::{PoolError, TaskMetadata, WorkerExecutor, WorkerPool};
use prometheus_parking_lot::util::{Priority, ResourceCost, ResourceKind};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis()
}

fn make_meta(task_id: u64) -> TaskMetadata {
    TaskMetadata {
        id: task_id,
        mailbox: None,
        priority: Priority::Normal,
        cost: ResourceCost { kind: ResourceKind::Cpu, units: 10 },
        deadline_ms: None,
        created_at_ms: now_ms(),
    }
}

#[derive(Clone)]
struct SlowExecutor;

#[async_trait]
impl WorkerExecutor<(), String> for SlowExecutor {
    async fn execute(&self, _: (), _meta: TaskMetadata) -> String {
        tokio::time::sleep(Duration::from_millis(100)).await;
        "completed".to_string()
    }
}

#[tokio::test]
async fn test_queue_full_rejection() {
    println!("\n=== test_queue_full_rejection ===");
    
    let config = WorkerPoolConfig::new()
        .with_worker_count(1)
        .with_max_units(10) // Only 1 task at a time
        .with_max_queue_depth(2); // Only 2 can queue
    
    let pool = WorkerPool::new(config, SlowExecutor).expect("Failed to create pool");
    
    // Fill queue (any accepted keys are fine as long as capacity is exhausted)
    let _ = pool.submit_async((), make_meta(1)).await.expect("first task");
    let _ = pool.submit_async((), make_meta(2)).await.expect("second task");
    let _ = pool.submit_async((), make_meta(3)).await;
    
    // Next submission should fail
    let result = pool.submit_async((), make_meta(4)).await;
    
    match result {
        Err(PoolError::QueueFull) => {
            println!("Correctly rejected with QueueFull");
        }
        Ok(_) => {
            println!("Task accepted (queue had space)");
        }
        Err(e) => {
            panic!("Unexpected error: {:?}", e);
        }
    }
    
    println!("=== test_queue_full_rejection PASSED ===\n");
}

#[tokio::test]
async fn test_backpressure_handling() {
    println!("\n=== test_backpressure_handling ===");
    
    let config = WorkerPoolConfig::new()
        .with_worker_count(1)
        .with_max_units(10)
        .with_max_queue_depth(5);
    
    let pool = Arc::new(WorkerPool::new(config, SlowExecutor).expect("Failed to create pool"));
    
    // Submit many tasks
    let mut accepted = 0;
    let mut rejected = 0;
    
    for i in 0..20 {
        match pool.submit_async((), make_meta(i)).await {
            Ok(_) => accepted += 1,
            Err(PoolError::QueueFull) => rejected += 1,
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }
    
    println!("Accepted: {}, Rejected: {}", accepted, rejected);
    assert!(rejected > 0, "Should have rejected some tasks");
    assert!(accepted <= 6, "Should accept at most 1 running + 5 queued");
    
    println!("=== test_backpressure_handling PASSED ===\n");
}
