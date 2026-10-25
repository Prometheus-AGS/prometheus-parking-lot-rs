//! OA-06 populated 0.3 migration and old-writer rejection on real PG17/PG18.
//! Execution is deferred to the complete OA-18 production boundary.

mod support;

use std::time::Duration;

use prometheus_parking_lot_core::{
    Delivery, DeliveryOutcome, LegacyDeliveryStatus, LegacyIdempotencyOwner, MailboxKey,
    ResultMailbox, TaskStatus,
};
use prometheus_parking_lot_pg::PgResultMailbox;
use serde_json::Value;
use sqlx::{PgPool, postgres::PgPoolOptions};
use support::{PgContainer, TestResult, pg_major};

const LEGACY_SCHEMA: &str = include_str!("fixtures/oa06/005_result_mailbox_0_3.sql");
const LEGACY_DATA: &str = include_str!("fixtures/oa06/populated_0_3.sql");
const UNTOUCHED_BACKUP: &str = include_str!("fixtures/oa06/untouched_backup_0_3.sql");

fn key(tenant: &str) -> MailboxKey {
    MailboxKey {
        tenant: tenant.into(),
        user_id: Some("user".into()),
        session_id: None,
    }
}

fn owners() -> Vec<(String, LegacyIdempotencyOwner)> {
    ["consumed-finite", "consumed-permanent"]
        .into_iter()
        .map(|caller| {
            (
                "legacy".into(),
                LegacyIdempotencyOwner::new(caller, key("tenant-a")),
            )
        })
        .collect()
}

async fn migrate_known(
    pool: &PgPool,
    owners: &[(String, LegacyIdempotencyOwner)],
) -> Result<(), prometheus_parking_lot_core::SchedulerError> {
    let statuses: Vec<_> = ["consumed-finite", "consumed-permanent"]
        .into_iter()
        .map(|caller| {
            (
                "legacy".into(),
                LegacyDeliveryStatus::new(caller, key("tenant-a"), TaskStatus::Completed),
            )
        })
        .collect();
    PgResultMailbox::<String>::migrate_with_idempotency_metadata(pool, owners, &statuses).await
}

fn delivery(tenant: &str, caller: &str) -> Delivery<String> {
    Delivery::new(
        key(tenant),
        999,
        TaskStatus::Completed,
        Some("retry".into()),
    )
    .with_idempotency_key(caller)
}

