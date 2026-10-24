Model and worker assignments for these tasks: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md` → Task model assignments, rows keyed `phase-correctness-and-durability / cr-23-multi-resource / <task id>`.

## 1. Multi-resource cost vectors (e.g. VRAM + worker slots)

- [x] 1.1 Vector-aware semaphore/dispatcher admission; multi-dimension limits via tech-lead config handoff; lifecycle events per dimension
- [x] 1.2 Extend loom/proptest invariants and negative controls to vectors
- [ ] 1.3 Independent review at the merge gate (invariant per dimension)
