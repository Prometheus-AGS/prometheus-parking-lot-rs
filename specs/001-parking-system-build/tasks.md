# Tasks: Prometheus Parking-Lot Scheduler & Distributed Parking Management Platform

**Input**: Design documents from `/specs/001-parking-system-build/`  
**Prerequisites**: plan.md (required), spec.md (required for user stories), research.md, data-model.md, contracts/

**Tests**: Tests are optional; success criteria rely on load/behavior validation tasks included below.

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)
- Include exact file paths in descriptions

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Project initialization and basic structure

- [X] T001 Create feature docs index in specs/001-parking-system-build/README.md summarizing plan, spec, research links
- [X] T002 Align src/ structure with plan (core/config/builders/infra/runtime/util) by adding placeholder mods in src/lib.rs and module directories
- [X] T003 [P] Pin toolchain and fmt/clippy settings in rust-toolchain.toml and .cargo/config.toml if absent

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core infrastructure that MUST be complete before ANY user story can be implemented

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [X] T004 Define config models scaffold in src/config/mod.rs for pools, backends, timeouts (without behavior)
- [X] T005 [P] Establish error types in src/core/error.rs using thiserror for library, anyhow for app-facing helpers
- [X] T006 [P] Add tracing/logging initialization helper in src/util/telemetry.rs covering structured events
- [X] T007 Create ResourcePool skeleton and traits (TaskQueue, Mailbox, Spawn) in src/core with capacity accounting placeholders
- [X] T008 Set up serialization helpers and type aliases in src/util/serde.rs for mailbox keys, priorities, resource costs
- [X] T009 Wire plan structure in tests/ by creating empty folders tests/unit, tests/integration, tests/contract to avoid path churn

**Checkpoint**: Foundation ready - user story implementation can now begin in parallel

---

## Phase 3: User Story 1 - Configure capacity-aware pools via declarative config (Priority: P1) 🎯 MVP

**Goal**: Operators define pools, limits, queue/mailbox backends in JSON/YAML so agents enforce capacity without code changes.

**Independent Test**: Load a config with multiple pools; submit tasks tagged per pool; verify immediate start when within capacity and rejection after queue depth reached.

### Implementation for User Story 1

- [X] T010 [P] [US1] Implement SchedulerConfig/PoolConfig parsing with validation in src/config/pool.rs
- [X] T011 [P] [US1] Implement ResourcePool capacity enforcement (no over-commit) in src/core/resource_pool.rs
- [X] T012 [US1] Add queue depth enforcement and rejection reason handling in src/core/resource_pool.rs
- [X] T013 [P] [US1] Build config→pool wiring in src/builders/pool_builder.rs to instantiate pools/backends from config
- [X] T014 [US1] Provide sample configs and doc snippets in examples/configs/ and docs/README additions per quickstart

**Checkpoint**: User Story 1 fully functional and independently testable

---

## Phase 4: User Story 2 - Manage overload, timeouts, and mailbox delivery (Priority: P2)

**Goal**: Park overflow, enforce deadlines, and deliver outcomes to mailbox for disconnected clients.

**Independent Test**: Simulate overload with deadlines; verify expired tasks are marked and recorded; mailbox fetch returns outcomes after client disconnects.

### Implementation for User Story 2

- [X] T015 [P] [US2] Implement deadline tracking and prune/expire in src/core/resource_pool.rs
- [X] T016 [US2] Implement mailbox abstractions and in-memory backend in src/infra/mailbox/memory.rs with delivery/fetch semantics
- [X] T017 [P] [US2] Implement queue abstraction and in-memory backend with priority/deadline ordering in src/infra/queue/memory.rs
- [X] T018 [US2] Implement task submission/status endpoints from contracts/openapi.yaml in src/runtime/api.rs mapping to ResourcePool
- [X] T019 [US2] Implement mailbox fetch endpoint per contracts/openapi.yaml in src/runtime/api.rs using mailbox abstraction
- [X] T020 [US2] Add overload/zero-capacity rejection path surfaced via API responses consistent with contracts/openapi.yaml

**Checkpoint**: User Stories 1 AND 2 should both work independently

---

## Phase 5: User Story 3 - Provide persistence, auditability, and fairness across deployments (Priority: P3)

**Goal**: Durable queues/mailboxes, audit events, and fairness across deployments with restart recovery.

