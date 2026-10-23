//! Smoke tests for the CR-43 Postgres substrate images.
//!
//! These need Docker, so they are `#[ignore]`d by default:
//!
//! ```text
//! cargo test -p prometheus-parking-lot-pg --test pg_image_smoke -- --ignored
//! ```

mod support;

use support::{PgContainer, TestResult, pg_major};

/// Pinned in `docker/postgres*/Dockerfile` (`PGMQ_VERSION`).
const PGMQ_VERSION: &str = "1.13.1";

async fn smoke(major: u8) -> TestResult<()> {
    let pg = PgContainer::start(major).await?;

    let version: String = sqlx::query_scalar("SELECT version()")
        .fetch_one(&pg.pool)
        .await?;
    assert!(
        version.starts_with(&format!("PostgreSQL {major}.")),
        "unexpected server version: {version}"
    );

    sqlx::query("CREATE EXTENSION IF NOT EXISTS pgmq")
        .execute(&pg.pool)
        .await?;
    let ext: String =
        sqlx::query_scalar("SELECT extversion FROM pg_extension WHERE extname = 'pgmq'")
            .fetch_one(&pg.pool)
            .await?;
    assert_eq!(ext, PGMQ_VERSION);

    // The tuned conf is in effect (not the stock image config).
    let max_conn: String = sqlx::query_scalar("SHOW max_connections")
        .fetch_one(&pg.pool)
        .await?;
    assert_eq!(max_conn, "200");
    let preload: String = sqlx::query_scalar("SHOW shared_preload_libraries")
        .fetch_one(&pg.pool)
        .await?;
    assert!(preload.contains("pg_stat_statements"));

    // pgmq actually works: create, send, read.
    sqlx::query("SELECT pgmq.create('smoke')")
        .execute(&pg.pool)
        .await?;
    let msg_id: i64 =
        sqlx::query_scalar("SELECT * FROM pgmq.send('smoke', '{\"ok\":true}'::jsonb)")
            .fetch_one(&pg.pool)
            .await?;
    let read_id: i64 = sqlx::query_scalar("SELECT msg_id FROM pgmq.read('smoke', 30, 1)")
        .fetch_one(&pg.pool)
        .await?;
    assert_eq!(msg_id, read_id);

    // template1 carries pgmq, so per-test databases inherit it.
    sqlx::query("CREATE DATABASE smoke_child")
        .execute(&pg.pool)
        .await?;
    let child = sqlx::PgPool::connect(&pg.url_for("smoke_child")).await?;
    let inherited: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pg_extension WHERE extname = 'pgmq'")
            .fetch_one(&child)
            .await?;
    assert_eq!(inherited, 1);
    Ok(())
}

/// The image of the major under test (`PARKING_LOT_PG_MAJOR`, default 18) has
/// pgmq and the tuned configuration. The CI matrix runs it once per major.
#[tokio::test]
#[ignore = "requires Docker"]
async fn postgres_image_has_pgmq() -> TestResult<()> {
    smoke(pg_major()).await
}
