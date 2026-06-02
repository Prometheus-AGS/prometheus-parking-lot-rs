# CR-07 — Extensibility & hooks architecture

- **Track:** B · **Severity:** 🟡 Medium · **Depends on:** CR-05
- **Skill:** `clean-architecture` · **Agent:** Plan → general-purpose
- **Files:** `src/core/audit.rs` (AuditSink), `src/core/resource_pool.rs`, new `src/core/hooks.rs`

## Design
Generalize the existing `AuditSink` into a lifecycle **`Hook`** observer:
- Events: `Submitted`, `Queued`, `Started`, `Completed`, `Failed`, `Rejected`, `Evicted`.
- Multiple registered observers (`Vec<Arc<dyn Hook>>`); zero-cost when empty.
- Extension-trait ergonomics for registering hooks on a pool builder.
- `AuditSink` re-expressed as one built-in `Hook` implementation (no behavior change).

## Tasks
- [ ] Define `Hook` trait + `LifecycleEvent` enum (documented)
- [ ] Wire emission points in `ResourcePool`/`WorkerPool` (guarded when no hooks)
- [ ] Register API on pool builder; support N observers
- [ ] Refactor `AuditSink` onto `Hook`
- [ ] Example: custom metrics/logging hook
- [ ] Tests: event ordering + no-hook fast path

## Acceptance
- [ ] Documented hooks API + example; audit refactored without behavior change
- [ ] No measurable overhead when zero hooks registered
