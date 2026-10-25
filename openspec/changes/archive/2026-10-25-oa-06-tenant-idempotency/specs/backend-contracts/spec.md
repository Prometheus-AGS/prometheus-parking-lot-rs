# Spec Delta

## ADDED Requirements

### Requirement: Tenant-scoped mailbox idempotency

Result idempotency SHALL be scoped to the tenant/mailbox key: two tenants can independently use the same caller key, while retries within one tenant retain first-write-wins behavior across consume and restart.

#### Scenario: Scope result idempotency to tenant/mailbox key

- **WHEN** the supported operation encounters the named condition in OA-06
- **THEN** Two tenants reusing the same caller key independently retrieve their own first result; a duplicate within one tenant remains first-write-wins after consume/restart/TTL rules. Migration preserves existing results and dedup meaning under the recorded upgrade policy.

#### Scenario: Discriminating regression

- **WHEN** the prior defective behavior is deliberately reintroduced in an isolated test fixture or control
- **THEN** The old mailbox-global-key behavior must fail the cross-tenant case; a duplicate within one tenant must not produce a second stored result.
