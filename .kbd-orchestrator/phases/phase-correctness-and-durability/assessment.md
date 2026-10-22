ASSESSMENT: phase-correctness-and-durability
Project: prometheus_parking_lot
Date: 2026-10-22
Codebase baseline: `main` @ 90a6194 — the 2026-10-22 audit (written against 7939b07) plus the merged execution-timeout fix (#2), agent tooling (#3, #5, #6) and this phase's start (#7).
Cross-tool progress: none — this phase has 0 registered changes; no other tool has worked it.

## Method

Fact-finding only. Findings were re-checked on `90a6194` by grep/read or by running
the command shown, except those marked "(audit, not re-verified)", which come from
the audit alone. Revised after an adversarial review (PASS, 7 warnings, 2 suggestions;
`review/assess/findings.json`). Model preflight status: `degraded` — no judge providers are
configured in liter-llm, so the adversarial vet of this file may share a model family
with its author.

## Recalled lessons that apply (from prior-context.md)

- "External integrations require at least one real-service test" — shapes acceptance for
  CR-26/27 (real Postgres via testcontainers) and CR-32 (real Iggy container); mocks do not
  count.
- "Boundary gates must launch the packaged artifact" — for a library this means the gate
  must build the published surface (`cargo package` + compile examples/benches), not only
  `--lib --tests`; today that surface does not compile (see BUILD HEALTH).
- "Discover test environment state instead of encoding checkout assumptions" — relevant to
  one-worktree-per-role: tests must not assume the primary checkout path or a warm
  build-dir.
- "Operator controls PR merges and requires end-of-turn KBD status" and "Ask once at phase
  boundary for operator-only actions" — consistent with this operator's pattern so far
  (every merge done by the operator); collect CR-15 force-push approval once, up front.
- "Check harness configs before removing worktrees" — applies to
  `scripts/agent-worktree.sh remove`.

## Knowledge gaps

None recorded in prior-context.md.

IMPLEMENTATION STATUS

Promise 1 — weighted capacity is never exceeded
- `ResourcePool` admission (`src/core/resource_pool.rs`): PARTIAL — enforces `max_units`; execution is now bounded by `default_timeout`/deadline (#2). Re-enqueue on wake (L569–572, L596) logs and drops on failure (C4, verified by read). C3 lost wakeup verified by read (L303–341): the capacity check, the `queue.len()` depth check and `enqueue` take separate locks, so a completion between them can find an empty queue and leave the task parked; the same split lets concurrent submitters exceed `max_queue_depth`.
- `WorkerPool` native (`src/core/worker_pool/native.rs`): STUB for capacity — L621 `active_units.fetch_add(units)` unconditionally after dequeue; no admission against `max_units` (C1, verified). Priority not honoured (FIFO channel; audit). Execution is unbounded: L635–637 awaits `executor.execute` with no timeout (#2 covered only ResourcePool).
- `WorkerPool` WASM (`src/core/worker_pool/wasm.rs`): STUB for capacity — `Semaphore::new(worker_count)` (L193) bounds task count, and L287 adds units unconditionally (C1 also applies); L292 executes with no budget.
- Panic containment: MISSING — the only `catch_unwind` in `src/` is in `hooks.rs:304`; no execution path catches executor panics (C2, verified).
- Weighted semaphore / priority dispatcher / skip-ahead / multi-resource cost (CR-19, 20, 23): MISSING.
- `sync_wake_worker_loop` (`resource_pool.rs:762`): STUB, publicly exported, discards the task (L848) (C5, verified).

Promise 2 — accepted work is never silently lost
- Postgres queue (`src/infra/queue/postgres.rs`): NON-FUNCTIONAL (verified by read) — each of the 4 DB calls (L114–117, 180–183, 225–228, 248–250) takes `Handle::try_current()` and then `block_on`s on it, which panics when called inside a runtime; `ResourcePool::submit` is async and calls `enqueue`/`len` (resource_pool.rs L320, L340), so any Postgres-backed pool that parks a task panics. The dequeue comment claims `SKIP LOCKED` but the code uses `.lock_exclusive()` (L188–193), i.e. plain `FOR UPDATE`, which serializes dispatchers. Delete-on-dequeue (at-most-once, audit). CR-26 has no working baseline to preserve.
- Postgres mailbox: NON-FUNCTIONAL by the same `block_on` pattern (L182); read-back gap (audit).
- Native worker loop: `block_on` at `native.rs:635`. **Verified silent loss on shutdown:** after `task_rx.recv()` returns a task, a set shutdown flag breaks the loop while holding it (L603–616) — no result, no requeue, `queued_tasks` never decremented; the waiter times out. CR-29's DrainThenCancel must cover WorkerPool, not only ResourcePool.
- Embedded "Yaque" queue: PARTIAL — JSONL, not crash-safe, ignores priority (audit, not re-verified). The Yaque mailbox (`src/infra/mailbox/yaque.rs`) was not assessed this run.
- Async lease/ack/nack queue trait (CR-25), redb store (CR-28): MISSING.
- Shutdown/cancellation (`shutdown.rs`, `cancellation.rs`): STUB — types exist; `ShutdownPolicy`, `CancellationSink`, `SinkOutcome::Requeue` are not wired into either pool (audit; consistent with no call sites found).

Promise 3 — results reach disconnected clients
- Mailbox delivery from `ResourcePool`: PARTIAL — delivers `Completed` or `Failed("…timeout budget")` (#2); no TTL, no notifier composition.
- Hooks / events: STUB — `HookBus` and `emit_*` are exported from `lib.rs`/`core/mod.rs` but never called from a pool (verified: no call sites outside `hooks.rs`).
- Notifier contract: MISSING — no notifier trait or type exists anywhere in `src/` (the old CLAUDE.md's `HttpCallbackNotifier` never existed). CR-32 and CR-34 both presuppose one, so it is a tech-lead contract that must land before they fan out.
- Iggy hook/notifier (CR-32..34): MISSING.

Platform and repository
- WASM pool (`worker_pool/wasm.rs:267`): STUB — uses `tokio::spawn`, never built for `wasm32` (wasm target not installed locally; no CI job).
- Mobile (UniFFI/cargo-ndk/iOS): MISSING.
- Config (`build_pools_from_scheduler_config`, backend params): MISSING (CR-37; audit).
- Docs: PARTIAL/STALE — ~30% of `docs/` describes a different crate (Mutex/RwLock/Once/Condvar specs); README samples use non-existent APIs (audit, not re-verified this run).

CROSS-TOOL PROGRESS
- NONE — `progress.json` for this phase has 0 changes; no IN_PROGRESS work or blockers from other tools.
- Prior phase `phase-certification-remediation` is 14/14 COMPLETE in canonical state; the audit (section 10.2) argues several of those completions overstate delivery (hooks, cancellation capture, shutdown policy exist only as types). CR-17 must correct this through typed `prometheus kbd` commands, not hand edits.

SPEC GAP SUMMARY
- No canonical specs: `openspec/specs/` is empty and `openspec/changes/` holds only `archive/`. The de facto spec is `docs/update-plan-10-22-2026.md` §11 plus `PARKING_LOT.md`; acceptance criteria exist only as plan-table prose.
- Implemented but unspecified / contradicting spec: public `sync_wake_worker_loop`; WASM claim without a build; `pgmq-ext` feature on an alpha client that breaks MSRV.
- Specified but unimplemented: everything in IMPLEMENTATION STATUS marked MISSING/STUB.
- The audit predates #2: its ResourcePool findings still hold, and ResourcePool execution is now bounded. Both WorkerPool implementations still execute without a budget (native.rs:635, wasm.rs:292); no CR names this, so it belongs in CR-19 or CR-21 scope.

BUILD HEALTH
- build check (`cargo check --lib --tests`, default features): PASS (also exercised by the #2 test run).
- build check (`cargo ck` = `check --all-targets`): FAIL — `benches/queue_bench.rs` and `examples/yaque_pool.rs` miss `TaskMetadata.idempotency_key` (E0063); `examples/postgres_pool.rs` has no `main` (E0601). `cargo t` (nextest) fails on the same targets. Consequence: the per-role aliases added in #5 cannot pass until benches/examples compile.
- `--all-features`: FAIL — does not resolve on the pinned 1.91.1 (`pgmq 0.34.0-alpha.*` → `sqlx 0.9` needs rustc 1.94); `Cargo.lock` is gitignored, so resolution drifts (CI resolved alpha.7, local alpha.5).
- CI on `main` (run after #7): every job fails — Clippy, Docs, Test (1.91.1), Coverage: MSRV resolution error above; Format: 355 hunks; Deny: RUSTSEC-2025-0111 (`tokio-tar`, via dev-dependency testcontainers — inferred) and a licenses failure; Test (stable): cancelled.
- known violations: clippy 157 findings with `-D warnings` (default features, all targets attempted); rustfmt 355 hunks; `#![deny(warnings)]` in `src/lib.rs:76`; `repository`/`homepage` point to `prometheus-ai/prometheus-parking-lot` (not this repo).
- Known flake: `worker_pool_test::test_timeout_handling` failed 3/8 runs (2026-10-22).
- test coverage: PARTIAL — `tests/unit/*` and `tests/integration/*` are not compiled (no top-level module declaration; `tests/*.rs` lists only six root files) and use the removed testcontainers `clients::Cli`/`RunnableImage` API. The 55.73% coverage baseline excludes them; Postgres backends have no running tests.

CONSTRAINT CHECK
- AGENTS.md violations: none of its hard rules are newly violated by code; but "Public API changes go in CHANGELOG" and "src/core never imports infra crates" are only checkable once the build is green.
- constraints.md violations (blocking): build-passes FAIL (all-targets); clippy-clean FAIL; tests-pass FAIL (all-features / nextest); fmt-clean FAIL; docs-build FAIL in CI; supply-chain-clean FAIL (cargo deny). no-unsafe-code: PASS (crate denies unsafe). core-has-no-infra-deps: PASS — no `use sea_orm|sqlx|pgmq|reqwest` in `src/core` (grep, 2026-10-22).
- **Constraint conflicts:**
  - constraints.md's Path Ownership clause lists `.codex/` as "tracked tooling state — do not delete" and says it is excluded from the package via `Cargo.toml exclude`. The exclude claim is false (no `.codex/` entry), and CR-15's purge, `.gitignore` and `include` allowlist contradict the clause. The operator must amend it before CR-15 runs.
  - The blocking build/clippy/test/docs constraints all use `--all-features`, which cannot resolve on MSRV 1.91.1 while `pgmq` is present. Plan §11 puts the pgmq decision (CR-30) in M2, so no change before M2 can satisfy its blocking constraints. Before M1, either pull the pgmq drop/pin into the M0 build-baseline change, or have the owner amend those constraint commands temporarily.
- Security (not a constraint file item, highest severity): `.codex/auth.json`, `.codex/config.toml` and `.codex/history.jsonl` are tracked in a PUBLIC repository (21 tracked `.codex/` files). The audit's `.codex/sessions` concern is also live. `Cargo.toml` uses an `exclude` list that does not cover `.codex/`, so `cargo package` would ship them.

GOAL PROGRESS
- G1 three promises true: NOT MET — each promise has at least one verified violation (C1, C2, C4/C5; block_on in backends; hooks never emitted).
- G2 M0 containment (CR-15/16/17): NOT MET — credentials still tracked; docs stale; KBD certification progress uncorrected.
- G3 M1 core rework (CR-18..24): NOT MET — no dispatcher/semaphore; no loom/property tests.
- G4 M2 durable backends (CR-25..30): NOT MET — sync trait, block_on, delete-on-dequeue, JSONL queue.
- G5 M3 events (CR-31..34): NOT MET — HookBus unwired; no Iggy.
- G6 M4 platform/CI (CR-35..40): NOT MET — CI fully red; suites uncompiled; no wasm/mobile builds.
- G7 delivered by the agent team: PARTIAL — team `parking-lot-core` installed for six harnesses + Zed (source-verified only); no role has been invoked yet.

## Risks and concerns for analyze/plan

1. **M0 is larger than the plan states.** Beyond CR-15/16/17, nothing in M1 can be gated while all-targets, CI and the MSRV lock are broken. A build-baseline change should precede M1: compile benches and examples, commit `Cargo.lock`, pull the pgmq drop/pin forward from CR-30, and do a fmt pass. It is platform-engineer and tech-lead work, and it blocks every implementer's alias loop.
2. **History purge is operator-gated and disruptive.** It force-pushes `main` of a public repo and invalidates all open worktrees; do it before worktrees are created.
3. **Single crate vs workspace split** decides ownership boundaries and rebuild scope for all four implementers; the ADR must land before M1 fan-out, or the split later forces every branch to rebase across file moves.
4. **Spec-of-record gap:** with `openspec/specs/` empty, reviewer verdicts have no frozen acceptance text. The product-manager's OpenSpec changes are on the critical path, not paperwork.
5. **Multi-agent cost:** research puts multi-agent runs at ~15× chat tokens; M0 and CR-18/19 are sequential and should run with one or two roles, not the full team.

## Adopted-outcome contract inputs

| Input | Value |
|---|---|
| Adoption source | Explicit operator instructions on 2026-10-22: "that is the plan for what we do next" (docs/update-plan-10-22-2026.md, merged in #4); "create a suitable agent team to do the work specified … and the new plan"; "do #2 and #3, then start the phase"; "run /kbd-assess". |
| User operation (one useful shipped operation) | A service or Tauri app submits weighted AI tasks (e.g. GPU-VRAM units) to a pool; the pool never runs more than `max_units`, never silently loses an accepted task across panic, shutdown or restart (Postgres or embedded store), and the caller retrieves the result later from the mailbox even after disconnecting. |
| Observed baseline | See IMPLEMENTATION STATUS. UNKNOWN: real throughput/latency (benchmarks do not compile); behaviour on PG 18; whether WASM/mobile targets compile at all; actual coverage including the uncompiled suites. |
| Desired observable result | Plan §11 acceptance rows: loom/shuttle model proves Σ active cost ≤ max_units and no lost wakeup; kill -9 mid-task redelivers after lease expiry; consume-once mailbox; Iggy subscriber receives `mailbox.ready`; CI green on PG 17 and 18. |
| Supported environments | macOS aarch64 + Linux x86_64 (tests); Postgres 17 and 18; Tauri desktop (embedded store); wasm32-unknown-unknown and iOS/Android (check-only; WASM kept or dropped by CR-38 decision). |
| Required safety/security checks | Leaked `.codex` credentials revoked (operator) and purged; `cargo package --list` allowlist check; gitleaks/trufflehog in CI; `cargo deny` advisories clean; no `unsafe`; no secrets in package. |
| Exclusions / deferred | Multi-node scheduler coordination (plan, explicitly out of scope). CR-33 stream replay is stretch. |
| Risk acceptance | UNKNOWN — operator has not stated whether the public-API breaks in M1/M2 (sync→async queue trait, Result executors) may ship as 0.3.0, nor whether CI may stay red until M0 completes. Needed before plan freeze. |
| Decision owner | Operator (Travis James); merges are operator-only. |

ASSESSMENT COMPLETE
