# durable-queue-postgres Specification

## Purpose
Provide a concurrent Postgres lease queue with acknowledgement, requeue, lease extension and expired-work recovery.

## Requirements

### Requirement: At-least-once with leases

A task leased from Postgres SHALL be redelivered after lease expiry if its consumer dies, and concurrent dispatchers SHALL never hold the same lease.

#### Scenario: Crash redelivery

- **WHEN** a consumer process is killed with SIGKILL mid-task
- **THEN** the task is redelivered after the lease expires

#### Scenario: No double claim

- **WHEN** eight dispatchers lease concurrently from one queue
- **THEN** no task is leased by two dispatchers at once
