//! Shut a durable pool down mid-work and finish the work after a restart.
//!
//! A pool on the embedded redb backend accepts three long tasks and can run
//! one at a time. `shutdown_with(DrainThenCancel)` gives running work a short
//! drain deadline, then cancels what is left and hands every unfinished task
//! back to the queue. A new pool opened on the same file runs them, and each
//! result lands in the durable mailbox exactly once.
//!
//! No server is needed:
//!
//! ```bash
//! cargo run --example shutdown_requeue --features embedded
//! ```
//!
//! Guide: `website/docs/guides/shutdown-and-requeue.md`.

use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use prometheus_parking_lot::ShutdownPolicy;
use prometheus_parking_lot::builders::{BuiltScheduler, build_pools_from_scheduler_config};
use prometheus_parking_lot::config::SchedulerConfig;
use prometheus_parking_lot::core::{ScheduledTask, TaskError, TaskExecutor, TaskMetadata};
use prometheus_parking_lot::runtime::TokioSpawner;
use prometheus_parking_lot::util::{MailboxKey, Priority, ResourceCost, ResourceKind, now_ms};

/// Sleeps for `delay`, then echoes the payload. The first process is slow (it
/// is shut down mid-run); the second is fast.
#[derive(Clone)]
struct Work {
    delay: Duration,
}

#[async_trait]
impl TaskExecutor<String, String> for Work {
    async fn execute(&self, text: String, _meta: TaskMetadata) -> Result<String, TaskError> {
        tokio::time::sleep(self.delay).await;
        Ok(format!("done: {text}"))
    }
}

type Scheduler = BuiltScheduler<String, String, Work, TokioSpawner>;

fn config(path: &Path) -> Result<SchedulerConfig, Box<dyn std::error::Error + Send + Sync>> {
    let json = serde_json::json!({
        "backends": {
            "local": {
                "type": "redb",
                "path": path,
                "max_depth": 100,
                "lease_secs": 30
            }
        },
        "pools": {
            "jobs": {
                "max_units": 1,
                "max_queue_depth": 100,
                "default_timeout_secs": 60,
                "queue": { "backend": "local" },
                "mailbox": { "backend": "local" }
            }
        }
    });
    Ok(SchedulerConfig::from_json_str(&json.to_string())?)
}

async fn start(
    path: &Path,
    delay: Duration,
) -> Result<Scheduler, Box<dyn std::error::Error + Send + Sync>> {
    Ok(build_pools_from_scheduler_config(
        &config(path)?,
        |_name, _pool| Ok(Work { delay }),
        TokioSpawner::new(tokio::runtime::Handle::current()),
    )
    .await?)
}

fn inbox() -> MailboxKey {
    MailboxKey {
        tenant: "desktop".into(),
        user_id: Some("local-user".into()),
        session_id: None,
    }
}

fn task(id: u64) -> ScheduledTask<String> {
    ScheduledTask {
        meta: TaskMetadata {
            id,
            mailbox: Some(inbox()),
            priority: Priority::Normal,
            cost: ResourceCost::new(ResourceKind::Cpu, 1),
            deadline_ms: None,
            created_at_ms: now_ms(),
            // The key that makes a re-run store one result, not two.
            idempotency_key: Some(format!("job-{id}")),
            sequence: 0,
        },
        payload: format!("job {id}"),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("parking-lot.redb");

    // Process 1: accept three one-minute tasks, then shut down after 200 ms.
    {
        let scheduler = start(&path, Duration::from_secs(60)).await?;
        let jobs = &scheduler.durable_pools["jobs"];
        for id in 1..=3 {
            jobs.submit(task(id)).await?;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
        let report = jobs
            .shutdown_with(ShutdownPolicy::DrainThenCancel {
                deadline: Duration::from_millis(100),
            })
            .await;
        println!("process 1 shutdown: {report:?}");
        assert_eq!(report.completed, 0);
        assert_eq!(report.unsettled, 0, "nothing is left running");
        // Every task this process held when the deadline passed was cancelled
        // and handed back to the queue (`requeued`); tasks it had not leased
        // yet never left the queue.
        assert!(report.cancelled >= 1);
        assert_eq!(report.cancelled, report.requeued);
        assert!(jobs.consume(&inbox()).await?.is_none(), "no result yet");
    }

    // Process 2: the same file, a fast executor. The queued work runs.
    let scheduler = start(&path, Duration::from_millis(10)).await?;
    let jobs = &scheduler.durable_pools["jobs"];
    let mut results = Vec::new();
    for _ in 0..500 {
        while let Some(entry) = jobs.consume(&inbox()).await? {
            results.push(entry.payload.unwrap_or_default());
        }
        if results.len() == 3 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    results.sort();
    println!("process 2 results: {results:?}");
    assert_eq!(results, ["done: job 1", "done: job 2", "done: job 3"]);

    let report = jobs.shutdown_with(ShutdownPolicy::default()).await;
    println!("process 2 shutdown: {report:?}");
    Ok(())
}
