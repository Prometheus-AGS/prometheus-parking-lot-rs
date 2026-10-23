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
) WITH (autovacuum_vacuum_scale_factor = 0.02, autovacuum_vacuum_threshold = 50);

CREATE UNIQUE INDEX IF NOT EXISTS pl_pgmq_aux_idem_key
    ON pl_pgmq_aux (queue_name, idem_key)
    WHERE idem_key IS NOT NULL;
