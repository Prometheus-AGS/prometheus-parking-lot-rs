## ADDED Requirements

### Requirement: Lean, semver-checked dependency graph

The workspace SHALL have no unused dependencies, and CI SHALL run semver checks on published crates.

#### Scenario: udeps

- **WHEN** cargo udeps runs
- **THEN** no unused dependency is reported
