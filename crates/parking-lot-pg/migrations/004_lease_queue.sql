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
-- Safe to run repeatedly and concurrently: the advisory lock serialises the
-- catalog changes of two processes migrating at once.

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
CREATE INDEX IF NOT EXISTS idx_pl_lease_queue_claim
    ON pl_lease_queue (queue_name, priority DESC, id)
    WHERE state <> 'dead';

-- Reaper path: leases ordered by expiry.
CREATE INDEX IF NOT EXISTS idx_pl_lease_queue_expiry
    ON pl_lease_queue (queue_name, lease_expires_at)
    WHERE state = 'leased';

-- First-write-wins idempotency: at most one non-dead entry per key per queue.
-- A dead letter releases its key; an ack deletes the row, which releases it too.
CREATE UNIQUE INDEX IF NOT EXISTS uq_pl_lease_queue_idempotency
    ON pl_lease_queue (queue_name, idempotency_key)
    WHERE idempotency_key IS NOT NULL AND state <> 'dead';

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

DROP TRIGGER IF EXISTS pl_lease_queue_wake_insert ON pl_lease_queue;
CREATE TRIGGER pl_lease_queue_wake_insert
    AFTER INSERT ON pl_lease_queue
    FOR EACH ROW WHEN (NEW.state = 'ready' AND NEW.not_before <= now())
    EXECUTE FUNCTION pl_lease_queue_notify();

DROP TRIGGER IF EXISTS pl_lease_queue_wake_update ON pl_lease_queue;
CREATE TRIGGER pl_lease_queue_wake_update
    AFTER UPDATE OF state, not_before ON pl_lease_queue
    FOR EACH ROW WHEN (
        NEW.state = 'ready' AND NEW.not_before <= now()
        AND (OLD.state IS DISTINCT FROM NEW.state OR OLD.not_before IS DISTINCT FROM NEW.not_before)
    )
    EXECUTE FUNCTION pl_lease_queue_notify();

COMMENT ON TABLE pl_lease_queue IS 'Leasing task queue: at-least-once delivery with fenced leases (cr-26)';
COMMENT ON COLUMN pl_lease_queue.attempts IS 'Leases granted so far; also the fencing token of the current lease';
