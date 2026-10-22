# Archived documents

These files describe an API, a plan or a state of the code that no longer
exists. They are kept for history only. Do not build against them, and do not
cite them as describing the current crate. The verdicts come from the docs
audit in [`docs/update-plan-10-22-2026.md`](../update-plan-10-22-2026.md) §10.1;
the move is OpenSpec change `cr-16-docs-truth`.

For the current crate, read the [README](../../README.md), the
[CHANGELOG](../../CHANGELOG.md), the ADRs in [`docs/adr/`](../adr/) and the
rustdoc (`cargo doc --workspace --no-deps --open`).

| File | Archived because |
| --- | --- |
| `PLAN-MUTEX.md`, `SPEC-MUTEX.md`, `TASKS-MUTEX.md` | Describe a `parking_lot` re-export wrapper (`src/mutex.rs`) from a different crate; no such module exists here. |
| `PLAN-RWLOCK.md`, `SPEC-RWLOCK.md`, `TASKS-RWLOCK.md` | Same primitives crate (`src/rwlock.rs`); no such module exists here. |
| `PLAN-ONCE.md`, `SPEC-ONCE.md` | Same primitives crate (`Once`/`OnceCell`, `src/once.rs`); no such module exists here. |
| `PLAN-CONDVAR.md`, `SPEC-CONDVAR.md`, `TASKS-CONDVAR.md` | Same primitives crate (`src/condvar.rs`); no such module exists here. |
| `TODO.md` | Sprint list of the primitives crate (2024-12-06, "Condvar awaiting verification"); replaced by `docs/TODO.md`, which points at the live plan. |
| `API.md` | Documents the `Mutex<T>`/`RwLock<T>`/`Condvar` API of the primitives crate; replaced by `docs/API.md` and the rustdoc. |
| `CHANGELOG.md` | Its entries are the primitives crate's (Mutex, RwLock, Once, Condvar); the crate's changelog is the root `CHANGELOG.md`. |
| `DISCUSSION.md` | The original design prompt chain (ReddDb, "Option C", early layouts); superseded by the ADRs and the update plan. |
| `IMPLEMENTATION.md` | The 0.1 phase plan (Phases 0-6); partly done, partly abandoned (pgmq, HTTP notifier, `local_tauri.rs`); superseded by the update plan §11 and the OpenSpec changes. |
| `PARKING_LOT_FINAL_MIGRATION.md` | 0.2 `WorkerPool` guide: infallible executors, `PoolError`, and capacity enforcement it implied but 0.2 did not do; the README now documents the 0.3.0 `WorkerPool`. |
| `Comprehensive Architectural Report ... Audit Infrastructur.md` | Physical parking-garage design (vehicles, spots, pricing) and a Redis audit pattern; not about this library. |
| `IMPLEMENTATION_SUMMARY.md` | Pre-workspace-split report on `src/infra/queue/postgres.rs` and friends; those files moved to `crates/parking-lot-pg` and the "production-ready" claims are contradicted by the update plan (B1, B2). |
| `POSTGRES_IMPLEMENTATION.md` | Same pre-split Postgres report (`src/infra/...` paths); the SeaORM backends it describes are replaced by the lease queue and mailbox (cr-26, cr-27). |
