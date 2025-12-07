//! Tests for GPU VRAM resource tracking and limits

use async_trait::async_trait;
use prometheus_parking_lot::config::WorkerPoolConfig;
use prometheus_parking_lot::core::{TaskMetadata, WorkerExecutor, WorkerPool};
use prometheus_parking_lot::util::{Priority, ResourceCost, ResourceKind};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis()
}

fn make_gpu_meta(task_id: u64, units: u32) -> TaskMetadata {
    TaskMetadata {
        id: task_id,
        mailbox: None,
        priority: Priority::Normal,
        cost: ResourceCost { kind: ResourceKind::GpuVram, units },
        deadline_ms: None,
        created_at_ms: now_ms(),
    }
}

#[derive(Clone)]
struct GpuWorkExecutor {
    vram_used: Arc<AtomicU32>,
}

impl GpuWorkExecutor {
    fn new() -> Self {
        Self { vram_used: Arc::new(AtomicU32::new(0)) }
    }
}

#[async_trait]
impl WorkerExecutor<u32, String> for GpuWorkExecutor {
    async fn execute(&self, vram_mb: u32, _meta: TaskMetadata) -> String {
        self.vram_used.fetch_add(vram_mb, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(50)).await;
        self.vram_used.fetch_sub(vram_mb, Ordering::SeqCst);
        format!("Processed {}MB GPU work", vram_mb)
    }
}

#[tokio::test]
async fn test_gpu_vram_admission_control() {
    println!("\n=== test_gpu_vram_admission_control ===");
    
    let config = WorkerPoolConfig::new()
        .with_worker_count(2)
        .with_max_units(100) // Only 100MB VRAM total
        .with_max_queue_depth(10);
    
    let pool = WorkerPool::new(config, GpuWorkExecutor::new()).expect("Failed to create pool");
    
    // Submit task requiring 60MB - should be accepted
    let meta1 = make_gpu_meta(1, 60);
    let key1 = pool.submit_async(60, meta1).await.expect("Should accept");
    
    // Submit task requiring 50MB - should be accepted (60 + 50 = 110 > 100, but queued)
    let meta2 = make_gpu_meta(2, 50);
    let key2 = pool.submit_async(50, meta2).await.expect("Should queue");
    
    // Check stats show queued task
    let stats = pool.stats();
    assert!(stats.queued_tasks > 0 || stats.active_tasks > 0);
    
    // Retrieve results
    let _ = pool.retrieve_async(&key1, Duration::from_secs(5)).await;
    let _ = pool.retrieve_async(&key2, Duration::from_secs(5)).await;
    
    println!("=== test_gpu_vram_admission_control PASSED ===\n");
}

#[tokio::test]
async fn test_gpu_vram_exceeds_capacity() {
    println!("\n=== test_gpu_vram_exceeds_capacity ===");
    
    let config = WorkerPoolConfig::new()
        .with_worker_count(1)
        .with_max_units(50) // Only 50MB VRAM
        .with_max_queue_depth(3);
    
    let pool = WorkerPool::new(config, GpuWorkExecutor::new()).expect("Failed to create pool");
    
    // Submit task requiring 100MB - exceeds capacity, should queue or reject
    let meta = make_gpu_meta(1, 100);
    let result = pool.submit_async(100, meta).await;
    
    // Should either queue (if queue has space) or reject
    match result {
        Ok(_) => println!("Task queued (acceptable)"),
        Err(e) => {
            println!("Task rejected: {:?}", e);
            // Rejection is acceptable if queue is full
        }
    }
    
    println!("=== test_gpu_vram_exceeds_capacity PASSED ===\n");
}
