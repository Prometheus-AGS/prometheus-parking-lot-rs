## Why

.codex/auth.json (OPENAI_API_KEY, OAuth tokens) was committed in ec6b4b4 to a public repository; Cargo.toml uses an exclude list that would publish agent tooling. Untracking landed with this plan; rotation and history purge remain. Rotation, not the purge, is the containment guarantee: GitHub keeps refs/pull/* and forks may hold the old commits.

## What Changes

Operator rotates the exposed keys first. After this plan branch is merged and with recorded operator approval, purge the files from history in a fresh mirror clone (no worktrees exist), force-push, record the old main SHA, and ask GitHub Support to purge cached PR refs. Every later branch is created from post-purge main; a CI gate rejects any branch that still contains the old main SHA. Switch Cargo.toml to an include allowlist; add gitleaks and a cargo package --list allowlist check to CI.

## Capabilities

### New Capabilities

- `supply-chain-safety`: No credentials in history or package

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone M0; owner role `operator` (team `parking-lot-core`, docs/agent-team.md).
- Depends on: none.
- Plan of record: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md`; source audit `docs/update-plan-10-22-2026.md`.
