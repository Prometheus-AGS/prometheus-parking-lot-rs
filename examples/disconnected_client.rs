//! A client submits work, goes away, and collects the result later, even
//! after the service restarted in between.
//!
//! Each task names a mailbox address (tenant, user, session). The pool stores
//! the outcome there, success or failure, whether or not anyone is listening.
//! The client comes back with the same address and consumes each outcome
//! once. On a durable backend (here the embedded redb file) the outcome also
//! survives a restart of the service.
//!
//! No server is needed:
//!
//! ```bash
//! cargo run --example disconnected_client --features embedded
//! ```
//!
//! Guide: `website/docs/guides/disconnected-clients.md`.

use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use prometheus_parking_lot::ShutdownPolicy;
use prometheus_parking_lot::builders::{BuiltScheduler, build_pools_from_scheduler_config};
use prometheus_parking_lot::config::SchedulerConfig;
use prometheus_parking_lot::core::{
    ScheduledTask, TaskError, TaskExecutor, TaskMetadata, TaskStatus,
};
use prometheus_parking_lot::runtime::TokioSpawner;
use prometheus_parking_lot::util::{MailboxKey, Priority, ResourceCost, ResourceKind, now_ms};

/// Summarises a document; an empty document is an error the client must see.
#[derive(Clone)]
struct Summarise;

#[async_trait]
impl TaskExecutor<String, String> for Summarise {
    async fn execute(&self, doc: String, _meta: TaskMetadata) -> Result<String, TaskError> {
        if doc.is_empty() {
            return Err(TaskError::executor("empty document"));
        }
        Ok(format!("{} words", doc.split_whitespace().count()))
    }
}

type Scheduler = BuiltScheduler<String, String, Summarise, TokioSpawner>;

async fn start_service(path: &Path) -> Result<Scheduler, Box<dyn std::error::Error + Send + Sync>> {
    let json = serde_json::json!({
        "backends": {
            "local": { "type": "redb", "path": path, "max_depth": 1000, "lease_secs": 30 }
        },
        "pools": {
            "summaries": {
                "max_units": 4,
                "max_queue_depth": 1000,
                "default_timeout_secs": 60,
                "queue": { "backend": "local" },
                "mailbox": { "backend": "local" }
            }
        }
    });
    let config = SchedulerConfig::from_json_str(&json.to_string())?;
    Ok(build_pools_from_scheduler_config(
        &config,
        |_name, _pool| Ok(Summarise),
        TokioSpawner::new(tokio::runtime::Handle::current()),
    )
    .await?)
}

/// The address a client reads its results from: one per user session.
fn address(user: &str, session: &str) -> MailboxKey {
    MailboxKey {
        tenant: "acme".into(),
        user_id: Some(user.into()),
        session_id: Some(session.into()),
    }
}

fn task(id: u64, to: &MailboxKey, doc: &str) -> ScheduledTask<String> {
    ScheduledTask {
        meta: TaskMetadata {
            id,
            mailbox: Some(to.clone()),
            priority: Priority::Normal,
            cost: ResourceCost::new(ResourceKind::Cpu, 1),
            deadline_ms: None,
            created_at_ms: now_ms(),
            idempotency_key: Some(format!("summary-{id}")),
            sequence: 0,
        },
        payload: doc.into(),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("parking-lot.redb");
    let alice = address("alice", "tab-1");
    let bob = address("bob", "phone");

    // The service runs; both clients submit and disconnect without waiting.
    // The service then stops: `submit` returned only after each task was
    // stored, so stopping now loses nothing, whether a task ran yet or not.
    {
        let service = start_service(&path).await?;
        let pool = &service.durable_pools["summaries"];
        pool.submit(task(1, &alice, "the quick brown fox")).await?;
        pool.submit(task(2, &alice, "")).await?;
        pool.submit(task(3, &bob, "jumps over the lazy dog today"))
            .await?;
        let report = pool
            .shutdown_with(ShutdownPolicy::Drain {
                deadline: Duration::from_secs(10),
            })
            .await;
        assert_eq!(report.unsettled, 0, "{report:?}");
        println!("service stopped: {report:?}");
    }

    // The service restarts and runs whatever is unfinished. Alice reconnects
    // and reads her mailbox until both of her outcomes are there.
    let service = start_service(&path).await?;
    let pool = &service.durable_pools["summaries"];
    let mut alice_seen = Vec::new();
    for _ in 0..500 {
        while let Some(entry) = pool.consume(&alice).await? {
            println!(
                "alice: task {} -> {:?} {:?}",
                entry.task_id, entry.status, entry.payload
            );
            alice_seen.push((entry.task_id, entry.status));
        }
        if alice_seen.len() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    alice_seen.sort_by_key(|(id, _)| *id);
    assert_eq!(alice_seen[0], (1, TaskStatus::Completed));
    assert!(matches!(alice_seen[1], (2, TaskStatus::Failed(_))));

    // Consume-once: a second read finds nothing, and Bob's result is his.
    assert!(pool.consume(&alice).await?.is_none());
    let mut bob_entry = None;
    for _ in 0..500 {
        bob_entry = pool.consume(&bob).await?;
        if bob_entry.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let bob_entry = bob_entry.expect("bob's result waits for him");
    assert_eq!(bob_entry.payload.as_deref(), Some("6 words"));
    println!("bob: task {} -> {:?}", bob_entry.task_id, bob_entry.payload);

    pool.shutdown_with(ShutdownPolicy::default()).await;
    Ok(())
}
