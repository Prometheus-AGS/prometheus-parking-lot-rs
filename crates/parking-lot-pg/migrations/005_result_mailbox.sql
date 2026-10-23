-- Migration: result mailbox (cr-27)
--
-- Two tables back one named mailbox (`mailbox_name` scopes every row, so
-- several PgResultMailbox instances share the tables without sharing entries):
--
--   pl_result_mailbox       undelivered results, one row per delivery
--   pl_result_mailbox_keys  remembered idempotency keys (first write wins)
--
-- Entry lifecycle: inserted by `deliver`, deleted by `consume` (DELETE ...
-- RETURNING, one statement), or deleted by the TTL sweep once `expires_at`
-- has passed. An expired row is never returned even before the sweep runs.
--
-- A key outlives the entry it arrived with: consuming an entry does not
-- forget its key, so a task redelivered by its queue after the client already
-- took the result cannot hand the client a second one. A key is forgotten when
-- its `forget_at` (the entry's expiry) passes; keys of deliveries without a
-- TTL are kept for good.
--
-- Isolation: every statement runs under READ COMMITTED (the default) as one
-- autocommitted statement. See mailbox/sql.rs for the per-statement argument.
--
-- Safe to run repeatedly and concurrently: the advisory lock serialises the
-- catalog changes of two processes migrating at once.

SELECT pg_advisory_xact_lock(hashtextextended('pl_result_mailbox.migration', 0));

CREATE TABLE IF NOT EXISTS pl_result_mailbox (
    -- Delivery order within a mailbox key.
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    mailbox_name TEXT NOT NULL,
    -- The MailboxKey (tenant, user, session) as canonical JSON text.
    mailbox_key TEXT NOT NULL,
    task_id BIGINT NOT NULL,
    -- TaskStatus (JSON), including a failure's full TaskError.
    status JSONB NOT NULL,
    payload JSONB,
    idempotency_key TEXT,
    -- Database time: expiry does not depend on any client clock.
    delivered_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- NULL: kept until consumed.
    expires_at TIMESTAMPTZ
);

-- Consume / peek path: oldest entry of one key.
CREATE INDEX IF NOT EXISTS idx_pl_result_mailbox_key
    ON pl_result_mailbox (mailbox_name, mailbox_key, id);

-- Sweep path: only rows that can expire.
CREATE INDEX IF NOT EXISTS idx_pl_result_mailbox_expiry
    ON pl_result_mailbox (expires_at)
    WHERE expires_at IS NOT NULL;

CREATE TABLE IF NOT EXISTS pl_result_mailbox_keys (
    mailbox_name TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    -- NULL: remembered for good.
    forget_at TIMESTAMPTZ,
    -- The unique key is what makes the first write win.
    PRIMARY KEY (mailbox_name, idempotency_key)
);

CREATE INDEX IF NOT EXISTS idx_pl_result_mailbox_keys_forget
    ON pl_result_mailbox_keys (forget_at)
    WHERE forget_at IS NOT NULL;

-- Autovacuum (cr-36). Queue tables churn constantly: a row is inserted, updated
-- a few times and deleted. A backlog can also grow them large (a deep queue,
-- undelivered results, dead letters), and the default trigger (50 + 20% of the
-- live rows) then waits for hundreds of thousands of dead tuples; even the
-- cluster floor in docker/postgres18/postgresql.conf (5%) scales with the
-- backlog. Scale factor 0 with a fixed threshold makes the trigger independent
-- of table size: vacuum after about 1000 dead tuples, whatever the
-- table holds. That bounds the dead rows the claim index scans step over, which
-- is what keeps FOR UPDATE SKIP LOCKED cheap. The analyze pair keeps the
-- planner's row estimates fresh enough to choose the partial claim index as the
-- ready/leased/dead mix shifts. The TOAST table behind the JSONB columns needs no
-- toast.* options: with none set it uses the table's own (PostgreSQL docs,
-- CREATE TABLE, storage parameters). ALTER TABLE ... SET takes SHARE UPDATE
-- EXCLUSIVE, which does not block queue reads or writes, and re-applying it is
-- a no-op, so the migration stays safe to run at every start.
-- The keys table is narrow and insert-then-delete only, so its threshold is
-- lower and cheap to reach.
ALTER TABLE pl_result_mailbox SET (
    autovacuum_vacuum_scale_factor = 0,
    autovacuum_vacuum_threshold = 1000,
    autovacuum_analyze_scale_factor = 0,
    autovacuum_analyze_threshold = 1000
);
ALTER TABLE pl_result_mailbox_keys SET (
    autovacuum_vacuum_scale_factor = 0,
    autovacuum_vacuum_threshold = 500,
    autovacuum_analyze_scale_factor = 0,
    autovacuum_analyze_threshold = 500
);

COMMENT ON TABLE pl_result_mailbox IS 'Result mailbox: consume-once delivery with TTL eviction (cr-27)';
COMMENT ON TABLE pl_result_mailbox_keys IS 'Remembered idempotency keys of the result mailbox; first write wins (cr-27)';
