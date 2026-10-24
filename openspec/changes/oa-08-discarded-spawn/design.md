# Design: Settle accepted work when a spawner discards its future

## Context

See proposal.md for motivation. Baseline is main 92da0d3 after 0.3.0 and the completed predecessor phase. #81 is open in evidence/issue-inventory.json. Use the actual workspace layout in docs/agent-context.md, not old project.json paths.

## Goals / Non-Goals

Goal: Discarding an unpolled supervise future produces one documented Failed/Cancelled terminal outcome and mailbox result where configured, releases units and settles the ledger; shutdown does not wait out its deadline for the discarded task. A normal spawner still completes once.
Non-goals: unrelated backlog, credential rotation/KBD signer replacement (operator-deferred), new pool technologies, automatic publishing or history rewriting.

## Decisions

Establish ownership/settlement before handing the future to Spawn. Preserve panic containment and avoid duplicate terminal reporting. Do not claim durable execution for an in-memory ResourcePool.

Use the incumbent production entry: ResourcePool::submit/launch with a legal Spawn implementation that discards supervise without polling, followed by shutdown_with.
Real regression fixtures belong to this change, not a later catch-all testing task. Only final execution of the integration packet is postponed to oa-18.

Owner scheduler-engineer writes crates/parking-lot-runtime-tokio/src/resource_pool.rs; shutdown ledger; runtime integration tests. Tech-lead owns shared public contracts/manifests; other owners receive exact interface handoffs before backend work. Dependencies: none.
Alternative rejected: mock-only or task-count completion, because it cannot establish the observable oracle.

## Risks / Trade-offs

No new dependency from core to an async runtime; preserve capacity and shutdown ordering.
Unavailable host, privilege or collaborator yields BLOCKED evidence for this scenario; unrelated implementation may continue. No operator-account access is assumed.

## Migration Plan

Deliver the bounded code/tooling and its runbook with a changelog entry for user-visible behavior; retain the prior configuration and receipts for comparison.

## Verification

Scenario OA-08: Discarding an unpolled supervise future produces one documented Failed/Cancelled terminal outcome and mailbox result where configured, releases units and settles the ledger; shutdown does not wait out its deadline for the discarded task. A normal spawner still completes once.
Negative control: Constructing the settlement guard only inside the unpolled async body must fail the task-outcome/ledger oracle.
Evidence: candidate SHA, contract digest, exact argv/exit and raw redacted logs, fixture/tool/image/OS identity; final execution by oa-18 task 2 only.

## Safe implementation prerequisites

Recheck actual tool/model availability and version pins from official primary documentation before adding tools. Use existing approved resources; no implicit paid jobs or global host installation. Material scope changes require a contract amendment, not an untracked TODO.
