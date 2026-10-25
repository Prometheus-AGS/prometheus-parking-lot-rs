# Design: Emit terminal events for explicitly pruned work

## Context

See proposal.md for motivation. Baseline is main 92da0d3 after 0.3.0 and the completed predecessor phase. #57 is open in evidence/issue-inventory.json. Use the actual workspace layout in docs/agent-context.md, not old project.json paths.

## Goals / Non-Goals

Goal: Each pruned task emits exactly one Expired terminal event, releases/settles its accepted-work accounting, and cannot execute; non-expired work still runs. The public return behavior is documented and all backends implement the changed metadata contract.
Non-goals: unrelated backlog, credential rotation/KBD signer replacement (operator-deferred), new pool technologies, automatic publishing or history rewriting.

## Decisions

Return pruned task metadata through the contract; implement across queue adapters, retain the public count if compatible, and update conformance and changelog.

Use the incumbent production entry: ResourcePool::prune_expired on parked tasks through real TaskQueue implementations and recording hooks.
Real regression fixtures belong to this change, not a later catch-all testing task. Only final execution of the integration packet is postponed to oa-18.

Owner tech-lead writes crates/parking-lot-core/src/contracts/queue.rs; dispatcher and all TaskQueue implementers; ResourcePool pruning; lifecycle tests. Tech-lead owns shared public contracts/manifests; other owners receive exact interface handoffs before backend work. Dependencies: none.
Alternative rejected: mock-only or task-count completion, because it cannot establish the observable oracle.

## Risks / Trade-offs

Do not emit terminal events while holding dispatcher locks; preserve existing event causality and panic boundaries.
Unavailable host, privilege or collaborator yields BLOCKED evidence for this scenario; unrelated implementation may continue. No operator-account access is assumed.

## Migration Plan

Deliver the bounded code/tooling and its runbook with a changelog entry for user-visible behavior; retain the prior configuration and receipts for comparison.

## Verification

Scenario OA-10: Each pruned task emits exactly one Expired terminal event, releases/settles its accepted-work accounting, and cannot execute; non-expired work still runs. The public return behavior is documented and all backends implement the changed metadata contract.
Negative control: Count-only pruning or suppressing Expired events must fail the per-task lifecycle sequence oracle.
Evidence: candidate SHA, contract digest, exact argv/exit and raw redacted logs, fixture/tool/image/OS identity; final execution by oa-18 task 2 only.

## Safe implementation prerequisites

Recheck actual tool/model availability and version pins from official primary documentation before adding tools. Use existing approved resources; no implicit paid jobs or global host installation. Material scope changes require a contract amendment, not an untracked TODO.
