## ADDED Requirements

### Requirement: Docs describe this crate

Every Rust code sample in README.md SHALL compile as a doctest, and docs/ outside docs/archive/ SHALL describe only APIs that exist.

#### Scenario: README doctests

- **WHEN** `cargo test --doc -p prometheus_parking_lot` runs
- **THEN** README samples are executed and pass
