# Design: Rehearse a populated PostgreSQL 17→18 upgrade

## Context

See proposal.md for motivation. Baseline is main 92da0d3 after 0.3.0 and the completed predecessor phase. #66 is open in evidence/issue-inventory.json. Use the actual workspace layout in docs/agent-context.md, not old project.json paths.

## Goals / Non-Goals

Goal: Queued payloads, mailbox results, tenant dedup records and pgmq state survive the checksum-compatible upgrade; lease/mailbox/pgmq suites pass on upgraded PG18. Source fixture remains recoverable; version/checksum mismatch is detected before destructive work.
Non-goals: unrelated backlog, credential rotation/KBD signer replacement (operator-deferred), new pool technologies, automatic publishing or history rewriting.

## Decisions

Use image digests and disposable volumes; populate a real 0.3.0 store, perform the 0.4 schema migration in its documented order relative to pg_upgrade, verify invariants and correct the runbook.

Use the incumbent production entry: Project PG17 image populated with queue/mailbox/pgmq fixtures, pg_upgrade into project PG18 image, then real integration suites.
Real regression fixtures belong to this change, not a later catch-all testing task. Only final execution of the integration packet is postponed to oa-18.

Owner postgres-engineer writes docker images; docs/runbooks/postgres18.md; proposed disposable upgrade rehearsal script and PG suites. Tech-lead owns shared public contracts/manifests; other owners receive exact interface handoffs before backend work. Dependencies: oa-06-tenant-idempotency, oa-07-stored-terminal-outcome.
Alternative rejected: mock-only or task-count completion, because it cannot establish the observable oracle.

## Risks / Trade-offs

Never use operator data directories or non-test Docker volumes; record cleanup handles and backup checks.
Unavailable host, privilege or collaborator yields BLOCKED evidence for this scenario; unrelated implementation may continue. No operator-account access is assumed.

## Migration Plan

Use a digest-recorded populated 0.3.0 fixture and an untouched backup; document schema/store conversion, old/new binary compatibility and rollback/read restrictions before applying migration. Never point a test at a production store.

## Verification

Scenario OA-14: Queued payloads, mailbox results, tenant dedup records and pgmq state survive the checksum-compatible upgrade; lease/mailbox/pgmq suites pass on upgraded PG18. Source fixture remains recoverable; version/checksum mismatch is detected before destructive work.
Negative control: Wrong checksum/version fixture must be rejected before upgrade; removing a seeded result must fail post-upgrade data verification.
Evidence: candidate SHA, contract digest, exact argv/exit and raw redacted logs, fixture/tool/image/OS identity; final execution by oa-18 task 2 only.

## Safe implementation prerequisites

Recheck actual tool/model availability and version pins from official primary documentation before adding tools. Use existing approved resources; no implicit paid jobs or global host installation. Material scope changes require a contract amendment, not an untracked TODO.
