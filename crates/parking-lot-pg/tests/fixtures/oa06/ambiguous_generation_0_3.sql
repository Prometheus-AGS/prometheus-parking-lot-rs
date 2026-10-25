-- Synthetic ambiguous legacy generation, applied only to a disposable copy
-- of populated_0_3.sql. Explicit authority may select either retained owner,
-- but may not assign this exact-generation fence to an unrelated address.
INSERT INTO pl_result_mailbox
    (id, mailbox_name, mailbox_key, task_id, status, payload, idempotency_key,
     delivered_at, expires_at) OVERRIDING SYSTEM VALUE
VALUES
    (7, 'legacy', '{"tenant":"candidate-a","user_id":"user","session_id":null}',
     107, '"Completed"', '"candidate-a-result"', 'ambiguous-generation',
     '2026-10-24 10:00:06.123456+00', '2099-10-24 10:00:06.654321+00'),
    (8, 'legacy', '{"tenant":"candidate-b","user_id":"user","session_id":null}',
     108, '"Completed"', '"candidate-b-result"', 'ambiguous-generation',
     '2026-10-24 10:00:07.123456+00', '2099-10-24 10:00:06.654321+00');
INSERT INTO pl_result_mailbox_keys (mailbox_name, idempotency_key, forget_at)
VALUES ('legacy', 'ambiguous-generation', '2099-10-24 10:00:06.654321+00');
