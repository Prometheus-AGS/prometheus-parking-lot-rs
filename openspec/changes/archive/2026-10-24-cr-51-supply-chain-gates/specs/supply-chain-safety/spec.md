## ADDED Requirements

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
