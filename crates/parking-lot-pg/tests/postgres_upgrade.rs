//! OA-14 requires the explicit run-bound upgraded cluster from the disposable
//! rehearsal. Ordinary PG17/18 ignored legs exclude this binary. Missing URL
//! is an error, never a skipped assertion or an unexercised PASS.

use std::time::Duration;

use prometheus_parking_lot_core::{
    Delivery, DeliveryOutcome, LeaseQueue, LegacyDeliveryStatus, LegacyIdempotencyOwner,
    MailboxKey, QueueDepth, ResultMailbox, TaskError, TaskStatus,
};
use prometheus_parking_lot_pg::{PgLeaseQueue, PgResultMailbox, PgmqLeaseQueue};
use serde::Deserialize;
use sqlx::{PgPool, postgres::PgPoolOptions};

type TestResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Deserialize)]
struct Evidence {
    owners: Vec<(String, LegacyIdempotencyOwner)>,
    statuses: Vec<(String, LegacyDeliveryStatus)>,
}

fn key(tenant: &str) -> MailboxKey {
    MailboxKey {
        tenant: tenant.into(),
        user_id: Some("user".into()),
        session_id: None,
    }
}

async fn owned_upgraded_pool() -> TestResult<PgPool> {
    let url = std::env::var("PARKING_LOT_PG_UPGRADE_URL")
        .map_err(|_| "OA-14 requires the run-bound disposable upgraded URL")?;
    let run = std::env::var("PARKING_LOT_PG_UPGRADE_RUN_ID")?;
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await?;
    let owned: bool = sqlx::query_scalar(
        "SELECT current_setting('server_version_num')::int = 180006 \
         AND current_setting('data_checksums') = 'on' \
         AND EXISTS (SELECT 1 FROM oa14_rehearsal_identity WHERE run_id = $1 \
                     AND fixture_id = 'oa14-populated-0.3-pg17-v1') \
         AND (SELECT extversion FROM pg_extension WHERE extname = 'pgmq') = '1.13.1'",
    )
    .bind(run)
    .fetch_one(&pool)
    .await?;
    if !owned {
        return Err("OA-14 unsafe upgraded fixture/version/checksum identity".into());
    }
    Ok(pool)
}

#[tokio::test]
#[ignore = "OA-14 explicit disposable pg_upgrade runner only; requires upgraded run-bound URL"]
async fn postgres_upgrade_populated_0_3_to_schema3() -> TestResult<()> {
    let pool = owned_upgraded_pool().await?;
    sqlx::query("SELECT oa14_assert_source_preserved()")
        .execute(&pool)
        .await?;
    println!("OA-14:physical-upgrade-observed server=18.6 mailbox-schema=0.3");
    let evidence: Evidence = serde_json::from_str(include_str!("fixtures/oa14/metadata.json"))?;
    assert!(
        PgResultMailbox::<String>::migrate_with_idempotency_owners(&pool, &evidence.owners)
            .await
            .is_err()
    );
    let refused_atomically: bool = sqlx::query_scalar(
        "SELECT to_regclass('pl_result_mailbox_protocol') IS NULL \
         AND to_regclass('pl_result_mailbox_v2') IS NULL \
         AND to_regclass('pl_result_mailbox_v3') IS NULL",
    )
    .fetch_one(&pool)
    .await?;
    assert!(
        refused_atomically,
        "OA-14:atomic-schema-refusal committed partial conversion"
    );
    sqlx::query("SELECT oa14_assert_source_preserved()")
        .execute(&pool)
        .await?;
    PgResultMailbox::<String>::migrate_with_idempotency_metadata(
        &pool,
        &evidence.owners,
        &evidence.statuses,
    )
    .await?;
    println!("OA-14:atomic-schema3-conversion-complete");
    assert_mailbox_copy(&pool).await?;
    assert_mailbox_api(&pool).await?;
    assert_queue_apis(&pool).await?;
    pool.close().await;
    println!("OA-14:populated-upgrade-api-oracles-pass");
    Ok(())
}

