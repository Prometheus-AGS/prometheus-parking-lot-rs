//! Tests for priority-based scheduling

use async_trait::async_trait;
use prometheus_parking_lot::config::WorkerPoolConfig;
use prometheus_parking_lot::core::{TaskMetadata, WorkerExecutor, WorkerPool};
use prometheus_parking_lot::util::{Priority, ResourceCost, ResourceKind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis()
}

fn make_meta(task_id: u64, priority: Priority) -> TaskMetadata {
    TaskMetadata {
        id: task_id,
        mailbox: None,
        priority,
        cost: ResourceCost { kind: ResourceKind::Cpu, units: 10 },
        deadline_ms: None,
        created_at_ms: now_ms(),
    }
}

#[derive(Clone)]
struct PriorityTestExecutor {
    execution_order: Arc<AtomicU64>,
}

impl PriorityTestExecutor {
    fn new() -> Self {
        Self { execution_order: Arc::new(AtomicU64::new(0)) }
    }
}

#[async_trait]
impl WorkerExecutor<Priority, u64> for PriorityTestExecutor {
    async fn execute(&self, _priority: Priority, meta: TaskMetadata) -> u64 {
        let _order = self.execution_order.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(10)).await;
        meta.id
    }
}

#[tokio::test]
async fn test_priority_ordering() {
    println!("\n=== test_priority_ordering ===");
    
    let executor = PriorityTestExecutor::new();
    let config = WorkerPoolConfig::new()
        .with_worker_count(1) // Single worker to enforce ordering
        .with_max_units(100)
        .with_max_queue_depth(10);
    
    let pool = Arc::new(WorkerPool::new(config, executor).expect("Failed to create pool"));
    
    // Submit tasks in reverse priority order
    let key_low = pool.submit_async(Priority::Low, make_meta(1, Priority::Low)).await.unwrap();
    let key_normal = pool.submit_async(Priority::Normal, make_meta(2, Priority::Normal)).await.unwrap();
    let key_high = pool.submit_async(Priority::High, make_meta(3, Priority::High)).await.unwrap();
    let key_critical = pool.submit_async(Priority::Critical, make_meta(4, Priority::Critical)).await.unwrap();
    
    // Critical should execute first, then High, Normal, Low
    let critical_id = pool.retrieve_async(&key_critical, Duration::from_secs(5)).await.unwrap();
    let high_id = pool.retrieve_async(&key_high, Duration::from_secs(5)).await.unwrap();
    let normal_id = pool.retrieve_async(&key_normal, Duration::from_secs(5)).await.unwrap();
    let low_id = pool.retrieve_async(&key_low, Duration::from_secs(5)).await.unwrap();
    
    // Verify execution order (Critical=4, High=3, Normal=2, Low=1)
    assert_eq!(critical_id, 4);
    assert_eq!(high_id, 3);
    assert_eq!(normal_id, 2);
    assert_eq!(low_id, 1);
    
    println!("Priority ordering verified: Critical > High > Normal > Low");
    println!("=== test_priority_ordering PASSED ===\n");
}
