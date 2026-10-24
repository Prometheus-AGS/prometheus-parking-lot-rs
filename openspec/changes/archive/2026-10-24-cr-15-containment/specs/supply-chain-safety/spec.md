## ADDED Requirements

### Requirement: No credentials in history or package

After the operator completes the deferred containment work, no reachable commit on any branch SHALL contain an agent-tool credential or runtime-state path and no branch SHALL reintroduce pre-purge history. `cargo package` SHALL include only allowlisted paths. For 0.3.0, credential rotation, history rewriting and GitHub cache removal remain deferred; phase closure carries cr-15 forward under issue #88 and does not certify containment. This scope follows the operator's 2026-10-24 closure decision `phase-correctness-and-durability-close-deferrals-20261024`.

#### Scenario: Path-based history check

- **WHEN** `git log --all --oneline -- .codex/auth.json .codex/history.jsonl .codex/sessions .codex/config.toml .codex/version.json` runs on a fresh mirror clone after the purge
- **THEN** the output is empty
