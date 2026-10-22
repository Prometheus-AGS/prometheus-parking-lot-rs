## ADDED Requirements

### Requirement: All suites run

Every test file in the repository SHALL belong to a compiled test target that CI executes.

#### Scenario: No orphan tests

- **WHEN** a script lists *.rs under tests/ and crates/*/tests/
- **THEN** each file is a target root or is reachable from one
