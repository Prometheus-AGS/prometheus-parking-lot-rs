-- Migration: pgmq lease queue side table (cr-30)
--
-- `PgmqLeaseQueue` keeps task state in pgmq queues (one per priority plus a
-- dead-letter queue). pgmq has no idempotency keys and cannot tell a message
-- delayed by a nack from one under a lease, so this table holds exactly those
-- two facts. A row exists only for an entry that carries an idempotency key or
-- was nacked with a delay; it is deleted when the entry is acked or
-- dead-lettered.
--
-- `entry_id` is the entry id the queue hands out: pgmq `msg_id * 4 + lane`.
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
-- Safe to run repeatedly and concurrently.

SET LOCAL lock_timeout = '5s';
SELECT pg_advisory_xact_lock(hashtextextended('pl_pgmq_aux.migration', 0));

-- pgmq itself. A no-op when the image or an earlier migration installed it.
CREATE EXTENSION IF NOT EXISTS pgmq;

CREATE TABLE IF NOT EXISTS pl_pgmq_aux (
    queue_name TEXT NOT NULL,
    entry_id BIGINT NOT NULL,
    -- Task idempotency key; unique per queue while the entry is live.
    idem_key TEXT,
    -- Set by a delayed nack: the entry is "delayed" (not "leased") until then.
    delayed_until TIMESTAMPTZ,
    PRIMARY KEY (queue_name, entry_id)
);

DO $$
BEGIN
    IF to_regclass('pl_pgmq_aux_idem_key') IS NULL THEN
        CREATE UNIQUE INDEX pl_pgmq_aux_idem_key
    ON pl_pgmq_aux (queue_name, idem_key)
    WHERE idem_key IS NOT NULL;
    END IF;
END
$$;

-- Autovacuum (cr-36); this replaces the cr-30 settings (0.02 / 50). Queue tables
-- churn constantly: a row is inserted, updated a few times and deleted. A
-- backlog can also grow them large, and the default trigger (50 + 20% of the
-- live rows) then waits for hundreds of thousands of dead tuples; even the
-- cluster floor in docker/postgres18/postgresql.conf (5%) scales with the
-- backlog. Scale factor 0 with a fixed threshold makes the trigger independent
-- of table size: vacuum after about 500 dead tuples, whatever the
-- table holds. That bounds the dead rows the claim index scans step over, which
-- is what keeps FOR UPDATE SKIP LOCKED cheap. The analyze pair keeps the
-- planner's row estimates fresh enough to choose the partial claim index as the
-- ready/leased/dead mix shifts. The TOAST table behind the JSONB columns needs no
-- toast.* options: with none set it uses the table's own (PostgreSQL docs,
-- CREATE TABLE, storage parameters). ALTER TABLE ... SET takes SHARE UPDATE
-- EXCLUSIVE, which does not block queue reads or writes; the guard skips it
-- when the options are already in place, so a re-run touches nothing.
-- The side table holds a row only for entries with an idempotency key or a
-- nack delay, so it is the smallest of the queue tables.
DO $$
BEGIN
    IF NOT coalesce((SELECT c.reloptions @> ARRAY[
                'autovacuum_vacuum_scale_factor=0',
                'autovacuum_vacuum_threshold=500',
                'autovacuum_analyze_scale_factor=0',
                'autovacuum_analyze_threshold=500']
                       FROM pg_class c WHERE c.oid = 'pl_pgmq_aux'::regclass), false) THEN
        ALTER TABLE pl_pgmq_aux SET (
            autovacuum_vacuum_scale_factor = 0,
            autovacuum_vacuum_threshold = 500,
            autovacuum_analyze_scale_factor = 0,
            autovacuum_analyze_threshold = 500
        );
    END IF;
END
$$;

-- Is the entry delayed by a nack right now? The fence guard of every statement
-- keyed by a lease token (src/pgmq/sql.rs, `held`) calls this in the WHERE
-- clause of its `FOR UPDATE` row lock. It is plpgsql and VOLATILE on purpose:
-- when a statement waits for the row lock and then re-checks the new row
-- version (READ COMMITTED's EvalPlanQual), a plain subquery on this table would
-- still read the statement's old snapshot and miss a nack that committed
-- meanwhile, but every query a volatile plpgsql function runs takes a fresh
-- snapshot. Do not rewrite it as LANGUAGE sql or mark it STABLE: the planner
-- would inline or cache it and the re-check would go blind again.
CREATE OR REPLACE FUNCTION pl_pgmq_is_delayed(queue text, entry bigint) RETURNS boolean
    LANGUAGE plpgsql VOLATILE AS $$
BEGIN
    RETURN EXISTS (SELECT 1 FROM pl_pgmq_aux
                    WHERE queue_name = queue AND entry_id = entry
                      AND delayed_until > clock_timestamp());
END;
$$;

-- pgmq's own tables. pgmq.create() builds pgmq.q_<name> (and pgmq.a_<name>, the
-- archive, which PgmqLeaseQueue never writes to: it deletes). The q_ tables take
-- the same churn as pl_lease_queue (send, read bumps vt and read_ct, delete), so
-- they get the same settings. pgmq creates them with default options and has no
-- hook for per-table storage parameters, hence this function; prepare() calls it
-- after pgmq.create(). The identifier goes through format(%I), never string
-- concatenation, and the function is a no-op for a queue that does not exist.
CREATE OR REPLACE FUNCTION pl_pgmq_tune_queue(queue text) RETURNS void
    LANGUAGE plpgsql AS $$
DECLARE
    q_table regclass := to_regclass(format('pgmq.%I', 'q_' || queue));
BEGIN
    -- Skip the ALTER when the options are already set: prepare() runs at every
    -- start, and the guard keeps it from touching a live queue table at all.
    IF q_table IS NOT NULL AND NOT coalesce((SELECT c.reloptions @> ARRAY[
                'autovacuum_vacuum_scale_factor=0',
                'autovacuum_vacuum_threshold=1000',
                'autovacuum_analyze_scale_factor=0',
                'autovacuum_analyze_threshold=1000']
                FROM pg_class c WHERE c.oid = q_table), false) THEN
        EXECUTE format(
            'ALTER TABLE %s SET (
                autovacuum_vacuum_scale_factor = 0,
                autovacuum_vacuum_threshold = 1000,
                autovacuum_analyze_scale_factor = 0,
                autovacuum_analyze_threshold = 1000)',
            q_table);
    END IF;
END;
$$;
