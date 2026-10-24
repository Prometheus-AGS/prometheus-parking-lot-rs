# postgres-test-substrate Specification

## Purpose
Provide pinned Postgres images and discoverable test helpers for real database and pgmq integration tests.

## Requirements

### Requirement: Reproducible Postgres substrate

The project SHALL provide a pinned Postgres 18 (and 17) image with the pgmq extension available, used by every Postgres test locally and in CI.

#### Scenario: Extension present

- **WHEN** the image starts and `CREATE EXTENSION pgmq` runs
- **THEN** it succeeds and `SELECT version()` reports PostgreSQL 18

#### Scenario: Tests use the image

- **WHEN** the Postgres integration suite runs
- **THEN** containers are created from the project image tag, not a floating upstream tag
