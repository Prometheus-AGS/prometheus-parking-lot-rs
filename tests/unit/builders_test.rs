//! Building pools and backends from configuration (cr-37, scenario S-22).
//!
//! Every test is named `config_*` so the S-22 entry point selects them. The
//! documented example configs are built through the real path: in memory, redb
//! in a temporary directory, and Postgres in the project's PG 18 image (that
//! test needs Docker and is `#[ignore]`d; run it with `--run-ignored all`).

use std::cell::Cell;
use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use prometheus_parking_lot::builders::{
    BuiltScheduler, build_pools, build_pools_from_scheduler_config,
};
use prometheus_parking_lot::config::{PoolConfig, SchedulerConfig, StorageConfig};
use prometheus_parking_lot::core::{
    ScheduledTask, SchedulerError, TaskError, TaskExecutor, TaskMetadata, TaskStatus,
};
use prometheus_parking_lot::infra::{InMemoryMailbox, InMemoryQueue};
use prometheus_parking_lot::runtime::TokioSpawner;
use prometheus_parking_lot::util::{MailboxKey, Priority, ResourceCost, ResourceKind, now_ms};

#[derive(Clone)]
struct Echo;

#[async_trait]
impl TaskExecutor<String, String> for Echo {
    async fn execute(&self, payload: String, _meta: TaskMetadata) -> Result<String, TaskError> {
        Ok(payload.to_uppercase())
    }
}

type Built = BuiltScheduler<String, String, Echo, TokioSpawner>;

fn spawner() -> TokioSpawner {
    TokioSpawner::new(tokio::runtime::Handle::current())
}

const fn pool_config(max_units: u32) -> PoolConfig {
    PoolConfig {
        max_units,
        max_queue_depth: 50,
        default_timeout_secs: 60,
        queue: StorageConfig::InMemory,
        mailbox: StorageConfig::InMemory,
    }
}

fn read_example(name: &str) -> SchedulerConfig {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples/configs")
        .join(name);
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    SchedulerConfig::from_json_str(&text)
        .unwrap_or_else(|e| panic!("{} does not load: {e}", path.display()))
}

fn task(id: u64, key: &MailboxKey, text: &str) -> ScheduledTask<String> {
    ScheduledTask {
        meta: TaskMetadata {
            id,
            mailbox: Some(key.clone()),
            priority: Priority::Normal,
            cost: ResourceCost::new(ResourceKind::Cpu, 1),
            deadline_ms: None,
            created_at_ms: now_ms(),
            idempotency_key: None,
            sequence: 0,
        },
        payload: text.to_owned(),
    }
}

