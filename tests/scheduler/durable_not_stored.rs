//! S3: an enqueue that fails before anything is written is a refusal. The
//! Postgres queues (raw and pgmq) report such failures as
//! `SchedulerError::NotStored`, and the durable pool emits `Rejected` for
//! them, so the task's events end. Only a failure whose outcome is unknown (a
//! lost commit) is left without a terminal event.
//!
//! Two failures before any write, on each backend: a payload that cannot be
//! serialised, and a connection pool that is closed (no transaction can be
//! opened). Tests need Docker and are `#[ignore]`d.

use std::time::Duration;

use async_trait::async_trait;
use prometheus_parking_lot::core::{
    LeaseQueue, ScheduledTask, SchedulerError, TaskError, TaskExecutor, TaskMetadata,
};
use prometheus_parking_lot::infra::PgPool;
use prometheus_parking_lot::infra::mailbox::InMemoryResultMailbox;
use prometheus_parking_lot::infra::queue::{PgLeaseQueue, PgmqLeaseQueue};
use prometheus_parking_lot::{
    DurableOptions, DurableResourcePool, LifecycleEvent, RejectionReason,
};
use serde::de::{Deserializer, IgnoredAny};
use serde::ser::{Error as _, Serializer};
use serde::{Deserialize, Serialize};

use crate::support::pg::{Pg18, connect};
use crate::support::{event_log, limits, spawner, task};

/// A payload that serialises as its string, or refuses to serialise at all.
#[derive(Debug, Clone)]
enum Payload {
    Text(String),
    Refuse,
}

impl Serialize for Payload {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Text(text) => serializer.serialize_str(text),
            Self::Refuse => Err(S::Error::custom("this payload refuses to be serialised")),
        }
    }
}

impl<'de> Deserialize<'de> for Payload {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        IgnoredAny::deserialize(deserializer)?;
        Ok(Self::Text(String::new()))
    }
}

/// Never reached: nothing is stored, so nothing is leased.
#[derive(Clone)]
struct Unreachable;

#[async_trait]
impl TaskExecutor<Payload, String> for Unreachable {
    async fn execute(&self, _payload: Payload, _meta: TaskMetadata) -> Result<String, TaskError> {
        panic!("a task that was never stored ran");
    }
}

fn with_payload(id: u64, payload: Payload) -> ScheduledTask<Payload> {
    let ScheduledTask { mut meta, .. } = task("not-stored", "unused");
    meta.id = id;
    ScheduledTask { meta, payload }
}

/// Submit an unserialisable task, then close `pg` and submit a valid one:
/// both fail with `NotStored`, emit `Submitted` then `Rejected`
/// (`NotStored`), and leave the queue empty.
async fn refused_before_any_write<Q>(queue: Q, pg: &PgPool, backend: &str)
where
    Q: LeaseQueue<Payload> + 'static,
{
    let (bus, mut log) = event_log();
    let pool = DurableResourcePool::start(
        limits(1),
        queue,
        InMemoryResultMailbox::<String>::new(),
        Unreachable,
        spawner(),
        DurableOptions::new(Duration::from_secs(3))
            .with_name("not-stored")
            .with_hooks(bus.clone()),
    )
    .expect("start the pool");

    for (id, payload, operation) in [
        (1, Payload::Refuse, "serialize task payload"),
        (2, Payload::Text("valid".into()), "enqueue begin"),
    ] {
        if id == 2 {
            assert_eq!(
                pool.queue().depth().await.expect("depth").live(),
                0,
                "{backend}: the unserialisable task was stored"
            );
            // No transaction can be opened from here on.
            pg.close().await;
        }
        let error = pool
            .submit(with_payload(id, payload))
            .await
            .expect_err("nothing can be stored");
        assert!(
            matches!(&error, SchedulerError::NotStored { context, .. } if context.ends_with(operation)),
            "{backend}: task {id}: {error:?}"
        );
        let rejected = log
            .wait_for(
                |event| matches!(event, LifecycleEvent::Rejected { task_id, .. } if *task_id == id),
            )
            .await;
        assert!(
            matches!(&rejected, Some(LifecycleEvent::Rejected { reason: RejectionReason::NotStored(text), .. }) if text.contains(operation)),
            "{backend}: task {id}: {rejected:?}"
        );
    }
    assert!(bus.flush(Duration::from_secs(5)), "events delivered");
    log.drain_ready();
    for id in [1, 2] {
        assert_eq!(
            log.kinds_of(id),
            ["submitted", "rejected"],
            "{backend}: task {id}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs Docker: starts a parking-lot-pg container (PARKING_LOT_PG_MAJOR, default 18)"]
async fn durable_pg_enqueue_failures_before_any_write_are_rejected() {
    let pg = Pg18::start().await;

    let pool = connect(&pg.url).await;
    PgLeaseQueue::<Payload>::migrate(&pool)
        .await
        .expect("migrate queue");
    refused_before_any_write(
        PgLeaseQueue::<Payload>::new(pool.clone(), "not_stored", 100),
        &pool,
        "pg",
    )
    .await;

    let pool = connect(&pg.url).await;
    let pgmq = PgmqLeaseQueue::<Payload>::new(pool.clone(), "not_stored", 100).expect("pgmq queue");
    pgmq.prepare().await.expect("prepare pgmq");
    refused_before_any_write(pgmq, &pool, "pgmq").await;
}
