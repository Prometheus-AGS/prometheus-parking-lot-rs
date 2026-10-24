## Why

Long LLM streams outlive client connections.

## What Changes

IggyStreamSink publishes streamed tokens; a reconnecting client resumes from its last offset.

**Stretch:** never adopted as required scope; deferrable without blocking the phase.

## Capabilities

### New Capabilities

- `stream-replay`: Gap-free resume

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone M3; owner role `events-engineer` (team `parking-lot-core`, docs/agent-team.md).
- Depends on: `cr-32-iggy-events`.
- Plan of record: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md`; source audit `docs/update-plan-10-22-2026.md`.