async fn assert_mailbox_copy(pool: &PgPool) -> TestResult<()> {
    #[cfg(parking_lot_negative_control = "oa-14-missing-result")]
    sqlx::query("DELETE FROM pl_result_mailbox_v3 WHERE id = 1")
        .execute(pool)
        .await?;
    let seeded: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pl_result_mailbox_v3 \
         WHERE id = 1 AND task_id = 101 AND payload = '\"retained\"'::jsonb)",
    )
    .fetch_one(pool)
    .await?;
    assert!(
        seeded,
        "OA-14:seeded-result missing after actual pg_upgrade and schema3 conversion"
    );
    let exact: bool = sqlx::query_scalar(
        "SELECT NOT EXISTS ((SELECT * FROM pl_result_mailbox EXCEPT SELECT * FROM pl_result_mailbox_v3) \
         UNION ALL (SELECT * FROM pl_result_mailbox_v3 EXCEPT SELECT * FROM pl_result_mailbox)) \
         AND NOT EXISTS (SELECT 1 FROM pl_result_mailbox_metadata_v3 n JOIN pl_result_mailbox_keys o \
         USING (mailbox_name, idempotency_key) WHERE n.forget_at IS DISTINCT FROM o.forget_at) \
         AND (SELECT last_value FROM pl_result_mailbox_id_seq) = \
             (SELECT last_value FROM pl_result_mailbox_v3_id_seq)",
    ).fetch_one(pool).await?;
    assert!(
        exact,
        "OA-14:mailbox-native-copy result/status/timestamp/TTL/counter changed"
    );
    // Queue/pgmq are untouched by the atomic mailbox conversion.
    sqlx::query("SELECT oa14_assert_source_preserved()")
        .execute(pool)
        .await?;
    Ok(())
}

async fn assert_mailbox_api(pool: &PgPool) -> TestResult<()> {
    let mailbox = PgResultMailbox::<String>::new(pool.clone(), "legacy");
    let failed = TaskStatus::Failed(TaskError::executor("original failure"));
    let retry = Delivery::new(
        key("tenant-failed"),
        999,
        TaskStatus::Completed,
        Some("retry".into()),
    )
    .with_idempotency_key("consumed-failed");
    assert_eq!(
        mailbox.deliver(retry).await?,
        DeliveryOutcome::Duplicate {
            status: failed.clone()
        },
        "OA-14:consumed-status provenance lost at upgrade"
    );
    assert_eq!(
        mailbox
            .consume(&key("tenant-failed"))
            .await?
            .expect("retained failure")
            .status,
        failed
    );
    assert!(mailbox.consume(&key("tenant-failed")).await?.is_none());
    let other = Delivery::new(
        key("other-tenant"),
        1000,
        TaskStatus::Completed,
        Some("independent".into()),
    )
    .with_idempotency_key("consumed-failed");
    assert_eq!(mailbox.deliver(other).await?, DeliveryOutcome::Stored);
    assert_eq!(
        mailbox
            .consume(&key("other-tenant"))
            .await?
            .expect("other tenant")
            .payload
            .as_deref(),
        Some("independent")
    );
    assert_eq!(
        mailbox
            .consume(&key("tenant-a"))
            .await?
            .expect("retained original")
            .task_id,
        101
    );
    assert!(mailbox.consume(&key("tenant-a")).await?.is_none());
    Ok(())
}

async fn assert_queue_apis(pool: &PgPool) -> TestResult<()> {
    PgLeaseQueue::<String>::migrate(pool).await?;
    let queue = PgLeaseQueue::<String>::new(pool.clone(), "oa14_sql", 100).with_max_attempts(10);
    assert_eq!(queue.identity(), "pg:pl_lease_queue:oa14_sql");
    assert_eq!(queue.depth().await?, QueueDepth::new(2, 1, 0, 1));
    let tasks = queue.lease(10, Duration::from_secs(30)).await?;
    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0].task.payload, "ready payload");
    assert_eq!(tasks[1].task.payload, "expired lease payload");
    assert_eq!(
        tasks[1].token.fence(),
        3,
        "OA-14:lease-fence reset at upgrade"
    );
    for task in tasks {
        queue.ack(task.token).await?;
    }
    let pgmq = PgmqLeaseQueue::<String>::new(pool.clone(), "oa14", 100)?;
    pgmq.prepare().await?;
    assert_eq!(pgmq.identity(), "pgmq:oa14");
    assert_eq!(pgmq.depth().await?, QueueDepth::new(2, 1, 1, 1));
    let tasks = pgmq.lease(10, Duration::from_secs(30)).await?;
    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0].task.payload, "pgmq lane 3");
    assert_eq!(tasks[1].task.payload, "pgmq lane 0");
    for task in tasks {
        pgmq.ack(task.token).await?;
    }
    let archived: i64 = sqlx::query_scalar("SELECT count(*) FROM pgmq.a_pl_oa14_p0")
        .fetch_one(pool)
        .await?;
    assert_eq!(archived, 1, "OA-14:pgmq-archive state lost at upgrade");
    Ok(())
}
