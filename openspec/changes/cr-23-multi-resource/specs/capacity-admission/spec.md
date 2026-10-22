## ADDED Requirements

### Requirement: Multi-dimensional admission

A task SHALL be admitted only when every resource dimension of its cost fits within remaining capacity.

#### Scenario: One dimension short

- **WHEN** a task needs (8 GB, 1 slot) while 8 GB is free and 0 slots are free
- **THEN** the task waits
