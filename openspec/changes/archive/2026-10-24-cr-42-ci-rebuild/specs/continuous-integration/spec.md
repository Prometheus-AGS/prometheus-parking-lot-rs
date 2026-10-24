## ADDED Requirements

### Requirement: Required CI jobs pass on main

CI SHALL run fmt, clippy, tests on 1.99.1 and 1.95, docs, cargo-deny and coverage with --locked, and all SHALL pass on main.

#### Scenario: Deny clean

- **WHEN** `cargo deny check` runs
- **THEN** advisories, bans, licenses and sources all pass

#### Scenario: Cache reuse

- **WHEN** CI runs twice on the same commit
- **THEN** the second run restores the cache and skips dependency compilation
