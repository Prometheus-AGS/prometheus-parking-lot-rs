## ADDED Requirements

### Requirement: Capacity invariant

At every instant the sum of the costs of running tasks SHALL be at most max_units, for every pool type.

#### Scenario: Model check

- **WHEN** the loom models of the dispatcher and semaphore run with LOOM_MAX_PREEMPTIONS=3
- **THEN** no explored schedule exceeds max_units and no accepted task is lost

#### Scenario: Property test

- **WHEN** random submit/complete sequences run against ResourcePool and the native WorkerPool
- **THEN** the invariant and queue-depth limit hold

#### Scenario: Negative control

- **WHEN** the same models run with `--cfg parking_lot_negative_control` (semaphore acquire skipped)
- **THEN** the model reports a violation

### Requirement: No lost wakeup

A parked task SHALL be dispatched when capacity sufficient for it becomes free.

#### Scenario: Completion races submit

- **WHEN** a task completes while another is being parked
- **THEN** the parked task is dispatched without waiting for a later completion

### Requirement: Bounded execution

Every pool SHALL bound each execution by the task deadline or the pool default timeout.

#### Scenario: Wedged executor

- **WHEN** an executor never returns in a WorkerPool
- **THEN** it is cancelled at the budget, its units are released, and the mailbox records a timeout
