# Design: Verify operator-signed release tags

## Context

See proposal.md for motivation. Baseline is main 92da0d3 after 0.3.0 and the completed predecessor phase. #87 is open in evidence/issue-inventory.json. Use the actual workspace layout in docs/agent-context.md, not old project.json paths.

## Goals / Non-Goals

Goal: A scratch annotated tag signed by an operator-authorized identity verifies at the frozen candidate; an unsigned, wrong-identity or wrong-commit tag is rejected. Real v0.4.0 tag creation remains an approved publication step.
Non-goals: unrelated backlog, credential rotation/KBD signer replacement (operator-deferred), new pool technologies, automatic publishing or history rewriting.

## Decisions

Do not generate, import or copy private signing keys. Operator selects existing release authority; workflow verifies an explicit identity and exact commit, not merely presence of a signature.

Use the incumbent production entry: git verify-tag <candidate-tag> using the operator-selected SSH/GPG trust configuration.
Real regression fixtures belong to this change, not a later catch-all testing task. Only final execution of the integration packet is postponed to oa-18.

Owner platform-engineer writes .github/workflows planned release workflow; .github/scripts signing verifier; docs/PUBLISHING.md. Tech-lead owns shared public contracts/manifests; other owners receive exact interface handoffs before backend work. Dependencies: oa-02-release-versioning.
Alternative rejected: mock-only or task-count completion, because it cannot establish the observable oracle.

## Risks / Trade-offs

Release signing setup is a separate scoped account/key action; no private key bytes in repo, workflow output or receipts.
Unavailable host, privilege or collaborator yields BLOCKED evidence for this scenario; unrelated implementation may continue. No operator-account access is assumed.

## Migration Plan

Deliver the bounded code/tooling and its runbook with a changelog entry for user-visible behavior; retain the prior configuration and receipts for comparison.

## Verification

Scenario OA-03: A scratch annotated tag signed by an operator-authorized identity verifies at the frozen candidate; an unsigned, wrong-identity or wrong-commit tag is rejected. Real v0.4.0 tag creation remains an approved publication step.
Negative control: Unsigned tag and a signed tag pointing at a different commit each fail candidate authorization.
Evidence: candidate SHA, contract digest, exact argv/exit and raw redacted logs, fixture/tool/image/OS identity; final execution by oa-18 task 2 only.

## Safe implementation prerequisites

Recheck actual tool/model availability and version pins from official primary documentation before adding tools. Use existing approved resources; no implicit paid jobs or global host installation. Material scope changes require a contract amendment, not an untracked TODO.
