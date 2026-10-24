# core-contracts Specification

## Purpose
Provide the executor, error, clock and resource-cost contracts used consistently by pool implementations.

## Requirements

### Requirement: Executors report failure

Task execution SHALL produce a typed result distinguishing success, executor error, timeout, panic and cancellation, delivered to the mailbox.

#### Scenario: Executor error

- **WHEN** an executor returns an error
- **THEN** the mailbox records a failed status carrying the error, and capacity is released

### Requirement: Deterministic ordering

Tasks of equal priority SHALL be dispatched in submission order using a monotonic sequence, with Eq/Ord consistent.

#### Scenario: FIFO within priority

- **WHEN** 1,000 equal-priority tasks are submitted from one thread to a one-unit pool
- **THEN** they complete in submission order
