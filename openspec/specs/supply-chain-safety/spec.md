# supply-chain-safety Specification

## Purpose
Exclude credentials and unwanted files from tracked inputs and published packages while separating active checks from deferred history containment.

## Requirements

### Requirement: Contained history and packages

CI SHALL fail tracked credential paths, credential additions within the adopted diff-scan scope, and packages containing a path outside the allowlist. After the deferred cr-15 purge (#88), CI SHALL also reject pre-purge ancestry and credential paths in history. For 0.3.0, full-history checks remain informational and the pre-purge check remains inert without recorded roots, per the 2026-10-23 boundary amendment and the operator's 2026-10-24 closure decision.

#### Scenario: Pre-purge branch rejected

- **WHEN** the operator has completed the purge, recorded the pre-purge root set and armed the blocking check, and a scratch branch containing a pre-purge commit opens a pull request
- **THEN** the pre-purge gate fails the job

#### Scenario: Path history

- **WHEN** the deferred purge is complete and the blocking path-history query runs on the PR head with full history
- **THEN** it is empty; before that purge, the tracked-credential and adopted diff-scan checks still reject newly introduced credential paths or secrets

#### Scenario: Package allowlist

- **WHEN** cargo package --list runs for each published crate
- **THEN** only allowlisted paths are listed

### Requirement: No credentials in history or package

After the operator completes deferred containment, no reachable commit SHALL contain an agent-tool credential or runtime-state path and no branch SHALL reintroduce pre-purge history. Published packages SHALL contain only allowlisted paths.

#### Scenario: Path-based history check

- **WHEN** `git log --all --oneline -- .codex/auth.json .codex/history.jsonl .codex/sessions .codex/config.toml .codex/version.json` runs on a fresh mirror clone after the purge
- **THEN** the output is empty

#### Scenario: Deferred containment disposition

- **WHEN** the operator closes this phase under decision `phase-correctness-and-durability-close-deferrals-20261024`
- **THEN** rotation, history rewriting and GitHub cache removal remain deferred under #88; administrative cr-15 closure does not certify containment
