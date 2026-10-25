# Design: Build and launch the Tauri embedded example

## Context

See proposal.md for motivation. Baseline is main 92da0d3 after 0.3.0 and the completed predecessor phase. #67 is open in evidence/issue-inventory.json. Use the actual workspace layout in docs/agent-context.md, not old project.json paths.

## Goals / Non-Goals

Goal: Tauri shell builds on macOS/Linux/Windows. At least macOS actual GUI launch submits work, restarts the app and retrieves the persisted result; platform-specific manual checks are attributable and may not be replaced by queue-service unit tests.
Non-goals: unrelated backlog, credential rotation/KBD signer replacement (operator-deferred), new pool technologies, automatic publishing or history rewriting.

## Decisions

Keep the incumbent UI unchanged. Exact supported desktop launch matrix is frozen in the contract: builds on all three OS, actual GUI restart operation on macOS; if Linux/Windows runtime support is claimed, require corresponding launch evidence before that claim.

Use the incumbent production entry: Real Tauri application launch on macOS and Linux/Windows shell builds, plus app submit/restart/retrieve operation.
Real regression fixtures belong to this change, not a later catch-all testing task. Only final execution of the integration packet is postponed to oa-18.

Owner platform-engineer writes bindings/tauri-desktop; .github/workflows/platform.yml; desktop launch/restart evidence runbook. Tech-lead owns shared public contracts/manifests; other owners receive exact interface handoffs before backend work. Dependencies: oa-06-tenant-idempotency.
Alternative rejected: mock-only or task-count completion, because it cannot establish the observable oracle.

## Risks / Trade-offs

No unattended GUI input to unrelated apps; no production user directories; no new paid runner or host package installation without approval.
Unavailable host, privilege or collaborator yields BLOCKED evidence for this scenario; unrelated implementation may continue. No operator-account access is assumed.

## Migration Plan

Deliver the bounded code/tooling and its runbook with a changelog entry for user-visible behavior; retain the prior configuration and receipts for comparison.

## Verification

Scenario OA-15: Tauri shell builds on macOS/Linux/Windows. At least macOS actual GUI launch submits work, restarts the app and retrieves the persisted result; platform-specific manual checks are attributable and may not be replaced by queue-service unit tests.
Negative control: Delete only the test app's store before restart; retrieval oracle fails. A build-only or queue-service-only run cannot satisfy GUI evidence.
Evidence: candidate SHA, contract digest, exact argv/exit and raw redacted logs, fixture/tool/image/OS identity; final execution by oa-18 task 2 only.

## Safe implementation prerequisites

Recheck actual tool/model availability and version pins from official primary documentation before adding tools. Use existing approved resources; no implicit paid jobs or global host installation. Material scope changes require a contract amendment, not an untracked TODO.
