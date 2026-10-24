## ADDED Requirements

### Requirement: pgmq-backed queue

The pgmq backend SHALL satisfy the queue conformance suite using visibility timeouts as leases and per-priority queues.

#### Scenario: Priority order

- **WHEN** tasks of three priorities are enqueued
- **THEN** higher-priority tasks are leased first
