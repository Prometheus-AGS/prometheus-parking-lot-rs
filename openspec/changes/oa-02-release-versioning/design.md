# Design: Choose and automate seven-crate versioning

## Context

See proposal.md for motivation. Baseline is main 92da0d3 after 0.3.0 and the completed predecessor phase. #86 is open in evidence/issue-inventory.json. Use the actual workspace layout in docs/agent-context.md, not old project.json paths.

## Goals / Non-Goals

Goal: The adopted ADR chooses lockstep 0.4.0 (proposal) or an operator-amended policy; a dry run yields consistent internal dependency versions and changelog entries for exactly seven publishable crates, excluding FFI; a second run is unchanged.
Non-goals: unrelated backlog, credential rotation/KBD signer replacement (operator-deferred), new pool technologies, automatic publishing or history rewriting.

## Decisions

Recommend lockstep to match the existing uniform 0.3.0 release. Validate official release-plz/git-cliff capabilities and versions before introducing dependencies; freeze exact tool versions and executable dry-run argv in the boundary manifest during implementation.

Use the incumbent production entry: Proposed release-tool dry run against a scratch copy of the seven-crate workspace, invoked by the boundary runner.
Real regression fixtures belong to this change, not a later catch-all testing task. Only final execution of the integration packet is postponed to oa-18.

Owner tech-lead writes Cargo.toml; crates/*/Cargo.toml; release-plz/git-cliff configuration; docs/adr; CHANGELOG.md. Tech-lead owns shared public contracts/manifests; other owners receive exact interface handoffs before backend work. Dependencies: none.
Alternative rejected: mock-only or task-count completion, because it cannot establish the observable oracle.

## Risks / Trade-offs

No registry publishing in a dry run. New tooling/version choices verified from primary sources before installation.
Unavailable host, privilege or collaborator yields BLOCKED evidence for this scenario; unrelated implementation may continue. No operator-account access is assumed.

## Migration Plan

Deliver the bounded code/tooling and its runbook with a changelog entry for user-visible behavior; retain the prior configuration and receipts for comparison.

## Verification

Scenario OA-02: The adopted ADR chooses lockstep 0.4.0 (proposal) or an operator-amended policy; a dry run yields consistent internal dependency versions and changelog entries for exactly seven publishable crates, excluding FFI; a second run is unchanged.
Negative control: Inject a mismatched internal dependency/version in the scratch copy; the release-preparation check must fail.
Evidence: candidate SHA, contract digest, exact argv/exit and raw redacted logs, fixture/tool/image/OS identity; final execution by oa-18 task 2 only.

## Safe implementation prerequisites

Recheck actual tool/model availability and version pins from official primary documentation before adding tools. Use existing approved resources; no implicit paid jobs or global host installation. Material scope changes require a contract amendment, not an untracked TODO.
