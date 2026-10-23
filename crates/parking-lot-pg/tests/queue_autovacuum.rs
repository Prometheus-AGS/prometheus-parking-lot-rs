//! Per-table autovacuum settings of the queue tables (cr-36) on the project
//! Postgres image, both majors.
//!
//! Needs Docker, so each test is `#[ignore]`d and compiles in CI without it:
//!
//! ```text
//! cargo nextest run -p prometheus-parking-lot-pg --locked --run-ignored all -E 'test(autovacuum_)'
//! ```
//!
//! `PARKING_LOT_PG_MAJOR=17` selects the PG 17 image (default 18).
//!
//! - `autovacuum_settings_are_applied_to_every_queue_table`: the migrations (and
//!   `PgmqLeaseQueue::prepare` for pgmq's own `q_*` tables) leave the storage
//!   parameters documented in the migrations on the tables;
//! - `autovacuum_reclaims_a_churned_lease_queue`: the settings do something. A
//!   few thousand rows are inserted and deleted (a drained queue) and the server's
//!   own autovacuum, not a manual `VACUUM`, removes the dead tuples.

mod support;

use std::time::{Duration, Instant};

use prometheus_parking_lot_pg::{PgLeaseQueue, PgResultMailbox, PgmqLeaseQueue};
use sqlx::PgPool;
use support::{PgContainer, TestResult, pg_major};

/// The storage parameters (`pg_class.reloptions`) of `table`.
async fn reloptions(pool: &PgPool, table: &str) -> TestResult<Vec<String>> {
    let options: Option<Vec<String>> =
        sqlx::query_scalar("SELECT reloptions FROM pg_class WHERE oid = $1::regclass")
            .bind(table)
            .fetch_one(pool)
            .await?;
    Ok(options.unwrap_or_default())
}

fn has(options: &[String], wanted: &str) -> bool {
    options.iter().any(|o| o == wanted)
}

/// A table is tuned: vacuum and analyze both fire on a fixed dead/changed
/// tuple count, independent of table size.
fn assert_tuned(table: &str, options: &[String], threshold: u32) {
    for wanted in [
        "autovacuum_vacuum_scale_factor=0".to_owned(),
        format!("autovacuum_vacuum_threshold={threshold}"),
        "autovacuum_analyze_scale_factor=0".to_owned(),
        format!("autovacuum_analyze_threshold={threshold}"),
    ] {
        assert!(
            has(options, &wanted),
            "{table}: {wanted} missing in {options:?}"
        );
    }
}

#[tokio::test]
#[ignore = "needs Docker: starts a parking-lot-pg container"]
async fn autovacuum_settings_are_applied_to_every_queue_table() -> TestResult<()> {
    let pg = PgContainer::start(pg_major()).await?;
    PgLeaseQueue::<String>::migrate(&pg.pool).await?;
    PgResultMailbox::<String>::migrate(&pg.pool).await?;
    let pgmq = PgmqLeaseQueue::<String>::new(pg.pool.clone(), "avq", 100)?;
    pgmq.prepare().await?;

    for (table, threshold) in [
        ("pl_lease_queue", 1000),
        ("pl_result_mailbox", 1000),
        ("pl_result_mailbox_keys", 500),
        ("pl_pgmq_aux", 500),
    ] {
        assert_tuned(table, &reloptions(&pg.pool, table).await?, threshold);
    }

    // pgmq's tables: every priority lane and the dead-letter queue.
    let queues: Vec<String> = sqlx::query_scalar("SELECT queue_name FROM pgmq.list_queues()")
        .fetch_all(&pg.pool)
        .await?;
    assert_eq!(
        queues.len(),
        5,
        "four lanes and a dead-letter queue: {queues:?}"
    );
    for queue in queues {
        let table = format!("pgmq.q_{queue}");
        assert_tuned(&table, &reloptions(&pg.pool, &table).await?, 1000);
    }

    // Applying the migrations again changes nothing and does not fail.
    PgLeaseQueue::<String>::migrate(&pg.pool).await?;
    PgResultMailbox::<String>::migrate(&pg.pool).await?;
    pgmq.prepare().await?;
    assert_tuned(
        "pl_lease_queue",
        &reloptions(&pg.pool, "pl_lease_queue").await?,
        1000,
    );
    Ok(())
}

#[tokio::test]
#[ignore = "needs Docker: starts a parking-lot-pg container"]
async fn autovacuum_reclaims_a_churned_lease_queue() -> TestResult<()> {
    let pg = PgContainer::start(pg_major()).await?;
    PgLeaseQueue::<String>::migrate(&pg.pool).await?;

    // Insert-triggered vacuums are switched off for this table, so that only the
    // dead-tuple trigger under test can start autovacuum. (Without this, the
    // bulk insert below can queue one: the connection's insert counts reach the
    // statistics after the manual VACUUM has reset them.)
    sqlx::query("ALTER TABLE pl_lease_queue SET (autovacuum_vacuum_insert_threshold = -1)")
        .execute(&pg.pool)
        .await?;

    // A backlog: 100k live entries, vacuumed and analyzed so the table starts
    // clean and `reltuples` is 100k. The cluster-wide floor (5% of the live rows
    // + 50, docker/postgres18/postgresql.conf) then needs 5050 dead tuples to
    // fire; the per-table threshold needs 1000.
    sqlx::query(
        "INSERT INTO pl_lease_queue (queue_name, priority, meta, payload)
         SELECT 'backlog', 1, '{}'::jsonb, '{}'::jsonb FROM generate_series(1, 100000)",
    )
    .execute(&pg.pool)
    .await?;
    sqlx::query("VACUUM (ANALYZE) pl_lease_queue")
        .execute(&pg.pool)
        .await?;

    // Churn: 3000 entries enqueued and acked (deleted). 3000 dead tuples are
    // above the table threshold and below the floor, so only the per-table
    // settings make autovacuum act.
    sqlx::query(
        "INSERT INTO pl_lease_queue (queue_name, priority, meta, payload)
         SELECT 'churn', 1, '{}'::jsonb, '{}'::jsonb FROM generate_series(1, 3000)",
    )
    .execute(&pg.pool)
    .await?;
    sqlx::query("DELETE FROM pl_lease_queue WHERE queue_name = 'churn'")
        .execute(&pg.pool)
        .await?;
    // Database time as epoch seconds (no timestamp codec is enabled in this crate).
    let churned_at: f64 =
        sqlx::query_scalar("SELECT extract(epoch FROM clock_timestamp())::float8")
            .fetch_one(&pg.pool)
            .await?;

    // The server's own autovacuum (naptime 10 s in the conf), not a manual VACUUM.
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let vacuumed: bool = sqlx::query_scalar(
            "SELECT coalesce(last_autovacuum > to_timestamp($1), false) FROM pg_stat_user_tables
              WHERE relname = 'pl_lease_queue'",
        )
        .bind(churned_at)
        .fetch_one(&pg.pool)
        .await?;
        if vacuumed {
            return Ok(());
        }
        assert!(
            Instant::now() < deadline,
            "autovacuum did not process pl_lease_queue within 90 s of 3000 deletes"
        );
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
