## Why

Containment (cr-15) must stay contained: a branch carrying any pre-purge commit would reintroduce the credential history on merge, and packaging could again ship agent tooling.

## What Changes

Created from post-purge main only. CI with fetch-depth 0 runs gitleaks over full history, the path-history query (must be empty), a rejection of any branch containing a commit from the recorded pre-purge root set, and cargo package --list per published crate against its include allowlist.

## Capabilities

### New Capabilities

- `supply-chain-safety`: Contained history and packages

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone M0; owner role `platform-engineer` (team `parking-lot-core`, docs/agent-team.md).
- Depends on: `cr-15-containment`, `cr-42-ci-rebuild`, `cr-48-workspace-split`.
- Plan of record: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md`; source audit `docs/update-plan-10-22-2026.md`.
