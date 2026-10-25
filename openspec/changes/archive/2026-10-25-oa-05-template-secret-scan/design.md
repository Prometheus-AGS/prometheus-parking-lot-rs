# Design: Make template scans precise without hiding real secrets

## Context

See proposal.md for motivation. Baseline is main 92da0d3 after 0.3.0 and the completed predecessor phase. #89 is open in evidence/issue-inventory.json. Use the actual workspace layout in docs/agent-context.md, not old project.json paths.

## Goals / Non-Goals

Goal: The two known non-secret template examples no longer fail the full-tree scan; a synthetic nonfunctional secret-pattern canary outside narrowly scoped exceptions still fails. Scan metadata contains no unredacted secret.
Non-goals: unrelated backlog, credential rotation/KBD signer replacement (operator-deferred), new pool technologies, automatic publishing or history rewriting.

## Decisions

Prefer removing ambiguous placeholder material or narrowly fingerprinted example exceptions with rationale. Do not blanket-allowlist template folders or weaken default rules.

Use the incumbent production entry: gitleaks detect --no-git --redact, plus repository staged/diff scans.
Real regression fixtures belong to this change, not a later catch-all testing task. Only final execution of the integration packet is postponed to oa-18.

Owner platform-engineer writes .gitleaks.toml; docs/coding-standards/README.md; .e2b-template/SETUP.md. Tech-lead owns shared public contracts/manifests; other owners receive exact interface handoffs before backend work. Dependencies: none.
Alternative rejected: mock-only or task-count completion, because it cannot establish the observable oracle.

## Risks / Trade-offs

Synthetic nonfunctional test values only; never insert a real key to test a detector.
Unavailable host, privilege or collaborator yields BLOCKED evidence for this scenario; unrelated implementation may continue. No operator-account access is assumed.

## Migration Plan

Deliver the bounded code/tooling and its runbook with a changelog entry for user-visible behavior; retain the prior configuration and receipts for comparison.

## Verification

Scenario OA-05: The two known non-secret template examples no longer fail the full-tree scan; a synthetic nonfunctional secret-pattern canary outside narrowly scoped exceptions still fails. Scan metadata contains no unredacted secret.
Negative control: Synthetic canary outside the exact example paths/rules must be detected; a broad path or rule disable is rejected.
Evidence: candidate SHA, contract digest, exact argv/exit and raw redacted logs, fixture/tool/image/OS identity; final execution by oa-18 task 2 only.

## Safe implementation prerequisites

Recheck actual tool/model availability and version pins from official primary documentation before adding tools. Use existing approved resources; no implicit paid jobs or global host installation. Material scope changes require a contract amendment, not an untracked TODO.
