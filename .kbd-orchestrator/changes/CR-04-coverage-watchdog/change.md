# CR-04 — Unblock & run coverage + suite watchdog

- **Track:** A · **Severity:** 🟡 Medium · **Depends on:** CR-01..03
- **Skill:** `rust-best-practices` · **Agent:** general-purpose
- **Files:** CI config, `tests/`, `Cargo.toml` (`[profile.coverage]` exists)

## Tasks
- [ ] Run `cargo llvm-cov --all-features --workspace` to a baseline report
- [ ] Record baseline % in phase progress + CHANGELOG
- [ ] Add suite-level wall-clock watchdog (fail loudly, never hang)
- [ ] Identify under-covered modules; file follow-up test gaps

## Acceptance
- [ ] Coverage report generated across all 6 suites with no hang
- [ ] Baseline committed; watchdog in place
