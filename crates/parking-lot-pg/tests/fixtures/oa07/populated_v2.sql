-- Frozen OA-06 conversion output, independent of the new migration entry point.
-- Run after frozen 005 + populated_0_3 + frozen 008 in one disposable DB.
INSERT INTO pl_result_mailbox_owners_v2 (mailbox_name, idempotency_key, mailbox_key)
VALUES
    ('legacy', 'retained', '{"tenant":"tenant-a","user_id":"user","session_id":null}'),
    ('legacy', 'reused', '{"tenant":"tenant-b","user_id":"user","session_id":null}'),
    ('legacy', 'consumed-finite', '{"tenant":"tenant-a","user_id":"user","session_id":null}'),
    ('legacy', 'consumed-permanent', '{"tenant":"tenant-a","user_id":"user","session_id":null}');
INSERT INTO pl_result_mailbox_keys_v2 (mailbox_name, mailbox_key, idempotency_key, forget_at)
SELECT k.mailbox_name, o.mailbox_key, k.idempotency_key, k.forget_at
FROM pl_result_mailbox_keys k JOIN pl_result_mailbox_owners_v2 o
USING (mailbox_name, idempotency_key);
INSERT INTO pl_result_mailbox_v2 OVERRIDING SYSTEM VALUE SELECT * FROM pl_result_mailbox;
SELECT setval('pl_result_mailbox_v2_id_seq', last_value, is_called) FROM pl_result_mailbox_id_seq;
CREATE TRIGGER pl_result_mailbox_owners_insert
    BEFORE INSERT ON pl_result_mailbox_owners_v2
    FOR EACH STATEMENT EXECUTE FUNCTION pl_result_mailbox_retired();
INSERT INTO pl_result_mailbox_protocol (singleton, version) VALUES (true, 2);
