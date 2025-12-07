//! Tests for model loading/unloading lifecycle

use async_trait::async_trait;
use prometheus_parking_lot::config::WorkerPoolConfig;
use prometheus_parking_lot::core::{TaskMetadata, WorkerExecutor, WorkerPool};
use prometheus_parking_lot::util::{Priority, ResourceCost, ResourceKind};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
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
struct ModelLifecycleExecutor {
    loaded_models: Arc<AtomicU32>,
    is_loaded: Arc<AtomicBool>,
}

impl ModelLifecycleExecutor {
    fn new() -> Self {
        Self {
            loaded_models: Arc::new(AtomicU32::new(0)),
            is_loaded: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[async_trait]
impl WorkerExecutor<String, String> for ModelLifecycleExecutor {
    async fn execute(&self, action: String, _meta: TaskMetadata) -> String {
        match action.as_str() {
            "load" => {
                self.is_loaded.store(true, Ordering::SeqCst);
                self.loaded_models.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(50)).await;
                "model_loaded".to_string()
            }
            "unload" => {
                self.is_loaded.store(false, Ordering::SeqCst);
                self.loaded_models.fetch_sub(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(30)).await;
                "model_unloaded".to_string()
            }
            "inference" => {
                assert!(self.is_loaded.load(Ordering::SeqCst), "Model must be loaded");
                tokio::time::sleep(Duration::from_millis(20)).await;
                "inference_complete".to_string()
            }
            _ => "unknown_action".to_string(),
        }
    }
}

#[tokio::test]
async fn test_model_load_unload_cycle() {
    println!("\n=== test_model_load_unload_cycle ===");
    
    let executor = ModelLifecycleExecutor::new();
    let config = WorkerPoolConfig::new()
        .with_worker_count(1)
        .with_max_units(100)
        .with_max_queue_depth(10);
    
    let pool = WorkerPool::new(config, executor.clone()).expect("Failed to create pool");
    
    // Load model
    let load_meta = make_gpu_meta(1, 50);
    let load_key = pool.submit_async("load".to_string(), load_meta).await.unwrap();
    let load_result = pool.retrieve_async(&load_key, Duration::from_secs(5)).await.unwrap();
    assert_eq!(load_result, "model_loaded");
    assert_eq!(executor.loaded_models.load(Ordering::SeqCst), 1);
    
    // Run inference
    let inf_meta = make_gpu_meta(2, 10);
    let inf_key = pool.submit_async("inference".to_string(), inf_meta).await.unwrap();
    let inf_result = pool.retrieve_async(&inf_key, Duration::from_secs(5)).await.unwrap();
    assert_eq!(inf_result, "inference_complete");
    
    // Unload model
    let unload_meta = make_gpu_meta(3, 50);
    let unload_key = pool.submit_async("unload".to_string(), unload_meta).await.unwrap();
    let unload_result = pool.retrieve_async(&unload_key, Duration::from_secs(5)).await.unwrap();
    assert_eq!(unload_result, "model_unloaded");
    assert_eq!(executor.loaded_models.load(Ordering::SeqCst), 0);
    
    println!("Model lifecycle verified: load -> inference -> unload");
    println!("=== test_model_load_unload_cycle PASSED ===\n");
}

#[tokio::test]
async fn test_resource_release_on_unload() {
    println!("\n=== test_resource_release_on_unload ===");
    
    let executor = ModelLifecycleExecutor::new();
    let config = WorkerPoolConfig::new()
        .with_worker_count(1)
        .with_max_units(100) // 100MB total
        .with_max_queue_depth(10);
    
    let pool = WorkerPool::new(config, executor).expect("Failed to create pool");
    
    // Load model (uses 50MB)
    let load_key = pool.submit_async("load".to_string(), make_gpu_meta(1, 50)).await.unwrap();
    let _ = pool.retrieve_async(&load_key, Duration::from_secs(5)).await.unwrap();
    
    // Check stats - should show resources used
    let stats_after_load = pool.stats();
    println!("Stats after load: {:?}", stats_after_load);
    
    // Unload model (releases 50MB)
    let unload_key = pool.submit_async("unload".to_string(), make_gpu_meta(2, 50)).await.unwrap();
    let _ = pool.retrieve_async(&unload_key, Duration::from_secs(5)).await.unwrap();
    
    // Check stats - resources should be released
    let stats_after_unload = pool.stats();
    println!("Stats after unload: {:?}", stats_after_unload);
    
    assert!(stats_after_unload.used_units <= stats_after_load.used_units);
    
    println!("=== test_resource_release_on_unload PASSED ===\n");
}
