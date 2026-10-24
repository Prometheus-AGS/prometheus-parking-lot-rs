# postgres-compatibility Specification

## Purpose
Keep the Postgres backends compatible with versions 17 and 18 and document their operational and upgrade requirements.

## Requirements

### Requirement: Two-version support

All Postgres suites SHALL pass on PostgreSQL 17 and 18 in CI.

#### Scenario: Matrix

- **WHEN** CI runs the Postgres suites
- **THEN** jobs for 17 and 18 both pass
