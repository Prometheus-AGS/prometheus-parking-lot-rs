//! Comprehensive integration tests for WorkerPool
//!
//! These tests validate real-world functionality including:
//! - Basic task execution with real executors
//! - Blocking and async APIs
//! - Concurrent task submission
//! - Resource limits and queueing
//! - Non-serializable streaming results (candle-vllm pattern)
//! - Timeout handling
//! - Graceful shutdown

use async_trait::async_trait;
use prometheus_parking_lot::config::WorkerPoolConfig;
use prometheus_parking_lot::core::{PoolError, TaskMetadata, WorkerExecutor, WorkerPool};
use prometheus_parking_lot::util::{Priority, ResourceCost, ResourceKind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
}

fn make_meta(task_id: u64, units: u32) -> TaskMetadata {
    TaskMetadata {
        id: task_id,
        mailbox: None,
        priority: Priority::Normal,
        cost: ResourceCost {
            kind: ResourceKind::Cpu,
            units,
        },
        deadline_ms: None,
        created_at_ms: now_ms(),
    }
}

fn make_gpu_meta(task_id: u64, units: u32) -> TaskMetadata {
    TaskMetadata {
        id: task_id,
        mailbox: None,
        priority: Priority::Normal,
        cost: ResourceCost {
            kind: ResourceKind::GpuVram,
            units,
        },
        deadline_ms: None,
        created_at_ms: now_ms(),
    }
}

// ============================================================================
// TEST EXECUTORS - Real implementations for testing
// ============================================================================

/// Simple executor that adds two numbers
#[derive(Clone)]
struct AddExecutor;

#[async_trait]
impl WorkerExecutor<(i32, i32), i32> for AddExecutor {
    async fn execute(&self, payload: (i32, i32), _meta: TaskMetadata) -> i32 {
        // Simulate some work
        tokio::time::sleep(Duration::from_millis(10)).await;
        payload.0 + payload.1
    }
}

/// Executor that simulates CPU-bound work with configurable duration
#[derive(Clone)]
struct CpuWorkExecutor {
    work_duration_ms: u64,
}

impl CpuWorkExecutor {
    fn new(work_duration_ms: u64) -> Self {
        Self { work_duration_ms }
    }
}

#[async_trait]
impl WorkerExecutor<String, String> for CpuWorkExecutor {
    async fn execute(&self, payload: String, meta: TaskMetadata) -> String {
        let start = Instant::now();
        // Simulate CPU work (not just sleeping)
        let mut result = payload.clone();
        let iterations = self.work_duration_ms * 1000;
        for i in 0..iterations {
            if i % 10000 == 0 {
                result = format!("{}_{}", result, i);
            }
        }
        let elapsed = start.elapsed();
        format!(
            "Processed '{}' in {:?}, task_id={}",
            payload, elapsed, meta.id
        )
    }
}

/// Executor that tracks execution count for concurrency testing
#[derive(Clone)]
struct CountingExecutor {
    execution_count: Arc<AtomicU64>,
    concurrent_count: Arc<AtomicU64>,
    max_concurrent: Arc<AtomicU64>,
}

impl CountingExecutor {
    fn new() -> Self {
        Self {
            execution_count: Arc::new(AtomicU64::new(0)),
            concurrent_count: Arc::new(AtomicU64::new(0)),
            max_concurrent: Arc::new(AtomicU64::new(0)),
        }
    }

    fn execution_count(&self) -> u64 {
        self.execution_count.load(Ordering::SeqCst)
    }

