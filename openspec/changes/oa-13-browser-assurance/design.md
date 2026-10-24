# Design: Exercise WASM admission and events in a browser

## Context

See proposal.md for motivation. Baseline is main 92da0d3 after 0.3.0 and the completed predecessor phase. #65 is open in evidence/issue-inventory.json. Use the actual workspace layout in docs/agent-context.md, not old project.json paths.

## Goals / Non-Goals

Goal: Smoke, vector capacity, execution-budget, event-order and applicable expiry scenarios execute and pass in a real headless browser with nonzero test discovery; existing Node path also passes.
Non-goals: unrelated backlog, credential rotation/KBD signer replacement (operator-deferred), new pool technologies, automatic publishing or history rewriting.

## Decisions

Resolve browser/driver and wasm-bindgen runner versions against Cargo.lock and official docs before implementation; preserve no-tokio core graph. Browser installation is an environment prerequisite, not a planning side effect.

Use the incumbent production entry: wasm-bindgen-test runner in pinned headless Chromium, exercising real browser event loop/timers.
Real regression fixtures belong to this change, not a later catch-all testing task. Only final execution of the integration packet is postponed to oa-18.

Owner platform-engineer writes crates/parking-lot-wasm/tests/wasm_pool.rs; wasm_events.rs; .github/workflows/ci.yml. Tech-lead owns shared public contracts/manifests; other owners receive exact interface handoffs before backend work. Dependencies: oa-10-expiry-events.
Alternative rejected: mock-only or task-count completion, because it cannot establish the observable oracle.

## Risks / Trade-offs

Do not change UI design; this is platform verification, not a UI redesign.
Unavailable host, privilege or collaborator yields BLOCKED evidence for this scenario; unrelated implementation may continue. No operator-account access is assumed.

## Migration Plan

Deliver the bounded code/tooling and its runbook with a changelog entry for user-visible behavior; retain the prior configuration and receipts for comparison.

## Verification

Scenario OA-13: Smoke, vector capacity, execution-budget, event-order and applicable expiry scenarios execute and pass in a real headless browser with nonzero test discovery; existing Node path also passes.
Negative control: A broken capacity/event-budget control fails in the browser; Node-only execution cannot satisfy the browser receipt.
Evidence: candidate SHA, contract digest, exact argv/exit and raw redacted logs, fixture/tool/image/OS identity; final execution by oa-18 task 2 only.

## Safe implementation prerequisites

Recheck actual tool/model availability and version pins from official primary documentation before adding tools. Use existing approved resources; no implicit paid jobs or global host installation. Material scope changes require a contract amendment, not an untracked TODO.
