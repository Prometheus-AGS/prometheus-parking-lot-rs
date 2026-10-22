## Why

The WASM pool uses tokio::spawn and has never been built for wasm32.

## What Changes

Port the WASM WorkerPool onto the shared dispatcher in parking-lot-wasm with wasm-bindgen-futures::spawn_local and JS timers, including capacity admission (fixes C1 at wasm.rs:287) and execution budgets; getrandom wasm_js config; document that executor panics abort the module on wasm32 stable; CI check, a core dependency check (`cargo tree -p prometheus-parking-lot-core --target wasm32-unknown-unknown -e normal` contains no tokio), and a wasm-bindgen-test smoke/budget test.

## Capabilities

### New Capabilities

- `platform-targets`: Web target

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone M4; owner role `platform-engineer` (team `parking-lot-core`, docs/agent-team.md).
- Depends on: `cr-19-dispatcher-semaphore`.
- Plan of record: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md`; source audit `docs/update-plan-10-22-2026.md`.
