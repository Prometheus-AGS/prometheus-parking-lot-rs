Model and worker assignments for these tasks: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md` → Task model assignments, rows keyed `phase-correctness-and-durability / cr-34-mailbox-composition / <task id>`.

## 1. Composable mailbox: storage plus optional notifier

- [x] 1.1 StorageMailbox<S, N> behaviour in crates/parking-lot-core/src/mailbox/ with off-path bounded notification
- [x] 1.2 Failure-injection tests in prometheus-parking-lot-core with in-memory storage and a failing notifier
- [x] 1.3 Independent review at the merge gate (ordering of store vs notify)
