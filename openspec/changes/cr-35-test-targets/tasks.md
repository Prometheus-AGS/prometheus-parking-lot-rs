Model and worker assignments for these tasks: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md` → Task model assignments, rows keyed `phase-correctness-and-durability / cr-35-test-targets / <task id>`.

## 1. Unit and integration suites become real test targets on the current testcontainers API

- [ ] 1.1 Register unit/integration suites as test targets in their crates
- [ ] 1.2 Port Postgres integration tests to the current testcontainers API and the CR-43 image (with postgres-engineer)
- [ ] 1.3 Add the orphan-test check script to CI
- [ ] 1.4 Independent review at the merge gate (no test silently dropped)
