# Design: Prove no stream publication beyond an abandoned chunk

## Context

See proposal.md for motivation. Baseline is main 92da0d3 after 0.3.0 and the completed predecessor phase. #68 is open in evidence/issue-inventory.json. Use the actual workspace layout in docs/agent-context.md, not old project.json paths.

## Goals / Non-Goals

Goal: Exhausting the publish budget poisons the writer, later publishes return Poisoned, and a reader observes no sequence beyond the hole; reconnect semantics for valid prefixes remain correct.
Non-goals: unrelated backlog, credential rotation/KBD signer replacement (operator-deferred), new pool technologies, automatic publishing or history rewriting.

## Decisions

Introduce deterministic fault scheduling at the actual publisher/server boundary without replacing the real sink/reader with mocks. Preserve the documented consumer-dedup requirement; #62 is not solved here.

Use the incumbent production entry: IggyStreamSink/StreamWriter against a pinned real Iggy server with failure at one chosen chunk.
Real regression fixtures belong to this change, not a later catch-all testing task. Only final execution of the integration packet is postponed to oa-18.

Owner events-engineer writes crates/parking-lot-iggy/tests/iggy_stream.rs; stream writer; pinned Iggy test support. Tech-lead owns shared public contracts/manifests; other owners receive exact interface handoffs before backend work. Dependencies: none.
Alternative rejected: mock-only or task-count completion, because it cannot establish the observable oracle.

## Risks / Trade-offs

Use per-run stream/topic names and delete only owned test resources.
Unavailable host, privilege or collaborator yields BLOCKED evidence for this scenario; unrelated implementation may continue. No operator-account access is assumed.

## Migration Plan

Deliver the bounded code/tooling and its runbook with a changelog entry for user-visible behavior; retain the prior configuration and receipts for comparison.

## Verification

Scenario OA-16: Exhausting the publish budget poisons the writer, later publishes return Poisoned, and a reader observes no sequence beyond the hole; reconnect semantics for valid prefixes remain correct.
Negative control: Disable writer poisoning or allow a later sequence to publish; the real-server reader oracle fails.
Evidence: candidate SHA, contract digest, exact argv/exit and raw redacted logs, fixture/tool/image/OS identity; final execution by oa-18 task 2 only.

## Safe implementation prerequisites

Recheck actual tool/model availability and version pins from official primary documentation before adding tools. Use existing approved resources; no implicit paid jobs or global host installation. Material scope changes require a contract amendment, not an untracked TODO.
