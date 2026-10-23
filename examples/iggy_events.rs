//! Lifecycle events and `mailbox.ready` notifications on Apache Iggy.
//!
//! A durable pool on the embedded redb store publishes every task's lifecycle
//! (`submitted`, `queued`, `started`, then one terminal event) to the
//! `lifecycle` topic through an `IggyHook`, and a `mailbox.ready` pointer to
//! the `mailbox.ready` topic after each result is stored, through an
//! `IggyNotifier` wrapped in a `StorageMailbox`. A gateway subscribed to
//! `mailbox.ready` tells a reconnecting client where its result is; the result
//! itself stays in the mailbox. Publishing never runs on the dispatch path.
//!
//! ## Setup
//!
//! 1. Start an Iggy 0.9 server (the version the tests pin):
//!    ```bash
//!    docker run -d --name iggy -p 8090:8090 \
//!      -e IGGY_ROOT_USERNAME=iggy -e IGGY_ROOT_PASSWORD=iggy \
//!      --cap-add SYS_NICE --security-opt seccomp=unconfined --ulimit memlock=-1:-1 \
//!      apache/iggy:0.9.0
//!    ```
//! 2. Run (`IGGY_URL` defaults to `iggy://iggy:iggy@127.0.0.1:8090`):
//!    ```bash
//!    cargo run --example iggy_events --features "embedded iggy"
//!    ```
//!
//! Guide: `website/docs/guides/iggy-events.md`.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use prometheus_parking_lot::config::BackendConfig;
use prometheus_parking_lot::core::{
    BackgroundNotifier, HookBus, PoolLimits, ResultMailbox, ScheduledTask, StorageMailbox,
    TaskError, TaskExecutor, TaskMetadata,
};
use prometheus_parking_lot::infra::EmbeddedStore;
use prometheus_parking_lot::infra::events::{
    IggyHook, IggyNotifier, TokioHandleSpawner, TokioSleeper,
};
use prometheus_parking_lot::runtime::TokioSpawner;
use prometheus_parking_lot::util::{MailboxKey, Priority, ResourceCost, ResourceKind, now_ms};
use prometheus_parking_lot::{DurableOptions, DurableResourcePool, ShutdownPolicy};

#[derive(Clone)]
struct Reply;

#[async_trait]
impl TaskExecutor<String, String> for Reply {
    async fn execute(&self, prompt: String, _meta: TaskMetadata) -> Result<String, TaskError> {
        Ok(format!("reply to '{prompt}'"))
    }
}

fn task(id: u64, to: &MailboxKey) -> ScheduledTask<String> {
    ScheduledTask {
        meta: TaskMetadata {
            id,
            mailbox: Some(to.clone()),
            priority: Priority::Normal,
            cost: ResourceCost::new(ResourceKind::Cpu, 1),
            deadline_ms: None,
            created_at_ms: now_ms(),
            // The Iggy message ids derive from this key, so a retried publish
            // carries the same id.
            idempotency_key: Some(format!("chat-{id}")),
            sequence: 0,
        },
        payload: format!("message {id}"),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let url =
        std::env::var("IGGY_URL").unwrap_or_else(|_| "iggy://iggy:iggy@127.0.0.1:8090".into());

    // The publisher settings come from an `"iggy"` backend section, the same
    // one a scheduler configuration file carries under `backends`.
    let section = serde_json::json!({
        "type": "iggy", "connection_string": url, "stream": "parking-lot-demo"
    });
    let section: BackendConfig = serde_json::from_value(section)?;
    section.validate("backends.events")?;
    let BackendConfig::Iggy(events) = &section else {
        return Err("not an iggy section".into());
    };
    let options = events.options();

    // Lifecycle events: a hook on the pool's bus.
    let hook = IggyHook::start(&options)?;
    let bus = HookBus::new();
    bus.register(Arc::new(hook.clone()));

    // Result notifications: store first, then publish `mailbox.ready`, off
    // the delivery path and with bounded retries.
    let notifier = IggyNotifier::start(&options)?;
    let dir = tempfile::tempdir()?;
    let store = EmbeddedStore::open(dir.path().join("parking-lot.redb"))?;
    let mailbox = StorageMailbox::new(
        store.result_mailbox::<String>(),
        BackgroundNotifier::new(
            notifier.clone(),
            TokioHandleSpawner::current(),
            TokioSleeper,
        ),
    );

    let pool = DurableResourcePool::start(
        PoolLimits {
            max_units: 2,
            max_queue_depth: 100,
            default_timeout: Duration::from_secs(30),
        },
        store.lease_queue::<String>(100),
        mailbox,
        Reply,
        TokioSpawner::new(tokio::runtime::Handle::current()),
        DurableOptions::new(Duration::from_secs(30))
            .with_name("chat")
            .with_hooks(bus),
    )?;

    let inbox = MailboxKey {
        tenant: "acme".into(),
        user_id: Some("ana".into()),
        session_id: Some("chat-1".into()),
    };
    for id in 1..=3 {
        pool.submit(task(id, &inbox)).await?;
    }
    let mut results = Vec::new();
    for _ in 0..500 {
        while let Some(entry) = pool.mailbox().consume(&inbox).await? {
            results.push(entry.payload.unwrap_or_default());
        }
        if results.len() == 3 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(results.len(), 3, "{results:?}");
    pool.shutdown_with(ShutdownPolicy::default()).await;

    // Wait for the publishers to drain, then read their counters.
    assert!(
        pool.hooks().flush(Duration::from_secs(5)),
        "hook bus drained"
    );
    assert!(
        hook.flush(Duration::from_secs(30)).await,
        "lifecycle events published"
    );
    assert!(
        notifier.flush(Duration::from_secs(30)).await,
        "notifications published"
    );
    let (events, ready) = (hook.stats(), notifier.stats());
    println!("lifecycle: {events:?}");
    println!("mailbox.ready: {ready:?}");
    assert_eq!(ready.published, 3, "one mailbox.ready per stored result");
    // Submitted, Started and Completed at least, for each of the three tasks.
    assert!(events.published >= 9, "{events:?}");
    assert_eq!(events.dropped + ready.dropped, 0);
    Ok(())
}
