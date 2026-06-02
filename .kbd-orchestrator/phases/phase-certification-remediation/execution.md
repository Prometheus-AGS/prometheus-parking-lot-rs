# KBD Execution — phase-certification-remediation

- **Backend:** `native-tool` (Claude Code edits the Rust crate directly; no OpenSpec present)
- **Started:** 2026-06-02
- **Order:** Milestone 1 first — **CR-01** (critical path) ∥ **CR-08** (independent publish blocker)
- **QA gate:** artifact-refiner per change where >3 files & not docs-only; CR-08 (docs/config) eligible to skip

## Dispatch contract
| Change | Backend | Notes |
|--------|---------|-------|
| CR-01 | native-tool | Fix `retrieve_async` leak; verify suite no longer hangs |
| CR-08 | native-tool | Metadata + `LICENSE-APACHE`; parallel, independent |
| CR-02 | native-tool | After CR-01 |
| CR-03→CR-13→CR-14 (M2) | native-tool | Land `TaskMetadata.idempotency_key` early, coordinated w/ CR-05 |
| CR-04..CR-12 | native-tool | Per plan milestones |

## Acceptance for Milestone 1
- CR-01: `cargo test --test worker_pool_test` exits cleanly with **no `kill`/hang**.
- CR-08: `cargo publish --dry-run` metadata clean; both license files present.
