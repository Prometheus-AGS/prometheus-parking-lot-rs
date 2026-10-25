# backend-contracts Specification

## Purpose
Define shared asynchronous queue, mailbox and notification contracts so storage backends can be substituted and checked by the same conformance suite.

## Requirements

### Requirement: Lease semantics

A leased task SHALL be invisible to other consumers until acked, nacked or its lease expires, after which it SHALL be redelivered.

#### Scenario: Lease expiry

- **WHEN** a consumer leases a task and never acks it
- **THEN** after the lease duration the task is delivered to another consumer

### Requirement: Conformance suite

Every queue and mailbox backend SHALL pass one shared conformance suite.

#### Scenario: Reference backend passes

- **WHEN** the conformance suite runs against the in-memory backends
- **THEN** all cases pass

### Requirement: Tenant-scoped mailbox idempotency

Result idempotency SHALL be scoped to the tenant/mailbox key: two tenants can independently use the same caller key, while retries within one tenant retain first-write-wins behavior across consume and restart.

#### Scenario: Scope result idempotency to tenant/mailbox key

- **WHEN** the supported operation encounters the named condition in OA-06
- **THEN** Two tenants reusing the same caller key independently retrieve their own first result; a duplicate within one tenant remains first-write-wins after consume/restart/TTL rules. Migration preserves existing results and dedup meaning under the recorded upgrade policy.

#### Scenario: Discriminating regression

- **WHEN** the prior defective behavior is deliberately reintroduced in an isolated test fixture or control
- **THEN** The old mailbox-global-key behavior must fail the cross-tenant case; a duplicate within one tenant must not produce a second stored result.

### Requirement: Authoritative duplicate delivery status

A duplicate durable delivery SHALL expose the authoritative retained terminal status, including after result consumption while the idempotency record remains valid; emitted terminal events SHALL agree with that status.

#### Scenario: Emit durable terminal events from the stored result

- **WHEN** the supported operation encounters the named condition in OA-07
- **THEN** After restart and redelivery, stored status and terminal event agree on Failed, exactly one result is consumable, and delivery still precedes lease resolution. The status remains knowable after consume while the idempotency record is retained.

#### Scenario: Discriminating regression

- **WHEN** the prior defective behavior is deliberately reintroduced in an isolated test fixture or control
- **THEN** Old bool-only delivery handling that emits local Completed after a stored Failed must fail the event/result consistency oracle.
