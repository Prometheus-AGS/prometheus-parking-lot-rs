## ADDED Requirements

### Requirement: Bounded skip-ahead

The dispatcher SHALL dispatch fitting tasks behind a non-fitting head, and the head SHALL run within the configured number of dispatch cycles.

#### Scenario: Head not starved

- **WHEN** small tasks keep arriving while a large head task waits
- **THEN** the head starts within N dispatch cycles

#### Scenario: Utilization

- **WHEN** the mixed-size benchmark runs
- **THEN** utilization exceeds strict FIFO dispatch
