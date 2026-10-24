Model and worker assignments for these tasks: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md` → Task model assignments, rows keyed `phase-correctness-and-durability / cr-52-release / <task id>`.

## 1. Version, publish and tag 0.3.0 (crates.io trusted publishing, signed tag, GitHub release, docs deploy)

- [x] 1.1 Versioning ADR (SemVer, workspace version 0.3.0, inter-crate pins, MSRV policy); release-plz.toml and cliff.toml
- [x] 1.2 Operator configures crates.io trusted publishing for each prometheus-parking-lot* crate and retires CARGO_REGISTRY_TOKEN from the shell profile
- [x] 1.3 Release workflow: requires boundary receipt + operator approval environment, publishes in dependency order via OIDC, creates signed tag v0.3.0 and GitHub release, deploys docs
- [x] 1.4 Operator approves and triggers the 0.3.0 release; record crate versions, tag SHA and release URL
- [x] 1.5 Post-release verification: fresh-project install for each feature set; docs site reachable at the tag
- [x] 1.6 Independent review at the merge gate (release gating, publish order, tag integrity)

## Closure disposition — 2026-10-24

Tasks 1.1–1.3 are administratively closed as DEFERRED to 0.4.0 issues #86 (versioning/release tooling) and #85 (trusted publishing/release workflow). Signed release tags remain #87; v0.3.0 is annotated and unsigned. Checked boxes on those tasks record disposition, not implementation. Authority: operator decision `phase-correctness-and-durability-close-deferrals-20261024` ("Record both deferrals and close the phase").

Tasks 1.4–1.5 record the already-completed publication and the sender's post-release verification in `.prometheus/decisions.md` (2026-10-24); task 1.6 records the completed cumulative boundary review and accepted exit criteria. No new publication, tag change, credential retirement, or production test run is performed by this closure.
