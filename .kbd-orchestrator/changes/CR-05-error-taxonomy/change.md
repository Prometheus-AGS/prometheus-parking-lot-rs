# CR-05 — Unified, information-rich error taxonomy

- **Track:** B · **Severity:** 🟠 High · **Depends on:** CR-01
- **Skill:** `error-handling` · **Agent:** general-purpose
- **Files:** `src/core/error.rs`, `src/core/worker_pool.rs` (PoolError), all call sites

## Problem
Two overlapping error enums: `PoolError` (no context on `QueueFull`) and `SchedulerError`
(`QueueFull(String)`, `Backend(String)` stringly-typed). No `#[source]` chains. Not
`#[non_exhaustive]`.

## Design
- Single public `#[non_exhaustive]` error enum via `thiserror`, with `#[source]` chains.
- `QueueFull { pool: String, depth: usize, capacity: usize, retry_after: Option<Duration> }`.
- `Backend` wraps a typed source, not a bare `String`.
- `anyhow` stays for application layer only (`AppResult`).

## Tasks
- [ ] Define unified error type + variants with docs
- [ ] Enrich queue-full at all 3 sites (memory/yaque/resource_pool) with depth/capacity
- [ ] Add `#[source]`/`#[from]` chains; remove stringly-typed `Backend(String)`
- [ ] Mark `#[non_exhaustive]`; map old variants for compatibility
- [ ] Update tests asserting error display/match

## Acceptance
- [ ] One documented public error type; queue-full carries actionable detail
- [ ] `cargo clippy` clean; error docs render in rustdoc