async fn populate(pool: &PgPool) -> TestResult<()> {
    let mut tx = pool.begin().await?;
    sqlx::raw_sql(LEGACY_SCHEMA).execute(&mut *tx).await?;
    sqlx::raw_sql(LEGACY_DATA).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

/// Logical snapshot includes native timestamps, sequence state, schema objects
/// and table identity. Refusal must not install a marker, table or trigger.
async fn snapshot(pool: &PgPool) -> TestResult<Value> {
    Ok(sqlx::query_scalar(
        "SELECT jsonb_build_object( \
            'results', (SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM pl_result_mailbox r), \
            'fences', (SELECT jsonb_agg(to_jsonb(k) ORDER BY idempotency_key) \
                       FROM pl_result_mailbox_keys k), \
            'sequence', (SELECT jsonb_build_object( \
                'last_value', s.last_value, 'log_cnt', s.log_cnt, 'is_called', s.is_called) \
                FROM pl_result_mailbox_id_seq s), \
            'relations', (SELECT jsonb_agg(jsonb_build_array(c.relname, c.oid::bigint, c.relkind) \
                            ORDER BY c.relname) FROM pg_class c \
                           WHERE c.relname LIKE 'pl_result_mailbox%'), \
            'triggers', (SELECT jsonb_agg(t.tgname ORDER BY t.tgname) FROM pg_trigger t \
                          WHERE t.tgrelid IN ('pl_result_mailbox'::regclass, \
                                             'pl_result_mailbox_keys'::regclass)))",
    )
    .fetch_one(pool)
    .await?)
}

#[tokio::test]
#[ignore = "needs Docker: starts a parking-lot-pg container"]
async fn mailbox_legacy_refusal_is_atomic_and_backup_readable() -> TestResult<()> {
    let pg = PgContainer::start(pg_major()).await?;
    populate(&pg.pool).await?;
    let before = snapshot(&pg.pool).await?;
    let missing = PgResultMailbox::<String>::migrate(&pg.pool).await;
    assert!(format!("{missing:?}").contains("authoritative mapping required"));
    assert_eq!(
        snapshot(&pg.pool).await?,
        before,
        "refusal changed legacy state"
    );

    let mut conflicting = owners();
    conflicting.push((
        "legacy".into(),
        LegacyIdempotencyOwner::new("retained", key("wrong")),
    ));
    let mut duplicate = owners();
    duplicate.push(duplicate[0].clone());
    let mut unknown = owners();
    unknown.push((
        "legacy".into(),
        LegacyIdempotencyOwner::new("unknown", key("tenant-a")),
    ));
    let mut expired = owners();
    expired.push((
        "legacy".into(),
        LegacyIdempotencyOwner::new("expired-ownerless", key("tenant-a")),
    ));
    for assignments in [conflicting, duplicate, unknown, expired] {
        assert!(migrate_known(&pg.pool, &assignments,).await.is_err());
        assert_eq!(
            snapshot(&pg.pool).await?,
            before,
            "invalid mapping changed source"
        );
    }

    // Restore an independently frozen untouched SQL backup to another database.
    // No converted database is downgraded; this is the ADR's backup-only path.
    let backup_name = support::database_name()?;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "CREATE DATABASE {backup_name}"
    )))
    .execute(&pg.pool)
    .await?;
    let backup = PgPoolOptions::new()
        .max_connections(2)
        .connect(&pg.url_for(&backup_name))
        .await?;
    sqlx::raw_sql(UNTOUCHED_BACKUP).execute(&backup).await?;
    let restored = snapshot(&backup).await?;
    assert_eq!(restored["results"], before["results"]);
    assert_eq!(restored["fences"], before["fences"]);
    assert_eq!(restored["sequence"], before["sequence"]);
    // Exercise frozen 0.3 consume SQL against the untouched restored source.
    let result: String = sqlx::query_scalar(
        "DELETE FROM pl_result_mailbox WHERE id = (SELECT id FROM pl_result_mailbox \
         WHERE mailbox_name = $1 AND mailbox_key = $2 \
           AND (expires_at IS NULL OR expires_at > now()) \
         ORDER BY id LIMIT 1 FOR UPDATE SKIP LOCKED) RETURNING payload #>> '{}'",
    )
    .bind("legacy")
    .bind(serde_json::to_string(&key("tenant-a"))?)
    .fetch_one(&backup)
    .await?;
    assert_eq!(result, "retained");
    assert_eq!(
        snapshot(&pg.pool).await?,
        before,
        "backup read mutated source"
    );
    backup.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "needs Docker: starts a parking-lot-pg container"]
