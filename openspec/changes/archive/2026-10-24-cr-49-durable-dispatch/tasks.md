Model and worker assignments for these tasks: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md` → Task model assignments, rows keyed `phase-correctness-and-durability / cr-49-durable-dispatch / <task id>`.

## 1. Pools persist accepted work through the durable queue (persist, lease, ack after delivery, extend)

- [x] 1.1 Dispatcher sources from TaskQueue::lease when durable; persist-before-accept on submit; ack after mailbox write; lease heartbeat
- [x] 1.2 Contract additions (if any) via tech-lead handoff; facade config wiring
- [x] 1.3 Child-process SIGKILL tests for ResourcePool on PG18 and redb, ack-ordering test, long-task lease test, negative control
- [x] 1.4 Independent review at the merge gate (durability ordering: persist, lease, deliver, ack)
