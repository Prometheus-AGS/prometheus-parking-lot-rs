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
-- Safe to run repeatedly and concurrently.

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

CREATE UNIQUE INDEX IF NOT EXISTS pl_pgmq_aux_idem_key
    ON pl_pgmq_aux (queue_name, idem_key)
    WHERE idem_key IS NOT NULL;

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
-- EXCLUSIVE, which does not block queue reads or writes, and re-applying it is
-- a no-op, so the migration stays safe to run at every start.
-- The side table holds a row only for entries with an idempotency key or a
-- nack delay, so it is the smallest of the queue tables.
ALTER TABLE pl_pgmq_aux SET (
    autovacuum_vacuum_scale_factor = 0,
    autovacuum_vacuum_threshold = 500,
    autovacuum_analyze_scale_factor = 0,
    autovacuum_analyze_threshold = 500
);

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
    IF q_table IS NOT NULL THEN
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
