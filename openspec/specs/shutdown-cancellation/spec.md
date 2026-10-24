# shutdown-cancellation Specification

## Purpose
Apply explicit shutdown policies and capture cancelled work for documented replay and requeue behavior.

## Requirements

### Requirement: No loss on shutdown

On shutdown every accepted task SHALL either complete, be captured by the cancellation sink, or be requeued to a durable backend.

#### Scenario: Shutdown mid-dispatch

- **WHEN** shutdown is signalled while tasks are queued and one has been received by a worker
- **THEN** every task is completed, captured or requeued; none is silently dropped

#### Scenario: Requeue across restart

- **WHEN** a captured task is requeued to Postgres or redb and the process restarts
- **THEN** the task runs after restart
