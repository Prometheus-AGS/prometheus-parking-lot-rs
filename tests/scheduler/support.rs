//! Shared fixtures: an executor that counts its runs, task builders and
//! polling helpers.

#![allow(
    dead_code,
    reason = "shared by modules that are compiled per feature; each uses a subset"
)]

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use prometheus_parking_lot::core::{
    PoolLimits, ScheduledTask, TaskError, TaskExecutor, TaskMetadata,
};
use prometheus_parking_lot::runtime::TokioSpawner;
use prometheus_parking_lot::util::{MailboxKey, Priority, ResourceCost, ResourceKind, now_ms};

/// What an [`Executor`] does with a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Behaviour {
    /// Return `done:<payload>` at once.
    Succeed,
    /// Sleep this long, then return `done:<payload>`.
    SleepThenSucceed(Duration),
    /// Print `EXECUTING` to stdout and never return (the S-16 kill point).
    HangAfterAnnouncing,
}

/// An executor that counts its runs; payloads starting with `fail` return an
/// executor error and payloads starting with `panic` panic.
#[derive(Clone)]
pub struct Executor {
    pub behaviour: Behaviour,
    pub runs: Arc<AtomicU32>,
}

impl Executor {
    pub fn new(behaviour: Behaviour) -> Self {
        Self {
            behaviour,
            runs: Arc::new(AtomicU32::new(0)),
        }
    }

    pub fn runs(&self) -> u32 {
        self.runs.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl TaskExecutor<String, String> for Executor {
    async fn execute(&self, payload: String, _meta: TaskMetadata) -> Result<String, TaskError> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        match self.behaviour {
            Behaviour::Succeed => {}
            Behaviour::SleepThenSucceed(duration) => tokio::time::sleep(duration).await,
            Behaviour::HangAfterAnnouncing => {
                announce("EXECUTING");
                std::future::pending::<()>().await;
            }
        }
        if payload.starts_with("fail") {
            return Err(TaskError::executor(format!("{payload} failed")));
        }
        assert!(!payload.starts_with("panic"), "{payload} panicked");
        Ok(format!("done:{payload}"))
    }
}

/// Print a marker line for a parent process and flush it.
pub fn announce(marker: &str) {
    use std::io::Write as _;
    println!("{marker}");
    let _ = std::io::stdout().flush();
}

pub fn spawner() -> TokioSpawner {
    TokioSpawner::new(tokio::runtime::Handle::current())
}

pub const fn limits(max_units: u32) -> PoolLimits {
    PoolLimits {
        max_units,
        max_queue_depth: 100,
        default_timeout: Duration::from_secs(60),
    }
}

pub fn key(session: &str) -> MailboxKey {
    MailboxKey {
        tenant: "durable".into(),
        user_id: None,
        session_id: Some(session.to_owned()),
    }
}

/// A one-unit task whose outcome is delivered under `key(session)`.
pub fn task(session: &str, payload: &str) -> ScheduledTask<String> {
    ScheduledTask {
        meta: TaskMetadata {
            id: 1,
            mailbox: Some(key(session)),
            priority: Priority::Normal,
            cost: ResourceCost::new(ResourceKind::Cpu, 1),
            deadline_ms: None,
            created_at_ms: now_ms(),
            idempotency_key: None,
            sequence: 0,
        },
        payload: payload.to_owned(),
    }
}

/// Poll `probe` every 50 ms until it yields a value or `within` elapses.
pub async fn eventually<T, F, Fut>(within: Duration, mut probe: F) -> Option<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Option<T>>,
{
    let deadline = tokio::time::Instant::now() + within;
    loop {
        if let Some(value) = probe().await {
            return Some(value);
        }
        if tokio::time::Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Postgres 18 in the project image (`docker/postgres18`), built once per
/// machine as the Postgres crate's test support does.
#[cfg(feature = "postgres")]
pub mod pg {
    use std::process::Command;
    use std::time::Duration;

    use prometheus_parking_lot::infra::PgPool;
    use testcontainers::core::{ContainerPort, WaitFor};
    use testcontainers::runners::AsyncRunner;
    use testcontainers::{ContainerAsync, GenericImage, ImageExt};

    const PG_MAJOR: &str = "18";
    /// Test-only superuser password configured in the container.
    const PG_PASSWORD: &str = "postgres";

    /// A running container (removed on drop) and its connection URL.
    pub struct Pg18 {
        pub url: String,
        _container: ContainerAsync<GenericImage>,
    }

    impl Pg18 {
        pub async fn start() -> Self {
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
            Self {
                url: format!("postgres://postgres:{PG_PASSWORD}@{host}:{port}/postgres"),
                _container: container,
            }
        }
    }

    /// Connect, retrying while the server finishes starting (the image logs
    /// "ready" for its temporary init server too).
    pub async fn connect(url: &str) -> PgPool {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
        loop {
            match PgPool::connect(url).await {
                Ok(pool) => return pool,
                Err(err) if tokio::time::Instant::now() >= deadline => {
                    panic!("connect to {url}: {err}")
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(250)).await,
            }
        }
    }
}
