Model and worker assignments for these tasks: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md` → Task model assignments, rows keyed `phase-correctness-and-durability / cr-52-release / <task id>`.

## 1. Version, publish and tag 0.3.0 (crates.io trusted publishing, signed tag, GitHub release, docs deploy)

- [ ] 1.1 Versioning ADR (SemVer, workspace version 0.3.0, inter-crate pins, MSRV policy); release-plz.toml and cliff.toml
- [ ] 1.2 Operator configures crates.io trusted publishing for each prometheus-parking-lot* crate and retires CARGO_REGISTRY_TOKEN from the shell profile
- [ ] 1.3 Release workflow: requires boundary receipt + operator approval environment, publishes in dependency order via OIDC, creates signed tag v0.3.0 and GitHub release, deploys docs
- [ ] 1.4 Operator approves and triggers the 0.3.0 release; record crate versions, tag SHA and release URL
- [ ] 1.5 Post-release verification: fresh-project install for each feature set; docs site reachable at the tag
- [ ] 1.6 Independent review at the merge gate (release gating, publish order, tag integrity)
