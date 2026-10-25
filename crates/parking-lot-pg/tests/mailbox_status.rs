//! OA-07 original-status persistence and atomic migration through real PG17/18.
//! Final execution belongs to the completed OA-18 boundary.

mod support;

use std::sync::Arc;
use std::time::Duration;

use prometheus_parking_lot_core::{
    Delivery, DeliveryOutcome, LegacyDeliveryStatus, LegacyIdempotencyOwner, MailboxKey,
    ResultMailbox, TaskError, TaskStatus,
};
use prometheus_parking_lot_pg::PgResultMailbox;
use serde_json::Value;
use sqlx::{PgPool, postgres::PgPoolOptions};
use support::{PgContainer, TestResult, pg_major};
use tokio::sync::Barrier;

const OLD_SCHEMA: &str = include_str!("fixtures/oa06/005_result_mailbox_0_3.sql");
const OLD_DATA: &str = include_str!("fixtures/oa06/populated_0_3.sql");
const SCOPED_SCHEMA: &str = include_str!("fixtures/oa07/008_scoped_result_mailbox_v2.sql");
const SCOPED_DATA: &str = include_str!("fixtures/oa07/populated_v2.sql");

fn key(tenant: &str) -> MailboxKey {
    MailboxKey {
        tenant: tenant.into(),
        user_id: Some("user".into()),
        session_id: None,
    }
}

fn failed() -> TaskStatus {
    TaskStatus::Failed(TaskError::executor("original failure"))
}

fn owners() -> Vec<(String, LegacyIdempotencyOwner)> {
    [
        ("consumed-finite", "tenant-a"),
        ("consumed-permanent", "tenant-a"),
        ("consumed-failed", "tenant-failed"),
    ]
    .into_iter()
    .map(|(caller, tenant)| {
        (
            "legacy".into(),
            LegacyIdempotencyOwner::new(caller, key(tenant)),
        )
    })
    .collect()
}

fn statuses() -> Vec<(String, LegacyDeliveryStatus)> {
    vec![
        (
            "legacy".into(),
            LegacyDeliveryStatus::new("consumed-finite", key("tenant-a"), TaskStatus::Completed),
        ),
        (
            "legacy".into(),
            LegacyDeliveryStatus::new("consumed-permanent", key("tenant-a"), TaskStatus::Completed),
        ),
        (
            "legacy".into(),
            LegacyDeliveryStatus::new("consumed-failed", key("tenant-failed"), failed()),
        ),
    ]
}

fn statuses_for(intermediate: bool) -> Vec<(String, LegacyDeliveryStatus)> {
    let mut evidence = statuses();
    if intermediate {
        evidence.push((
            "legacy".into(),
            LegacyDeliveryStatus::new(
                "consumed-failed",
                key("second-address"),
                TaskStatus::Running,
            ),
        ));
    }
    evidence
}

