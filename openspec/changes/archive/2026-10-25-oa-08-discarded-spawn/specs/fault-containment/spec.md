# Spec Delta

## ADDED Requirements

### Requirement: Discarded spawn settlement

Accepted work whose scheduled future is discarded without being polled SHALL receive one documented failure or cancellation outcome, release capacity and settle shutdown accounting.

#### Scenario: Settle accepted work when a spawner discards its future

- **WHEN** the supported operation encounters the named condition in OA-08
- **THEN** Discarding an unpolled supervise future produces one documented Failed/Cancelled terminal outcome and mailbox result where configured, releases units and settles the ledger; shutdown does not wait out its deadline for the discarded task. A normal spawner still completes once.

#### Scenario: Discriminating regression

- **WHEN** the prior defective behavior is deliberately reintroduced in an isolated test fixture or control
- **THEN** Constructing the settlement guard only inside the unpolled async body must fail the task-outcome/ledger oracle.
