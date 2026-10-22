Model and worker assignments for these tasks: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md` → Task model assignments, rows keyed `phase-correctness-and-durability / cr-42-ci-rebuild / <task id>`.

## 1. Green, cached, locked CI on stable and MSRV

- [x] 1.1 Rewrite .github/workflows/ci.yml: rust-cache, --locked, nextest, toolchain matrix 1.99.1 + 1.95
- [x] 1.2 Resolve cargo-deny: upgrade testcontainers (tokio-tar RUSTSEC-2025-0111) via tech-lead handoff; fix deny.toml license policy
- [x] 1.3 Update .cargo/config.toml aliases and docs/build-performance.md for the workspace
- [ ] 1.4 Independent review at the merge gate (CI covers every required job; no job silently skipped)
