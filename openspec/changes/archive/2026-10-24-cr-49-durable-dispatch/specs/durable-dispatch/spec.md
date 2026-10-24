## ADDED Requirements

### Requirement: Crash-durable acceptance

With a durable backend, once submit reports a task accepted, the task SHALL complete exactly once from the caller perspective (result consumable) even if the pool process is killed at any point during its execution.

#### Scenario: Pool killed mid-execution (Postgres)

- **WHEN** a process running ResourcePool on the Postgres backend is killed with SIGKILL while a task executes, and a new process starts
- **THEN** the task re-runs after lease expiry and exactly one result is consumable

#### Scenario: Pool killed mid-execution (redb)

- **WHEN** the same test runs on the embedded redb backend
- **THEN** the task re-runs after restart and exactly one result is consumable

#### Scenario: Ack after delivery

- **WHEN** the process is killed after execution but before the mailbox write
- **THEN** the task is redelivered rather than lost

#### Scenario: Negative control

- **WHEN** the test runs with `--cfg parking_lot_negative_control` (ack before mailbox write)
- **THEN** the task result is lost and the test detects it

### Requirement: Lease extension

A task running longer than the lease duration SHALL extend its lease and SHALL NOT be redelivered while alive.

#### Scenario: Long task

- **WHEN** a task runs for three lease durations
- **THEN** it is never leased by a second dispatcher
