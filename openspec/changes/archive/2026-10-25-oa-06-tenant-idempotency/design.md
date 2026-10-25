# Design: Scope result idempotency to tenant/mailbox key

## Context

See proposal.md for motivation. Baseline is main 92da0d3 after 0.3.0 and the completed predecessor phase. #80 is open in evidence/issue-inventory.json. Use the actual workspace layout in docs/agent-context.md, not old project.json paths.

## Goals / Non-Goals

Goal: Two tenants reusing the same caller key independently retrieve their own first result; a duplicate within one tenant remains first-write-wins after consume/restart/TTL rules. Migration preserves existing results and dedup meaning under the recorded upgrade policy.
Non-goals: unrelated backlog, credential rotation/KBD signer replacement (operator-deferred), new pool technologies, automatic publishing or history rewriting.

## Decisions

Tech-lead owns the public contract and conformance change; postgres-engineer and events-engineer implement the two backends after that contract. Preserve old database/store fixtures and require explicit safe migration.

Use the incumbent production entry: ResultMailbox::deliver/consume through PG17, PG18 and redb, including populated 0.3.0 stores.
Real regression fixtures belong to this change, not a later catch-all testing task. Only final execution of the integration packet is postponed to oa-18.

Owner tech-lead writes crates/parking-lot-core/src/contracts (ResultMailbox and conformance); crates/parking-lot-pg/src/mailbox and migrations; crates/parking-lot-embedded/src/mailbox.rs; docs/API.md. Tech-lead owns shared public contracts/manifests; other owners receive exact interface handoffs before backend work. Dependencies: none.
Alternative rejected: mock-only or task-count completion, because it cannot establish the observable oracle.

## Risks / Trade-offs

No production store mutation; migration fixtures are isolated copies with manifests and rollback/read-compatibility policy.
Unavailable host, privilege or collaborator yields BLOCKED evidence for this scenario; unrelated implementation may continue. No operator-account access is assumed.

## Migration Plan

Use a digest-recorded populated 0.3.0 fixture and an untouched backup; document schema/store conversion, old/new binary compatibility and rollback/read restrictions before applying migration. Never point a test at a production store.

## Verification

Scenario OA-06: Two tenants reusing the same caller key independently retrieve their own first result; a duplicate within one tenant remains first-write-wins after consume/restart/TTL rules. Migration preserves existing results and dedup meaning under the recorded upgrade policy.
Negative control: The old mailbox-global-key behavior must fail the cross-tenant case; a duplicate within one tenant must not produce a second stored result.
Evidence: candidate SHA, contract digest, exact argv/exit and raw redacted logs, fixture/tool/image/OS identity; final execution by oa-18 task 2 only.

## Safe implementation prerequisites

Recheck actual tool/model availability and version pins from official primary documentation before adding tools. Use existing approved resources; no implicit paid jobs or global host installation. Material scope changes require a contract amendment, not an untracked TODO.
