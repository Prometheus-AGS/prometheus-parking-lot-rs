# Design: Model ledger races and kind-limited release

## Context

See proposal.md for motivation. Baseline is main 92da0d3 after 0.3.0 and the completed predecessor phase. #64 is open in evidence/issue-inventory.json. Use the actual workspace layout in docs/agent-context.md, not old project.json paths.

## Goals / Non-Goals

Goal: Ledger submit/shutdown and kind-limited permit release explore the bounded schedules without violation; each corresponding broken-order/release negative control compiles then triggers the intended model assertion.
Non-goals: unrelated backlog, credential rotation/KBD signer replacement (operator-deferred), new pool technologies, automatic publishing or history rewriting.

## Decisions

Model the production state machine with explicit mapping to source ordering; no runtime dependency in core normal graph. Bound exploration and retain seed/preemption/compiler receipts.

Use the incumbent production entry: Loom model runs under --cfg parking_lot_loom with LOOM_MAX_PREEMPTIONS=3 and isolated target directories.
Real regression fixtures belong to this change, not a later catch-all testing task. Only final execution of the integration packet is postponed to oa-18.

Owner scheduler-engineer writes crates/parking-lot-core/tests/loom_*; runtime shutdown ledger model and cfg wiring; .github/workflows/ci.yml. Tech-lead owns shared public contracts/manifests; other owners receive exact interface handoffs before backend work. Dependencies: oa-08-discarded-spawn, oa-09-backend-admission, oa-10-expiry-events.
Alternative rejected: mock-only or task-count completion, because it cannot establish the observable oracle.

## Risks / Trade-offs

A model is supporting evidence; real runtime/backends and shutdown/crash paths remain required.
Unavailable host, privilege or collaborator yields BLOCKED evidence for this scenario; unrelated implementation may continue. No operator-account access is assumed.

## Migration Plan

Deliver the bounded code/tooling and its runbook with a changelog entry for user-visible behavior; retain the prior configuration and receipts for comparison.

## Verification

Scenario OA-11: Ledger submit/shutdown and kind-limited permit release explore the bounded schedules without violation; each corresponding broken-order/release negative control compiles then triggers the intended model assertion.
Negative control: Remove the named happens-before edge or kind-release synchronization in an isolated cfg build and require the model to fail for that reason.
Evidence: candidate SHA, contract digest, exact argv/exit and raw redacted logs, fixture/tool/image/OS identity; final execution by oa-18 task 2 only.

## Safe implementation prerequisites

Recheck actual tool/model availability and version pins from official primary documentation before adding tools. Use existing approved resources; no implicit paid jobs or global host installation. Material scope changes require a contract amendment, not an untracked TODO.
