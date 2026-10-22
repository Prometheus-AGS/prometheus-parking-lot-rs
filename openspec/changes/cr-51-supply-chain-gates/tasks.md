Model and worker assignments for these tasks: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md` → Task model assignments, rows keyed `phase-correctness-and-durability / cr-51-supply-chain-gates / <task id>`.

## 1. CI gates: secrets, purged paths, pre-purge history and package allowlists

- [ ] 1.1 Add CI jobs (fetch-depth 0): gitleaks full history, path-history emptiness assertion, pre-purge root-set ancestry rejection, per-crate cargo package --list allowlist check
- [ ] 1.2 Prove each gate with a negative-control scratch branch and record the failing run URLs
- [ ] 1.3 Independent review at the merge gate (gate completeness, shallow-clone pitfalls)
