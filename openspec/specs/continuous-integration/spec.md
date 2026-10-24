# continuous-integration Specification

## Purpose
Make build, test, dependency and compatibility checks reproducible and visible in CI.

## Requirements

### Requirement: Required CI jobs pass on main

CI SHALL run fmt, clippy, tests on 1.99.1 and 1.95, docs, cargo-deny and coverage with --locked, and all SHALL pass on main.

#### Scenario: Deny clean

- **WHEN** `cargo deny check` runs
- **THEN** advisories, bans, licenses and sources all pass

#### Scenario: Cache reuse

- **WHEN** CI runs twice on the same commit
- **THEN** the second run restores the cache and skips dependency compilation

### Requirement: Lean, semver-checked dependency graph

The workspace SHALL have no unused dependencies, and CI SHALL run semver checks on published crates.

#### Scenario: udeps

- **WHEN** cargo udeps runs
- **THEN** no unused dependency is reported
