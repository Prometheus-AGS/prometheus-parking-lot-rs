# Design: Run every scenario negative control in CI

## Context

See proposal.md for motivation. Baseline is main 92da0d3 after 0.3.0 and the completed predecessor phase. #79 is open in evidence/issue-inventory.json. Use the actual workspace layout in docs/agent-context.md, not old project.json paths.

## Goals / Non-Goals

Goal: All listed controls compile, discover the expected nonzero tests and fail only at their designated oracle; normal builds pass. Omitted controls, compile failures, timeouts and missing tests fail the CI job rather than count as successful negative controls.
Non-goals: unrelated backlog, credential rotation/KBD signer replacement (operator-deferred), new pool technologies, automatic publishing or history rewriting.

## Decisions

Create a checked manifest from actual cfg sites and expected assertion/test identities. Existing controls include S-1–S-9 where defined, S-11,S-16–S-21,S-33; use enumerated source inventory rather than assuming every integer exists. Independent targets avoid RUSTFLAGS cache collisions.

Use the incumbent production entry: CI matrix of every existing parking_lot_negative_control cfg plus each new OA regression control, compiled separately before test invocation.
Real regression fixtures belong to this change, not a later catch-all testing task. Only final execution of the integration packet is postponed to oa-18.

Owner platform-engineer writes .github/workflows/ci.yml; proposed negative-control manifest/runner; existing scenario suites. Tech-lead owns shared public contracts/manifests; other owners receive exact interface handoffs before backend work. Dependencies: oa-06-tenant-idempotency, oa-07-stored-terminal-outcome, oa-08-discarded-spawn, oa-09-backend-admission, oa-10-expiry-events, oa-11-concurrency-models.
Alternative rejected: mock-only or task-count completion, because it cannot establish the observable oracle.

## Risks / Trade-offs

No CI green result from arbitrary nonzero exit; retain diagnostic classifier and all raw output.
Unavailable host, privilege or collaborator yields BLOCKED evidence for this scenario; unrelated implementation may continue. No operator-account access is assumed.

## Migration Plan

Deliver the bounded code/tooling and its runbook with a changelog entry for user-visible behavior; retain the prior configuration and receipts for comparison.

## Verification

Scenario OA-12: All listed controls compile, discover the expected nonzero tests and fail only at their designated oracle; normal builds pass. Omitted controls, compile failures, timeouts and missing tests fail the CI job rather than count as successful negative controls.
Negative control: Remove one matrix row or let one defective control pass; the completeness/expected-failure verifier must fail.
Evidence: candidate SHA, contract digest, exact argv/exit and raw redacted logs, fixture/tool/image/OS identity; final execution by oa-18 task 2 only.

## Safe implementation prerequisites

Recheck actual tool/model availability and version pins from official primary documentation before adding tools. Use existing approved resources; no implicit paid jobs or global host installation. Material scope changes require a contract amendment, not an untracked TODO.