/// Submit one task to every built pool and collect its outcome from the
/// pool's mailbox: the configured pools really run work and deliver results.
async fn run_one_task_per_pool(built: &Built) {
    for (id, (name, configured)) in (1..).zip(&built.pools) {
        let key = MailboxKey {
            tenant: "config-test".into(),
            user_id: None,
            session_id: Some(name.clone()),
        };
        configured
            .pool
            .submit(task(id, &key, name))
            .await
            .unwrap_or_else(|e| panic!("submit to pool `{name}`: {e}"));
        let mut outcome = None;
        for _ in 0..500 {
            outcome = configured.mailbox.take(&key);
            if outcome.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let (status, value) = outcome.unwrap_or_else(|| panic!("no outcome from pool `{name}`"));
        assert_eq!(status, TaskStatus::Completed, "pool `{name}`");
        assert_eq!(value, Some(name.to_uppercase()), "pool `{name}`");
    }
}

// ── build_pools (caller-supplied factories) ─────────────────────────────────

#[tokio::test]
async fn config_build_pools_builds_one_pool_per_configured_name() {
    let pools = BTreeMap::from([
        ("fast".to_string(), pool_config(10)),
        ("slow".to_string(), pool_config(2)),
    ]);
    let cfg = SchedulerConfig {
        backends: BTreeMap::new(),
        pools,
    };

    let built = build_pools::<String, String, _, _, _, _, _, _, _>(
        &cfg,
        |_, c| Ok(InMemoryQueue::<String>::new(c.max_queue_depth)),
        |_, _| Ok(InMemoryMailbox::<String>::new()),
        |_, _| Ok(Echo),
        spawner(),
    )
    .expect("valid configuration builds");

    let names: Vec<_> = built.keys().cloned().collect();
    assert_eq!(names, ["fast", "slow"]);
}

#[tokio::test]
async fn config_build_pools_rejects_invalid_config_before_calling_any_factory() {
    let cfg = SchedulerConfig {
        backends: BTreeMap::new(),
        pools: BTreeMap::from([("broken".to_string(), pool_config(0))]),
    };

    let queue_calls = Cell::new(0_u32);
    let result = build_pools::<String, String, _, _, _, _, _, _, _>(
        &cfg,
        |_, c| {
            queue_calls.set(queue_calls.get() + 1);
            Ok(InMemoryQueue::<String>::new(c.max_queue_depth))
        },
        |_, _| Ok(InMemoryMailbox::<String>::new()),
        |_, _| Ok(Echo),
        spawner(),
    );

    let Err(SchedulerError::InvalidConfig { message }) = result else {
        panic!("expected InvalidConfig");
    };
    assert!(message.contains("pools.broken.max_units"), "{message}");
    assert_eq!(queue_calls.get(), 0, "validation runs before any factory");
}

// ── build_pools_from_scheduler_config: the documented examples ──────────────

#[tokio::test]
async fn config_in_memory_example_builds_pools_that_run_tasks() {
    let cfg = read_example("in-memory.json");
    let built: Built = build_pools_from_scheduler_config(&cfg, |_, _| Ok(Echo), spawner())
        .await
        .expect("in-memory example builds");

    let names: Vec<_> = built.pools.keys().cloned().collect();
    assert_eq!(names, ["embeddings", "llm_inference"]);
    run_one_task_per_pool(&built).await;
}

#[cfg(feature = "embedded")]
#[tokio::test]
async fn config_redb_example_builds_its_pools_and_opens_the_store() {
    use prometheus_parking_lot::config::BackendConfig;
    use prometheus_parking_lot::core::{Enqueued, LeaseQueue};

    let dir = tempfile::tempdir().expect("tempdir");
    let mut cfg = read_example("redb.json");
    let file = dir.path().join("nested/parking-lot.redb");
    let Some(BackendConfig::Redb(section)) = cfg.backends.get_mut("local") else {
        panic!("redb.json has a redb backend named `local`");
    };
    section.path.clone_from(&file);

    let built: Built = build_pools_from_scheduler_config(&cfg, |_, _| Ok(Echo), spawner())
        .await
        .expect("redb example builds");
    assert!(file.exists(), "the store file is created");
    run_one_task_per_pool(&built).await;

    // The opened backend carries the section's parameters and works.
    let local = &built.redb["local"];
    assert_eq!(local.lease_for, Duration::from_secs(300));
    assert_eq!(local.queue.max_depth(), 1000);
    let key = MailboxKey {
        tenant: "config-test".into(),
        user_id: None,
        session_id: None,
    };
    let enqueued = local
        .queue
        .enqueue(task(1, &key, "durable"))
        .await
        .expect("enqueue");
    assert!(matches!(enqueued, Enqueued::Accepted(_)));
    let leased = local
        .queue
        .lease(1, local.lease_for)
        .await
        .expect("lease")
        .pop()
        .expect("one task ready");
    assert_eq!(leased.task.payload, "durable");
    local.queue.ack(leased.token).await.expect("ack");
}

#[cfg(feature = "embedded")]
#[tokio::test]
async fn config_invalid_config_opens_no_backend() {
    use prometheus_parking_lot::config::BackendConfig;

    let dir = tempfile::tempdir().expect("tempdir");
    let mut cfg = read_example("redb.json");
    let file = dir.path().join("never.redb");
    if let Some(BackendConfig::Redb(section)) = cfg.backends.get_mut("local") {
        section.path.clone_from(&file);
    }
    if let Some(pool) = cfg.pools.values_mut().next() {
        pool.max_units = 0;
    }

    let result: Result<Built, _> =
        build_pools_from_scheduler_config(&cfg, |_, _| Ok(Echo), spawner()).await;
    assert!(matches!(result, Err(SchedulerError::InvalidConfig { .. })));
    assert!(!file.exists(), "nothing is opened for an invalid config");
}

#[cfg(feature = "postgres")]
mod postgres {
    use std::process::Command;
    use std::time::Duration;

    use prometheus_parking_lot::config::BackendConfig;
    use prometheus_parking_lot::core::{Enqueued, LeaseQueue};
    use prometheus_parking_lot::util::MailboxKey;
    use testcontainers::core::{ContainerPort, WaitFor};
    use testcontainers::runners::AsyncRunner;
    use testcontainers::{ContainerAsync, GenericImage, ImageExt};

    use super::{Built, Echo, build_pools_from_scheduler_config, read_example, spawner, task};

    /// Postgres major version of the project image (`docker/postgres18`).
    const PG_MAJOR: &str = "18";
    /// Test-only superuser password configured in the container.
    const PG_PASSWORD: &str = "postgres";

    /// Build the `parking-lot-pg:18` image unless it exists (as the Postgres
    /// crate's test support does), then start a container.
    async fn start_pg18() -> (ContainerAsync<GenericImage>, String) {
        let tag = format!("parking-lot-pg:{PG_MAJOR}");
        let present = Command::new("docker")
            .args(["image", "inspect", &tag])
            .output()
            .expect("run docker")
            .status
            .success();
        if !present {
            let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
            let status = Command::new("docker")
                .arg("build")
                .arg("-f")
                .arg(root.join(format!("docker/postgres{PG_MAJOR}/Dockerfile")))
                .args(["-t", &tag])
                .arg(root.join("docker"))
                .status()
                .expect("run docker build");
            assert!(status.success(), "docker build of {tag} failed");
        }
        let container = GenericImage::new("parking-lot-pg", PG_MAJOR)
            .with_exposed_port(ContainerPort::Tcp(5432))
            .with_wait_for(WaitFor::message_on_stderr(
                "database system is ready to accept connections",
            ))
            .with_env_var("POSTGRES_PASSWORD", PG_PASSWORD)
            .start()
            .await
            .expect("start the parking-lot-pg container");
        let host = container.get_host().await.expect("host");
        let port = container.get_host_port_ipv4(5432).await.expect("port");
        let url = format!("postgres://postgres:{PG_PASSWORD}@{host}:{port}/postgres");
        (container, url)
    }

    #[tokio::test]
    #[ignore = "needs Docker: starts a parking-lot-pg:18 container; run with --run-ignored all"]
    async fn config_postgres_example_builds_against_pg18() {
        let (_container, url) = start_pg18().await;
        let mut cfg = read_example("postgres.json");
        let Some(BackendConfig::Postgres(section)) = cfg.backends.get_mut("cloud") else {
            panic!("postgres.json has a postgres backend named `cloud`");
        };
        section.url = url;

        // The image logs "ready" for its temporary init server too, so retry
        // until the real server accepts the connection.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
        let built: Built = loop {
            match build_pools_from_scheduler_config(&cfg, |_, _| Ok(Echo), spawner()).await {
                Ok(built) => break built,
                Err(err) if tokio::time::Instant::now() >= deadline => {
                    panic!("postgres example does not build: {err}")
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(250)).await,
            }
        };
        super::run_one_task_per_pool(&built).await;

        // The opened backend carries the section's parameters and works:
        // `migrate` created the schema, `listen` opened a listener.
        let cloud = &built.postgres["cloud"];
        assert_eq!(cloud.queue.queue_name(), "inference");
        assert_eq!(cloud.queue.max_depth(), 10_000);
        assert_eq!(cloud.lease_for, Duration::from_secs(300));
        assert!(cloud.listener.is_some(), "listen: true opens a listener");
        let key = MailboxKey {
            tenant: "config-test".into(),
            user_id: None,
            session_id: None,
        };
        let enqueued = cloud
            .queue
            .enqueue(task(1, &key, "durable"))
            .await
            .expect("enqueue");
        assert!(matches!(enqueued, Enqueued::Accepted(_)));
        let leased = cloud
            .queue
            .lease(1, cloud.lease_for)
            .await
            .expect("lease")
            .pop()
            .expect("one task ready");
        assert_eq!(leased.task.payload, "durable");
        cloud.queue.ack(leased.token).await.expect("ack");
    }
}
