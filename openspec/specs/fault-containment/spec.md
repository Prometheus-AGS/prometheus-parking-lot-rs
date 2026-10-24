# fault-containment Specification

## Purpose
Contain native executor panics so they produce task failures without leaking capacity or disabling the pool.

## Requirements

### Requirement: Panic containment

An executor panic SHALL release the task capacity, deliver a panicked status, increment failure counters and leave the pool able to run further tasks.

#### Scenario: Panicking executor

- **WHEN** one of 100 tasks panics in a one-worker pool
- **THEN** the other 99 complete, capacity returns to zero, and stats report one failure
