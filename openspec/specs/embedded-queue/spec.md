# embedded-queue Specification

## Purpose
Provide embedded queue and mailbox persistence with redb transactions and crash-recovery evidence.

## Requirements

### Requirement: Crash safety

An accepted task SHALL survive a process crash at any point of enqueue, lease or ack and be redelivered after restart.

#### Scenario: Fault injection

- **WHEN** the process is aborted at each instrumented crash point
- **THEN** after restart every accepted task is present exactly once or redelivered per lease rules
