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
use prometheus_parking_lot::{Hook, HookBus, LifecycleEvent};
use tokio::sync::mpsc;

/// Upper bound on any wait for an observed condition: it only keeps a broken
/// test from hanging. A passing test proceeds as soon as the condition holds.
pub const FAILSAFE: Duration = Duration::from_secs(120);

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

/// Forwards every lifecycle event to a channel, so a test can wait for an
/// event instead of polling on a timer.
struct ChannelHook(mpsc::UnboundedSender<LifecycleEvent>);

impl Hook for ChannelHook {
    fn on_event(&self, event: &LifecycleEvent) {
        let _ = self.0.send(event.clone());
    }
}

/// The events a pool emitted on its bus, received in emission order.
pub struct EventLog {
    rx: mpsc::UnboundedReceiver<LifecycleEvent>,
    /// Every event received so far.
    pub seen: Vec<LifecycleEvent>,
}

/// A bus for a pool's `DurableOptions::with_hooks`, and the log its events
/// arrive in.
pub fn event_log() -> (HookBus, EventLog) {
    let (tx, rx) = mpsc::unbounded_channel();
    let bus = HookBus::new();
    bus.register(Arc::new(ChannelHook(tx)));
    (
        bus,
        EventLog {
            rx,
            seen: Vec::new(),
        },
    )
}

impl EventLog {
    /// Wait for the next event matching `wanted` (recording every event on
    /// the way); `None` only if [`FAILSAFE`] elapses first.
    pub async fn wait_for(
        &mut self,
        wanted: impl Fn(&LifecycleEvent) -> bool,
    ) -> Option<LifecycleEvent> {
        let deadline = tokio::time::Instant::now() + FAILSAFE;
        loop {
            let event = tokio::time::timeout_at(deadline, self.rx.recv())
                .await
                .ok()??;
            self.seen.push(event.clone());
            if wanted(&event) {
                return Some(event);
            }
        }
    }

    /// Record every event already delivered (call after `HookBus::flush`).
    pub fn drain_ready(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            self.seen.push(event);
        }
    }

    /// The recorded events of task `task_id`, by kind.
    pub fn kinds_of(&self, task_id: u64) -> Vec<&'static str> {
        self.seen
            .iter()
            .filter(|event| event.task_id() == task_id)
            .map(LifecycleEvent::kind)
            .collect()
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

    /// Postgres major under test: `PARKING_LOT_PG_MAJOR` (the CI matrix sets 17 or
    /// 18), default 18.
    fn pg_major() -> String {
        std::env::var("PARKING_LOT_PG_MAJOR").unwrap_or_else(|_| "18".to_owned())
    }
    /// Test-only superuser password configured in the container.
    const PG_PASSWORD: &str = "postgres";

    /// A running container (removed on drop) and its connection URL.
    pub struct Pg18 {
        pub url: String,
        _container: ContainerAsync<GenericImage>,
    }

    impl Pg18 {
        pub async fn start() -> Self {
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
            let container = GenericImage::new("parking-lot-pg", major.as_str())
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
