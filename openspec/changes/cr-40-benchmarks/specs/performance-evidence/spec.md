## ADDED Requirements

### Requirement: Reproducible numbers

Published performance figures SHALL come from benchmarks a CI job runs and records.

#### Scenario: Bench job

- **WHEN** the benchmark job runs on main
- **THEN** results are stored as an artifact and match the documented figures
