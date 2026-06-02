# CR-09 — Documentation pass

- **Track:** C · **Severity:** 🟠 High · **Depends on:** CR-01..07
- **Skill:** `coding-guidelines` · **Agent:** general-purpose
- **Files:** `src/lib.rs`, all public modules, `README.md`, `docs/`, `docs/CHANGELOG.md`

## Tasks
- [ ] Ensure `#![deny(missing_docs)]` passes after API changes
- [ ] Convert `rust,ignore` examples to runnable doctests where feasible
- [ ] Crate-root docs: features matrix, backend selection, shutdown/Ctrl-C, hooks
- [ ] Sync README ↔ lib.rs (consider `#[doc = include_str!("../README.md")]`)
- [ ] Prune stale fork-era docs (`docs/SPEC-*`, `PLAN-*`, `TASKS-*` for mutex/rwlock/once)
- [ ] Update `docs/CHANGELOG.md` with the remediation entries
- [ ] `cargo doc --no-deps` warning-free; check intra-doc links

## Acceptance
- [ ] `cargo doc` clean; doctests pass; README example compiles
- [ ] No stale synchronization-primitive docs shipped in the package
