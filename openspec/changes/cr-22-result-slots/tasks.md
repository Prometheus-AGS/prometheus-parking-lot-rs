Model and worker assignments for these tasks: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md` → Task model assignments, rows keyed `phase-correctness-and-durability / cr-22-result-slots / <task id>`.

## 1. Async result slots with TTL that use no blocking threads

- [ ] 1.1 Replace condvar slots with oneshot/Notify slots; blocking API via blocking_recv; TTL sweeper
- [ ] 1.2 Load test for blocking-thread usage and TTL tests
- [ ] 1.3 Independent review at the merge gate (no blocking in async context)