async fn populate(pool: &PgPool, intermediate: bool) -> TestResult<()> {
    let mut tx = pool.begin().await?;
    sqlx::raw_sql(OLD_SCHEMA).execute(&mut *tx).await?;
    sqlx::raw_sql(OLD_DATA).execute(&mut *tx).await?;
    if intermediate {
        sqlx::raw_sql(SCOPED_SCHEMA).execute(&mut *tx).await?;
        sqlx::raw_sql(SCOPED_DATA).execute(&mut *tx).await?;
        sqlx::raw_sql(include_str!("fixtures/oa07/failed_generation_v2.sql"))
            .execute(&mut *tx)
            .await?;
    } else {
        sqlx::raw_sql(include_str!("fixtures/oa07/failed_generation_0_3.sql"))
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

async fn source_snapshot(pool: &PgPool, intermediate: bool) -> TestResult<Value> {
    // Only the two frozen fixture layouts select these identifiers.
    let (entries, fences, sequence) = if intermediate {
        (
            "pl_result_mailbox_v2",
            "pl_result_mailbox_keys_v2",
            "pl_result_mailbox_v2_id_seq",
        )
    } else {
        (
            "pl_result_mailbox",
            "pl_result_mailbox_keys",
            "pl_result_mailbox_id_seq",
        )
    };
    let sql = format!(
        "SELECT jsonb_build_object( \
            'results', (SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM {entries} r), \
            'fences', (SELECT jsonb_agg(to_jsonb(k) ORDER BY to_jsonb(k)::text) FROM {fences} k), \
            'sequence', (SELECT jsonb_build_object( \
                'last_value', s.last_value, 'log_cnt', s.log_cnt, 'is_called', s.is_called) \
                FROM {sequence} s), \
            'relations', (SELECT jsonb_agg(jsonb_build_array(c.relname, c.oid::bigint, c.relkind) \
                            ORDER BY c.relname) FROM pg_class c \
                           WHERE c.relname LIKE 'pl_result_mailbox%'), \
            'triggers', (SELECT jsonb_agg(t.tgname ORDER BY t.tgname) FROM pg_trigger t \
                         JOIN pg_class c ON c.oid = t.tgrelid \
                         WHERE c.relname LIKE 'pl_result_mailbox%'))"
    );
    let mut snapshot: Value = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .fetch_one(pool)
        .await?;
    if intermediate {
        let protocol: Value =
            sqlx::query_scalar("SELECT to_jsonb(p) FROM pl_result_mailbox_protocol p")
                .fetch_one(pool)
                .await?;
        snapshot["protocol"] = protocol;
    }
    Ok(snapshot)
}

#[tokio::test]
#[ignore = "needs Docker: starts parking-lot-pg containers"]
async fn mailbox_status_direct_and_v2_refuse_atomically_then_preserve_failed_provenance()
-> TestResult<()> {
    for intermediate in [false, true] {
        let pg = PgContainer::start(pg_major()).await?;
        populate(&pg.pool, intermediate).await?;
        let before = source_snapshot(&pg.pool, intermediate).await?;
        let owner_evidence = if intermediate { Vec::new() } else { owners() };
        let missing =
            PgResultMailbox::<String>::migrate_with_idempotency_owners(&pg.pool, &owner_evidence)
                .await;
        assert!(format!("{missing:?}").contains("authoritative status required"));
        assert_eq!(
            source_snapshot(&pg.pool, intermediate).await?,
            before,
            "OA-07:atomic-refusal committed a partial owner/status upgrade"
        );
        let mut contradictory = statuses_for(intermediate);
        contradictory.push((
            "legacy".into(),
            LegacyDeliveryStatus::new(
                "retained-failed",
                key("tenant-failed"),
                TaskStatus::Completed,
            ),
        ));
        let mut duplicate = statuses_for(intermediate);
        duplicate.push(duplicate[0].clone());
        let mut unknown = statuses_for(intermediate);
        unknown.push((
            "legacy".into(),
            LegacyDeliveryStatus::new("unknown", key("tenant-a"), TaskStatus::Completed),
        ));
        let mut wrong_address = statuses_for(intermediate);
        wrong_address[2].1.key = key("wrong-address");
        let mut expired = statuses_for(intermediate);
        expired.push((
            "legacy".into(),
            LegacyDeliveryStatus::new("expired-ownerless", key("tenant-a"), TaskStatus::Completed),
        ));
        for invalid in [contradictory, duplicate, unknown, wrong_address, expired] {
            assert!(
                PgResultMailbox::<String>::migrate_with_idempotency_metadata(
                    &pg.pool,
                    &owner_evidence,
                    &invalid,
                )
                .await
                .is_err()
            );
            assert_eq!(source_snapshot(&pg.pool, intermediate).await?, before);
        }
        if !intermediate {
            assert!(
                PgResultMailbox::<String>::migrate_with_idempotency_metadata(
                    &pg.pool,
                    &[],
                    &statuses(),
                )
                .await
                .is_err(),
                "status evidence must not replace missing owner evidence"
            );
            assert_eq!(source_snapshot(&pg.pool, intermediate).await?, before);
        }
        PgResultMailbox::<String>::migrate_with_idempotency_metadata(
            &pg.pool,
            &owner_evidence,
            &statuses_for(intermediate),
        )
        .await?;
        assert_preserved_and_failed(&pg, intermediate, &before).await?;
        assert_backup_readable(&pg, intermediate, &before).await?;
    }
    Ok(())
}

async fn assert_backup_readable(
    pg: &PgContainer,
    intermediate: bool,
    before: &Value,
) -> TestResult<()> {
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
    let source = if intermediate {
        include_str!("fixtures/oa07/untouched_backup_v2.sql")
    } else {
        include_str!("fixtures/oa07/untouched_backup_0_3.sql")
    };
    sqlx::raw_sql(source).execute(&backup).await?;
    let restored = source_snapshot(&backup, intermediate).await?;
    for field in ["results", "fences", "sequence"] {
        assert_eq!(restored[field], before[field], "backup lost source {field}");
    }
    let old_consume = if intermediate {
        "DELETE FROM pl_result_mailbox_v2 WHERE id = 7 RETURNING status"
    } else {
        "DELETE FROM pl_result_mailbox WHERE id = 7 RETURNING status"
    };
    let status: Value = sqlx::query_scalar(old_consume).fetch_one(&backup).await?;
    assert_eq!(serde_json::from_value::<TaskStatus>(status)?, failed());
    backup.close().await;
    Ok(())
}

async fn assert_preserved_and_failed(
    pg: &PgContainer,
    intermediate: bool,
    before: &Value,
) -> TestResult<()> {
    let (source, keys) = if intermediate {
        ("pl_result_mailbox_v2", "pl_result_mailbox_keys_v2")
    } else {
        ("pl_result_mailbox", "pl_result_mailbox_keys")
    };
    let address_match = if intermediate {
        " AND n.mailbox_key = o.mailbox_key"
    } else {
        ""
    };
    let identical: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT NOT EXISTS ((SELECT * FROM {source} EXCEPT SELECT * FROM pl_result_mailbox_v3) \
         UNION ALL (SELECT * FROM pl_result_mailbox_v3 EXCEPT SELECT * FROM {source})) \
         AND NOT EXISTS (SELECT 1 FROM pl_result_mailbox_metadata_v3 n JOIN {keys} o \
         ON n.mailbox_name = o.mailbox_name AND n.idempotency_key = o.idempotency_key \
         {address_match} WHERE n.forget_at IS DISTINCT FROM o.forget_at)"
    )))
    .fetch_one(&pg.pool)
    .await?;
    assert!(identical, "native rows and expiry changed");
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&pg.url)
        .await?;
    let mailbox = PgResultMailbox::<String>::new(pool.clone(), "legacy");
    for caller in ["retained-failed", "consumed-failed"] {
        let retry = Delivery::new(
            key("tenant-failed"),
            999,
            TaskStatus::Completed,
            Some("local success".into()),
        )
        .with_idempotency_key(caller);
        assert_eq!(
            mailbox.deliver(retry).await?,
            DeliveryOutcome::Duplicate { status: failed() },
            "OA-07:stored-status: local success replaced original failure"
        );
    }
    assert_eq!(
        mailbox
            .consume(&key("tenant-failed"))
            .await?
            .expect("one original failure")
            .status,
        failed()
    );
    assert!(mailbox.consume(&key("tenant-failed")).await?.is_none());
    if intermediate {
        let retry = Delivery::new(key("second-address"), 999, TaskStatus::Completed, None)
            .with_idempotency_key("consumed-failed");
        assert_eq!(
            mailbox.deliver(retry).await?,
            DeliveryOutcome::Duplicate {
                status: TaskStatus::Running
            },
            "OA-07:address-status mixed equal caller keys at different addresses"
        );
        assert!(mailbox.consume(&key("second-address")).await?.is_none());
    }
    drop(mailbox);
    pool.close().await;
    let reopened = PgPoolOptions::new()
        .max_connections(2)
        .connect(&pg.url)
        .await?;
    PgResultMailbox::<String>::migrate(&reopened).await?;
    let mailbox = PgResultMailbox::<String>::new(reopened.clone(), "legacy");
    let retry = Delivery::new(
        key("tenant-failed"),
        999,
        TaskStatus::Completed,
        Some("local success".into()),
    )
    .with_idempotency_key("retained-failed");
    assert_eq!(
        mailbox.deliver(retry).await?,
        DeliveryOutcome::Duplicate { status: failed() },
        "OA-07:consumed-status: reconnect lost original failure"
    );
    assert!(mailbox.consume(&key("tenant-failed")).await?.is_none());
    let after = source_snapshot(&pg.pool, intermediate).await?;
    for field in ["results", "fences", "sequence"] {
        assert_eq!(
            after[field], before[field],
            "source {field} provenance changed"
        );
    }
    reopened.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "needs Docker: starts a parking-lot-pg container"]
