//! Real-Postgres tests for [`PgResultMailbox`] (cr-27) on the PG 18 project
//! image.
//!
//! Every test needs Docker, so each is `#[ignore]`d and compiles in CI without
//! it. Run them with:
//!
//! ```text
//! cargo nextest run -p prometheus-parking-lot-pg --locked --run-ignored all -E "test(mailbox_)"
//! ```
//!
//! - the shared backend conformance suite (ADR 0003), one test per case, in
//!   `mailbox_conformance`;
//! - `mailbox_consume_once_race` (scenario S-7): two consumers released by a
//!   barrier take the same single entry; exactly one receives it;
//! - `mailbox_concurrent_drain`: eight consumers drain 300 entries, each
//!   delivered to exactly one of them;
//! - `mailbox_first_write_wins_race`: sixteen concurrent deliveries under one
//!   idempotency key store exactly one entry;
//! - TTL: expired entries are never returned, are removed from the table by
//!   eviction and by the background sweeper, and release their key.
//!
//! Negative control (`RUSTFLAGS='--cfg parking_lot_negative_control="s-7"'`,
//! run with nextest): consume selects and deletes in two statements, with a
//! barrier between them, so `mailbox_consume_once_race` observes double
//! delivery and fails.

mod support;

use std::collections::BTreeSet;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use prometheus_parking_lot_core::conformance::MailboxHarness;
use prometheus_parking_lot_core::{
    Delivery, DeliveryOutcome, MailboxKey, ResultMailbox, TaskStatus, result_mailbox_conformance,
};
use prometheus_parking_lot_pg::PgResultMailbox;
use support::{PgContainer, TestResult};
use tokio::sync::Barrier;

/// Postgres major version under test. The 17/18 matrix is cr-36.
const PG_MAJOR: u8 = 18;

