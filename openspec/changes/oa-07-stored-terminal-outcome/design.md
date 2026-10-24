# Design: Emit durable terminal events from the stored result

## Context

See proposal.md for motivation. Baseline is main 92da0d3 after 0.3.0 and the completed predecessor phase. #78 is open in evidence/issue-inventory.json. Use the actual workspace layout in docs/agent-context.md, not old project.json paths.

## Goals / Non-Goals

Goal: After restart and redelivery, stored status and terminal event agree on Failed, exactly one result is consumable, and delivery still precedes lease resolution. The status remains knowable after consume while the idempotency record is retained.
Non-goals: unrelated backlog, credential rotation/KBD signer replacement (operator-deferred), new pool technologies, automatic publishing or history rewriting.

## Decisions

Extend DeliveryOutcome duplicate metadata to retain authoritative terminal status in idempotency records, including consumed-result cases. Update all ResultMailbox implementers/conformance and migrate retained metadata; do not fetch destructive consume just to learn status.

Use the incumbent production entry: DurableResourcePool deliver-before-resolve path on real PG/redb, killed after Failed delivery and before dead-letter/ack.
Real regression fixtures belong to this change, not a later catch-all testing task. Only final execution of the integration packet is postponed to oa-18.

Owner tech-lead writes crates/parking-lot-core/src/contracts; PG/redb mailbox implementations; crates/parking-lot-runtime-tokio/src/durable_pool/mod.rs. Tech-lead owns shared public contracts/manifests; other owners receive exact interface handoffs before backend work. Dependencies: oa-06-tenant-idempotency.
Alternative rejected: mock-only or task-count completion, because it cannot establish the observable oracle.

## Risks / Trade-offs

Contract/migration changes depend on oa-06; no acknowledged task may lose its durable outcome.
Unavailable host, privilege or collaborator yields BLOCKED evidence for this scenario; unrelated implementation may continue. No operator-account access is assumed.

## Migration Plan

Use a digest-recorded populated 0.3.0 fixture and an untouched backup; document schema/store conversion, old/new binary compatibility and rollback/read restrictions before applying migration. Never point a test at a production store.

## Verification

Scenario OA-07: After restart and redelivery, stored status and terminal event agree on Failed, exactly one result is consumable, and delivery still precedes lease resolution. The status remains knowable after consume while the idempotency record is retained.
Negative control: Old bool-only delivery handling that emits local Completed after a stored Failed must fail the event/result consistency oracle.
Evidence: candidate SHA, contract digest, exact argv/exit and raw redacted logs, fixture/tool/image/OS identity; final execution by oa-18 task 2 only.

## Safe implementation prerequisites

Recheck actual tool/model availability and version pins from official primary documentation before adding tools. Use existing approved resources; no implicit paid jobs or global host installation. Material scope changes require a contract amendment, not an untracked TODO.
