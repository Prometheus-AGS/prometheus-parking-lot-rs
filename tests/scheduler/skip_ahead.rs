//! Skip-ahead (cr-20) through the facade: the `max_head_skips` config field
//! and a `ResourcePool` built from configuration.
//!
//! Tests named `config_*` check the schema (cr-37's `-E "test(config_)"`
//! selects them); `starvation_*` ones drive a configured pool through the real
//! path (config → builder → `ResourcePool` → core dispatcher behind the
//! `TaskQueueAdapter`).

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::Mutex;
use prometheus_parking_lot::builders::{BuiltScheduler, build_pools_from_scheduler_config};
use prometheus_parking_lot::config::{PoolConfig, SchedulerConfig, WorkerPoolConfig};
use prometheus_parking_lot::core::{ScheduledTask, TaskError, TaskExecutor, TaskMetadata};
use prometheus_parking_lot::runtime::TokioSpawner;
use prometheus_parking_lot::util::{MailboxKey, Priority, ResourceCost, ResourceKind, now_ms};
use tokio::sync::Notify;

const POOL: &str = r#"{ "max_units": 4, "max_queue_depth": 50,
    "default_timeout_secs": 60, "queue": "in_memory", "mailbox": "in_memory" }"#;

#[test]
fn config_max_head_skips_defaults_to_the_dispatcher_default() {
    let pool: PoolConfig = serde_json::from_str(POOL).expect("pool config without the field");
    assert_eq!(pool.max_head_skips, 8);
    let worker: WorkerPoolConfig = serde_json::from_str("{}").expect("empty worker config");
    assert_eq!(worker.max_head_skips, 8);
    assert_eq!(WorkerPoolConfig::default().max_head_skips, 8);
}

#[test]
fn config_max_head_skips_loads_and_zero_means_strict_order() {
    let pool: PoolConfig = serde_json::from_str(
        r#"{ "max_units": 4, "max_queue_depth": 50, "default_timeout_secs": 60,
             "queue": "in_memory", "mailbox": "in_memory", "max_head_skips": 0 }"#,
    )
    .expect("pool config with the field");
    assert_eq!(pool.max_head_skips, 0);
    assert!(pool.validate().is_ok());
    let worker: WorkerPoolConfig =
        serde_json::from_str(r#"{ "max_head_skips": 3 }"#).expect("worker config");
    assert_eq!(worker.max_head_skips, 3);
    assert_eq!(
        WorkerPoolConfig::new()
            .with_max_head_skips(5)
            .max_head_skips,
        5
    );
}

#[test]
fn config_misspelled_max_head_skips_is_rejected_naming_it() {
    let err = serde_json::from_str::<PoolConfig>(
        r#"{ "max_units": 4, "max_queue_depth": 50, "default_timeout_secs": 60,
             "queue": "in_memory", "mailbox": "in_memory", "max_head_skip": 2 }"#,
    )
    .expect_err("misspelled field");
    assert!(
        err.to_string().contains("unknown field `max_head_skip`"),
        "{err}"
    );
    let err = serde_json::from_str::<WorkerPoolConfig>(r#"{ "max_head_skip": 2 }"#)
        .expect_err("misspelled field");
    assert!(
        err.to_string().contains("unknown field `max_head_skip`"),
        "{err}"
    );
}

/// Records the order in which tasks start; `blocker` waits for a gate.
#[derive(Clone)]
struct Recorder {
    started: Arc<Mutex<Vec<String>>>,
    gate: Arc<Notify>,
}

#[async_trait]
impl TaskExecutor<String, String> for Recorder {
    async fn execute(&self, payload: String, _meta: TaskMetadata) -> Result<String, TaskError> {
        self.started.lock().push(payload.clone());
        if payload == "blocker" {
            self.gate.notified().await;
        }
        Ok(payload)
    }
}

fn key(name: &str) -> MailboxKey {
    MailboxKey {
        tenant: "skip-ahead".into(),
        user_id: None,
        session_id: Some(name.to_owned()),
    }
}

fn task(id: u64, name: &str, priority: Priority, units: u32) -> ScheduledTask<String> {
    ScheduledTask {
        meta: TaskMetadata {
            id,
            mailbox: Some(key(name)),
            priority,
            cost: ResourceCost::new(ResourceKind::Cpu, units),
            deadline_ms: None,
            created_at_ms: now_ms(),
            idempotency_key: None,
            sequence: 0,
        },
        payload: name.to_owned(),
    }
}

/// A configured pool with `max_head_skips: 2`: small tasks start beside a
/// running blocker while a four-unit head waits, then the guard reserves the
/// capacity, and the head starts as soon as the blocker finishes, before the
/// small task the guard held back.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn starvation_configured_resource_pool_skips_ahead_within_the_bound() {
    let cfg = SchedulerConfig::from_json_str(
        r#"{ "pools": { "gpu": { "max_units": 4, "max_queue_depth": 50,
             "default_timeout_secs": 60, "queue": "in_memory", "mailbox": "in_memory",
             "max_head_skips": 2 } } }"#,
    )
    .expect("config");
    let recorder = Recorder {
        started: Arc::new(Mutex::new(Vec::new())),
        gate: Arc::new(Notify::new()),
    };
    let factory_recorder = recorder.clone();
    let built: BuiltScheduler<String, String, Recorder, TokioSpawner> =
        build_pools_from_scheduler_config(
            &cfg,
            move |_, _| Ok(factory_recorder.clone()),
            TokioSpawner::new(tokio::runtime::Handle::current()),
        )
        .await
        .expect("build");
    let configured = &built.pools["gpu"];
    let pool = &configured.pool;

    pool.submit(task(1, "blocker", Priority::Normal, 2))
        .await
        .expect("blocker");
    pool.submit(task(2, "large", Priority::High, 4))
        .await
        .expect("large");
    for (id, name) in [(3, "small-1"), (4, "small-2"), (5, "small-3")] {
        pool.submit(task(id, name, Priority::Normal, 1))
            .await
            .expect("small");
    }
    // Two skips, then the guard holds small-3 back for the head.
    wait_until(|| configured.mailbox.take(&key("small-2")).is_some()).await;
    let stats = pool.skip_ahead_stats();
    assert_eq!((stats.skipped, stats.reservations), (2, 1), "{stats:?}");
    assert!(configured.mailbox.take(&key("small-3")).is_none());

    recorder.gate.notify_one();
    wait_until(|| configured.mailbox.take(&key("small-3")).is_some()).await;
    let order = recorder.started.lock().clone();
    // The first three run side by side on the runtime, so the order in which
    // their executors begin is not fixed; the head and small-3 are serialized
    // by capacity.
    let mut first_three = order[..3].to_vec();
    first_three.sort_unstable();
    assert_eq!(first_three, vec!["blocker", "small-1", "small-2"]);
    assert_eq!(
        &order[3..],
        &["large", "small-3"],
        "the head starts within two skips, before the held-back small task"
    );
}

async fn wait_until(mut done: impl FnMut() -> bool) {
    for _ in 0..500 {
        if done() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("condition not reached in 5 s");
}
