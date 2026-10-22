## ADDED Requirements

### Requirement: Consume-once delivery

A stored result SHALL be returned to exactly one consume call, the first write for an idempotency key SHALL win, and expired results SHALL be evicted.

#### Scenario: Concurrent consume

- **WHEN** two clients consume the same key concurrently
- **THEN** exactly one receives the result

#### Scenario: Duplicate write

- **WHEN** a retried task delivers twice with one idempotency key
- **THEN** the first stored result is kept