async fn mailbox_legacy_ambiguous_generation_requires_retained_candidate() -> TestResult<()> {
    let pg = PgContainer::start(pg_major()).await?;
    populate(&pg.pool).await?;
    sqlx::raw_sql(include_str!("fixtures/oa06/ambiguous_generation_0_3.sql"))
        .execute(&pg.pool)
        .await?;
    let before = snapshot(&pg.pool).await?;
    let missing = migrate_known(&pg.pool, &owners()).await;
    assert!(format!("{missing:?}").contains("authoritative mapping required"));
    assert_eq!(
        snapshot(&pg.pool).await?,
        before,
        "ambiguous refusal changed source"
    );

    let mut outside = owners();
    outside.push((
        "legacy".into(),
        LegacyIdempotencyOwner::new("ambiguous-generation", key("unrelated-address")),
    ));
    let rejected = migrate_known(&pg.pool, &outside).await;
    assert!(
        format!("{rejected:?}").contains("outside retained generation candidates"),
        "OA-06:retained-candidate oracle: unrelated owner was accepted: {rejected:?}"
    );
    assert_eq!(
        snapshot(&pg.pool).await?,
        before,
        "outside owner changed source"
    );

    let mut selected = owners();
    selected.push((
        "legacy".into(),
        LegacyIdempotencyOwner::new("ambiguous-generation", key("candidate-a")),
    ));
    migrate_known(&pg.pool, &selected).await?;
    let recorded: String = sqlx::query_scalar(
        "SELECT mailbox_key FROM pl_result_mailbox_owners_v2 \
         WHERE mailbox_name = 'legacy' AND idempotency_key = 'ambiguous-generation'",
    )
    .fetch_one(&pg.pool)
    .await?;
    assert_eq!(recorded, serde_json::to_string(&key("candidate-a"))?);
    let mailbox = PgResultMailbox::<String>::new(pg.pool.clone(), "legacy");
    assert_eq!(
        mailbox
            .deliver(delivery("candidate-a", "ambiguous-generation"))
            .await?,
        DeliveryOutcome::Duplicate {
            status: TaskStatus::Completed
        }
    );
    for (tenant, task_id) in [("candidate-a", 107), ("candidate-b", 108)] {
        assert_eq!(
            mailbox
                .consume(&key(tenant))
                .await?
                .expect("retained result")
                .task_id,
            task_id
        );
        assert!(mailbox.consume(&key(tenant)).await?.is_none());
    }
    let after = snapshot(&pg.pool).await?;
    assert_eq!(after["results"], before["results"]);
    assert_eq!(after["fences"], before["fences"]);
    assert_eq!(after["sequence"], before["sequence"]);
    Ok(())
}

