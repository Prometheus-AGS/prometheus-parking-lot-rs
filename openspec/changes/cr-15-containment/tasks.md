Model and worker assignments for these tasks: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md` → Task model assignments, rows keyed `phase-correctness-and-durability / cr-15-containment / <task id>`.

## 1. Contain leaked agent credentials and ship only allowlisted files

- [ ] 1.1 Operator rotates OPENAI_API_KEY and Codex OAuth tokens exposed in .codex/auth.json and records confirmation (no secret values) in execution.md
- [ ] 1.2 After the plan branch merges and with explicit operator approval: the KBD driver (no team worktree) purges .codex/{auth.json,config.toml,history.jsonl,version.json,sessions/} with git filter-repo in a fresh mirror clone, force-pushes, records old/new main SHAs and the pre-purge root commit set, and the operator re-clones every checkout
- [ ] 1.3 Operator asks GitHub Support to purge cached refs/pull/* for the removed paths and records the ticket ID
- [ ] 1.4 Verify on a fresh mirror clone that the path-history query is empty and record the output
