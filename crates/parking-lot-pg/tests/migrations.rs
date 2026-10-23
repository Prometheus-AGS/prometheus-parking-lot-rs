//! Migrations and `prepare()` must not take locks that block queue traffic when
//! they have nothing to do, and must give up instead of queueing behind a long
//! transaction when they do need one.
//!
//! Needs Docker, so each test is `#[ignore]`d and compiles in CI without it:
//!
//! ```text
//! cargo nextest run -p prometheus-parking-lot-pg --locked --run-ignored all -E 'test(migration_)'
//! ```
//!
//! - `migration_rerun_does_not_block_behind_open_writers`: another session holds
//!   `ROW EXCLUSIVE` on every queue table (an open transaction that inserted a
//!   row, which is what a busy producer looks like); each re-run finishes in
//!   bounded time. Before the guards, `CREATE INDEX IF NOT EXISTS` waited for a
//!   `SHARE` lock and `DROP TRIGGER` for `ACCESS EXCLUSIVE`.
//! - `migration_lock_wait_is_bounded_by_lock_timeout`: a migration that does need
//!   a lock (an index was dropped) fails within the `lock_timeout` instead of
//!   waiting for the blocking transaction.

mod support;

use std::time::{Duration, Instant};

use prometheus_parking_lot_pg::{PgLeaseQueue, PgResultMailbox, PgmqLeaseQueue, PostgresAuditSink};
use sqlx::{Connection, PgConnection, PgPool};
use support::{PgContainer, TestResult, pg_major};

/// Longer than any re-run should take, shorter than the migrations'
/// `lock_timeout` (5 s) so a blocked run shows up as a test failure, not as a
/// migration error.
const BOUND: Duration = Duration::from_secs(3);

async fn migrate_everything(pool: &PgPool, pgmq: &PgmqLeaseQueue<String>) -> TestResult<()> {
    PgLeaseQueue::<String>::migrate(pool).await?;
    PgResultMailbox::<String>::migrate(pool).await?;
    PostgresAuditSink::migrate(pool).await?;
    pgmq.prepare().await?;
    Ok(())
}

/// An open transaction that has written to every queue table.
async fn open_writer(url: &str) -> TestResult<PgConnection> {
    let mut conn = PgConnection::connect(url).await?;
    sqlx::query("BEGIN").execute(&mut conn).await?;
    for statement in [
        "INSERT INTO pl_lease_queue (queue_name, priority, meta, payload)
         VALUES ('held', 1, '{}', '{}')",
        "INSERT INTO pl_result_mailbox (mailbox_name, mailbox_key, task_id, status)
         VALUES ('held', 'k', 1, '{}')",
        "INSERT INTO pl_result_mailbox_keys (mailbox_name, idempotency_key) VALUES ('held', 'k')",
        "INSERT INTO pl_pgmq_aux (queue_name, entry_id) VALUES ('held', 1)",
        "INSERT INTO pl_audit_events (event_id, task_id, pool, tenant, action)
         VALUES ('held', 't', 'p', 't', 'a')",
        "SELECT pgmq.send('pl_held_p1', '{}'::jsonb)",
    ] {
        sqlx::query(statement).execute(&mut conn).await?;
    }
    Ok(conn)
}

#[tokio::test]
#[ignore = "needs Docker: starts a parking-lot-pg container"]
async fn migration_rerun_does_not_block_behind_open_writers() -> TestResult<()> {
    let pg = PgContainer::start(pg_major()).await?;
    let pgmq = PgmqLeaseQueue::<String>::new(pg.pool.clone(), "held", 10)?;
    migrate_everything(&pg.pool, &pgmq).await?;

    let writer = open_writer(&pg.url).await?;
    let started = Instant::now();
    let rerun = tokio::time::timeout(BOUND, migrate_everything(&pg.pool, &pgmq)).await;
    let waited = started.elapsed();
    writer.close().await?;
    let result = rerun.unwrap_or_else(|elapsed| {
        panic!("a migration re-run blocked behind an open transaction for {waited:?} ({elapsed})")
    });
    result?;
    Ok(())
}

#[tokio::test]
#[ignore = "needs Docker: starts a parking-lot-pg container"]
async fn migration_lock_wait_is_bounded_by_lock_timeout() -> TestResult<()> {
    let pg = PgContainer::start(pg_major()).await?;
    PgLeaseQueue::<String>::migrate(&pg.pool).await?;
    sqlx::query("DROP INDEX idx_pl_lease_queue_claim")
        .execute(&pg.pool)
        .await?;

    let writer = open_writer_on_lease_queue(&pg.url).await?;
    let started = Instant::now();
    let outcome = tokio::time::timeout(
        Duration::from_secs(15),
        PgLeaseQueue::<String>::migrate(&pg.pool),
    )
    .await;
    let waited = started.elapsed();
    writer.close().await?;

    let outcome = outcome.expect("migrate must give up on its own, not wait for the writer");
    assert!(outcome.is_err(), "migrate cannot have built the index");
    let message = format!("{outcome:?}");
    assert!(message.contains("lock timeout"), "{message}");
    assert!(
        waited >= Duration::from_secs(4) && waited < Duration::from_secs(10),
        "waited {waited:?}, lock_timeout is 5 s"
    );
    Ok(())
}

async fn open_writer_on_lease_queue(url: &str) -> TestResult<PgConnection> {
    let mut conn = PgConnection::connect(url).await?;
    sqlx::query("BEGIN").execute(&mut conn).await?;
    sqlx::query(
        "INSERT INTO pl_lease_queue (queue_name, priority, meta, payload)
         VALUES ('held', 1, '{}', '{}')",
    )
    .execute(&mut conn)
    .await?;
    Ok(conn)
}
