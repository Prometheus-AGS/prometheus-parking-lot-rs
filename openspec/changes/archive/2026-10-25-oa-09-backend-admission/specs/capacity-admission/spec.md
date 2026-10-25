# Spec Delta

## ADDED Requirements

### Requirement: Pre-populated backend admission

Previously stored adapter work SHALL undergo capacity validation on first observation; impossible total or per-kind costs SHALL be rejected with a reason without blocking valid followers.

#### Scenario: Reject impossible pre-populated adapter work

- **WHEN** the supported operation encounters the named condition in OA-09
- **THEN** An impossible head task is rejected/dead-lettered with a reason once first observed, valid following work runs, and active vector/kind cost never exceeds limits. Pre-populated valid work remains FIFO within the applicable policy.

#### Scenario: Discriminating regression

- **WHEN** the prior defective behavior is deliberately reintroduced in an isolated test fixture or control
- **THEN** Skip first-observation capacity validation; the invalid head blocks valid followers and the oracle fails.
