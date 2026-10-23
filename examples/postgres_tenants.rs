//! A multi-tenant cloud service on durable Postgres pools.
//!
//! Two pools, one per service tier, each on its own Postgres backend (a lease
//! queue and a result mailbox; one backend drives at most one pool). Tenants
//! share a tier's pool; the tenant in each task's mailbox address keeps their
//! results apart. A request retried with the same idempotency key is stored
//! once and answered once, so a client that timed out can safely resubmit.
//!
//! ## Setup
//!
//! 1. Start Postgres (the project image):
//!    ```bash
//!    docker compose -f docker-compose.test.yml up -d --build postgres18
//!    ```
//! 2. Run (`DATABASE_URL` defaults to the compose service):
//!    ```bash
//!    cargo run --example postgres_tenants --features postgres
//!    ```
//!
//! Guide: `website/docs/guides/cloud-postgres.md`.

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
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Completion {
    prompt: String,
}

/// Stands in for a hosted model call.
#[derive(Clone)]
struct HostedModel {
    tier: String,
}

#[async_trait]
impl TaskExecutor<Completion, String> for HostedModel {
    async fn execute(&self, job: Completion, _meta: TaskMetadata) -> Result<String, TaskError> {
        tokio::time::sleep(Duration::from_millis(20)).await;
        Ok(format!("{} tier answer to '{}'", self.tier, job.prompt))
    }
}

/// Two tiers on one database: one backend (queue + mailbox) per pool.
fn config(
    url: &str,
    run: u128,
) -> Result<SchedulerConfig, Box<dyn std::error::Error + Send + Sync>> {
    let backend = |queue: &str| {
        serde_json::json!({
            "type": "postgres",
            "url": url,
            "queue_name": format!("{queue}-{run}"),
            "max_depth": 10_000,
            "lease_secs": 60,
            "max_attempts": 5,
            "listen": true,
            "migrate": true
        })
    };
    let pool = |max_units: u32, backend: &str| {
        serde_json::json!({
            "max_units": max_units,
            "max_queue_depth": 10_000,
            "default_timeout_secs": 120,
            "queue": { "backend": backend },
            "mailbox": { "backend": backend }
        })
    };
    let json = serde_json::json!({
        "backends": { "premium_db": backend("premium"), "standard_db": backend("standard") },
        "pools": { "premium": pool(8, "premium_db"), "standard": pool(2, "standard_db") }
    });
    Ok(SchedulerConfig::from_json_str(&json.to_string())?)
}

fn address(tenant: &str, user: &str) -> MailboxKey {
    MailboxKey {
        tenant: tenant.into(),
        user_id: Some(user.into()),
        session_id: None,
    }
}

fn request(id: u64, to: &MailboxKey, request_key: &str, prompt: &str) -> ScheduledTask<Completion> {
    ScheduledTask {
        meta: TaskMetadata {
            id,
            mailbox: Some(to.clone()),
            priority: Priority::Normal,
            cost: ResourceCost::new(ResourceKind::GpuVram, 1),
            deadline_ms: None,
            created_at_ms: now_ms(),
            // The client's request id, scoped by tenant: a retry carries the same key.
            idempotency_key: Some(format!("{}:{request_key}", to.tenant)),
            sequence: 0,
        },
        payload: Completion {
            prompt: prompt.into(),
        },
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost:5433/postgres".into());
    let scheduler: BuiltScheduler<Completion, String, HostedModel, TokioSpawner> =
        build_pools_from_scheduler_config(
            &config(&url, now_ms())?,
            |name, _pool| {
                Ok(HostedModel {
                    tier: name.to_owned(),
                })
            },
            TokioSpawner::new(tokio::runtime::Handle::current()),
        )
        .await?;
    let premium = &scheduler.durable_pools["premium"];
    let standard = &scheduler.durable_pools["standard"];

    let acme = address("acme", "ana");
    let globex = address("globex", "gus");
    premium
        .submit(request(1, &acme, "req-1", "plan the launch"))
        .await?;
    standard
        .submit(request(2, &globex, "req-1", "draft an email"))
        .await?;
    // The client timed out and retried: same request key, same tenant.
    let retry = standard
        .submit(request(3, &globex, "req-1", "draft an email"))
        .await?;
    assert_eq!(retry, TaskStatus::Queued, "a duplicate counts as accepted");

    let acme_results = collect(premium, &acme, 1).await?;
    let globex_results = collect(standard, &globex, 1).await?;
    println!("acme:   {acme_results:?}");
    println!("globex: {globex_results:?}");
    assert_eq!(acme_results, ["premium tier answer to 'plan the launch'"]);
    assert_eq!(globex_results, ["standard tier answer to 'draft an email'"]);

    // Give a stray duplicate time to show up; none does.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        standard.consume(&globex).await?.is_none(),
        "one answer per request key"
    );
    // Tenants never see each other's results.
    assert!(premium.consume(&globex).await?.is_none());

    for pool in [premium, standard] {
        let report = pool.shutdown_with(ShutdownPolicy::default()).await;
        println!("shutdown: {report:?}");
    }
    Ok(())
}

/// Read `key`'s mailbox on `pool` until `want` results arrived (up to 10 s).
async fn collect(
    pool: &prometheus_parking_lot::builders::ConfiguredDurablePool<
        Completion,
        String,
        HostedModel,
        TokioSpawner,
    >,
    key: &MailboxKey,
    want: usize,
) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
    let mut results = Vec::new();
    for _ in 0..500 {
        while let Some(entry) = pool.consume(key).await? {
            results.push(entry.payload.unwrap_or_default());
        }
        if results.len() >= want {
            return Ok(results);
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    Err(format!("{} results for {key:?} after 10 s", results.len()).into())
}
