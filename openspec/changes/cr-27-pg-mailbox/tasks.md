Model and worker assignments for these tasks: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md` → Task model assignments, rows keyed `phase-correctness-and-durability / cr-27-pg-mailbox / <task id>`.

## 1. Postgres mailbox: consume-once, TTL eviction, first-write-wins idempotency

- [x] 1.1 Mailbox SQL (DELETE ... RETURNING, TTL, ON CONFLICT) and migration
- [x] 1.2 Async implementation + conformance tests on PG18; consume-once race test with two transactions coordinated by a barrier; remove sea-orm/sqlx 0.8 from parking-lot-pg via tech-lead Cargo handoff
- [ ] 1.3 Independent review at the merge gate (consume-once race, TTL)
