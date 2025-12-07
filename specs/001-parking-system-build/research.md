# Research: Prometheus Parking-Lot Scheduler & Distributed Parking Management Platform

## Decision: Auth boundary and trust model
- **Decision**: Authentication/authorization remain upstream; scheduler trusts tenant/user/session metadata and enforces isolation via mailbox/task metadata only.
- **Rationale**: Matches spec clarification, avoids scope creep, keeps scheduler runtime-agnostic across desktop/web/cloud.
- **Alternatives considered**: (a) Token validation inside scheduler — rejected for coupling and duplicated auth; (b) Role-based controls inside scheduler — rejected for added complexity and non-goal.

## Decision: Queue and mailbox backends
- **Decision**: Provide interchangeable backends: in-memory for dev, file/embedded (e.g., Yaque) for desktop, Postgres/pgmq-style for cloud; behavior consistent across all.
- **Rationale**: Aligns with spec and README; supports durability spectrum while keeping one scheduling API.
- **Alternatives considered**: Single backend only — rejected for environment mismatch; broker-only approach (Kafka/SQS) — rejected because local/desktop needs embedded durability.

## Decision: Runtime adapters
- **Decision**: Runtime-agnostic core with adapters for native async (Tokio), web/worker, and cloud worker processes; same scheduling logic reused.
- **Rationale**: Required by spec to run across desktop, web workers, and cloud; preserves clean architecture.
- **Alternatives considered**: Tokio-only — rejected as it breaks WASM/web-worker support; per-runtime forks — rejected for maintenance cost.

## Decision: Observability and audit
- **Decision**: Emit structured lifecycle events (submit/enqueue/start/complete/expire/reject) with subjects (task, pool, tenant) and ensure fetchable within 2s over 24h window.
- **Rationale**: Matches success criteria and audit requirements; supports debugging and compliance.
- **Alternatives considered**: Minimal logging only — rejected for failing auditability; tracing without subject indices — rejected for weak retrieval guarantees.

## Decision: Performance and safety guardrails
- **Decision**: Enforce hard capacity limits (0 over-commit), queue wake target ≤1s for 95% non-expired tasks, and mailbox durability for 99% retrieval after restart/disconnect.
- **Rationale**: Directly tied to success criteria SC-001/002/003 and edge cases on misestimation/overload.
- **Alternatives considered**: Soft limits or best-effort wake — rejected for violating SC-001/002; transient-only mailbox — rejected for SC-003.
