## Why

Users need explicit, working examples per deployment scenario and a clear statement of value for agentic software development.

## What Changes

Guides: GPU-VRAM inference pool, Tauri local LLM with embedded queue, cloud multi-tenant Postgres, disconnected-client retrieval, streaming replay (only if stretch cr-33 merged), shutdown/requeue, Iggy events, configuration, release/upgrade notes; an agentic-value chapter; rustdoc API reference linked; every code sample compiled in CI (examples/ or doctests).

## Capabilities

### New Capabilities

- `documentation-content`: Working examples per scenario

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone M4; owner role `product-manager` (team `parking-lot-core`, docs/agent-team.md).
- Depends on: `cr-46-docs-theory`, `cr-37-config-schema`, `cr-29-shutdown-cancellation`, `cr-44-mobile-desktop-bindings`, `cr-32-iggy-events`, `cr-23-multi-resource`.
- Plan of record: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md`; source audit `docs/update-plan-10-22-2026.md`.
