//! Durable cloud parking with the Postgres lease queue and result mailbox.
//!
//! A service accepts work into a durable queue, runs it, stores each result in
//! a durable mailbox, and acknowledges the task only after the result is
//! stored. Killing the process at any point loses nothing: unacknowledged work
//! is redelivered when its lease expires, and the idempotency key keeps the
//! first stored result. The client collects its result later, exactly once.
//!
//! ## Setup
//!
//! 1. Start Postgres (the project image, see `docker/postgres18`):
//!    ```bash
//!    docker compose -f docker-compose.test.yml up -d --build postgres18
//!    ```
//!
//! 2. Run this example (`DATABASE_URL` defaults to the compose service):
//!    ```bash
//!    cargo run --example postgres_pool --features postgres
//!    ```

use std::time::Duration;

use prometheus_parking_lot::core::{
    Delivery, Enqueued, LeaseQueue, ResultMailbox, ScheduledTask, TaskMetadata, TaskStatus,
};
use prometheus_parking_lot::infra::PgPool;
use prometheus_parking_lot::infra::mailbox::PgResultMailbox;
use prometheus_parking_lot::infra::queue::PgLeaseQueue;
use prometheus_parking_lot::util::clock::now_ms;
use prometheus_parking_lot::util::serde::{MailboxKey, Priority, ResourceCost, ResourceKind};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Prompt {
    text: String,
}

fn task(id: u64, priority: Priority, text: &str) -> ScheduledTask<Prompt> {
    ScheduledTask {
        meta: TaskMetadata {
            id,
            mailbox: Some(mailbox_key()),
            priority,
            cost: ResourceCost::new(ResourceKind::GpuVram, 4),
            deadline_ms: None,
            created_at_ms: now_ms(),
            idempotency_key: Some(format!("prompt-{id}")),
            sequence: id,
        },
        payload: Prompt { text: text.into() },
    }
}

fn mailbox_key() -> MailboxKey {
    MailboxKey {
        tenant: "demo-tenant".into(),
        user_id: Some("demo-user".into()),
        session_id: None,
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost:5433/postgres".into());
    println!("connecting to {database_url}");
    let pool = PgPool::connect(&database_url).await?;

    // Idempotent; run once per deployment (or at startup).
    PgLeaseQueue::<Prompt>::migrate(&pool).await?;
    PgResultMailbox::<String>::migrate(&pool).await?;

    // A unique queue and mailbox name per run keeps reruns independent.
    let run = now_ms();
    let queue = PgLeaseQueue::<Prompt>::new(pool.clone(), format!("demo-queue-{run}"), 100);
    let mailbox = PgResultMailbox::<String>::new(pool, format!("demo-results-{run}"));
    let _reaper = queue.spawn_reaper(Duration::from_secs(5));
    let _sweeper = mailbox.spawn_sweeper(Duration::from_secs(30));

    for (id, priority, text) in [
        (1, Priority::Low, "summarise the changelog"),
        (2, Priority::High, "explain quantum computing"),
    ] {
        let accepted = queue.enqueue(task(id, priority, text)).await?;
        assert!(matches!(accepted, Enqueued::Accepted(_)));
        println!("accepted task {id} ({priority:?})");
    }

    // Highest priority first.
    while let Some(leased) = queue.lease(1, Duration::from_secs(30)).await?.pop() {
        let meta = &leased.task.meta;
        println!(
            "running task {} (attempt {}): {}",
            meta.id, leased.attempt, leased.task.payload.text
        );
        let result = format!("answer to '{}'", leased.task.payload.text);
        // Store the result first, then acknowledge: a crash in between re-runs
        // the task, and the idempotency key keeps the first result.
        let mut delivery =
            Delivery::new(mailbox_key(), meta.id, TaskStatus::Completed, Some(result))
                .with_ttl(Duration::from_secs(3600));
        if let Some(key) = &meta.idempotency_key {
            delivery = delivery.with_idempotency_key(key.clone());
        }
        let _ = mailbox.deliver(delivery).await?;
        queue.ack(leased.token).await?;
    }

    // The client collects its results later, once each.
    while let Some(entry) = mailbox.consume(&mailbox_key()).await? {
        println!("client received: {:?}", entry.payload);
    }
    Ok(())
}
