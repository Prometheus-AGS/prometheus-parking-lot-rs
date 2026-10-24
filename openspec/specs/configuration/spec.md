# configuration Specification

## Purpose
Turn validated scheduler configuration into pools with explicit backend parameters and useful configuration errors.

## Requirements

### Requirement: Config-driven pools

A valid scheduler configuration SHALL build every configured pool and backend, and invalid configuration SHALL be rejected with a precise error.

#### Scenario: Merged backends

- **WHEN** configs for in-memory, Postgres and redb backends are loaded
- **THEN** each builds its pool; later backends (pgmq, Iggy) add their own section and example test in their changes

#### Scenario: Unknown field

- **WHEN** a config contains a misspelled field
- **THEN** loading fails naming the field