/// Tests that need Docker compile everywhere and run only when asked.
macro_rules! docker_test {
    ($(#[$meta:meta])* async fn $name:ident() $body:block) => {
        $(#[$meta])*
        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        #[ignore = "needs Docker: starts a parking-lot-pg container"]
        async fn $name() $body
    };
}

fn key(tenant: &str) -> MailboxKey {
    MailboxKey {
        tenant: tenant.to_owned(),
        user_id: Some("user".to_owned()),
        session_id: None,
    }
}

fn completed(key: &MailboxKey, task_id: u64, payload: &str) -> Delivery<String> {
    Delivery::new(
        key.clone(),
        task_id,
        TaskStatus::Completed,
        Some(payload.to_owned()),
    )
}

/// A container with the schema applied.
async fn start() -> TestResult<PgContainer> {
    let pg = PgContainer::start(PG_MAJOR).await?;
    PgResultMailbox::<String>::migrate(&pg.pool).await?;
    Ok(pg)
}

async fn row_count(pg: &PgContainer, table: &str, mailbox: &str) -> i64 {
    // `table` is one of two literals from this file.
    let sql = format!("SELECT count(*) FROM {table} WHERE mailbox_name = $1");
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(mailbox)
        .fetch_one(&pg.pool)
        .await
        .expect("count rows")
}

// ── conformance ─────────────────────────────────────────────────────────────

/// Builds a fresh mailbox name per mailbox; time moves by really sleeping,
/// because the mailbox reads the database clock.
struct PgMailboxHarness {
    pg: PgContainer,
    next: AtomicU32,
}

impl PgMailboxHarness {
    async fn start() -> Self {
        Self {
            pg: start().await.expect("start Postgres and migrate"),
            next: AtomicU32::new(0),
        }
    }
}

impl MailboxHarness for PgMailboxHarness {
    type Mailbox = PgResultMailbox<String>;

    fn mailbox(&self) -> impl Future<Output = Self::Mailbox> + Send {
        let name = format!("conformance-{}", self.next.fetch_add(1, Ordering::Relaxed));
        std::future::ready(PgResultMailbox::new(self.pg.pool.clone(), name))
    }

    fn advance(&self, by: Duration) -> impl Future<Output = ()> + Send {
        tokio::time::sleep(by)
    }

    fn ttl(&self) -> Duration {
        Duration::from_secs(1)
    }
}

mod mailbox_conformance {
    use super::*;

    result_mailbox_conformance!(
        test_attr = #[cfg_attr(all(), tokio::test, ignore = "needs Docker: starts a parking-lot-pg container")],
        harness = PgMailboxHarness::start().await,
    );
}

// ── S-7: consume once ───────────────────────────────────────────────────────

docker_test! {
    /// Two consumers on separate connections, released together by a barrier,
    /// race for one stored entry; every round exactly one receives it.
    async fn mailbox_consume_once_race() {
        #[cfg(parking_lot_negative_control = "s-7")]
        prometheus_parking_lot_pg::mailbox::negative_control::arm_consume_gap();

        const ROUNDS: u64 = 25;
        let pg = start().await.expect("start Postgres and migrate");
        let mailbox = PgResultMailbox::<String>::new(pg.pool.clone(), "race");

        for round in 0..ROUNDS {
            let k = key(&format!("race-{round}"));
            let stored = mailbox
                .deliver(completed(&k, round, "the result"))
                .await
                .expect("deliver");
            assert_eq!(stored, DeliveryOutcome::Stored);

            let barrier = Arc::new(Barrier::new(2));
            let consumers: Vec<_> = (0..2)
                .map(|_| {
                    let (mailbox, k, barrier) = (mailbox.clone(), k.clone(), Arc::clone(&barrier));
                    tokio::spawn(async move {
                        barrier.wait().await;
                        mailbox.consume(&k).await.expect("consume")
                    })
                })
                .collect();
            let mut received = 0;
            for consumer in consumers {
                received += usize::from(consumer.await.expect("join").is_some());
            }
            assert_eq!(received, 1, "round {round}: {received} consumers received the one entry");
            assert!(mailbox.consume(&k).await.expect("consume").is_none());
        }
    }
}

docker_test! {
    /// Eight consumers drain 300 entries of one key; no entry reaches two
    /// consumers and none is lost.
    async fn mailbox_concurrent_drain() {
        const ENTRIES: u64 = 300;
        let pg = start().await.expect("start Postgres and migrate");
        let mailbox = PgResultMailbox::<String>::new(pg.pool.clone(), "drain");
        let k = key("drain");
        for id in 0..ENTRIES {
            let outcome = mailbox.deliver(completed(&k, id, "x")).await.expect("deliver");
            assert_eq!(outcome, DeliveryOutcome::Stored);
        }

        let consumers: Vec<_> = (0..8)
            .map(|_| {
                let (mailbox, k) = (mailbox.clone(), k.clone());
                tokio::spawn(async move {
                    let mut taken = Vec::new();
                    // `None` can mean "the rest is locked by other consumers".
                    while let Some(entry) = mailbox.consume(&k).await.expect("consume") {
                        taken.push(entry.task_id);
                    }
                    taken
                })
            })
            .collect();
        let mut all = Vec::new();
        for consumer in consumers {
            all.extend(consumer.await.expect("join"));
        }
        while let Some(entry) = mailbox.consume(&k).await.expect("consume") {
            all.push(entry.task_id);
        }

        let unique: BTreeSet<u64> = all.iter().copied().collect();
        assert_eq!(all.len(), unique.len(), "an entry was consumed twice");
        assert_eq!(unique, (0..ENTRIES).collect::<BTreeSet<_>>(), "an entry was lost");
    }
}

// ── first write wins ────────────────────────────────────────────────────────

docker_test! {
    /// Sixteen deliveries under one key, started together with different
    /// payloads: one is stored, fifteen are duplicates, and the entry that is
    /// stored is the stored winner's.
    async fn mailbox_first_write_wins_race() {
        const WRITERS: u64 = 16;
        let pg = start().await.expect("start Postgres and migrate");
        let mailbox = PgResultMailbox::<String>::new(pg.pool.clone(), "fww");
        let k = key("fww");
        let barrier = Arc::new(Barrier::new(usize::try_from(WRITERS).expect("small")));

        let writers: Vec<_> = (0..WRITERS)
            .map(|writer| {
                let (mailbox, k, barrier) = (mailbox.clone(), k.clone(), Arc::clone(&barrier));
                tokio::spawn(async move {
                    barrier.wait().await;
                    let delivery = completed(&k, 1, &format!("writer-{writer}"))
                        .with_idempotency_key("one-key");
                    (writer, mailbox.deliver(delivery).await.expect("deliver"))
                })
            })
            .collect();
        let mut stored = Vec::new();
        for writer in writers {
            let (id, outcome) = writer.await.expect("join");
            if outcome == DeliveryOutcome::Stored {
                stored.push(id);
            }
        }
        assert_eq!(stored.len(), 1, "stored by writers {stored:?}");

        let entry = mailbox.consume(&k).await.expect("consume").expect("the winner's entry");
        assert_eq!(entry.payload, Some(format!("writer-{}", stored[0])));
        assert!(mailbox.consume(&k).await.expect("consume").is_none());
        assert_eq!(row_count(&pg, "pl_result_mailbox_keys", "fww").await, 1);
    }
}

// ── TTL ─────────────────────────────────────────────────────────────────────

docker_test! {
    /// An expired entry is invisible before eviction, physically removed by it,
    /// and its idempotency key is released.
    async fn mailbox_expired_results_are_evicted() {
        let pg = start().await.expect("start Postgres and migrate");
        let mailbox = PgResultMailbox::<String>::new(pg.pool.clone(), "ttl");
        let k = key("ttl");
        let short = completed(&k, 1, "short-lived")
            .with_idempotency_key("ttl-key")
            .with_ttl(Duration::from_secs(1));
        let kept = completed(&k, 2, "kept");
        let _ = mailbox.deliver(short.clone()).await.expect("deliver");
        let _ = mailbox.deliver(kept).await.expect("deliver");
        assert_eq!(row_count(&pg, "pl_result_mailbox", "ttl").await, 2);

        tokio::time::sleep(Duration::from_millis(1500)).await;

        // Never delivered once expired, though the row is still in the table.
        assert_eq!(row_count(&pg, "pl_result_mailbox", "ttl").await, 2);
        let seen = mailbox.peek(&k).await.expect("peek").expect("the unexpired entry");
        assert_eq!(seen.payload.as_deref(), Some("kept"));

        assert_eq!(mailbox.evict_expired().await.expect("evict"), 1);
        assert_eq!(row_count(&pg, "pl_result_mailbox", "ttl").await, 1);
        assert_eq!(row_count(&pg, "pl_result_mailbox_keys", "ttl").await, 0);
        assert_eq!(mailbox.evict_expired().await.expect("evict again"), 0);

        // The key is forgotten, so the same delivery is stored again.
        assert_eq!(mailbox.deliver(short).await.expect("deliver"), DeliveryOutcome::Stored);
    }
}

docker_test! {
    /// The background sweeper removes expired entries without being asked.
    async fn mailbox_sweeper_evicts_expired() {
        let pg = start().await.expect("start Postgres and migrate");
        let mailbox = PgResultMailbox::<String>::new(pg.pool.clone(), "sweep");
        let k = key("sweep");
        for id in 0..5 {
            let delivery = completed(&k, id, "x").with_ttl(Duration::from_millis(300));
            let _ = mailbox.deliver(delivery).await.expect("deliver");
        }
        let _sweeper = mailbox.spawn_sweeper(Duration::from_millis(200));

        let deadline = Instant::now() + Duration::from_secs(10);
        while row_count(&pg, "pl_result_mailbox", "sweep").await > 0 {
            assert!(Instant::now() < deadline, "the sweeper did not evict in 10 s");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

// ── schema and decoding ─────────────────────────────────────────────────────

docker_test! {
    /// Several processes migrating at once is safe.
    async fn mailbox_migrate_is_concurrent_and_idempotent() {
        let pg = PgContainer::start(PG_MAJOR).await.expect("start Postgres");
        let runs: Vec<_> = (0..4)
            .map(|_| {
                let pool = pg.pool.clone();
                tokio::spawn(async move { PgResultMailbox::<String>::migrate(&pool).await })
            })
            .collect();
        for run in runs {
            run.await.expect("join").expect("migrate");
        }
        PgResultMailbox::<String>::migrate(&pg.pool).await.expect("migrate again");
    }
}

docker_test! {
    /// A stored payload that no longer decodes as `T` fails `peek` without
    /// removing the entry; the entry is still readable as its real type.
    async fn mailbox_undecodable_payload_is_an_error_not_a_loss() {
        let pg = start().await.expect("start Postgres and migrate");
        let k = key("decode");
        let writer = PgResultMailbox::<String>::new(pg.pool.clone(), "decode");
        let _ = writer.deliver(completed(&k, 1, "text")).await.expect("deliver");

        let wrong = PgResultMailbox::<u64>::new(pg.pool.clone(), "decode");
        assert!(wrong.peek(&k).await.is_err(), "a string decoded as u64");
        let entry = writer.peek(&k).await.expect("peek").expect("entry still stored");
        assert_eq!(entry.payload.as_deref(), Some("text"));
    }
}
