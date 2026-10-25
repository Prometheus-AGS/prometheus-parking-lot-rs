# Design: Prepare and enforce post-purge containment controls

## Context

See proposal.md for motivation. Baseline is main 92da0d3 after 0.3.0 and the completed predecessor phase. #88 is open in evidence/issue-inventory.json. Use the actual workspace layout in docs/agent-context.md, not old project.json paths.

## Goals / Non-Goals

Goal: After separately approved purge, history/path scans are blocking and clean, roots are nonempty, and a scratch branch with an old root is rejected; a clean candidate passes. Operator cache-removal evidence is recorded without credential values.
Non-goals: unrelated backlog, credential rotation/KBD signer replacement (operator-deferred), new pool technologies, automatic publishing or history rewriting.

## Decisions

Implement truthful pre-purge versus post-purge state and prepare a backup/ref-map/reclone/support runbook. Operator executes the approved rewrite; the driver does not force-push. Rotation and KBD signer replacement are explicitly deferred by the 2026-10-24 operator reply and do not block implementation.

Use the incumbent production entry: bash .github/scripts/supply-chain.sh tracked-credentials; post-purge secrets-history/path-history/pre-purge in an approved fresh mirror.
Real regression fixtures belong to this change, not a later catch-all testing task. Only final execution of the integration packet is postponed to oa-18.

Owner platform-engineer writes .github/scripts/supply-chain.sh; .github/pre-purge-roots.txt; docs security runbook. Tech-lead owns shared public contracts/manifests; other owners receive exact interface handoffs before backend work. Dependencies: none.
Alternative rejected: mock-only or task-count completion, because it cannot establish the observable oracle.

## Risks / Trade-offs

No history rewrite, remote ref deletion, Support submission, credential change or enrollment operation without scoped authorization.
Unavailable host, privilege or collaborator yields BLOCKED evidence for this scenario; unrelated implementation may continue. No operator-account access is assumed.

## Migration Plan

Prepare an exact backup/ref-map/reclone/support plan; operator approval precedes any destructive action. Tooling can be implemented before that approval.

## Verification

Scenario OA-01: After separately approved purge, history/path scans are blocking and clean, roots are nonempty, and a scratch branch with an old root is rejected; a clean candidate passes. Operator cache-removal evidence is recorded without credential values.
Negative control: A scratch clone/ref deliberately retaining one recorded pre-purge root must fail the pre-purge check; an intentionally tracked credential path must fail.
Evidence: candidate SHA, contract digest, exact argv/exit and raw redacted logs, fixture/tool/image/OS identity; final execution by oa-18 task 2 only.

## Safe implementation prerequisites

Recheck actual tool/model availability and version pins from official primary documentation before adding tools. Use existing approved resources; no implicit paid jobs or global host installation. Material scope changes require a contract amendment, not an untracked TODO.
