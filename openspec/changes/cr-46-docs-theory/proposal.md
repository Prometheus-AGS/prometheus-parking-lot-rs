## Why

The operator requires extensive documentation of the theory and history behind the library and its value for agentic software.

## What Changes

Cited concept chapters: parking lots (futex 2002, WebKit WTF::ParkingLot 2015, Rust parking_lot) versus this crate's admission-control parking lot; admission control and bin packing; priority scheduling and starvation; leases (Gray & Cheriton 1989) and at-least-once delivery; SKIP LOCKED queues; mailboxes and the actor model (Hewitt 1973, Erlang); thread and worker pools, work stealing (Cilk), Tokio; idempotency and outbox; event logs (Kafka, Iggy); why agentic workloads (GPU VRAM, long tasks, disconnecting clients) need all of them.

## Capabilities

### New Capabilities

- `documentation-content`: Cited concept documentation

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone M4; owner role `product-manager` (team `parking-lot-core`, docs/agent-team.md).
- Depends on: `cr-45-docs-site`.
- Plan of record: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md`; source audit `docs/update-plan-10-22-2026.md`.
