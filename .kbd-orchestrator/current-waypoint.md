# Current Waypoint

- **Phase:** `phase-certification-remediation`
- **Stage:** Plan ✅ complete → next is **Execute**
- **Goal:** Fix 100% of certification issues and make the crate worthy of public crates.io release.
- **Change backend:** native KBD (`.kbd-orchestrator/changes/`)

## Next action
```
/kbd-execute phase-certification-remediation
```
Start **Milestone 1 (CR-01)**; run **CR-08** in parallel (independent publish blocker).

## Change inventory (12)
| Milestone | Changes |
|-----------|---------|
| M1 — Certify | CR-01 🔴, CR-02, CR-04 |
| M2 — Harden | CR-03, CR-05, CR-06, CR-07 |
| M3 — Publish | CR-08 🔴, CR-09, CR-10, CR-11 ✅, CR-12 |

CR-11 (publishing playbook) delivered this phase: `docs/PUBLISHING.md`.

## Decisions locked
- Shutdown: Tokio three-part model + `CancellationToken` + `ctrl_c`/SIGTERM.
- Errors: single `#[non_exhaustive]` thiserror enum; rich `QueueFull` context.
- Mailbox: at-least-once + TTL/eviction; notifier failure ≠ data loss.
- Hooks: generalize `AuditSink` → multi-observer `Hook` lifecycle bus.
- R2 policy: **drain-with-deadline, then cancel** (configurable).