    fn max_concurrent(&self) -> u64 {
        self.max_concurrent.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl WorkerExecutor<u64, u64> for CountingExecutor {
    async fn execute(&self, payload: u64, _meta: TaskMetadata) -> u64 {
        // Track concurrent executions
        let current = self.concurrent_count.fetch_add(1, Ordering::SeqCst) + 1;

        // Update max concurrent seen
        let mut max = self.max_concurrent.load(Ordering::SeqCst);
        while current > max {
            match self.max_concurrent.compare_exchange_weak(
                max,
                current,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => break,
                Err(m) => max = m,
            }
        }

        // Do some work
        tokio::time::sleep(Duration::from_millis(50)).await;

        self.concurrent_count.fetch_sub(1, Ordering::SeqCst);
        self.execution_count.fetch_add(1, Ordering::SeqCst);

        payload * 2
    }
}

/// Executor that returns a streaming channel (NON-SERIALIZABLE - key candle-vllm pattern)
#[derive(Clone)]
struct StreamingExecutor;

/// Non-serializable result containing a channel receiver
struct StreamingResult {
    receiver: flume::Receiver<String>,
    total_tokens: usize,
}

#[async_trait]
impl WorkerExecutor<String, StreamingResult> for StreamingExecutor {
    async fn execute(&self, prompt: String, _meta: TaskMetadata) -> StreamingResult {
        let (tx, rx) = flume::unbounded();
        let total_tokens = 5;

        // Simulate LLM token generation during execution
        // Each token takes some time to generate
        for i in 0..total_tokens {
            // Simulate token generation time
            tokio::time::sleep(Duration::from_millis(10)).await;
            let token = format!("{}:token_{}", prompt, i);
            let _ = tx.send(token);
        }
        // Drop sender to signal stream completion
        drop(tx);

        StreamingResult {
            receiver: rx,
            total_tokens,
        }
    }
}

/// Executor that can timeout
#[derive(Clone)]
struct SlowExecutor {
    delay_ms: u64,
}

impl SlowExecutor {
    fn new(delay_ms: u64) -> Self {
        Self { delay_ms }
    }
}

#[async_trait]
impl WorkerExecutor<(), String> for SlowExecutor {
    async fn execute(&self, _payload: (), _meta: TaskMetadata) -> String {
        tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
        "completed".to_string()
    }
}

// ============================================================================
// TESTS
// ============================================================================

/// Test basic task submission and retrieval with async API
#[tokio::test]
async fn test_basic_async_api() {
    println!("\n=== test_basic_async_api ===");

    let config = WorkerPoolConfig::new()
        .with_worker_count(2)
        .with_max_units(100)
        .with_max_queue_depth(10);

    let pool = WorkerPool::new(config, AddExecutor).expect("Failed to create pool");

    println!("Pool created with 2 workers");

    // Submit a task
    let meta = make_meta(1, 10);
    let key = pool
        .submit_async((5, 3), meta)
        .await
        .expect("Failed to submit");

    println!("Task submitted, key: {:?}", key);

    // Retrieve result
    let result = pool
        .retrieve_async(&key, Duration::from_secs(5))
        .await
        .expect("Failed to retrieve");

    println!("Result: {}", result);
    assert_eq!(result, 8);

    // Check stats
    let stats = pool.stats();
    println!("Final stats: {:?}", stats);
    assert_eq!(stats.completed_tasks, 1);

    println!("=== test_basic_async_api PASSED ===\n");
}

/// Test blocking API (native only)
#[tokio::test]
async fn test_blocking_api() {
    println!("\n=== test_blocking_api ===");

    let config = WorkerPoolConfig::new()
        .with_worker_count(2)
        .with_max_units(100)
        .with_max_queue_depth(10);

    let pool = WorkerPool::new(config, AddExecutor).expect("Failed to create pool");

    println!("Pool created, testing blocking API");

    // Use blocking API
    let meta = make_meta(2, 10);
    let key = pool.submit((10, 20), meta).expect("Failed to submit");

    println!("Task submitted via blocking API");

    let result = pool
        .retrieve(&key, Duration::from_secs(5))
        .expect("Failed to retrieve");

    println!("Result: {}", result);
    assert_eq!(result, 30);

    println!("=== test_blocking_api PASSED ===\n");
}

/// Test concurrent task submission and execution
#[tokio::test]
async fn test_concurrent_execution() {
    println!("\n=== test_concurrent_execution ===");

    let executor = CountingExecutor::new();
    let executor_clone = executor.clone();

    let config = WorkerPoolConfig::new()
        .with_worker_count(4)
        .with_max_units(1000)
        .with_max_queue_depth(100);

    let pool = Arc::new(WorkerPool::new(config, executor).expect("Failed to create pool"));

    println!("Pool created with 4 workers");

    // Submit 20 tasks concurrently
    let num_tasks = 20;
    let mut handles = Vec::new();
    let mut keys = Vec::new();

    for i in 0..num_tasks {
        let meta = make_meta(i as u64, 10);
        let key = pool
            .submit_async(i as u64, meta)
            .await
            .expect("Failed to submit");
        keys.push(key);
    }

    println!("Submitted {} tasks", num_tasks);

    // Retrieve all results
    let pool_ref = pool.clone();
    for key in keys {
        let pool_clone = pool_ref.clone();
        handles.push(tokio::spawn(async move {
            pool_clone
                .retrieve_async(&key, Duration::from_secs(10))
                .await
                .expect("Failed to retrieve")
        }));
    }

    let results: Vec<u64> = futures::future::join_all(handles)
        .await
        .into_iter()
        .map(|r| r.expect("Task panicked"))
        .collect();

    println!("All results retrieved: {:?}", results);

    // Verify all tasks executed
    assert_eq!(executor_clone.execution_count(), num_tasks as u64);
    println!(
        "Max concurrent executions observed: {}",
        executor_clone.max_concurrent()
    );

    // With 4 workers and 50ms tasks, we should see some concurrency
    assert!(
        executor_clone.max_concurrent() > 1,
        "Expected concurrent execution"
    );

    let stats = pool.stats();
    println!("Final stats: {:?}", stats);
    assert_eq!(stats.completed_tasks, num_tasks as u64);

    println!("=== test_concurrent_execution PASSED ===\n");
}

/// Test resource limits cause queueing
#[tokio::test]
async fn test_resource_limits_queueing() {
    println!("\n=== test_resource_limits_queueing ===");

    let executor = CountingExecutor::new();

    // Low max_units to force queueing
    let config = WorkerPoolConfig::new()
        .with_worker_count(4)
        .with_max_units(30) // Only allow 3 tasks of cost 10 at a time
        .with_max_queue_depth(50);

    let pool = Arc::new(WorkerPool::new(config, executor).expect("Failed to create pool"));

    println!("Pool created with max_units=30 (only 3 concurrent tasks of cost 10)");

    // Submit 10 tasks, each requiring 10 units
    let mut keys = Vec::new();
    for i in 0..10 {
        let meta = make_meta(i as u64, 10);
        let key = pool
            .submit_async(i as u64, meta)
            .await
            .expect("Failed to submit");
        keys.push(key);
    }

    println!("Submitted 10 tasks");

    // Check that some are queued
    let stats = pool.stats();
    println!("Stats after submission: {:?}", stats);

    // Retrieve all results
    for key in keys {
        let _ = pool
            .retrieve_async(&key, Duration::from_secs(10))
            .await
            .expect("Failed to retrieve");
    }

    let final_stats = pool.stats();
    println!("Final stats: {:?}", final_stats);
    assert_eq!(final_stats.completed_tasks, 10);

    println!("=== test_resource_limits_queueing PASSED ===\n");
}

/// Test non-serializable streaming results (THE KEY CANDLE-VLLM PATTERN)
#[tokio::test]
async fn test_streaming_non_serializable_results() {
    println!("\n=== test_streaming_non_serializable_results ===");
    println!("This tests the candle-vllm streaming pattern with flume channels");

    let config = WorkerPoolConfig::new()
        .with_worker_count(2)
        .with_max_units(100)
        .with_max_queue_depth(10);

    let pool = WorkerPool::new(config, StreamingExecutor).expect("Failed to create pool");

    println!("Pool created with StreamingExecutor");

    // Submit a streaming task
    let meta = make_gpu_meta(1, 50);
    let key = pool
        .submit_async("hello".to_string(), meta)
        .await
        .expect("Failed to submit");

    println!("Streaming task submitted");

    // Retrieve the streaming result (contains a channel!)
    let result = pool
        .retrieve_async(&key, Duration::from_secs(5))
        .await
        .expect("Failed to retrieve");

    println!(
        "Got StreamingResult with {} expected tokens",
        result.total_tokens
    );

    // Consume the stream
    let mut tokens = Vec::new();
    while let Ok(token) = result.receiver.recv_async().await {
        println!("  Received token: {}", token);
        tokens.push(token);
    }

    println!("Received {} tokens total", tokens.len());
    assert_eq!(tokens.len(), 5);
    assert!(tokens[0].contains("hello:token_0"));
    assert!(tokens[4].contains("hello:token_4"));

    println!("=== test_streaming_non_serializable_results PASSED ===\n");
}

/// Test timeout on slow tasks
#[tokio::test]
async fn test_timeout_handling() {
    println!("\n=== test_timeout_handling ===");

    // Executor that takes 500ms
    let config = WorkerPoolConfig::new()
        .with_worker_count(1)
        .with_max_units(100)
        .with_max_queue_depth(10);

    let pool = WorkerPool::new(config, SlowExecutor::new(500)).expect("Failed to create pool");

    println!("Pool created with slow executor (500ms delay)");

    // Submit task
    let meta = make_meta(1, 10);
    let key = pool
        .submit_async((), meta)
        .await
        .expect("Failed to submit");

    println!("Task submitted, attempting retrieve with 100ms timeout...");

    // Try to retrieve with short timeout - should fail
    let start = Instant::now();
    let result = pool.retrieve_async(&key, Duration::from_millis(100)).await;
    let elapsed = start.elapsed();

    println!("Retrieve returned after {:?}", elapsed);

    match result {
        Err(PoolError::Timeout) => {
            println!("Correctly got Timeout error");
        }
        other => {
            panic!("Expected Timeout error, got: {:?}", other);
        }
    }

    // Verify timeout was respected (should be ~100ms, not 500ms)
    assert!(elapsed < Duration::from_millis(200), "Timeout took too long");

    println!("=== test_timeout_handling PASSED ===\n");
}

/// Test graceful shutdown
#[tokio::test]
async fn test_graceful_shutdown() {
    println!("\n=== test_graceful_shutdown ===");

    let executor = CountingExecutor::new();
    let executor_clone = executor.clone();

    let config = WorkerPoolConfig::new()
        .with_worker_count(2)
        .with_max_units(100)
        .with_max_queue_depth(10);

    let pool = WorkerPool::new(config, executor).expect("Failed to create pool");

    println!("Pool created");

    // Submit a few tasks
    let mut keys = Vec::new();
    for i in 0..3 {
        let meta = make_meta(i as u64, 10);
        let key = pool
            .submit_async(i as u64, meta)
            .await
            .expect("Failed to submit");
        keys.push(key);
    }

    println!("Submitted 3 tasks");

    // Retrieve them
    for key in keys {
        let _ = pool
            .retrieve_async(&key, Duration::from_secs(5))
            .await
            .expect("Failed to retrieve");
    }

    println!("All tasks completed");

    // Explicit shutdown
    let start = Instant::now();
    pool.shutdown();
    let shutdown_time = start.elapsed();

    println!("Shutdown completed in {:?}", shutdown_time);

    // Shutdown should be fast (not waiting for any timeout)
    assert!(
        shutdown_time < Duration::from_millis(500),
        "Shutdown took too long"
    );

    // Verify all tasks completed
    assert_eq!(executor_clone.execution_count(), 3);

    println!("=== test_graceful_shutdown PASSED ===\n");
}

/// Test submitting after shutdown fails gracefully
#[tokio::test]
async fn test_submit_after_shutdown() {
    println!("\n=== test_submit_after_shutdown ===");

    let config = WorkerPoolConfig::new()
        .with_worker_count(1)
        .with_max_units(100)
        .with_max_queue_depth(10);

    let pool = WorkerPool::new(config, AddExecutor).expect("Failed to create pool");

    println!("Pool created, shutting down...");
    pool.shutdown();
    println!("Pool shut down");

    // Try to submit - should fail
    let meta = make_meta(1, 10);
    let result = pool.submit_async((1, 2), meta).await;

    match result {
        Err(PoolError::PoolShutdown) => {
            println!("Correctly got PoolShutdown error");
        }
        other => {
            panic!("Expected PoolShutdown error, got: {:?}", other);
        }
    }

    println!("=== test_submit_after_shutdown PASSED ===\n");
}

/// Test CPU-bound work doesn't block the async runtime
#[tokio::test]
async fn test_cpu_work_isolation() {
    println!("\n=== test_cpu_work_isolation ===");

    let config = WorkerPoolConfig::new()
        .with_worker_count(2)
        .with_max_units(100)
        .with_max_queue_depth(10);

    // Executor that does actual CPU work
    let pool = WorkerPool::new(config, CpuWorkExecutor::new(5)).expect("Failed to create pool");

    println!("Pool created with CPU-intensive executor");

    // Submit CPU-intensive task
    let meta = make_meta(1, 10);
    let key = pool
        .submit_async("test_data".to_string(), meta)
        .await
        .expect("Failed to submit");

    println!("CPU task submitted");

    // Meanwhile, async runtime should remain responsive
    let async_start = Instant::now();
    tokio::time::sleep(Duration::from_millis(10)).await;
    let async_elapsed = async_start.elapsed();

    println!("Async sleep took {:?} (should be ~10ms)", async_elapsed);

    // If CPU work was blocking the runtime, this would take much longer
    assert!(
        async_elapsed < Duration::from_millis(50),
        "Async runtime was blocked!"
    );

    // Get the result
    let result = pool
        .retrieve_async(&key, Duration::from_secs(5))
        .await
        .expect("Failed to retrieve");

    println!("CPU task result: {}", result);
    assert!(result.contains("Processed 'test_data'"));

    println!("=== test_cpu_work_isolation PASSED ===\n");
}

/// Test queue depth limit
#[tokio::test]
async fn test_queue_depth_limit() {
    println!("\n=== test_queue_depth_limit ===");

    let config = WorkerPoolConfig::new()
        .with_worker_count(1)
        .with_max_units(10) // Only 1 task at a time
        .with_max_queue_depth(3); // Only 3 can queue

    let pool = WorkerPool::new(config, SlowExecutor::new(200)).expect("Failed to create pool");

    println!("Pool created with queue_depth=3, 1 worker, slow tasks");

    // Submit tasks until queue is full
    let mut keys = Vec::new();
    let mut rejected = 0;

    for i in 0..10 {
        let meta = make_meta(i as u64, 10);
        match pool.submit_async((), meta).await {
            Ok(key) => {
                println!("Task {} accepted", i);
                keys.push(key);
            }
            Err(PoolError::QueueFull) => {
                println!("Task {} rejected (queue full)", i);
                rejected += 1;
            }
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    println!(
        "Accepted {} tasks, rejected {} due to queue full",
        keys.len(),
        rejected
    );

    // Should have rejected some due to queue limit
    assert!(rejected > 0, "Expected some rejections");
    // Max should be 1 running + 3 queued = 4 accepted
    assert!(keys.len() <= 4, "Too many tasks accepted");

    // Clean up - retrieve what we can
    for key in keys {
        let _ = pool.retrieve_async(&key, Duration::from_secs(5)).await;
    }

    println!("=== test_queue_depth_limit PASSED ===\n");
}

/// Test multiple result retrievals for same key
#[tokio::test]
async fn test_result_consumed_once() {
    println!("\n=== test_result_consumed_once ===");

    let config = WorkerPoolConfig::new()
        .with_worker_count(1)
        .with_max_units(100)
        .with_max_queue_depth(10);

    let pool = WorkerPool::new(config, AddExecutor).expect("Failed to create pool");

    // Submit and retrieve
    let meta = make_meta(1, 10);
    let key = pool
        .submit_async((1, 2), meta)
        .await
        .expect("Failed to submit");

    let result = pool
        .retrieve_async(&key, Duration::from_secs(5))
        .await
        .expect("Failed to retrieve");
    assert_eq!(result, 3);

    println!("First retrieval succeeded: {}", result);

    // Second retrieval should fail (result already consumed)
    let result2 = pool.retrieve_async(&key, Duration::from_millis(100)).await;

    match result2 {
        Err(PoolError::Timeout) | Err(PoolError::ResultNotFound) => {
            println!("Second retrieval correctly failed");
        }
        Ok(v) => {
            panic!("Should not get result twice, got: {}", v);
        }
        Err(e) => {
            println!("Got error (acceptable): {:?}", e);
        }
    }

    println!("=== test_result_consumed_once PASSED ===\n");
}
