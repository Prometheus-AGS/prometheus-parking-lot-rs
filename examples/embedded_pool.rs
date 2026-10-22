//! Crash-safe local parking with the embedded redb backends.
//!
//! A desktop or Tauri app accepts work into a durable queue, runs it later (or
//! after a restart), stores each result in a durable mailbox, and acknowledges
//! the task only after the result is stored. Killing the process at any point
//! loses nothing: unacknowledged work is redelivered when its lease expires.
//!
//! ```bash
//! cargo run --example embedded_pool --features embedded
//! ```

use std::time::Duration;

use prometheus_parking_lot::core::{
    Delivery, Enqueued, LeaseQueue, ResultMailbox, ScheduledTask, TaskMetadata, TaskStatus,
};
use prometheus_parking_lot::infra::EmbeddedStore;
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
        tenant: "desktop-app".into(),
        user_id: Some("local-user".into()),
        session_id: None,
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::temp_dir().join("prometheus_parking_lot_embedded_demo");
    let _ = std::fs::remove_file(dir.join("parking-lot.redb"));
    let db_path = dir.join("parking-lot.redb");
    println!("database: {}", db_path.display());

    // Session 1: accept work, then "crash" (drop everything) before running it.
    {
        let store = EmbeddedStore::open(&db_path)?;
        let queue = store.lease_queue::<Prompt>(100);
        for (id, priority, text) in [
            (1, Priority::Low, "summarise the changelog"),
            (2, Priority::High, "explain quantum computing"),
        ] {
            let accepted = queue.enqueue(task(id, priority, text)).await?;
            assert!(matches!(accepted, Enqueued::Accepted(_)));
            println!("accepted task {id} ({priority:?})");
        }
        println!("-- process exits before running anything --");
    }

    // Session 2: reopen; the accepted work is still there, highest priority first.
    let store = EmbeddedStore::open(&db_path)?;
    let queue = store.lease_queue::<Prompt>(100);
    let mailbox = store.result_mailbox::<String>();
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
            Delivery::new(mailbox_key(), meta.id, TaskStatus::Completed, Some(result));
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
