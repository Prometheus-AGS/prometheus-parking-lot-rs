# Design: Assemble and evaluate one candidate boundary packet

## Context

See proposal.md for motivation. Baseline is main 92da0d3 after 0.3.0 and the completed predecessor phase. All accepted phase issues is open in evidence/issue-inventory.json. Use the actual workspace layout in docs/agent-context.md, not old project.json paths.

## Goals / Non-Goals

Goal: One receipt set binds all 17 scenarios, current candidate SHA/contract digest, required OS/backend jobs, raw logs and approvals; no omitted, stale, blocked or not-exercised required scenario is reported PASS. Code completion, certification and publication are reported separately.
Non-goals: unrelated backlog, credential rotation/KBD signer replacement (operator-deferred), new pool technologies, automatic publishing or history rewriting.

## Decisions

Implement this proposed runner and freeze exact per-scenario argv/tool versions before it can run. It coordinates fresh collaborators after all production changes are complete; independent cumulative review has one correction/confirmation cycle. No actual publish or history rewrite inside unattended validation.

Use the incumbent production entry: Proposed bash .github/scripts/operational-assurance-gate.sh --candidate <full-sha> --contract <path> --out <owned-evidence-dir>.
Real regression fixtures belong to this change, not a later catch-all testing task. Only final execution of the integration packet is postponed to oa-18.

Owner tech-lead writes .github/scripts proposed operational-assurance boundary runner; .github/workflows; phase evidence and release report. Tech-lead owns shared public contracts/manifests; other owners receive exact interface handoffs before backend work. Dependencies: oa-01-containment-controls, oa-02-release-versioning, oa-03-signed-release-tags, oa-04-trusted-publishing, oa-05-template-secret-scan, oa-06-tenant-idempotency, oa-07-stored-terminal-outcome, oa-08-discarded-spawn, oa-09-backend-admission, oa-10-expiry-events, oa-11-concurrency-models, oa-12-negative-control-ci, oa-13-browser-assurance, oa-14-postgres-upgrade, oa-15-desktop-assurance, oa-16-iggy-poisoning, oa-17-backend-benchmarks.
Alternative rejected: mock-only or task-count completion, because it cannot establish the observable oracle.

## Risks / Trade-offs

Complete phase implementation may be recorded while external publication/containment authorization remains pending; this never certifies a release.
Unavailable host, privilege or collaborator yields BLOCKED evidence for this scenario; unrelated implementation may continue. No operator-account access is assumed.

## Migration Plan

Deliver the bounded code/tooling and its runbook with a changelog entry for user-visible behavior; retain the prior configuration and receipts for comparison.

## Verification

Scenario boundary aggregate: One receipt set binds all 17 scenarios, current candidate SHA/contract digest, required OS/backend jobs, raw logs and approvals; no omitted, stale, blocked or not-exercised required scenario is reported PASS. Code completion, certification and publication are reported separately.
Negative control: Alter the candidate SHA, omit a required OS receipt, or replace a required scenario with a skip; boundary verification rejects release readiness.
Evidence: candidate SHA, contract digest, exact argv/exit and raw redacted logs, fixture/tool/image/OS identity; final execution by oa-18 task 2 only.

## Safe implementation prerequisites

Recheck actual tool/model availability and version pins from official primary documentation before adding tools. Use existing approved resources; no implicit paid jobs or global host installation. Material scope changes require a contract amendment, not an untracked TODO.
