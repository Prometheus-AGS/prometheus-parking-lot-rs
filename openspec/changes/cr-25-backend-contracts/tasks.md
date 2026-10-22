Model and worker assignments for these tasks: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md` → Task model assignments, rows keyed `phase-correctness-and-durability / cr-25-backend-contracts / <task id>`.

## 1. Async lease/ack queue, async mailbox and Notifier contracts with a conformance suite

- [ ] 1.1 Define async TaskQueue (lease/ack/nack/extend), async Mailbox (deliver/consume-once/TTL) and Notifier traits
- [ ] 1.2 In-memory reference implementations; StorageMailbox<S, N> type signature only (behaviour in cr-34)
- [ ] 1.3 Backend conformance test suite (generic over backend) published for backend crates
- [ ] 1.4 Independent review at the merge gate (contract completeness, cancellation safety of async fns)
