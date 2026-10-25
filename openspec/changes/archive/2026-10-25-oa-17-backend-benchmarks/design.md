# Design: Measure PG throughput, contention and memory

## Context

See proposal.md for motivation. Baseline is main 92da0d3 after 0.3.0 and the completed predecessor phase. #71 is open in evidence/issue-inventory.json. Use the actual workspace layout in docs/agent-context.md, not old project.json paths.

## Goals / Non-Goals

Goal: Quiet-machine baseline and candidate run the identical fixed workload with >=5 samples; report throughput, p50/p95/p99 latency, contention parameters and peak RSS, raw samples and environment. No performance gain or broad regression-budget claim is made from task counts or incomparable hosts; any observed regression is reported explicitly for the operator's release decision.
Non-goals: unrelated backlog, credential rotation/KBD signer replacement (operator-deferred), new pool technologies, automatic publishing or history rewriting.

## Decisions

Freeze hardware/OS/CPU/thread counts, toolchain, features and image digests; no speedup claim if confidence intervals overlap noise. This issue requires reproducible measurement, not an invented numerical performance target. No release benchmark during implementation.

Use the incumbent production entry: Docker-backed Postgres queue/mailbox/durable-pool benchmarks plus multi-thread contention and peak-memory measurement.
Real regression fixtures belong to this change, not a later catch-all testing task. Only final execution of the integration packet is postponed to oa-18.

Owner scheduler-engineer writes benches/; benches/README.md; docs/ARCHITECTURE.md section 7; disposable PG fixtures. Tech-lead owns shared public contracts/manifests; other owners receive exact interface handoffs before backend work. Dependencies: oa-06-tenant-idempotency, oa-07-stored-terminal-outcome, oa-08-discarded-spawn, oa-09-backend-admission, oa-10-expiry-events.
Alternative rejected: mock-only or task-count completion, because it cannot establish the observable oracle.

## Risks / Trade-offs

One build/benchmark writer, no active agents/builds on the benchmark host; no privileged host tuning without approval.
Unavailable host, privilege or collaborator yields BLOCKED evidence for this scenario; unrelated implementation may continue. No operator-account access is assumed.

## Migration Plan

Deliver the bounded code/tooling and its runbook with a changelog entry for user-visible behavior; retain the prior configuration and receipts for comparison.

## Verification

Scenario OA-17: Quiet-machine baseline and candidate run the identical fixed workload with >=5 samples; report throughput, p50/p95/p99 latency, contention parameters and peak RSS, raw samples and environment. No performance gain or broad regression-budget claim is made from task counts or incomparable hosts; any observed regression is reported explicitly for the operator's release decision.
Negative control: Drop a raw sample or change workload/machine identity; report validation fails. A deliberately slowed workload validates regression classification.
Evidence: candidate SHA, contract digest, exact argv/exit and raw redacted logs, fixture/tool/image/OS identity; final execution by oa-18 task 2 only.

## Safe implementation prerequisites

Recheck actual tool/model availability and version pins from official primary documentation before adding tools. Use existing approved resources; no implicit paid jobs or global host installation. Material scope changes require a contract amendment, not an untracked TODO.
