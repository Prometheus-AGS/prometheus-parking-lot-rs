# CR-06 — Mailbox hardening (semantics, TTL, errors)

- **Track:** B · **Severity:** 🟠 High · **Depends on:** CR-05
- **Skill:** `actor-model` · **Agent:** general-purpose
- **Files:** `src/core/resource_pool.rs` (Mailbox trait), `src/infra/mailbox/{memory,postgres,yaque}.rs`

## Design
Define an explicit public `Mailbox<T>` contract:
- **Delivery:** at-least-once; idempotent `retrieve`/consume-once semantics documented.
- **Lifetime:** optional TTL/expiry + eviction policy for disconnected clients.
- **Errors:** typed (CR-05); notifier failure does not lose the stored result.
- **Backpressure:** bounded storage with typed full error.

## Tasks
- [ ] Document `Mailbox<T>` trait contract (delivery, ordering, expiry, errors)
- [ ] Add TTL/expiry + eviction to `InMemoryMailbox`
- [ ] Audit Postgres/Yaque impls for conformance (status transitions, redelivery)
- [ ] Decouple notifier failure from storage durability
- [ ] Tests: expiry eviction, redelivery, notifier-error isolation, consume-once

## Acceptance
- [ ] Documented trait + semantics; all three backends conform
- [ ] New mailbox tests pass
