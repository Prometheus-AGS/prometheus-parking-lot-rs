Model and worker assignments for these tasks: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md` → Task model assignments, rows keyed `phase-correctness-and-durability / cr-48-workspace-split / <task id>`.

## 1. Split into a Cargo workspace with a wasm-safe core and per-backend crates

- [ ] 1.1 Write docs/adr/0001-workspace-split.md (crate map, dependency rules, feature mapping, MSRV per crate)
- [ ] 1.2 Perform the split in one change: move modules and tests to crates/*, workspace Cargo.toml, facade re-exports, per-crate include allowlists; no behaviour change
- [ ] 1.3 Rewrite constraints.md checks for the workspace (core boundary via cargo tree, unsafe grep over crates/** with the FFI exemption) as an operator-adopted amendment
- [ ] 1.4 Independent review at the merge gate (dependency boundaries, no behaviour drift)
