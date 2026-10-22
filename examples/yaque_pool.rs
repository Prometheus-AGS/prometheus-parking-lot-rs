//! Example of using YaqueQueue for file-backed persistent task scheduling.
//!
//! This example demonstrates a desktop/Tauri deployment scenario with:
//! - File-backed persistent queue (survives app restarts)
//! - In-memory mailbox (results stored in RAM)
//! - Task submission and processing
//!
//! ## Running
//!
//! ```bash
//! cargo run --example yaque_pool
//! ```

use prometheus_parking_lot::core::{
    PoolLimits, ResourcePool, ScheduledTask, Spawn, TaskExecutor, TaskMetadata,
};
use prometheus_parking_lot::infra::queue::yaque::YaqueQueue;
use prometheus_parking_lot::infra::mailbox::memory::InMemoryMailbox;
use prometheus_parking_lot::util::clock::now_ms;
use prometheus_parking_lot::util::serde::{MailboxKey, Priority, ResourceCost, ResourceKind};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

/// Example task payload for a local LLM inference.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LlmInferenceTask {
    model: String,
    prompt: String,
    max_tokens: u32,
}

/// Example result from LLM inference.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LlmResult {
    text: String,
    tokens_used: u32,
    inference_time_ms: u64,
}

/// Simulated LLM executor.
#[derive(Clone)]
struct LocalLlmExecutor;

#[async_trait]
impl TaskExecutor<LlmInferenceTask, LlmResult> for LocalLlmExecutor {
    async fn execute(&self, payload: LlmInferenceTask, _meta: TaskMetadata) -> LlmResult {
        // Simulate LLM inference
        println!("  🤖 Processing: {}", payload.prompt);
        tokio::time::sleep(Duration::from_millis(500)).await;
        
        LlmResult {
            text: format!(
                "Generated response for '{}' using model {}",
                payload.prompt, payload.model
            ),
            tokens_used: 128,
            inference_time_ms: 500,
        }
    }
}

/// Tokio-based spawner.
#[derive(Clone)]
struct TokioSpawner;

impl Spawn for TokioSpawner {
    fn spawn<F>(&self, fut: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        tokio::spawn(fut);
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Yaque Pool Example (Desktop/Tauri Scenario) ===\n");

    // Create data directory
    let data_dir = std::env::temp_dir().join("prometheus_parking_lot_demo");
    std::fs::create_dir_all(&data_dir)?;
    println!("📁 Queue directory: {}", data_dir.display());

    // Create file-backed queue
    let queue = YaqueQueue::new(&data_dir, "llm_tasks", 100)?;
    let mailbox = InMemoryMailbox::new();

    println!("✓ File-backed queue created");
    println!("✓ In-memory mailbox created\n");

    // Configure pool
    let limits = PoolLimits {
        max_units: 8,  // Simulating 8GB VRAM limit
        max_queue_depth: 100,
        default_timeout: Duration::from_secs(120),
    };

    // Create resource pool
    let pool = Arc::new(ResourcePool::new(
        limits,
        queue,
        mailbox,
        LocalLlmExecutor,
        TokioSpawner,
    ));

    println!("✓ Resource pool created with:");
    println!("  - Max units: 8 (simulating 8GB VRAM)");
    println!("  - Max queue depth: 100");
    println!("  - File-backed persistent queue");
    println!("  - In-memory mailbox\n");

    // Submit LLM inference tasks
    println!("Submitting LLM inference tasks...\n");
    
    let prompts = vec![
        "Explain quantum computing in simple terms",
        "Write a haiku about programming",
        "What is the meaning of life?",
    ];

    for (i, prompt) in prompts.iter().enumerate() {
        let task_id = (i + 1) as u64;
        
        let task = ScheduledTask {
            meta: TaskMetadata {
                id: task_id,
                mailbox: Some(MailboxKey {
                    tenant: "desktop-app".into(),
                    user_id: Some("local-user".into()),
                    session_id: Some(format!("session-{}", task_id)),
                }),
                priority: Priority::Normal,
                cost: ResourceCost {
                    kind: ResourceKind::GpuVram,
                    units: 4,  // 4GB VRAM per task
                },
                deadline_ms: None,
                created_at_ms: now_ms(),
                idempotency_key: None,
            },
            payload: LlmInferenceTask {
                model: "llama-3.1-8b".into(),
                prompt: prompt.to_string(),
                max_tokens: 256,
            },
        };

        let status = pool.submit(task, now_ms()).await?;
        println!("Task {}: {:?}", task_id, status);
        println!("  Prompt: \"{}\"", prompt);
    }

    println!("\n✓ All tasks submitted");
    println!("\n⏳ Waiting for tasks to process...\n");
    
    // Wait for tasks to complete
    tokio::time::sleep(Duration::from_secs(3)).await;

    println!("\n✓ All tasks processed");
    
    // Show persistence
    println!("\n📝 Queue File Status:");
    let queue_file = data_dir.join("llm_tasks.jsonl");
    if queue_file.exists() {
        let metadata = std::fs::metadata(&queue_file)?;
        println!("  File: {}", queue_file.display());
        println!("  Size: {} bytes", metadata.len());
        println!("  ✓ Queue state persisted to disk");
    }

    println!("\n=== Example Complete ===");
    println!("\nKey Features Demonstrated:");
    println!("  ✓ File-based persistence (survives restarts)");
    println!("  ✓ Resource-aware scheduling (VRAM limits)");
    println!("  ✓ Task queueing when capacity exhausted");
    println!("  ✓ Desktop/Tauri deployment pattern");

    Ok(())
}
