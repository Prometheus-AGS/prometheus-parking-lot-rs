//! Atomic ownership and original-status conversion (ADRs 0006 and 0007).

mod owners;

use std::collections::BTreeMap;

use prometheus_parking_lot_core::{
    LegacyDeliveryStatus, LegacyIdempotencyOwner, SchedulerError, TaskStatus,
};
use sqlx::{PgConnection, PgPool, Row};

use super::{backend, key_text};

type StatusEvidence = BTreeMap<(String, String, String), TaskStatus>;

const STATUS_SCHEMA: &str = include_str!("../../migrations/009_result_mailbox_status.sql");

pub(super) async fn migrate(
    pool: &PgPool,
    owners: &[(String, LegacyIdempotencyOwner)],
    statuses: &[(String, LegacyDeliveryStatus)],
) -> Result<(), SchedulerError> {
    let mut supplied_owners = BTreeMap::new();
    for (name, owner) in owners {
        let identity = (name.clone(), owner.idempotency_key.clone());
        if supplied_owners
            .insert(identity, key_text(&owner.key)?)
            .is_some()
        {
            return Err(invalid("duplicate legacy owner assignment"));
        }
    }
    let mut supplied_statuses = BTreeMap::new();
    for (name, evidence) in statuses {
        let identity = (
            name.clone(),
            key_text(&evidence.key)?,
            evidence.idempotency_key.clone(),
        );
        if supplied_statuses
            .insert(identity, evidence.status.clone())
            .is_some()
        {
            return Err(invalid("duplicate legacy status assignment"));
        }
    }

    let mut tx = pool.begin().await.map_err(backend("begin migration"))?;
    sqlx::raw_sql(
        "SET LOCAL lock_timeout = '5s'; \
         SELECT pg_advisory_xact_lock(hashtextextended('pl_result_mailbox.migration', 0));",
    )
    .execute(&mut *tx)
    .await
    .map_err(backend("lock migration"))?;
    let has_protocol: bool =
        sqlx::query_scalar("SELECT to_regclass('pl_result_mailbox_protocol') IS NOT NULL")
            .fetch_one(&mut *tx)
            .await
            .map_err(backend("inspect protocol"))?;
    let version: i32 = if has_protocol {
        sqlx::query_scalar("SELECT version FROM pl_result_mailbox_protocol WHERE singleton")
            .fetch_one(&mut *tx)
            .await
            .map_err(backend("read protocol"))?
    } else {
        1
    };
    match version {
        1 => owners::convert(&mut tx, supplied_owners).await?,
        2 | 3 => owners::validate_recorded(&mut tx, supplied_owners).await?,
        _ => return Err(invalid("unsupported mailbox protocol")),
    }
    if version == 3 {
        validate_status_replay(&mut tx, supplied_statuses).await?;
    } else {
        // The ownership conversion above has NOT committed. Refusing missing
        // status rolls back direct 0.3 -> 3, including every protocol-2 object.
        sqlx::raw_sql(
            "LOCK TABLE pl_result_mailbox_v2, pl_result_mailbox_keys_v2 IN ACCESS EXCLUSIVE MODE",
        )
        .execute(&mut *tx)
        .await
        .map_err(backend("lock scoped data"))?;
        let resolved = resolve_statuses(&mut tx, supplied_statuses).await?;
        install_status_schema(&mut tx, resolved).await?;
    }
    tx.commit().await.map_err(backend("commit migration"))
}

