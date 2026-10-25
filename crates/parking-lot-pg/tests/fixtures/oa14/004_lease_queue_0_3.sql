-- Migration: lease queue (cr-26)
--
-- One table holds every queue's live entries and dead letters. Rows are
-- identified by `id`; `queue_name` scopes them to one `PgLeaseQueue`.
--
-- Entry lifecycle (state):
--   ready  -> leased -> (deleted by ack)
--   leased -> ready   (nack requeue, or the reaper returning an expired lease)
--   leased -> dead    (nack dead-letter, or max attempts exceeded)
--   ready  -> dead    (max attempts exceeded, found by a lease or the reaper)
--
-- Isolation: every statement the queue issues runs under READ COMMITTED
-- (the Postgres default) as a single autocommitted statement, except enqueue,
-- which is one short transaction (see queue/sql.rs for the per-statement
-- argument). Nothing here needs REPEATABLE READ or SERIALIZABLE.
--
-- Locks: a re-run must not queue behind, or in front of, queue traffic. Three
-- statements would take strong table locks even when there is nothing to do,
-- so each is guarded by a catalog check (no table lock) and runs only when the
-- object is missing or different:
--   CREATE INDEX IF NOT EXISTS   takes SHARE on the table before it looks for
--                                the index (blocks every writer);
--   (CREATE OR REPLACE) TRIGGER  takes ACCESS EXCLUSIVE / SHARE ROW EXCLUSIVE;
--   ALTER TABLE ... SET (...)    takes SHARE UPDATE EXCLUSIVE (harmless to
--                                traffic, but it rewrites the catalog row).
-- A lock a first run needs is bounded by lock_timeout: the migration fails
-- after 5 s instead of queueing behind a long transaction, and with it every
-- later query on the table.
--
-- Apply with one call (sqlx does) or `psql --single-transaction`: the advisory
-- lock and SET LOCAL last for the transaction.
--
-- Safe to run repeatedly and concurrently: the advisory lock serialises the
-- catalog changes of two processes migrating at once.

SET LOCAL lock_timeout = '5s';
SELECT pg_advisory_xact_lock(hashtextextended('pl_lease_queue.migration', 0));

CREATE TABLE IF NOT EXISTS pl_lease_queue (
    -- Enqueue order within a priority: the FIFO key.
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    queue_name TEXT NOT NULL,
    -- 0 = low .. 3 = critical.
    priority SMALLINT NOT NULL CHECK (priority BETWEEN 0 AND 3),
    -- Task idempotency key; unique among the queue's non-dead entries.
    idempotency_key TEXT,
    -- TaskMetadata (JSON) and the caller's payload (JSON).
    meta JSONB NOT NULL,
    payload JSONB NOT NULL,
    state TEXT NOT NULL DEFAULT 'ready' CHECK (state IN ('ready', 'leased', 'dead')),
    -- Number of leases ever granted. It doubles as the fencing token: each
    -- lease increments it, so a token minted for lease N is stale once the
    -- entry has been leased again (N + 1) and can never be reused.
    attempts INTEGER NOT NULL DEFAULT 0,
    -- Not leasable before this instant (nack backoff). now() at insert.
    not_before TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Set while state = 'leased': the lease is valid strictly before this.
    lease_expires_at TIMESTAMPTZ,
    -- Set while state = 'dead': why the entry was given up on.
    dead_reason TEXT,
    enqueued_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT pl_lease_queue_lease_shape CHECK (
        (state = 'leased') = (lease_expires_at IS NOT NULL)
    ),
    CONSTRAINT pl_lease_queue_dead_shape CHECK (
        (state = 'dead') = (dead_reason IS NOT NULL)
    )
);

-- Claim path: priority-then-FIFO over the entries that can still be leased.
-- Dead letters and acked rows (deleted) stay out of the index.
DO $$
BEGIN
    IF to_regclass('idx_pl_lease_queue_claim') IS NULL THEN
        CREATE INDEX idx_pl_lease_queue_claim
    ON pl_lease_queue (queue_name, priority DESC, id)
    WHERE state <> 'dead';
    END IF;
END
$$;

-- Reaper path: leases ordered by expiry.
DO $$
BEGIN
    IF to_regclass('idx_pl_lease_queue_expiry') IS NULL THEN
        CREATE INDEX idx_pl_lease_queue_expiry
    ON pl_lease_queue (queue_name, lease_expires_at)
    WHERE state = 'leased';
    END IF;
END
$$;

