# lifecycle-events Specification

## Purpose
Expose pool lifecycle transitions through registered observers with the documented delivery and platform limits.

## Requirements

### Requirement: Ordered lifecycle events

Each task SHALL emit lifecycle events in causal order, including exactly one Expired terminal event when explicitly pruned while parked. Pools with no hooks SHALL incur no measurable overhead.

#### Scenario: Event ordering

- **WHEN** a recording hook observes 1,000 tasks
- **THEN** each task shows submitted before started before a terminal event

#### Scenario: Zero overhead

- **WHEN** the dispatch benchmark runs with and without the hook feature
- **THEN** no regression beyond noise

#### Scenario: Explicit expiry pruning

- **WHEN** accepted parked tasks expire and the operator or caller prunes them
- **THEN** each pruned task emits Expired exactly once, never executes, and settles accepted-work accounting; non-expired work remains runnable