async fn mailbox_status_migration_requires_coherent_owner_status_pair() -> TestResult<()> {
    let pg = PgContainer::start(pg_major()).await?;
    populate(&pg.pool, false).await?;
    sqlx::raw_sql(include_str!("fixtures/oa07/ambiguous_status_0_3.sql"))
        .execute(&pg.pool)
        .await?;
    let before = source_snapshot(&pg.pool, false).await?;
    let mut owner_evidence = owners();
    owner_evidence.push((
        "legacy".into(),
        LegacyIdempotencyOwner::new("retained-failed", key("tenant-failed")),
    ));
    let mut status_evidence = statuses();
    status_evidence.push((
        "legacy".into(),
        LegacyDeliveryStatus::new(
            "retained-failed",
            key("tenant-failed"),
            TaskStatus::Completed,
        ),
    ));
    let rejected = PgResultMailbox::<String>::migrate_with_idempotency_metadata(
        &pg.pool,
        &owner_evidence,
        &status_evidence,
    )
    .await;
    assert!(
        format!("{rejected:?}").contains("outside retained generation candidates"),
        "OA-07:coherent-proof mixed another owner's status into this fence"
    );
    assert_eq!(source_snapshot(&pg.pool, false).await?, before);
    status_evidence
        .last_mut()
        .expect("selected status")
        .1
        .status = failed();
    PgResultMailbox::<String>::migrate_with_idempotency_metadata(
        &pg.pool,
        &owner_evidence,
        &status_evidence,
    )
    .await?;
    let mailbox = PgResultMailbox::<String>::new(pg.pool.clone(), "legacy");
    let retry = Delivery::new(key("tenant-failed"), 999, TaskStatus::Completed, None)
        .with_idempotency_key("retained-failed");
    assert_eq!(
        mailbox.deliver(retry).await?,
        DeliveryOutcome::Duplicate { status: failed() }
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs Docker: starts a parking-lot-pg container"]
async fn mailbox_duplicate_status_matches_concurrent_winner_after_consume() -> TestResult<()> {
    let pg = PgContainer::start(pg_major()).await?;
    PgResultMailbox::<String>::migrate(&pg.pool).await?;
    let mailbox = PgResultMailbox::<String>::new(pg.pool.clone(), "status-race");
    let barrier = Arc::new(Barrier::new(18));
    let consumer = {
        let mailbox = mailbox.clone();
        let barrier = Arc::clone(&barrier);
        tokio::spawn(async move {
            barrier.wait().await;
            loop {
                if let Some(entry) = mailbox.consume(&key("race")).await.expect("consume") {
                    return entry;
                }
                tokio::task::yield_now().await;
            }
        })
    };
    let evictor = {
        let mailbox = mailbox.clone();
        let barrier = Arc::clone(&barrier);
        tokio::spawn(async move {
            barrier.wait().await;
            for _ in 0..16 {
                assert_eq!(mailbox.evict_expired().await.expect("eviction"), 0);
            }
        })
    };
    let mut writers = Vec::new();
    for writer in 0..16 {
        let mailbox = mailbox.clone();
        let barrier = Arc::clone(&barrier);
        writers.push(tokio::spawn(async move {
            let status = TaskStatus::Failed(TaskError::executor(format!("winner-{writer}")));
            barrier.wait().await;
            let outcome = mailbox
                .deliver(
                    Delivery::new(key("race"), writer, status.clone(), None)
                        .with_idempotency_key("same-key"),
                )
                .await
                .expect("delivery");
            (status, outcome)
        }));
    }
    let mut stored = Vec::new();
    let mut duplicates = Vec::new();
    for writer in writers {
        let (status, outcome) = writer.await?;
        match outcome {
            DeliveryOutcome::Stored => stored.push(status),
            DeliveryOutcome::Duplicate { status } => duplicates.push(status),
        }
    }
    assert_eq!(stored.len(), 1);
    assert_eq!(duplicates.len(), 15);
    for status in duplicates {
        assert_eq!(
            status, stored[0],
            "OA-07:concurrent-status did not return committed winner"
        );
    }
    let entry = tokio::time::timeout(Duration::from_secs(10), consumer).await??;
    evictor.await?;
    assert_eq!(entry.status, stored[0]);
    assert!(mailbox.consume(&key("race")).await?.is_none());
    let retry = Delivery::new(key("race"), 999, TaskStatus::Completed, None)
        .with_idempotency_key("same-key");
    assert_eq!(
        mailbox.deliver(retry).await?,
        DeliveryOutcome::Duplicate {
            status: stored[0].clone()
        }
    );
    Ok(())
}

#[tokio::test]
#[ignore = "needs Docker: starts a parking-lot-pg container"]
async fn mailbox_status_provenance_replay_does_not_resurrect_expired_fence() -> TestResult<()> {
    let pg = PgContainer::start(pg_major()).await?;
    populate(&pg.pool, true).await?;
    sqlx::query(
        "INSERT INTO pl_result_mailbox_keys_v2 (mailbox_name, mailbox_key, idempotency_key, forget_at) \
         VALUES ('legacy', $1, 'short-consumed', clock_timestamp() + interval '10 seconds')",
    ).bind(serde_json::to_string(&key("short"))?).execute(&pg.pool).await?;
    let mut evidence = statuses_for(true);
    evidence.push((
        "legacy".into(),
        LegacyDeliveryStatus::new("short-consumed", key("short"), failed()),
    ));
    PgResultMailbox::<String>::migrate_with_idempotency_metadata(&pg.pool, &[], &evidence).await?;
    let original_expiry: String = sqlx::query_scalar(
        "SELECT forget_at::text FROM pl_result_mailbox_metadata_v3 \
         WHERE mailbox_name = 'legacy' AND idempotency_key = 'short-consumed'",
    )
    .fetch_one(&pg.pool)
    .await?;
    let mailbox = PgResultMailbox::<String>::new(pg.pool.clone(), "legacy");
    let retry = Delivery::new(key("short"), 999, TaskStatus::Running, None)
        .with_idempotency_key("short-consumed");
    assert_eq!(
        mailbox.deliver(retry.clone()).await?,
        DeliveryOutcome::Duplicate { status: failed() },
        "OA-07:consumed-status original metadata was replaced before TTL"
    );
    sqlx::query("SELECT pg_sleep(greatest(0, extract(epoch FROM ($1::timestamptz - clock_timestamp()))) + 0.05)")
        .bind(&original_expiry).execute(&pg.pool).await?;
    assert_eq!(mailbox.evict_expired().await?, 1);
    PgResultMailbox::<String>::migrate_with_idempotency_metadata(&pg.pool, &[], &evidence).await?;
    assert!(mailbox.consume(&key("short")).await?.is_none());
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pl_result_mailbox_keys_v3 \
         WHERE mailbox_name = 'legacy' AND idempotency_key = 'short-consumed'",
    )
    .fetch_one(&pg.pool)
    .await?;
    assert_eq!(
        count, 0,
        "OA-07:proof-replay resurrected an expired consumed fence"
    );
    assert_eq!(
        mailbox.deliver(retry.clone()).await?,
        DeliveryOutcome::Stored
    );
    PgResultMailbox::<String>::migrate_with_idempotency_metadata(&pg.pool, &[], &evidence).await?;
    assert_eq!(
        mailbox.deliver(retry).await?,
        DeliveryOutcome::Duplicate {
            status: TaskStatus::Running
        }
    );
    let same_expiry: String = sqlx::query_scalar(
        "SELECT forget_at::text FROM pl_result_mailbox_metadata_v3 \
         WHERE mailbox_name = 'legacy' AND idempotency_key = 'short-consumed'",
    )
    .fetch_one(&pg.pool)
    .await?;
    assert_eq!(
        same_expiry, original_expiry,
        "immutable proof lifetime changed"
    );
    let mut conflicting = evidence;
    conflicting.last_mut().expect("short proof").1.status = TaskStatus::Completed;
    assert!(
        PgResultMailbox::<String>::migrate_with_idempotency_metadata(&pg.pool, &[], &conflicting,)
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
#[ignore = "needs Docker: starts a parking-lot-pg container"]
async fn mailbox_status_protocol_rejects_v1_and_v2_mutations() -> TestResult<()> {
    let pg = PgContainer::start(pg_major()).await?;
    populate(&pg.pool, true).await?;
    PgResultMailbox::<String>::migrate_with_idempotency_metadata(
        &pg.pool,
        &[],
        &statuses_for(true),
    )
    .await?;
    let before = source_snapshot(&pg.pool, true).await?;
    sqlx::raw_sql(OLD_SCHEMA).execute(&pg.pool).await?;
    let protocol: i32 =
        sqlx::query_scalar("SELECT version FROM pl_result_mailbox_protocol WHERE singleton")
            .fetch_one(&pg.pool)
            .await?;
    assert_eq!(
        protocol, 3,
        "intermediate v2 migration must reject this protocol"
    );
    for statement in [
        "INSERT INTO pl_result_mailbox (mailbox_name, mailbox_key, task_id, status) VALUES ('old', 'k', 1, '\"Completed\"')",
        "INSERT INTO pl_result_mailbox_keys (mailbox_name, idempotency_key) VALUES ('old', 'k')",
        "INSERT INTO pl_result_mailbox_v2 (mailbox_name, mailbox_key, task_id, status) VALUES ('old', 'k', 1, '\"Completed\"')",
        "INSERT INTO pl_result_mailbox_keys_v2 (mailbox_name, mailbox_key, idempotency_key) VALUES ('old', 'k', 'k') ON CONFLICT (mailbox_name, mailbox_key, idempotency_key) DO UPDATE SET forget_at = EXCLUDED.forget_at WHERE pl_result_mailbox_keys_v2.forget_at <= now()",
        "DELETE FROM pl_result_mailbox_v2 WHERE id = (SELECT id FROM pl_result_mailbox_v2 ORDER BY id LIMIT 1 FOR UPDATE SKIP LOCKED) RETURNING id",
        "DELETE FROM pl_result_mailbox_keys_v2 WHERE forget_at <= now()",
        "UPDATE pl_result_mailbox_keys_v2 SET forget_at = now() WHERE false",
        "TRUNCATE pl_result_mailbox_v2",
        "DELETE FROM pl_result_mailbox_metadata_v3 WHERE false",
    ] {
        let error = sqlx::query(statement)
            .execute(&pg.pool)
            .await
            .expect_err("retired SQL must fail");
        assert!(
            error.to_string().contains("old writers are unsupported"),
            "{error}"
        );
    }
    assert_eq!(source_snapshot(&pg.pool, true).await?, before);
    Ok(())
}

#[tokio::test]
#[ignore = "needs Docker: starts a parking-lot-pg container"]
async fn mailbox_undecodable_duplicate_status_is_error_without_consuming_result() -> TestResult<()>
{
    let pg = PgContainer::start(pg_major()).await?;
    PgResultMailbox::<String>::migrate(&pg.pool).await?;
    let mailbox = PgResultMailbox::<String>::new(pg.pool.clone(), "invalid-metadata");
    let first = Delivery::new(key("a"), 1, failed(), None).with_idempotency_key("key");
    assert_eq!(
        mailbox.deliver(first.clone()).await?,
        DeliveryOutcome::Stored
    );
    sqlx::query("UPDATE pl_result_mailbox_keys_v3 SET status = '\"not-a-status\"'::jsonb WHERE mailbox_name = 'invalid-metadata'")
        .execute(&pg.pool).await?;
    assert!(
        mailbox.deliver(first).await.is_err(),
        "invalid status cannot yield successful duplicate"
    );
    assert_eq!(
        mailbox
            .consume(&key("a"))
            .await?
            .expect("original result remains")
            .status,
        failed()
    );
    assert!(mailbox.consume(&key("a")).await?.is_none());
    Ok(())
}
