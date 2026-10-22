## ADDED Requirements

### Requirement: No credentials in history or package

No reachable commit on any branch SHALL contain an agent-tool credential or runtime-state path, no branch SHALL reintroduce pre-purge history, and `cargo package` SHALL include only allowlisted paths.

#### Scenario: Path-based history check

- **WHEN** `git log --all --oneline -- .codex/auth.json .codex/history.jsonl .codex/sessions .codex/config.toml .codex/version.json` runs on a fresh mirror clone after the purge
- **THEN** the output is empty
