Model and worker assignments for these tasks: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md` → Task model assignments, rows keyed `phase-correctness-and-durability / cr-38-wasm-target / <task id>`.

## 1. WASM pool on spawn_local with a CI check job

- [x] 1.1 Port the wasm pool onto the dispatcher in parking-lot-wasm (spawn_local + JS timers, capacity admission, budgets); getrandom config; document panic=abort limit
- [x] 1.2 CI wasm check, core dependency-graph check, and wasm-bindgen-test smoke/capacity/budget job
- [x] 1.3 Independent review at the merge gate (no tokio runtime on wasm)
