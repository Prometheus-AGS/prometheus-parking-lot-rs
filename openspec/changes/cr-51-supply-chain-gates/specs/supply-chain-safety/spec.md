## ADDED Requirements

### Requirement: Contained history and packages

CI SHALL fail any pull request whose history contains a pre-purge commit or a credential path, or whose packages include a path outside the allowlist.

#### Scenario: Pre-purge branch rejected

- **WHEN** a scratch branch created from pre-purge commit 88f805c opens a pull request
- **THEN** the pre-purge gate fails the job

#### Scenario: Path history

- **WHEN** the path-history query runs on the PR head with full history
- **THEN** it is empty, and a scratch branch re-adding .codex/auth.json fails the job

#### Scenario: Package allowlist

- **WHEN** cargo package --list runs for each published crate
- **THEN** only allowlisted paths are listed
