//! End-to-end integration test for LLM inference with OpenAI streaming.
//!
//! NOTE: This test needs to be updated to work with the new TaskExecutor trait.
//! See parking_lot_algorithm_test.rs for working examples of the complete implementation.
//!
//! This test validates the parking lot scheduler by:
//! 1. Creating a resource pool with max_units=3 (matching 3 tokio worker threads)
//! 2. Flooding the pool with 15 tasks to trigger parking/queuing behavior
//! 3. Making real OpenAI streaming API calls
//! 4. Collecting stream chunks via channels
//! 5. Verifying all tasks complete and parking behavior works correctly

#![allow(dead_code, unused_imports)]

use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use std::time::Duration;

use reqwest::Client;
use serde_json::json;
use tokio::sync::{mpsc, Mutex as TokioMutex};
use tokio::time::Instant;
use futures::StreamExt;

use prometheus_parking_lot::core::{PoolLimits, ResourcePool, ScheduledTask, TaskMetadata, TaskStatus, Spawn};
use prometheus_parking_lot::infra::queue::InMemoryQueue;
use prometheus_parking_lot::infra::mailbox::InMemoryMailbox;
use prometheus_parking_lot::runtime::TokioSpawner;
use prometheus_parking_lot::util::serde::{MailboxKey, Priority, ResourceCost, ResourceKind, TaskId};
use prometheus_parking_lot::util::clock::now_ms;

/// Payload for LLM inference tasks
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct LLMTaskPayload {
    prompt: String,
    #[serde(skip)]
    #[allow(dead_code)]
    stream_tx: Option<mpsc::UnboundedSender<String>>,
}