**Independent Test**: Run with persistent backend, restart mid-queue, confirm tasks and audit records persist; retrieve audit entries by subject within SLA.

### Implementation for User Story 3

- [X] T021 [P] [US3] Implement file/embedded queue/mailbox backend (e.g., Yaque) adapters in src/infra/queue/yaque.rs and src/infra/mailbox/yaque.rs
- [X] T022 [P] [US3] Implement Postgres/pgmq-style queue/mailbox adapters in src/infra/queue/postgres.rs and src/infra/mailbox/postgres.rs
- [X] T023 [US3] Add audit event emission and subject indexing in src/core/audit.rs and integrate with lifecycle hooks
- [X] T024 [US3] Implement pool listing/health endpoints from contracts/openapi.yaml in src/runtime/api.rs exposing config snapshot and health
- [X] T025 [US3] Add restart-resume behavior tests/scripts in tests/integration/restart_resume.rs to ensure queued tasks survive process restart

**Checkpoint**: All user stories should now be independently functional

---

## Phase N: Polish & Cross-Cutting Concerns

**Purpose**: Improvements that affect multiple user stories

- [X] T026 [P] Update quickstart in specs/001-parking-system-build/quickstart.md with final command examples and configs
- [X] T027 Harden error messaging and structured logging across backends in src/util/telemetry.rs and src/core/error.rs
- [X] T028 [P] Add load-test harness script in examples/loadtest/ to validate SC-001/002/003 scenarios
- [X] T029 Performance tuning and benchmarks in benches/ to verify queue wake and capacity guardrails
- [X] T030 Documentation sweep in docs/ and examples/ aligning with coding standards and clarifications

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies - can start immediately  
- **Foundational (Phase 2)**: Depends on Setup completion - BLOCKS all user stories  
- **User Stories (Phase 3+)**: Depend on Foundational phase completion; proceed in priority order (P1 → P2 → P3) though different stories may run in parallel once dependencies satisfied  
- **Polish (Final Phase)**: Depends on completion of desired user stories

### User Story Dependencies

- **User Story 1 (P1)**: Starts after Foundational; no dependencies on other stories.
- **User Story 2 (P2)**: Starts after Foundational; depends on US1 capacity/queue wiring.
- **User Story 3 (P3)**: Starts after Foundational; builds on US1 pools/config and US2 queue/mailbox API for persistence and audit.

### Within Each User Story

- Build models/traits before services/endpoints.
- Wire adapters before exposing endpoints relying on them.
- Keep mailbox/queue backends pluggable via config.
- Maintain upstream-auth boundary; no auth code inside scheduler.

### Parallel Opportunities

- Setup tasks T002/T003 can run in parallel after T001.
- Foundational tasks T005/T006/T008 can proceed in parallel after T004.
- In US1, T010/T011/T013 can proceed in parallel; T012 depends on T011.
- In US2, T015/T017 can proceed in parallel; T016 can proceed after T007 from foundational; T018/T019 depend on queue/mailbox availability.
- In US3, T021/T022 can run in parallel; T023 follows core hooks; T024 depends on runtime API; T025 can run after persistent backends exist.
- Polish tasks T026/T027/T028 can run in parallel once user stories complete.

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup  
2. Complete Phase 2: Foundational (CRITICAL - blocks all stories)  
3. Complete Phase 3: User Story 1  
4. **STOP and VALIDATE**: Load-test config enforcement and queue depth rejection per acceptance for US1  
5. Deploy/demo if ready

### Incremental Delivery

1. Setup + Foundational → foundation ready  
2. Add User Story 1 → validate independently (capacity & queue depth)  
3. Add User Story 2 → validate deadlines, mailbox delivery, overload rejection  
4. Add User Story 3 → validate persistence, audit retrieval SLA, restart-resume  
5. Demo after each story if stable

### Parallel Team Strategy

With multiple developers:

1. Team completes Setup + Foundational together  
2. After foundation, parallelize: Developer A on US1, Developer B on US2, Developer C on US3 backends  
3. Integrate via config and API contracts; validate each story independently before merging

---

## Notes

- [P] tasks = different files, no dependencies
- [Story] label maps task to specific user story for traceability
- Each user story should be independently completable and testable
- Avoid: vague tasks, same file conflicts, cross-story dependencies that break independence