-- First-write-wins idempotency: at most one non-dead entry per key per queue.
-- A dead letter releases its key; an ack deletes the row, which releases it too.
DO $$
BEGIN
    IF to_regclass('uq_pl_lease_queue_idempotency') IS NULL THEN
        CREATE UNIQUE INDEX uq_pl_lease_queue_idempotency
    ON pl_lease_queue (queue_name, idempotency_key)
    WHERE idempotency_key IS NOT NULL AND state <> 'dead';
    END IF;
END
$$;

-- Wake-up: tell idle dispatchers that an entry became leasable. One channel,
-- payload = queue name (channel identifiers are limited to 63 bytes, queue
-- names are not). NOTIFY is delivered at commit, so a listener never wakes for
-- a row it cannot yet see. Entries that are delayed (nack backoff) do not
-- notify: their consumers wake on a timer.
CREATE OR REPLACE FUNCTION pl_lease_queue_notify() RETURNS trigger
    LANGUAGE plpgsql AS $$
BEGIN
    PERFORM pg_notify('pl_lease_queue', NEW.queue_name);
    RETURN NULL;
END;
$$;

-- The triggers are created only when missing (a catalog check, no table lock):
-- CREATE OR REPLACE TRIGGER (PostgreSQL 14 and later) would take SHARE ROW
-- EXCLUSIVE on the table, which blocks every writer, and DROP TRIGGER takes
-- ACCESS EXCLUSIVE, which blocks readers too. A changed trigger definition
-- ships as a new numbered migration.
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_trigger
                    WHERE tgrelid = 'pl_lease_queue'::regclass
                      AND tgname = 'pl_lease_queue_wake_insert') THEN
        CREATE OR REPLACE TRIGGER pl_lease_queue_wake_insert
            AFTER INSERT ON pl_lease_queue
            FOR EACH ROW WHEN (NEW.state = 'ready' AND NEW.not_before <= now())
            EXECUTE FUNCTION pl_lease_queue_notify();
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_trigger
                    WHERE tgrelid = 'pl_lease_queue'::regclass
                      AND tgname = 'pl_lease_queue_wake_update') THEN
        CREATE OR REPLACE TRIGGER pl_lease_queue_wake_update
            AFTER UPDATE OF state, not_before ON pl_lease_queue
            FOR EACH ROW WHEN (
                NEW.state = 'ready' AND NEW.not_before <= now()
                AND (OLD.state IS DISTINCT FROM NEW.state OR OLD.not_before IS DISTINCT FROM NEW.not_before)
            )
            EXECUTE FUNCTION pl_lease_queue_notify();
    END IF;
END
$$;

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
DO $$
BEGIN
    IF NOT coalesce((SELECT c.reloptions @> ARRAY[
                'autovacuum_vacuum_scale_factor=0',
                'autovacuum_vacuum_threshold=1000',
                'autovacuum_analyze_scale_factor=0',
                'autovacuum_analyze_threshold=1000']
                       FROM pg_class c WHERE c.oid = 'pl_lease_queue'::regclass), false) THEN
        ALTER TABLE pl_lease_queue SET (
            autovacuum_vacuum_scale_factor = 0,
            autovacuum_vacuum_threshold = 1000,
            autovacuum_analyze_scale_factor = 0,
            autovacuum_analyze_threshold = 1000
        );
    END IF;
END
$$;

-- COMMENT ON takes SHARE UPDATE EXCLUSIVE, which conflicts with manual VACUUM
-- and ANALYZE, anti-wraparound autovacuum, CREATE INDEX CONCURRENTLY and
-- pg_repack, so a re-run under lock_timeout could fail start-up. Each comment is
-- set only when the catalog text differs (obj_description takes no table lock).
DO $$
BEGIN
    IF obj_description('pl_lease_queue'::regclass, 'pg_class') IS DISTINCT FROM 'Leasing task queue: at-least-once delivery with fenced leases (cr-26)' THEN
        COMMENT ON TABLE pl_lease_queue IS 'Leasing task queue: at-least-once delivery with fenced leases (cr-26)';
    END IF;
    IF col_description('pl_lease_queue'::regclass,
           (SELECT attnum FROM pg_attribute WHERE attrelid = 'pl_lease_queue'::regclass AND attname = 'attempts'))
       IS DISTINCT FROM 'Leases granted so far; also the fencing token of the current lease' THEN
        COMMENT ON COLUMN pl_lease_queue.attempts IS 'Leases granted so far; also the fencing token of the current lease';
    END IF;
END
$$;
