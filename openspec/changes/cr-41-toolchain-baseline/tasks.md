Model and worker assignments for these tasks: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md` → Task model assignments, rows keyed `phase-correctness-and-durability / cr-41-toolchain-baseline / <task id>`.

## 1. Rust 1.99.1 / MSRV 1.95 / edition 2024 build baseline where every target compiles

- [x] 1.1 Pin rust-toolchain.toml to 1.99.1, set rust-version = "1.95" and edition = "2024", run cargo fix --edition, and commit Cargo.lock (remove it from .gitignore)
- [x] 1.2 Drop the pgmq client crate and pgmq-ext feature wiring; keep sea-orm 1.x/sqlx 0.8 until cr-27 removes them; update constraints.md checks for MSRV 1.95 (operator-adopted amendment) and the role prompts via the team manifest
- [x] 1.3 Make benches/queue_bench.rs and both examples compile (idempotency_key, missing main), remove #![deny(warnings)], fix repository/homepage URLs; declare check-cfg for parking_lot_loom and keyed parking_lot_negative_control values under [lints.rust] unexpected_cfgs
- [x] 1.4 Single rustfmt pass, then resolve clippy -D warnings findings (fix; #[expect(..., reason)] only where a lint is wrong for this crate)
- [ ] 1.5 Independent review at the merge gate (MSRV correctness, no behaviour change hidden in lint fixes)
