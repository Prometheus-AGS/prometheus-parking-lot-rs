-- OA-07 protocol 3. Executed only inside the locked owner/status conversion.
-- Refusal rolls back direct 0.3 -> 3 AND intermediate 2 -> 3 atomically.
-- Retain untouched source backups; never downgrade a converted live store.

CREATE TABLE pl_result_mailbox_v3 (LIKE pl_result_mailbox_v2 INCLUDING ALL);
ALTER TABLE pl_result_mailbox_v3 SET (
    autovacuum_vacuum_scale_factor = 0,
    autovacuum_vacuum_threshold = 1000,
    autovacuum_analyze_scale_factor = 0,
    autovacuum_analyze_threshold = 1000
);
CREATE TABLE pl_result_mailbox_keys_v3 (
    mailbox_name TEXT NOT NULL,
    mailbox_key TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    forget_at TIMESTAMPTZ,
    status JSONB NOT NULL,
    PRIMARY KEY (mailbox_name, mailbox_key, idempotency_key)
) WITH (
    autovacuum_vacuum_scale_factor = 0,
    autovacuum_vacuum_threshold = 500,
    autovacuum_analyze_scale_factor = 0,
    autovacuum_analyze_threshold = 500
);
CREATE INDEX idx_pl_result_mailbox_keys_v3_forget
    ON pl_result_mailbox_keys_v3 (forget_at) WHERE forget_at IS NOT NULL;
-- Conversion proof remains after mutable active fences expire or are reused.
CREATE TABLE pl_result_mailbox_metadata_v3
    (LIKE pl_result_mailbox_keys_v3 INCLUDING ALL);

-- Intermediate writers must not create statusless fences or erase provenance.
-- Both actual old table names remain occupied, preventing startup recreation.
CREATE TRIGGER pl_result_mailbox_v2_retired_write
    BEFORE INSERT OR UPDATE OR DELETE OR TRUNCATE ON pl_result_mailbox_v2
    FOR EACH STATEMENT EXECUTE FUNCTION pl_result_mailbox_retired();
CREATE TRIGGER pl_result_mailbox_keys_v2_retired_write
    BEFORE INSERT OR UPDATE OR DELETE OR TRUNCATE ON pl_result_mailbox_keys_v2
    FOR EACH STATEMENT EXECUTE FUNCTION pl_result_mailbox_retired();

ALTER TABLE pl_result_mailbox_protocol
    DROP CONSTRAINT pl_result_mailbox_protocol_version_check,
    ADD CONSTRAINT pl_result_mailbox_protocol_version_check CHECK (version IN (2, 3));
