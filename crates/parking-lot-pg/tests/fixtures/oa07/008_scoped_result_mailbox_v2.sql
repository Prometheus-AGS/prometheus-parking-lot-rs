-- OA-06, protocol 2. Run ONLY inside migrate_with_idempotency_owners' locked
-- transaction after ownership resolution. 005 remains the immutable 0.3 schema.
-- Stop old readers/writers and keep an untouched backup before conversion.
-- Rollback means restoring that backup with all new writers stopped; a 0.3
-- binary must never open this converted store. No live upgrade is authorized.

CREATE TABLE pl_result_mailbox_v2 (
    LIKE pl_result_mailbox INCLUDING ALL
);
ALTER TABLE pl_result_mailbox_v2 SET (
    autovacuum_vacuum_scale_factor = 0,
    autovacuum_vacuum_threshold = 1000,
    autovacuum_analyze_scale_factor = 0,
    autovacuum_analyze_threshold = 1000
);

CREATE TABLE pl_result_mailbox_keys_v2 (
    mailbox_name TEXT NOT NULL,
    mailbox_key TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    forget_at TIMESTAMPTZ,
    PRIMARY KEY (mailbox_name, mailbox_key, idempotency_key)
) WITH (
    autovacuum_vacuum_scale_factor = 0,
    autovacuum_vacuum_threshold = 500,
    autovacuum_analyze_scale_factor = 0,
    autovacuum_analyze_threshold = 500
);
CREATE INDEX idx_pl_result_mailbox_keys_v2_forget
    ON pl_result_mailbox_keys_v2 (forget_at) WHERE forget_at IS NOT NULL;

-- Immutable ownership evidence from conversion, independent of mutable v2 TTL
-- fences. Retrying a conversion can validate assignments without extending TTL.
CREATE TABLE pl_result_mailbox_owners_v2 (
    mailbox_name TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    mailbox_key TEXT NOT NULL,
    PRIMARY KEY (mailbox_name, idempotency_key)
);
CREATE TABLE pl_result_mailbox_protocol (
    singleton BOOLEAN PRIMARY KEY CHECK (singleton),
    version INTEGER NOT NULL CHECK (version = 2)
);

-- Keep the actual old names occupied by their original tables. 0.3's startup
-- migration cannot recreate a second writable namespace. Statement triggers
-- reject even empty deletes/updates, and TRUNCATE cannot erase provenance.
CREATE FUNCTION pl_result_mailbox_retired() RETURNS trigger
    LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'retired result mailbox protocol: old writers are unsupported'
        USING ERRCODE = '55000';
END;
$$;
CREATE TRIGGER pl_result_mailbox_retired_write
    BEFORE INSERT OR UPDATE OR DELETE OR TRUNCATE ON pl_result_mailbox
    FOR EACH STATEMENT EXECUTE FUNCTION pl_result_mailbox_retired();
CREATE TRIGGER pl_result_mailbox_keys_retired_write
    BEFORE INSERT OR UPDATE OR DELETE OR TRUNCATE ON pl_result_mailbox_keys
    FOR EACH STATEMENT EXECUTE FUNCTION pl_result_mailbox_retired();
CREATE TRIGGER pl_result_mailbox_owners_retired_write
    BEFORE UPDATE OR DELETE OR TRUNCATE ON pl_result_mailbox_owners_v2
    FOR EACH STATEMENT EXECUTE FUNCTION pl_result_mailbox_retired();