async fn resolve_statuses(
    conn: &mut PgConnection,
    mut supplied: StatusEvidence,
) -> Result<StatusEvidence, SchedulerError> {
    let fences = sqlx::query(
        "SELECT k.mailbox_name, k.mailbox_key, k.idempotency_key, \
                coalesce(jsonb_agg(r.status ORDER BY r.id) \
                    FILTER (WHERE r.id IS NOT NULL), '[]'::jsonb) AS candidates \
         FROM pl_result_mailbox_keys_v2 k LEFT JOIN pl_result_mailbox_v2 r \
           ON r.mailbox_name = k.mailbox_name AND r.mailbox_key = k.mailbox_key \
          AND r.idempotency_key = k.idempotency_key \
          AND r.expires_at IS NOT DISTINCT FROM k.forget_at \
         WHERE k.forget_at IS NULL OR k.forget_at > now() \
         GROUP BY k.mailbox_name, k.mailbox_key, k.idempotency_key",
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(backend("inspect original statuses"))?;
    let mut resolved = BTreeMap::new();
    for fence in fences {
        let name: String = fence
            .try_get("mailbox_name")
            .map_err(backend("decode status namespace"))?;
        let address: String = fence
            .try_get("mailbox_key")
            .map_err(backend("decode status address"))?;
        let caller: String = fence
            .try_get("idempotency_key")
            .map_err(backend("decode status key"))?;
        let values: serde_json::Value = fence
            .try_get("candidates")
            .map_err(backend("decode statuses"))?;
        let candidates: Vec<TaskStatus> =
            serde_json::from_value(values).map_err(backend("decode original status"))?;
        let identity = (name, address, caller);
        let assigned = supplied.remove(&identity);
        let inferred = candidates
            .first()
            .filter(|first| candidates.iter().all(|s| s == *first));
        let status = match (inferred, assigned) {
            (Some(proved), Some(assigned)) if *proved != assigned => {
                return Err(invalid("status assignment contradicts retained generation"));
            }
            (Some(proved), _) => proved.clone(),
            (None, Some(assigned)) => {
                if !candidates.is_empty() && !candidates.contains(&assigned) {
                    return Err(invalid(
                        "status assignment is outside retained generation candidates",
                    ));
                }
                assigned
            }
            (None, None) => {
                return Err(invalid(
                    "active legacy fence has no unique original status; authoritative status required",
                ));
            }
        };
        resolved.insert(identity, status);
    }
    if !supplied.is_empty() {
        return Err(invalid(
            "unknown, expired or wrong-address status assignment",
        ));
    }
    Ok(resolved)
}

async fn install_status_schema(
    conn: &mut PgConnection,
    resolved: StatusEvidence,
) -> Result<(), SchedulerError> {
    sqlx::raw_sql(STATUS_SCHEMA)
        .execute(&mut *conn)
        .await
        .map_err(backend("create status schema"))?;
    for ((name, address, caller), status) in resolved {
        sqlx::query(
            "INSERT INTO pl_result_mailbox_metadata_v3 \
                 (mailbox_name, mailbox_key, idempotency_key, forget_at, status) \
             SELECT mailbox_name, mailbox_key, idempotency_key, forget_at, $4::jsonb \
             FROM pl_result_mailbox_keys_v2 \
             WHERE mailbox_name = $1 AND mailbox_key = $2 AND idempotency_key = $3",
        )
        .bind(name)
        .bind(address)
        .bind(caller)
        .bind(serde_json::to_value(status).map_err(backend("encode original status"))?)
        .execute(&mut *conn)
        .await
        .map_err(backend("record original status"))?;
    }
    sqlx::raw_sql(
        "INSERT INTO pl_result_mailbox_keys_v3 SELECT * FROM pl_result_mailbox_metadata_v3; \
         INSERT INTO pl_result_mailbox_v3 OVERRIDING SYSTEM VALUE SELECT * FROM pl_result_mailbox_v2; \
         SELECT setval('pl_result_mailbox_v3_id_seq', last_value, is_called) \
             FROM pl_result_mailbox_v2_id_seq; \
         CREATE TRIGGER pl_result_mailbox_metadata_v3_retired_write \
             BEFORE INSERT OR UPDATE OR DELETE OR TRUNCATE ON pl_result_mailbox_metadata_v3 \
             FOR EACH STATEMENT EXECUTE FUNCTION pl_result_mailbox_retired(); \
         UPDATE pl_result_mailbox_protocol SET version = 3 WHERE singleton;",
    )
    .execute(&mut *conn).await.map_err(backend("copy original outcomes"))?;
    Ok(())
}

async fn validate_status_replay(
    conn: &mut PgConnection,
    supplied: StatusEvidence,
) -> Result<(), SchedulerError> {
    for ((name, address, caller), status) in supplied {
        let recorded: Option<serde_json::Value> = sqlx::query_scalar(
            "SELECT status FROM pl_result_mailbox_metadata_v3 \
             WHERE mailbox_name = $1 AND mailbox_key = $2 AND idempotency_key = $3",
        )
        .bind(name)
        .bind(address)
        .bind(caller)
        .fetch_optional(&mut *conn)
        .await
        .map_err(backend("read original status proof"))?;
        let recorded = recorded
            .map(serde_json::from_value::<TaskStatus>)
            .transpose()
            .map_err(backend("decode original status proof"))?;
        if recorded.as_ref() != Some(&status) {
            return Err(invalid("unknown or conflicting original status replay"));
        }
    }
    Ok(())
}

fn invalid(message: &str) -> SchedulerError {
    SchedulerError::internal(format!("pg result mailbox migration: {message}"))
}
