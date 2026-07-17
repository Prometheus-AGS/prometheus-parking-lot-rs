//! Example of using PostgresQueue and PostgresMailbox for persistent task scheduling.
//!
//! This example demonstrates a cloud-deployment scenario with:
//! - Postgres-backed persistent queue
//! - Postgres-backed result mailbox
//! - Task submission and result retrieval
//!
//! ## Setup
//!
//! 1. Start Postgres using Docker Compose:
//!    ```bash
//!    docker-compose -f docker-compose.test.yml up -d
//!    ```
//!
//! 2. Run this example:
//!    ```bash
//!    cargo run --example postgres_pool --features postgres
//!    ```

#![cfg(feature = "postgres")]

use prometheus_parking_lot::core::{
    PoolLimits, ResourcePool, ScheduledTask, Spawn, TaskExecutor, TaskMetadata,
};
use prometheus_parking_lot::infra::queue::postgres::PostgresQueue;
use prometheus_parking_lot::infra::mailbox::postgres::PostgresMailbox;
use prometheus_parking_lot::util::clock::now_ms;
use prometheus_parking_lot::util::serde::{MailboxKey, Priority, ResourceCost, ResourceKind};

use async_trait::async_trait;
use sea_orm::Database;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

/// Example task payload for demonstration.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DemoTask {
    id: u64,
    operation: String,
    data: String,
}

/// Example result payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DemoResult {
    task_id: u64,
    output: String,
    processed_at: u128,
}

/// Simple executor that simulates some work.
#[derive(Clone)]
struct DemoExecutor;

#[async_trait]
impl TaskExecutor<DemoTask, DemoResult> for DemoExecutor {
    async fn execute(&self, payload: DemoTask, meta: TaskMetadata) -> DemoResult {
        // Simulate some work
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        DemoResult {
            task_id: meta.id,
            output: format!("Processed {}: {}", payload.operation, payload.data),
            processed_at: now_ms(),
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
    // Initialize tracing
    tracing_subscriber::fmt::init();

    println!("=== Postgres Pool Example ===\n");

    // Connect to database
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://testuser:testpass@localhost:5433/testdb".into());
    
    println!("Connecting to database: {}", database_url);
    let db = Database::connect(&database_url).await?;
    println!("✓ Connected to database\n");

    // Create queue and mailbox
    let queue = PostgresQueue::new(db.clone(), "demo_queue".into(), 1000);
    let mailbox = PostgresMailbox::new(db.clone());

    // Configure pool
    let limits = PoolLimits {
        max_units: 10,
        max_queue_depth: 1000,
        default_timeout: Duration::from_secs(60),
    };

    // Create resource pool
    let pool = Arc::new(ResourcePool::new(
        limits,
        queue,
        mailbox,
        DemoExecutor,
        TokioSpawner,
    ));

    println!("✓ Resource pool created with:");
    println!("  - Max units: 10");
    println!("  - Max queue depth: 1000");
    println!("  - Postgres queue backend");
    println!("  - Postgres mailbox backend\n");

    // Submit some tasks
    println!("Submitting tasks...");
    for i in 1..=5 {
        let task = ScheduledTask {
            meta: TaskMetadata {
                id: i,
                mailbox: Some(MailboxKey {
                    tenant: "demo-tenant".into(),
                    user_id: Some(format!("user-{}", i)),
                    session_id: None,
                }),
                priority: if i % 2 == 0 {
                    Priority::High
                } else {
                    Priority::Normal
                },
                cost: ResourceCost {
                    kind: ResourceKind::Cpu,
                    units: 1,
                },
                deadline_ms: None,
                created_at_ms: now_ms(),
            },
            payload: DemoTask {
                id: i,
                operation: format!("process_{}", i),
                data: format!("demo-data-{}", i),
            },
        };

        let status = pool.submit(task, now_ms()).await?;
        println!("  Task {} submitted with status: {:?}", i, status);
    }

    println!("\n✓ All tasks submitted");
    println!("\nWaiting for tasks to complete...");
    
    // Wait a bit for tasks to process
    tokio::time::sleep(Duration::from_secs(2)).await;

    println!("\n✓ Tasks processed");
    println!("\n=== Example Complete ===");
    println!("\nNote: Results are stored in the Postgres mailbox.");
    println!("Check the pl_mailbox_messages table to see the results.");

    Ok(())
}