#[tokio::test]
#[ignore = "Needs update for new TaskExecutor API - see parking_lot_algorithm_test.rs for working examples"]
async fn test_llm_inference_with_parking() {
    // TODO: Update this test to use the new TaskExecutor trait
    // This test is temporarily disabled until it can be updated to match the new API.
    // For working examples of the complete parking lot algorithm, see parking_lot_algorithm_test.rs
    return;
    
    /* Commented out until updated for new API
    // Initialize tracing for visibility
    let _ = tracing_subscriber::fmt()
        .with_env_filter("prometheus_parking_lot=info,llm_inference_test=info")
        .try_init();

    // Check for OpenAI API key
    let api_key = match env::var("OPENAI_API_KEY") {
        Ok(key) => key,
        Err(_) => {
            eprintln!("Skipping test: OPENAI_API_KEY environment variable not set");
            return;
        }
    };

    tracing::info!("Starting LLM inference test with 3-thread pool and 15 tasks");

    // Create tokio spawner with 3 worker threads
    let spawner = TokioSpawner::with_worker_threads(3)
        .expect("Failed to create tokio runtime");

    // Configure resource pool
    let limits = PoolLimits {
        max_units: 3,
        max_queue_depth: 50,
        default_timeout: Duration::from_secs(120),
    };

    let queue = InMemoryQueue::new(50);
    let mailbox = InMemoryMailbox::new();
    
    // Wrap pool in Arc<Mutex> for sharing across async tasks
    let pool = Arc::new(TokioMutex::new(
        ResourcePool::new(limits, queue, mailbox, spawner.clone())
    ));

    // Track active tasks and stream receivers
    let task_count = 15;
    let mut stream_receivers: HashMap<TaskId, mpsc::UnboundedReceiver<String>> = HashMap::new();
    let active_tasks = Arc::new(TokioMutex::new(0_u32));
    let peak_concurrent = Arc::new(TokioMutex::new(0_u32));

    // Prepare prompts
    let prompts = vec![
        "Count from 1 to 3",
        "Name 2 colors",
        "Say hello",
        "Count from 1 to 5",
        "Name 2 fruits",
        "Say goodbye",
        "What is 2+2?",
        "Name a planet",
        "Count backwards from 3",
        "Name an animal",
        "What is the sky color?",
        "Count to 4",
        "Name a vegetable",
        "Say thank you",
        "What is 1+1?",
    ];

    tracing::info!("Submitting {} tasks to pool with max_units=3", task_count);
    let start_time = Instant::now();

    // Submit all tasks rapidly to trigger parking
    for i in 0..task_count {
        let task_id = i as TaskId;
        let (stream_tx, stream_rx) = mpsc::unbounded_channel();
        stream_receivers.insert(task_id, stream_rx);

        let task = ScheduledTask {
            meta: TaskMetadata {
                id: task_id,
                mailbox: Some(MailboxKey {
                    tenant: "test-tenant".to_string(),
                    user_id: Some("test-user".to_string()),
                    session_id: Some(format!("session-{}", task_id)),
                }),
                priority: if i < 3 { Priority::High } else { Priority::Normal },
                cost: ResourceCost {
                    kind: ResourceKind::Cpu,
                    units: 1,
                },
                deadline_ms: None,
                created_at_ms: now_ms(),
            },
            payload: LLMTaskPayload {
                prompt: prompts[i % prompts.len()].to_string(),
                stream_tx: Some(stream_tx.clone()),
            },
        };

        let pool_clone = pool.clone();
        let active_tasks_clone = active_tasks.clone();
        let peak_concurrent_clone = peak_concurrent.clone();
        let api_key_clone = api_key.clone();
        let prompt = prompts[i % prompts.len()].to_string();

        // Submit to pool
        let mut pool_guard = pool_clone.lock().await;
        let status = pool_guard.submit(task, now_ms())
            .expect("Failed to submit task");
        
        match status {
            TaskStatus::Running => {
                tracing::info!("Task {} started immediately", task_id);
                
                // Spawn the actual LLM execution
                spawner.spawn(execute_llm_task(
                    task_id,
                    prompt,
                    stream_tx,
                    api_key_clone,
                    active_tasks_clone,
                    peak_concurrent_clone,
                ));
            }
            TaskStatus::Queued => {
                tracing::info!("Task {} queued (waiting for capacity)", task_id);
                // In a real implementation, we'd need a wake mechanism
                // For this test, we'll poll and start queued tasks manually
            }
            other => {
                panic!("Unexpected status: {:?}", other);
            }
        }
        drop(pool_guard);
        
        // Small delay to demonstrate rapid submission
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    tracing::info!("All tasks submitted in {:?}", start_time.elapsed());

    // Collect stream results
    let mut results: HashMap<TaskId, Vec<String>> = HashMap::new();
    let collection_timeout = Duration::from_secs(60);
    let collection_start = Instant::now();

    tracing::info!("Collecting stream chunks from tasks...");

    for (task_id, mut rx) in stream_receivers {
        let mut chunks = Vec::new();
        
        // Collect all chunks for this task with timeout
        while let Ok(result) = tokio::time::timeout(
            collection_timeout.saturating_sub(collection_start.elapsed()),
            rx.recv()
        ).await {
            if let Some(chunk) = result {
                chunks.push(chunk);
            } else {
                // Channel closed
                break;
            }
        }
        
        if !chunks.is_empty() {
            tracing::info!("Task {} received {} chunks", task_id, chunks.len());
            results.insert(task_id, chunks);
        }
    }

    let total_duration = start_time.elapsed();
    tracing::info!("Test completed in {:?}", total_duration);

    // Verification
    let peak = *peak_concurrent.lock().await;
    tracing::info!("Peak concurrent tasks: {}", peak);
    
    // We should have collected results from the first few tasks at minimum
    assert!(
        results.len() >= 3,
        "Expected at least 3 tasks to complete, got {}",
        results.len()
    );

    // Verify peak concurrency didn't exceed our limit
    assert!(
        peak <= 3,
        "Peak concurrency {} exceeded limit of 3",
        peak
    );

    tracing::info!("✓ Test passed: parking behavior verified, {} tasks completed", results.len());
}

/// Execute an LLM inference task with streaming using reqwest
async fn execute_llm_task(
    task_id: TaskId,
    prompt: String,
    stream_tx: mpsc::UnboundedSender<String>,
    api_key: String,
    active_tasks: Arc<TokioMutex<u32>>,
    peak_concurrent: Arc<TokioMutex<u32>>,
) {
    let task_start = Instant::now();
    
    // Track concurrency
    {
        let mut active = active_tasks.lock().await;
        *active += 1;
        let mut peak = peak_concurrent.lock().await;
        if *active > *peak {
            *peak = *active;
        }
        tracing::info!("Task {} executing (active: {})", task_id, *active);
    }

    // Execute OpenAI streaming call with reqwest
    let client = Client::new();
    
    let request_body = json!({
        "model": "gpt-3.5-turbo",
        "messages": [
            {
                "role": "system",
                "content": "You are a helpful assistant. Keep responses very brief."
            },
            {
                "role": "user",
                "content": prompt
            }
        ],
        "max_tokens": 50,
        "stream": true
    });

    let response = client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await;

    match response {
        Ok(resp) => {
            let mut stream = resp.bytes_stream();
            
            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(bytes) => {
                        // Parse SSE format: "data: {json}\n\n"
                        if let Ok(text) = String::from_utf8(bytes.to_vec()) {
                            for line in text.lines() {
                                if line.starts_with("data: ") && !line.contains("[DONE]") {
                                    let json_str = line.strip_prefix("data: ").unwrap_or("");
                                    if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(json_str) {
                                        if let Some(choices) = json_val.get("choices").and_then(|c| c.as_array()) {
                                            for choice in choices {
                                                if let Some(content) = choice.get("delta")
                                                    .and_then(|d| d.get("content"))
                                                    .and_then(|c| c.as_str()) {
                                                    let _ = stream_tx.send(content.to_string());
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Task {} stream error: {}", task_id, e);
                        break;
                    }
                }
            }
            
            tracing::info!(
                "Task {} completed in {:?}",
                task_id,
                task_start.elapsed()
            );
        }
        Err(e) => {
            tracing::error!("Task {} failed to start stream: {}", task_id, e);
        }
    }

    // Decrement active count
    {
        let mut active = active_tasks.lock().await;
        *active -= 1;
        tracing::info!("Task {} finished (active: {})", task_id, *active);
    }
    */
}
