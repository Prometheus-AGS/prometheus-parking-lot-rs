Model and worker assignments for these tasks: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md` → Task model assignments, rows keyed `phase-correctness-and-durability / cr-26-pg-lease-queue / <task id>`.

## 1. Postgres lease queue on raw sqlx with SKIP LOCKED, NOTIFY and a reaper

- [ ] 1.1 Lease/ack/nack/extend SQL and migrations; LISTEN/NOTIFY wake-up; reaper; backoff
- [ ] 1.2 Async sqlx 0.9 implementation of the TaskQueue contract; remove block_on from the queue
- [ ] 1.3 Conformance suite + SIGKILL redelivery + concurrent dispatcher tests on the PG18 image; negative controls under --cfg parking_lot_negative_control (lease never expires; SKIP LOCKED omitted)
- [ ] 1.4 Independent review at the merge gate (SQL correctness, isolation level, lease races)
