## ADDED Requirements

### Requirement: Ordered lifecycle events

Each task SHALL emit its lifecycle events in causal order, and pools with no hooks SHALL incur no measurable overhead.

#### Scenario: Event ordering

- **WHEN** a recording hook observes 1,000 tasks
- **THEN** each task shows submitted before started before a terminal event

#### Scenario: Zero overhead

- **WHEN** the dispatch benchmark runs with and without the hook feature
- **THEN** no regression beyond noise
