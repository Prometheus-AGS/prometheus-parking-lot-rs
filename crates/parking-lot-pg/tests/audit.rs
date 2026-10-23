//! Real-Postgres test for [`PostgresAuditSink`]: events recorded through the
//! synchronous `AuditSink::record` reach `pl_audit_events`.

mod support;

use prometheus_parking_lot_core::hooks::audit::{AuditSink, build_audit_event};
use prometheus_parking_lot_pg::PostgresAuditSink;
use support::{PgContainer, TestResult, pg_major};

#[tokio::test]
#[ignore = "needs Docker: starts a parking-lot-pg container"]
async fn recorded_events_are_written_once() -> TestResult<()> {
    let pg = PgContainer::start(pg_major()).await?;
    PostgresAuditSink::migrate(&pg.pool).await?;
    PostgresAuditSink::migrate(&pg.pool).await?; // idempotent

    let mut sink = PostgresAuditSink::spawn(pg.pool.clone(), 1024);
    for n in 0..300 {
        sink.record(build_audit_event(
            format!("evt-{n}"),
            format!("task-{n}"),
            "pool-a",
            "tenant-1",
            "complete",
            (n == 0).then(|| "not json {".to_owned()),
        ));
    }
    // The same event twice is stored once.
    sink.record(build_audit_event(
        "evt-1", "task-1", "pool-a", "tenant-1", "complete", None,
    ));
    assert_eq!(sink.shutdown().await, 0, "no event may be lost");

    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM pl_audit_events")
        .fetch_one(&pg.pool)
        .await?;
    assert_eq!(rows, 300);
    let (task, tenant, action, payload): (String, String, String, Option<String>) = sqlx::query_as(
        "SELECT task_id, tenant, action, payload FROM pl_audit_events WHERE event_id = 'evt-0'",
    )
    .fetch_one(&pg.pool)
    .await?;
    assert_eq!(
        (
            task.as_str(),
            tenant.as_str(),
            action.as_str(),
            payload.as_deref()
        ),
        ("task-0", "tenant-1", "complete", Some("not json {"))
    );
    let recent: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pl_audit_events WHERE created_at > now() - interval '1 minute'",
    )
    .fetch_one(&pg.pool)
    .await?;
    assert_eq!(recent, 300, "created_at is the event's own time");
    Ok(())
}
