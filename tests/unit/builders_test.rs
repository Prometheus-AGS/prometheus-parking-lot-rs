//! Building pools and backends from configuration (cr-37, scenario S-22).
//!
//! Every test is named `config_*` so the S-22 entry point selects them. The
//! documented example configs are built through the real path: in memory, redb
//! in a temporary directory, and Postgres and pgmq in the project's PG 18 image
//! (or the `PARKING_LOT_PG_MAJOR` image; those tests need Docker and are
//! `#[ignore]`d; run them with `--run-ignored all`).

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
        max_head_skips: 8,
        resource_limits: BTreeMap::new(),
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

/// Submit one task to every built durable pool and consume its outcome
/// through `consume`, which reads the backend's result mailbox.
#[cfg(any(feature = "postgres", feature = "embedded"))]
async fn run_one_task_per_durable_pool<C, F>(built: &Built, consume: C)
where
    C: Fn(MailboxKey) -> F,
    F: std::future::Future<Output = Option<(TaskStatus, Option<String>)>>,
{
    assert!(!built.durable_pools.is_empty(), "no durable pool was built");
    for (id, (name, pool)) in (1..).zip(&built.durable_pools) {
        let key = MailboxKey {
            tenant: "config-test".into(),
            user_id: None,
            session_id: Some(name.clone()),
        };
        let accepted = pool
            .submit(task(id, &key, name))
            .await
            .unwrap_or_else(|e| panic!("submit to durable pool `{name}`: {e}"));
        assert_eq!(accepted, TaskStatus::Queued, "accepted once persisted");
        let mut outcome = None;
        for _ in 0..1_000 {
            outcome = consume(key.clone()).await;
            if outcome.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let (status, value) =
            outcome.unwrap_or_else(|| panic!("no outcome from durable pool `{name}`"));
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

/// cr-23: a pool built from a `pools` section with `resource_limits` admits
/// by every dimension: a task over a kind limit is rejected, and one short in
/// one dimension waits.
#[tokio::test]
async fn config_build_pools_applies_resource_limits() {
    let mut section = pool_config(100);
    section.resource_limits.insert(ResourceKind::Cpu, 1);
    let cfg = SchedulerConfig {
        backends: BTreeMap::new(),
        pools: BTreeMap::from([("gpu".to_string(), section)]),
    };
    let built = build_pools::<String, String, _, _, _, _, _, _, _>(
        &cfg,
        |_, c| Ok(InMemoryQueue::<String>::new(c.max_queue_depth)),
        |_, _| Ok(InMemoryMailbox::<String>::new()),
        |_, _| Ok(Echo),
        spawner(),
    )
    .expect("valid configuration builds");
    let pool = &built["gpu"];
    assert_eq!(pool.active_units_of(ResourceKind::Cpu), Some(0));
    assert_eq!(pool.active_units_of(ResourceKind::GpuVram), None);
    let task = |id: u64, slots: u32| ScheduledTask {
        meta: TaskMetadata {
            id,
            mailbox: None,
            priority: Priority::Normal,
            cost: ResourceCost::new(ResourceKind::GpuVram, 8).with(ResourceKind::Cpu, slots),
            deadline_ms: None,
            created_at_ms: 0,
            idempotency_key: None,
            sequence: 0,
        },
        payload: String::from("x"),
    };
    let error = pool
        .submit(task(1, 2))
        .await
        .expect_err("two slots, limit one");
    assert!(
        matches!(
            error,
            SchedulerError::CapacityExceeded {
                requested: 2,
                available: 1,
                ..
            }
        ),
        "{error:?}"
    );
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

#[cfg(feature = "embedded")]
#[tokio::test]
async fn config_build_pools_leaves_durable_pools_to_the_config_builder() {
    let cfg = read_example("redb.json");
    let result = build_pools::<String, String, _, _, _, _, _, _, _>(
        &cfg,
        |_, c| Ok(InMemoryQueue::<String>::new(c.max_queue_depth)),
        |_, _| Ok(InMemoryMailbox::<String>::new()),
        |_, _| Ok(Echo),
        spawner(),
    );
    let Err(SchedulerError::InvalidConfig { message }) = result else {
        panic!("a pool on a durable backend must not be built on in-memory factories");
    };
    assert!(
        message.contains("build_pools_from_scheduler_config"),
        "{message}"
    );
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
async fn config_redb_example_builds_a_durable_pool_on_the_store() {
    use prometheus_parking_lot::config::BackendConfig;
    use prometheus_parking_lot::core::{LeaseQueue, ResultMailbox};

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
    assert!(built.pools.is_empty(), "the example's pool is durable");
    let names: Vec<_> = built.durable_pools.keys().cloned().collect();
    assert_eq!(names, ["tauri_local_llm"]);

    // The pool persists, runs and delivers into the configured store: the
    // outcome is consumed from the opened backend's own mailbox handle.
    let local = &built.redb["local"];
    assert_eq!(local.lease_for, Duration::from_secs(300));
    assert_eq!(local.queue.max_depth(), 1000);
    run_one_task_per_durable_pool(&built, |key| async move {
        local
            .mailbox
            .consume(&key)
            .await
            .expect("consume")
            .map(|entry| (entry.status, entry.payload))
    })
    .await;
    // The ack follows the delivery; give it a moment to land.
    let mut live = usize::MAX;
    for _ in 0..200 {
        live = local.queue.depth().await.expect("depth").live();
        if live == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(live, 0, "the pool acked its task");
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
    use prometheus_parking_lot::core::{Enqueued, LeaseQueue, ResultMailbox};
    use prometheus_parking_lot::util::MailboxKey;
    use testcontainers::core::{ContainerPort, WaitFor};
    use testcontainers::runners::AsyncRunner;
    use testcontainers::{ContainerAsync, GenericImage, ImageExt};

    use prometheus_parking_lot::config::SchedulerConfig;

    use super::{Built, Echo, build_pools_from_scheduler_config, read_example, spawner, task};

    /// Postgres major version of the project image under test: 18, or the
    /// `PARKING_LOT_PG_MAJOR` the CI matrix sets (`docker/postgres17`, `docker/postgres18`).
    fn pg_major() -> String {
        std::env::var("PARKING_LOT_PG_MAJOR").unwrap_or_else(|_| "18".to_owned())
    }
    /// Test-only superuser password configured in the container.
    const PG_PASSWORD: &str = "postgres";

    /// Build the `parking-lot-pg:18` image unless it exists (as the Postgres
    /// crate's test support does), then start a container.
    async fn start_pg18() -> (ContainerAsync<GenericImage>, String) {
        let major = pg_major();
        let tag = format!("parking-lot-pg:{major}");
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
                .arg(root.join(format!("docker/postgres{major}/Dockerfile")))
                .args(["-t", &tag])
                .arg(root.join("docker"))
                .status()
                .expect("run docker build");
            assert!(status.success(), "docker build of {tag} failed");
        }
        let container = GenericImage::new("parking-lot-pg", &major)
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

    /// Build `cfg` against the container. The image logs "ready" for its
    /// temporary init server too, so retry until the real server accepts the
    /// connection.
    async fn build_when_ready(cfg: &SchedulerConfig) -> Built {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
        loop {
            match build_pools_from_scheduler_config(cfg, |_, _| Ok(Echo), spawner()).await {
                Ok(built) => return built,
                Err(err) if tokio::time::Instant::now() >= deadline => {
                    panic!("example does not build: {err}")
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(250)).await,
            }
        }
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

        let built = build_when_ready(&cfg).await;
        assert!(built.pools.is_empty(), "the example's pool is durable");
        let names: Vec<_> = built.durable_pools.keys().cloned().collect();
        assert_eq!(names, ["llm_inference"]);

        // The opened backend carries the section's parameters: `migrate`
        // created both schemas, `listen` opened a listener.
        let cloud = &built.postgres["cloud"];
        assert_eq!(cloud.queue.queue_name(), "inference");
        assert_eq!(cloud.queue.max_depth(), 10_000);
        assert_eq!(cloud.lease_for, Duration::from_secs(300));
        assert!(cloud.listener.is_some(), "listen: true opens a listener");
        assert_eq!(cloud.mailbox.name(), "inference");

        // The pool persists, runs and delivers into the configured database:
        // the outcome is consumed from the opened backend's mailbox handle,
        // exactly once, and the queue is drained.
        super::run_one_task_per_durable_pool(&built, |key| async move {
            cloud
                .mailbox
                .consume(&key)
                .await
                .expect("consume")
                .map(|entry| (entry.status, entry.payload))
        })
        .await;
        let key = MailboxKey {
            tenant: "config-test".into(),
            user_id: None,
            session_id: Some("llm_inference".to_owned()),
        };
        assert!(
            cloud
                .mailbox
                .consume(&key)
                .await
                .expect("consume again")
                .is_none(),
            "an outcome is consumed once"
        );
        // The ack follows the delivery; give it a moment to land.
        let mut live = usize::MAX;
        for _ in 0..200 {
            live = cloud.queue.depth().await.expect("depth").live();
            if live == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(live, 0, "the pool acked its task");
    }

    /// The pgmq example opens a prepared pgmq queue (and its listener) next to
    /// an in-memory pool, and the queue round-trips a task through the real
    /// extension: enqueue, lease for the configured `lease_secs`, ack.
    #[tokio::test]
    #[ignore = "needs Docker: starts a parking-lot-pg:18 container; run with --run-ignored all"]
    async fn config_pgmq_example_opens_the_queue_against_pg18() {
        let (_container, url) = start_pg18().await;
        let mut cfg = read_example("pgmq.json");
        let Some(BackendConfig::Pgmq(section)) = cfg.backends.get_mut("events") else {
            panic!("pgmq.json has a pgmq backend named `events`");
        };
        section.url = url;

        let built = build_when_ready(&cfg).await;
        assert!(built.durable_pools.is_empty(), "no pool runs on pgmq");
        let names: Vec<_> = built.pools.keys().cloned().collect();
        assert_eq!(names, ["llm_inference"]);
        super::run_one_task_per_pool(&built).await;

        let events = &built.pgmq["events"];
        assert_eq!(events.queue.queue_name(), "events");
        assert_eq!(events.queue.max_depth(), 10_000);
        assert_eq!(events.lease_for, Duration::from_secs(30));
        assert!(events.listener.is_some(), "listen: true opens a listener");

        // `prepare: true` created the pgmq queues: a task round-trips.
        let key = MailboxKey {
            tenant: "config-test".into(),
            user_id: None,
            session_id: Some("events".to_owned()),
        };
        let enqueued = events
            .queue
            .enqueue(task(7, &key, "hello"))
            .await
            .expect("enqueue on the prepared pgmq queue");
        assert!(matches!(enqueued, Enqueued::Accepted(_)), "{enqueued:?}");
        let leased = events
            .queue
            .lease(4, events.lease_for)
            .await
            .expect("lease");
        let [leased] = <[_; 1]>::try_from(leased)
            .unwrap_or_else(|all| panic!("expected one leased task, got {}", all.len()));
        assert_eq!(leased.task.meta.id, 7);
        assert_eq!(leased.task.payload, "hello");
        assert_eq!(leased.attempt, 1);
        assert!(
            events
                .queue
                .lease(4, events.lease_for)
                .await
                .expect("lease again")
                .is_empty(),
            "a leased task is invisible until acked or expired"
        );
        events.queue.ack(leased.token).await.expect("ack");
        assert_eq!(events.queue.depth().await.expect("depth").live(), 0);
    }
}
