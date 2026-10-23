//! A GPU inference pool with per-resource limits and skip-ahead.
//!
//! One GPU has 24 units of VRAM (say, 1 unit = 1 GiB) and the host lets four
//! requests hold a CPU slot at once. Each request declares both. The pool
//! starts a request only when its VRAM and its CPU slot fit together; a
//! request that can never fit is rejected at submission; and a small request
//! may start ahead of a large one that is waiting for VRAM, at most
//! `max_head_skips` times (the starvation bound).
//!
//! No server is needed:
//!
//! ```bash
//! cargo run --example gpu_inference_pool
//! ```
//!
//! Guide: `website/docs/guides/gpu-inference-pool.md`.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use prometheus_parking_lot::builders::{BuiltScheduler, build_pools_from_scheduler_config};
use prometheus_parking_lot::config::SchedulerConfig;
use prometheus_parking_lot::core::{
    ScheduledTask, SchedulerError, TaskError, TaskExecutor, TaskMetadata, TaskStatus,
};
use prometheus_parking_lot::runtime::TokioSpawner;
use prometheus_parking_lot::util::{MailboxKey, Priority, ResourceCost, ResourceKind, now_ms};
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

/// The pool section. `max_units` bounds the total over every kind;
/// `resource_limits` bounds each listed kind on its own.
const CONFIG: &str = r#"{
  "pools": {
    "inference": {
      "max_units": 40,
      "max_queue_depth": 64,
      "default_timeout_secs": 120,
      "queue": "in_memory",
      "mailbox": "in_memory",
      "max_head_skips": 8,
      "resource_limits": { "gpu_vram": 24, "cpu": 4 }
    }
  }
}"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Generate {
    model: String,
    prompt: String,
}

/// Stands in for a model call. The gate holds every request until the example
/// releases it, so the admission decisions below are deterministic.
#[derive(Clone)]
struct FakeModel {
    gate: Arc<Semaphore>,
}

#[async_trait]
impl TaskExecutor<Generate, String> for FakeModel {
    async fn execute(&self, job: Generate, _meta: TaskMetadata) -> Result<String, TaskError> {
        let _pass = self.gate.acquire().await.map_err(TaskError::executor)?;
        Ok(format!("[{}] {}", job.model, job.prompt.to_uppercase()))
    }
}

/// A request that needs `vram` units of GPU memory and one CPU slot.
fn request(id: u64, model: &str, vram: u32) -> ScheduledTask<Generate> {
    ScheduledTask {
        meta: TaskMetadata {
            id,
            mailbox: Some(MailboxKey {
                tenant: "acme".into(),
                user_id: None,
                session_id: Some(format!("request-{id}")),
            }),
            priority: Priority::Normal,
            cost: ResourceCost::new(ResourceKind::GpuVram, vram).with(ResourceKind::Cpu, 1),
            deadline_ms: None,
            created_at_ms: now_ms(),
            idempotency_key: None,
            sequence: 0, // the pool stamps the submission order
        },
        payload: Generate {
            model: model.into(),
            prompt: format!("prompt {id}"),
        },
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = SchedulerConfig::from_json_str(CONFIG)?;
    let gate = Arc::new(Semaphore::new(0));
    let built: BuiltScheduler<Generate, String, FakeModel, TokioSpawner> =
        build_pools_from_scheduler_config(
            &config,
            |_name, _pool| {
                Ok(FakeModel {
                    gate: Arc::clone(&gate),
                })
            },
            TokioSpawner::new(tokio::runtime::Handle::current()),
        )
        .await?;
    let inference = &built.pools["inference"];
    let pool = &inference.pool;

    // A 13B model at 16 GiB starts: 16 of 24 VRAM, 1 of 4 CPU slots.
    assert_eq!(
        pool.submit(request(1, "13b", 16)).await?,
        TaskStatus::Running
    );
    // A second 16 GiB request waits: 32 VRAM would exceed the 24 limit, even
    // though the pool's total (34 of 40) and the CPU slots have room.
    assert_eq!(
        pool.submit(request(2, "13b", 16)).await?,
        TaskStatus::Queued
    );
    // A 7B model at 6 GiB fits the VRAM that is left, so it starts ahead of
    // the parked 13B request. That is one skip of the head.
    let small = pool.submit(request(3, "7b", 6)).await?;
    println!("7b request: {small:?}");
    // A 30 GiB request can never fit 24 GiB of VRAM, although its total (31)
    // is within the pool's 40: rejected at submission, not parked.
    let err = pool.submit(request(4, "34b", 30)).await.unwrap_err();
    assert!(matches!(err, SchedulerError::CapacityExceeded { .. }));
    println!("34b request rejected: {err}");

    wait_for(|| pool.running_tasks() == 2).await;
    println!(
        "running {} | queued {} | vram {:?}/24 | cpu {:?}/4 | skip-ahead {:?}",
        pool.running_tasks(),
        pool.queued_tasks(),
        pool.active_units_of(ResourceKind::GpuVram),
        pool.active_units_of(ResourceKind::Cpu),
        pool.skip_ahead_stats(),
    );
    assert_eq!(pool.active_units_of(ResourceKind::GpuVram), Some(22));
    assert_eq!(pool.skip_ahead_stats().skipped, 1);

    // Release the model calls. The parked 13B request starts when VRAM frees.
    gate.add_permits(3);
    for id in 1..=3 {
        let key = MailboxKey {
            tenant: "acme".into(),
            user_id: None,
            session_id: Some(format!("request-{id}")),
        };
        let (status, text) = collect(&inference.mailbox, &key).await;
        assert_eq!(status, TaskStatus::Completed);
        println!("request {id}: {}", text.unwrap_or_default());
    }
    assert_eq!(pool.active_units(), 0);
    Ok(())
}

/// Poll `ready` for up to five seconds.
async fn wait_for(ready: impl Fn() -> bool) {
    for _ in 0..500 {
        if ready() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("condition not reached within five seconds");
}

/// Take the outcome stored under `key`, waiting up to five seconds.
async fn collect(
    mailbox: &prometheus_parking_lot::builders::SharedInMemoryMailbox<String>,
    key: &MailboxKey,
) -> (TaskStatus, Option<String>) {
    for _ in 0..500 {
        if let Some(outcome) = mailbox.take(key) {
            return outcome;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("no outcome for {key:?}");
}
