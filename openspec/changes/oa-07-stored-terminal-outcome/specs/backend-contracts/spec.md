# Spec Delta

## ADDED Requirements

### Requirement: Authoritative duplicate delivery status

A duplicate durable delivery SHALL expose the authoritative retained terminal status, including after result consumption while the idempotency record remains valid; emitted terminal events SHALL agree with that status.

#### Scenario: Emit durable terminal events from the stored result

- **WHEN** the supported operation encounters the named condition in OA-07
- **THEN** After restart and redelivery, stored status and terminal event agree on Failed, exactly one result is consumable, and delivery still precedes lease resolution. The status remains knowable after consume while the idempotency record is retained.

#### Scenario: Discriminating regression

- **WHEN** the prior defective behavior is deliberately reintroduced in an isolated test fixture or control
- **THEN** Old bool-only delivery handling that emits local Completed after a stored Failed must fail the event/result consistency oracle.
