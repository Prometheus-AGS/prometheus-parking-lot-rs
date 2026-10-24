Model and worker assignments for these tasks: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md` → Task model assignments, rows keyed `phase-correctness-and-durability / cr-30-pgmq-sql-backend / <task id>`.

## 1. pgmq backend through SQL (one queue per priority), no client crate

- [x] 1.1 SQL implementation over pgmq functions with visibility-timeout leases and per-priority queues; add the pgmq config section and example test (tech-lead config handoff)
- [x] 1.2 Conformance suite against the PG18 image with pgmq
- [x] 1.3 Independent review at the merge gate (lease mapping to visibility timeout)
