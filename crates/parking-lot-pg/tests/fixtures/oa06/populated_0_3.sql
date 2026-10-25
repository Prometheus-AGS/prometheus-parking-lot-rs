-- Isolated synthetic 0.3 data; ownership is fixture authority only.
-- Native microsecond precision is intentional. Fixed far-future expiries keep
-- these legacy conversion scenarios independent of test execution speed.
INSERT INTO pl_result_mailbox
    (id, mailbox_name, mailbox_key, task_id, status, payload, idempotency_key,
     delivered_at, expires_at) OVERRIDING SYSTEM VALUE
VALUES
    (1, 'legacy', '{"tenant":"tenant-a","user_id":"user","session_id":null}',
     101, '"Completed"', '"retained"', 'retained',
     '2026-10-24 10:00:00.123456+00', '2099-10-24 10:00:00.654321+00'),
    (2, 'legacy', '{"tenant":"tenant-b","user_id":"user","session_id":null}',
     102, '"Completed"', '"unkeyed"', NULL,
     '2026-10-24 10:00:01.123456+00', NULL),
    (3, 'legacy', '{"tenant":"old-generation","user_id":"user","session_id":null}',
     103, '"Completed"', '"expired-generation"', 'reused',
     '2001-10-24 10:00:00.123456+00', '2001-10-24 10:00:00.654321+00'),
    (4, 'legacy', '{"tenant":"tenant-b","user_id":"user","session_id":null}',
     104, '"Completed"', '"current-generation"', 'reused',
     '2026-10-24 10:00:02.123456+00', '2099-10-24 10:00:02.654321+00'),
    (5, 'legacy', '{"tenant":"tenant-a","user_id":"user","session_id":null}',
     105, '"Completed"', '"already-consumed-finite"', 'consumed-finite',
     '2026-10-24 10:00:03.123456+00', '2099-10-24 10:00:03.654321+00'),
    (6, 'legacy', '{"tenant":"tenant-a","user_id":"user","session_id":null}',
     106, '"Completed"', '"already-consumed-permanent"', 'consumed-permanent',
     '2026-10-24 10:00:04.123456+00', NULL);

INSERT INTO pl_result_mailbox_keys (mailbox_name, idempotency_key, forget_at)
SELECT mailbox_name, idempotency_key, expires_at FROM pl_result_mailbox
WHERE id IN (1, 4, 5, 6);
INSERT INTO pl_result_mailbox_keys (mailbox_name, idempotency_key, forget_at)
VALUES ('legacy', 'expired-ownerless', '2001-10-24 10:00:05.654321+00');
DELETE FROM pl_result_mailbox WHERE id IN (5, 6);
-- A consumed/removed task may have advanced the sequence beyond surviving ids.
SELECT setval('pl_result_mailbox_id_seq', 100, true);
