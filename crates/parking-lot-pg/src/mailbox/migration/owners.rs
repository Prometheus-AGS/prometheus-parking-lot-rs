//! Frozen protocol-2 ownership conversion, executed inside protocol-3's transaction.

use std::collections::BTreeMap;

use prometheus_parking_lot_core::{MailboxKey, SchedulerError};
use sqlx::{PgConnection, Row};

use super::invalid;
use crate::mailbox::{backend, key_text};

const LEGACY: &str = include_str!("../../../migrations/005_result_mailbox.sql");
const SCOPED: &str = include_str!("../../../migrations/008_scoped_result_mailbox.sql");

pub(super) async fn convert(
    conn: &mut PgConnection,
    supplied: BTreeMap<(String, String), String>,
) -> Result<(), SchedulerError> {
    let (has_results, has_keys): (bool, bool) = sqlx::query_as(
        "SELECT to_regclass('pl_result_mailbox') IS NOT NULL, \
                to_regclass('pl_result_mailbox_keys') IS NOT NULL",
    )
    .fetch_one(&mut *conn)
    .await
    .map_err(backend("inspect legacy layout"))?;
    if has_results != has_keys {
        return Err(invalid("incomplete legacy mailbox layout"));
    }
    if !has_results {
        // 005 stays immutable. Existing source data is inspected before any
        // schema changes; only a fresh database needs the empty legacy layout.
        sqlx::raw_sql(LEGACY)
            .execute(&mut *conn)
            .await
            .map_err(backend("create legacy layout"))?;
    }
    sqlx::raw_sql("LOCK TABLE pl_result_mailbox, pl_result_mailbox_keys IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *conn)
        .await
        .map_err(backend("lock legacy data"))?;

    let resolved = resolve_owners(conn, supplied).await?;

    sqlx::raw_sql(SCOPED)
        .execute(&mut *conn)
        .await
        .map_err(backend("create scoped layout"))?;
    for (name, caller, key) in resolved {
        sqlx::query(
            "INSERT INTO pl_result_mailbox_owners_v2 \
             (mailbox_name, idempotency_key, mailbox_key) VALUES ($1, $2, $3)",
        )
        .bind(name)
        .bind(caller)
        .bind(key)
        .execute(&mut *conn)
        .await
        .map_err(backend("record legacy owner"))?;
    }
    sqlx::raw_sql(
        "INSERT INTO pl_result_mailbox_keys_v2 \
             (mailbox_name, mailbox_key, idempotency_key, forget_at) \
         SELECT k.mailbox_name, o.mailbox_key, k.idempotency_key, k.forget_at \
         FROM pl_result_mailbox_keys k JOIN pl_result_mailbox_owners_v2 o \
           USING (mailbox_name, idempotency_key); \
         INSERT INTO pl_result_mailbox_v2 OVERRIDING SYSTEM VALUE \
         SELECT * FROM pl_result_mailbox; \
         SELECT setval('pl_result_mailbox_v2_id_seq', last_value, is_called) \
         FROM pl_result_mailbox_id_seq; \
         CREATE TRIGGER pl_result_mailbox_owners_insert \
             BEFORE INSERT ON pl_result_mailbox_owners_v2 \
             FOR EACH STATEMENT EXECUTE FUNCTION pl_result_mailbox_retired(); \
         INSERT INTO pl_result_mailbox_protocol (singleton, version) VALUES (true, 2);",
    )
    .execute(&mut *conn)
    .await
    .map_err(backend("copy legacy data"))?;
    Ok(())
}

pub(super) async fn validate_recorded(
    conn: &mut PgConnection,
    supplied: BTreeMap<(String, String), String>,
) -> Result<(), SchedulerError> {
    for ((name, caller), key) in supplied {
        let recorded: Option<String> = sqlx::query_scalar(
            "SELECT mailbox_key FROM pl_result_mailbox_owners_v2 \
             WHERE mailbox_name = $1 AND idempotency_key = $2",
        )
        .bind(name)
        .bind(caller)
        .fetch_optional(&mut *conn)
        .await
        .map_err(backend("read legacy owner"))?;
        if recorded.as_deref() != Some(key.as_str()) {
            return Err(invalid("unknown or conflicting legacy owner assignment"));
        }
    }
    Ok(())
}

async fn resolve_owners(
    conn: &mut PgConnection,
    mut supplied: BTreeMap<(String, String), String>,
) -> Result<Vec<(String, String, String)>, SchedulerError> {
    // Timestamp equality stays in SQL at native precision. A stale expired row
    // of an earlier generation must never identify a reused key's owner.
    let fences = sqlx::query(
        "SELECT k.mailbox_name, k.idempotency_key, \
                (k.forget_at IS NULL OR k.forget_at > now()) AS active, \
                array_agg(r.mailbox_key ORDER BY r.id) \
                    FILTER (WHERE r.id IS NOT NULL) AS candidates \
         FROM pl_result_mailbox_keys k LEFT JOIN pl_result_mailbox r \
           ON r.mailbox_name = k.mailbox_name \
          AND r.idempotency_key = k.idempotency_key \
          AND r.expires_at IS NOT DISTINCT FROM k.forget_at \
         GROUP BY k.mailbox_name, k.idempotency_key, k.forget_at",
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(backend("inspect legacy fences"))?;
    let mut resolved = Vec::new();
    for fence in fences {
        let name: String = fence
            .try_get("mailbox_name")
            .map_err(backend("decode legacy name"))?;
        let caller: String = fence
            .try_get("idempotency_key")
            .map_err(backend("decode legacy key"))?;
        let active: bool = fence
            .try_get("active")
            .map_err(backend("decode legacy expiry"))?;
        if !active {
            // Expired ownerless provenance remains in the retired table. A
            // supplied assignment for it is rejected below as unnecessary.
            continue;
        }
        let candidates: Option<Vec<String>> = fence
            .try_get("candidates")
            .map_err(backend("decode legacy candidates"))?;
        let candidates: Vec<String> = candidates
            .unwrap_or_default()
            .iter()
            .map(|candidate| {
                let key: MailboxKey =
                    serde_json::from_str(candidate).map_err(backend("decode legacy address"))?;
                key_text(&key)
            })
            .collect::<Result<_, _>>()?;
        let mut candidates = candidates;
        candidates.sort();
        candidates.dedup();
        let inferred = match candidates.as_slice() {
            [candidate] => Some(candidate.clone()),
            _ => None,
        };
        let supplied_key = supplied.remove(&(name.clone(), caller.clone()));
        let key = match (inferred, supplied_key) {
            (Some(proved), Some(assigned)) if proved != assigned => {
                return Err(invalid(
                    "legacy owner assignment contradicts retained result",
                ));
            }
            (Some(proved), _) => proved,
            (None, Some(assigned)) => {
                // Authoritative evidence may choose between retained owners,
                // but cannot contradict every exact-generation candidate.
                // Consumed fences have no candidates and still allow mapping.
                if !candidates.is_empty() && !candidates.contains(&assigned) {
                    return Err(invalid(
                        "legacy owner assignment is outside retained generation candidates",
                    ));
                }
                assigned
            }
            (None, None) => {
                return Err(invalid(
                    "active legacy fence has no unique retained owner; authoritative mapping required",
                ));
            }
        };
        resolved.push((name, caller, key));
    }
    if !supplied.is_empty() {
        return Err(invalid("unknown or expired legacy owner assignment"));
    }
    Ok(resolved)
}
