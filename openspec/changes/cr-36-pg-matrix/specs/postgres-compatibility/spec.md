## ADDED Requirements

### Requirement: Two-version support

All Postgres suites SHALL pass on PostgreSQL 17 and 18 in CI.

#### Scenario: Matrix

- **WHEN** CI runs the Postgres suites
- **THEN** jobs for 17 and 18 both pass