#[tokio::test]
#[ignore = "needs Docker: starts a parking-lot-pg container"]
async fn mailbox_legacy_mapping_preserves_results_fences_and_native_expiry() -> TestResult<()> {
    let pg = PgContainer::start(pg_major()).await?;
    populate(&pg.pool).await?;
    let before = snapshot(&pg.pool).await?;
    migrate_known(&pg.pool, &owners()).await?;
    let copied: bool = sqlx::query_scalar(
        "SELECT NOT EXISTS ( \
             (SELECT * FROM pl_result_mailbox EXCEPT SELECT * FROM pl_result_mailbox_v3) \
             UNION ALL \
             (SELECT * FROM pl_result_mailbox_v3 EXCEPT SELECT * FROM pl_result_mailbox)) \
         AND NOT EXISTS ( \
             SELECT 1 FROM pl_result_mailbox_keys_v3 n JOIN pl_result_mailbox_keys o \
             USING (mailbox_name, idempotency_key) \
             WHERE n.forget_at IS DISTINCT FROM o.forget_at)",
    )
    .fetch_one(&pg.pool)
    .await?;
    assert!(copied, "conversion changed native results or expiry");
    let mailbox = PgResultMailbox::<String>::new(pg.pool.clone(), "legacy");
    for caller in ["consumed-finite", "consumed-permanent", "retained"] {
        assert_eq!(
            mailbox.deliver(delivery("tenant-a", caller)).await?,
            DeliveryOutcome::Duplicate {
                status: TaskStatus::Completed
            }
        );
        assert_eq!(
            mailbox.deliver(delivery("other-tenant", caller)).await?,
            DeliveryOutcome::Stored
        );
    }
    assert_eq!(
        mailbox.deliver(delivery("tenant-b", "reused")).await?,
        DeliveryOutcome::Duplicate {
            status: TaskStatus::Completed
        }
    );
    assert_eq!(
        mailbox
            .deliver(delivery("old-generation", "reused"))
            .await?,
        DeliveryOutcome::Stored
    );
    assert_eq!(
        mailbox
            .consume(&key("tenant-a"))
            .await?
            .expect("retained")
            .task_id,
        101
    );
    assert!(mailbox.consume(&key("tenant-a")).await?.is_none());
    for expected in [102, 104] {
        assert_eq!(
            mailbox
                .consume(&key("tenant-b"))
                .await?
                .expect("ordered entry")
                .task_id,
            expected
        );
    }
    assert!(mailbox.consume(&key("tenant-b")).await?.is_none());
    // The copied sequence must retain the original high water mark, including
    // ids whose rows were already consumed before migration.
    let min_new_id: i64 =
        sqlx::query_scalar("SELECT min(id) FROM pl_result_mailbox_v3 WHERE task_id = 999")
            .fetch_one(&pg.pool)
            .await?;
    assert_eq!(min_new_id, 101);
    let recorded: Value = sqlx::query_scalar(
        "SELECT jsonb_agg(to_jsonb(k) ORDER BY mailbox_name, mailbox_key, idempotency_key) \
         FROM pl_result_mailbox_keys_v3 k",
    )
    .fetch_one(&pg.pool)
    .await?;
    PgResultMailbox::<String>::migrate(&pg.pool).await?;
    migrate_known(&pg.pool, &owners()).await?;
    let retried: Value = sqlx::query_scalar(
        "SELECT jsonb_agg(to_jsonb(k) ORDER BY mailbox_name, mailbox_key, idempotency_key) \
         FROM pl_result_mailbox_keys_v3 k",
    )
    .fetch_one(&pg.pool)
    .await?;
    assert_eq!(retried, recorded, "retry extended or reset a fence");
    assert_eq!(mailbox.evict_expired().await?, 1);
    let after = snapshot(&pg.pool).await?;
    assert_eq!(
        after["results"], before["results"],
        "legacy results provenance changed"
    );
    assert_eq!(
        after["fences"], before["fences"],
        "legacy fences provenance changed"
    );
    assert_eq!(
        after["sequence"], before["sequence"],
        "legacy sequence changed"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "needs Docker: starts a parking-lot-pg container"]
async fn mailbox_legacy_sql_writers_are_rejected_after_conversion() -> TestResult<()> {
    let pg = PgContainer::start(pg_major()).await?;
    populate(&pg.pool).await?;
    migrate_known(&pg.pool, &owners()).await?;
    let before = snapshot(&pg.pool).await?;
    // This is the actual old startup schema; re-running it must not remove the
    // guard or create another writable mailbox under the old names.
    sqlx::raw_sql(LEGACY_SCHEMA).execute(&pg.pool).await?;
    let legacy_delivery = sqlx::query(include_str!("fixtures/oa06/legacy_keyed_delivery.sql"))
        .bind("legacy")
        .bind(serde_json::to_string(&key("tenant-a"))?)
        .bind(777_i64)
        .bind(serde_json::to_value(TaskStatus::Completed)?)
        .bind(serde_json::json!("old-writer"))
        .bind(None::<f64>)
        .bind("old-writer")
        .execute(&pg.pool)
        .await
        .expect_err("frozen legacy deliver must fail");
    assert!(
        legacy_delivery
            .to_string()
            .contains("old writers are unsupported")
    );
    for statement in [
        "INSERT INTO pl_result_mailbox (mailbox_name, mailbox_key, task_id, status) VALUES ('old', 'k', 1, '\"Completed\"')",
        "INSERT INTO pl_result_mailbox_keys (mailbox_name, idempotency_key) VALUES ('old', 'k') ON CONFLICT DO NOTHING",
        "UPDATE pl_result_mailbox_keys SET forget_at = now() WHERE false",
        "DELETE FROM pl_result_mailbox WHERE id = 1 RETURNING id",
        "DELETE FROM pl_result_mailbox_keys WHERE false",
        "TRUNCATE pl_result_mailbox_keys",
        "WITH gone AS (DELETE FROM pl_result_mailbox WHERE mailbox_name = 'legacy' AND expires_at IS NOT NULL AND expires_at <= now() RETURNING 1), forgotten AS (DELETE FROM pl_result_mailbox_keys WHERE mailbox_name = 'legacy' AND forget_at IS NOT NULL AND forget_at <= now() RETURNING 1) SELECT count(*) FROM gone",
    ] {
        let rejected = sqlx::query(statement)
            .execute(&pg.pool)
            .await
            .expect_err("old SQL must fail");
        assert!(
            rejected.to_string().contains("old writers are unsupported"),
            "{rejected}"
        );
    }
    assert_eq!(snapshot(&pg.pool).await?, before);
    Ok(())
}

#[tokio::test]
#[ignore = "needs Docker: starts a parking-lot-pg container"]
async fn mailbox_scoped_fences_survive_reconnect_and_expire_independently() -> TestResult<()> {
    let pg = PgContainer::start(pg_major()).await?;
    PgResultMailbox::<String>::migrate(&pg.pool).await?;
    let first_pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&pg.url)
        .await?;
    let mailbox = PgResultMailbox::<String>::new(first_pool.clone(), "restart");
    let a = delivery("tenant-a", "shared").with_ttl(Duration::from_secs(10));
    let b = delivery("tenant-b", "shared");
    assert_eq!(mailbox.deliver(a.clone()).await?, DeliveryOutcome::Stored);
    assert_eq!(
        mailbox.deliver(b.clone()).await?,
        DeliveryOutcome::Stored,
        "OA-06:address-scope oracle: another tenant suppressed delivery"
    );
    assert!(mailbox.consume(&a.key).await?.is_some());
    let expiry: String = sqlx::query_scalar(
        "SELECT forget_at::text FROM pl_result_mailbox_keys_v3 \
         WHERE mailbox_name = 'restart' AND mailbox_key = $1",
    )
    .bind(serde_json::to_string(&a.key)?)
    .fetch_one(&pg.pool)
    .await?;
    drop(mailbox);
    first_pool.close().await;
    let reopened = PgPoolOptions::new()
        .max_connections(2)
        .connect(&pg.url)
        .await?;
    PgResultMailbox::<String>::migrate(&reopened).await?;
    let mailbox = PgResultMailbox::<String>::new(reopened.clone(), "restart");
    assert_eq!(
        mailbox.deliver(a.clone()).await?,
        DeliveryOutcome::Duplicate {
            status: TaskStatus::Completed
        }
    );
    let same_expiry: String = sqlx::query_scalar(
        "SELECT forget_at::text FROM pl_result_mailbox_keys_v3 \
         WHERE mailbox_name = 'restart' AND mailbox_key = $1",
    )
    .bind(serde_json::to_string(&a.key)?)
    .fetch_one(&pg.pool)
    .await?;
    assert_eq!(same_expiry, expiry);
    // Wait for the original database expiry, rather than sleeping a fresh TTL
    // after reconnect (which could conceal a lifetime extension).
    sqlx::query("SELECT pg_sleep(greatest(0, extract(epoch FROM ($1::timestamptz - clock_timestamp()))) + 0.05)")
        .bind(&expiry).execute(&pg.pool).await?;
    assert_eq!(mailbox.evict_expired().await?, 0);
    assert_eq!(mailbox.deliver(a).await?, DeliveryOutcome::Stored);
    assert_eq!(
        mailbox.deliver(b).await?,
        DeliveryOutcome::Duplicate {
            status: TaskStatus::Completed
        }
    );
    assert!(mailbox.consume(&key("tenant-b")).await?.is_some());
    assert!(mailbox.consume(&key("tenant-b")).await?.is_none());
    reopened.close().await;
    Ok(())
}
