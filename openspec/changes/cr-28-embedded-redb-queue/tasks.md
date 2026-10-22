Model and worker assignments for these tasks: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md` → Task model assignments, rows keyed `phase-correctness-and-durability / cr-28-embedded-redb-queue / <task id>`.

## 1. Crash-safe embedded queue (redb) replacing the JSONL Yaque queue

- [ ] 1.1 One-day spike: redb vs rustqueue; record the decision in docs/adr
- [ ] 1.2 redb lease queue + mailbox implementing the contracts; remove Yaque
- [ ] 1.3 Conformance suite + crash-point fault-injection tests; negative control under --cfg parking_lot_negative_control skips the redb commit and must lose a task
- [ ] 1.4 Independent review at the merge gate (durability under crash)
