# Design: Reject impossible pre-populated adapter work

## Context

See proposal.md for motivation. Baseline is main 92da0d3 after 0.3.0 and the completed predecessor phase. #82 is open in evidence/issue-inventory.json. Use the actual workspace layout in docs/agent-context.md, not old project.json paths.

## Goals / Non-Goals

Goal: An impossible head task is rejected/dead-lettered with a reason once first observed, valid following work runs, and active vector/kind cost never exceeds limits. Pre-populated valid work remains FIFO within the applicable policy.
Non-goals: unrelated backlog, credential rotation/KBD signer replacement (operator-deferred), new pool technologies, automatic publishing or history rewriting.

## Decisions

Use the existing Dispatcher::check_capacity contract on first observation; persist or emit a terminal reason appropriate to the adapter, without silently dropping entries or looping forever.

Use the incumbent production entry: TaskQueueAdapter backend prefilled before ResourcePool startup with total-oversized and per-kind-oversized tasks.
Real regression fixtures belong to this change, not a later catch-all testing task. Only final execution of the integration packet is postponed to oa-18.

Owner scheduler-engineer writes TaskQueueAdapter and dispatcher first-observation path; core/runtime admission tests. Tech-lead owns shared public contracts/manifests; other owners receive exact interface handoffs before backend work. Dependencies: none.
Alternative rejected: mock-only or task-count completion, because it cannot establish the observable oracle.

## Risks / Trade-offs

Keep one dispatcher owner; distinguish impossible total/kind cost from temporary lack of free capacity.
Unavailable host, privilege or collaborator yields BLOCKED evidence for this scenario; unrelated implementation may continue. No operator-account access is assumed.

## Migration Plan

Deliver the bounded code/tooling and its runbook with a changelog entry for user-visible behavior; retain the prior configuration and receipts for comparison.

## Verification

Scenario OA-09: An impossible head task is rejected/dead-lettered with a reason once first observed, valid following work runs, and active vector/kind cost never exceeds limits. Pre-populated valid work remains FIFO within the applicable policy.
Negative control: Skip first-observation capacity validation; the invalid head blocks valid followers and the oracle fails.
Evidence: candidate SHA, contract digest, exact argv/exit and raw redacted logs, fixture/tool/image/OS identity; final execution by oa-18 task 2 only.

## Safe implementation prerequisites

Recheck actual tool/model availability and version pins from official primary documentation before adding tools. Use existing approved resources; no implicit paid jobs or global host installation. Material scope changes require a contract amendment, not an untracked TODO.
