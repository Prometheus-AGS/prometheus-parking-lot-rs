## ADDED Requirements

### Requirement: Lease semantics

A leased task SHALL be invisible to other consumers until acked, nacked or its lease expires, after which it SHALL be redelivered.

#### Scenario: Lease expiry

- **WHEN** a consumer leases a task and never acks it
- **THEN** after the lease duration the task is delivered to another consumer

### Requirement: Conformance suite

Every queue and mailbox backend SHALL pass one shared conformance suite.

#### Scenario: Reference backend passes

- **WHEN** the conformance suite runs against the in-memory backends
- **THEN** all cases pass
