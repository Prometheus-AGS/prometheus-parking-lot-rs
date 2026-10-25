# Design: Gate trusted publishing on candidate evidence and approval

## Context

See proposal.md for motivation. Baseline is main 92da0d3 after 0.3.0 and the completed predecessor phase. #85 is open in evidence/issue-inventory.json. Use the actual workspace layout in docs/agent-context.md, not old project.json paths.

## Goals / Non-Goals

Goal: Unapproved or stale boundary receipts, incorrect tag identities and missing environment approval prevent publication; approved candidate rehearsal resolves exactly seven crates and uses OIDC with minimum permissions and no long-lived token fallback. Actual publish/install evidence is required only after separate publication approval.
Non-goals: unrelated backlog, credential rotation/KBD signer replacement (operator-deferred), new pool technologies, automatic publishing or history rewriting.

## Decisions

Split preparation, evidence validation and approval-gated publication; require candidate SHA, contract digest and tag identity to agree. Owner GQAdonis/operator coordinates repository and crates.io permissions; availability is unverified until an attributable account receipt exists.

Use the incumbent production entry: Proposed approval-environment GitHub release workflow dispatch for the frozen candidate, initially a non-publishing rehearsal.
Real regression fixtures belong to this change, not a later catch-all testing task. Only final execution of the integration packet is postponed to oa-18.

Owner platform-engineer writes .github/workflows planned release workflow; docs/PUBLISHING.md; seven crates.io publisher settings (operator). Tech-lead owns shared public contracts/manifests; other owners receive exact interface handoffs before backend work. Dependencies: oa-01-containment-controls, oa-02-release-versioning, oa-03-signed-release-tags.
Alternative rejected: mock-only or task-count completion, because it cannot establish the observable oracle.

## Risks / Trade-offs

No credentials, account settings or third-party resources changed during planning. Publication is not authorized by plan adoption.
Unavailable host, privilege or collaborator yields BLOCKED evidence for this scenario; unrelated implementation may continue. No operator-account access is assumed.

## Migration Plan

Deliver the bounded code/tooling and its runbook with a changelog entry for user-visible behavior; retain the prior configuration and receipts for comparison.

## Verification

Scenario OA-04: Unapproved or stale boundary receipts, incorrect tag identities and missing environment approval prevent publication; approved candidate rehearsal resolves exactly seven crates and uses OIDC with minimum permissions and no long-lived token fallback. Actual publish/install evidence is required only after separate publication approval.
Negative control: Dispatch a scratch/rehearsal candidate with a stale receipt or unapproved environment; no cargo publish invocation may occur.
Evidence: candidate SHA, contract digest, exact argv/exit and raw redacted logs, fixture/tool/image/OS identity; final execution by oa-18 task 2 only.

## Safe implementation prerequisites

Recheck actual tool/model availability and version pins from official primary documentation before adding tools. Use existing approved resources; no implicit paid jobs or global host installation. Material scope changes require a contract amendment, not an untracked TODO.
