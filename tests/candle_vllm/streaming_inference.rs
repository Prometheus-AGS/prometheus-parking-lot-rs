//! Tests for streaming inference (non-serializable results)

use async_trait::async_trait;
use prometheus_parking_lot::config::WorkerPoolConfig;
use prometheus_parking_lot::core::{TaskMetadata, WorkerExecutor, WorkerPool};
use prometheus_parking_lot::util::{Priority, ResourceCost, ResourceKind};
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
struct StreamingExecutor;

struct StreamingResult {
    receiver: flume::Receiver<String>,
    total_tokens: usize,
}

#[async_trait]
impl WorkerExecutor<String, StreamingResult> for StreamingExecutor {
    async fn execute(&self, prompt: String, _meta: TaskMetadata) -> StreamingResult {
        let (tx, rx) = flume::unbounded();
        let total_tokens = 5;

        // Simulate token generation during execution
        for i in 0..total_tokens {
            tokio::time::sleep(Duration::from_millis(10)).await;
            let token = format!("{}:token_{}", prompt, i);
            let _ = tx.send(token);
        }
        drop(tx);

        StreamingResult { receiver: rx, total_tokens }
    }
}

#[tokio::test]
async fn test_streaming_inference() {
    println!("\n=== test_streaming_inference ===");
    
    let config = WorkerPoolConfig::new()
        .with_worker_count(2)
        .with_max_units(100)
        .with_max_queue_depth(10);
    
    let pool = WorkerPool::new(config, StreamingExecutor).expect("Failed to create pool");
    
    let meta = make_gpu_meta(1, 50);
    let key = pool.submit_async("hello".to_string(), meta).await.unwrap();
    
    let result = pool.retrieve_async(&key, Duration::from_secs(5)).await.unwrap();
    assert_eq!(result.total_tokens, 5);
    
    let mut tokens = Vec::new();
    while let Ok(token) = result.receiver.recv_async().await {
        tokens.push(token);
    }
    
    assert_eq!(tokens.len(), 5);
    assert!(tokens[0].contains("hello:token_0"));
    
    println!("Streaming inference verified: {} tokens received", tokens.len());
    println!("=== test_streaming_inference PASSED ===\n");
}
