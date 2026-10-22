## Why

Plan section 8: Iggy fits lifecycle-event propagation and mailbox notification; it is not suited to mailbox storage or the priority work queue.

## What Changes

iggy feature/crate: IggyHook and IggyNotifier, UUIDv5 message IDs from the idempotency key, bounded off-path publisher; integration test against a pinned Iggy container.

## Capabilities

### New Capabilities

- `iggy-events`: Event delivery via Iggy

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone M3; owner role `events-engineer` (team `parking-lot-core`, docs/agent-team.md).
- Depends on: `cr-31-hook-emission`, `cr-34-mailbox-composition`.
- Plan of record: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md`; source audit `docs/update-plan-10-22-2026.md`.
