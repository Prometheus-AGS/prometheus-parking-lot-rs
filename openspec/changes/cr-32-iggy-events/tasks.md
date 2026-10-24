Model and worker assignments for these tasks: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md` → Task model assignments, rows keyed `phase-correctness-and-durability / cr-32-iggy-events / <task id>`.

## 1. Iggy hook and notifier (events only, not storage or work queue)

- [x] 1.1 IggyHook + IggyNotifier with bounded publisher and UUIDv5 IDs; iggy config section + example test (tech-lead config handoff)
- [x] 1.2 Integration tests against a pinned Iggy 0.11-compatible server container: same idempotency key published twice yields one message ID; a paused server does not raise dispatch latency beyond the bound
- [ ] 1.3 Independent review at the merge gate (backpressure, ID determinism)
