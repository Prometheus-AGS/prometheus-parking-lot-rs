-- Migration: audit events (PostgresAuditSink)
--
-- One row per `AuditEvent`. `event_id` is the primary key, so a redelivered
-- event is stored once (the sink inserts with ON CONFLICT DO NOTHING).
-- `payload` is free text: `AuditEvent::payload` is an `Option<String>`, not
-- JSON.
--
-- Locks: guarded like the other migrations. A catalog check decides whether an
-- index must be created, because CREATE INDEX IF NOT EXISTS takes SHARE on the
-- table (blocking every writer) before it looks for the index, and
-- lock_timeout bounds any lock a first run needs.
--
-- Safe to run repeatedly and concurrently.

SET LOCAL lock_timeout = '5s';
SELECT pg_advisory_xact_lock(hashtextextended('pl_audit_events.migration', 0));

CREATE TABLE IF NOT EXISTS pl_audit_events (
    event_id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    pool TEXT NOT NULL,
    tenant TEXT NOT NULL,
    action TEXT NOT NULL,
    payload TEXT,
    -- The event's own timestamp (when it happened), not when it was written.
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

DO $$
BEGIN
    IF to_regclass('idx_pl_audit_events_tenant_created') IS NULL THEN
        CREATE INDEX idx_pl_audit_events_tenant_created
            ON pl_audit_events (tenant, created_at);
    END IF;
    IF to_regclass('idx_pl_audit_events_task') IS NULL THEN
        CREATE INDEX idx_pl_audit_events_task ON pl_audit_events (task_id);
    END IF;
    IF to_regclass('idx_pl_audit_events_pool') IS NULL THEN
        CREATE INDEX idx_pl_audit_events_pool ON pl_audit_events (pool);
    END IF;
END
$$;

COMMENT ON TABLE pl_audit_events IS 'Audit log written by PostgresAuditSink';
