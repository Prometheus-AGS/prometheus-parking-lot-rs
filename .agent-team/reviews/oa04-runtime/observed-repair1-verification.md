Scope: observed Rust integration repair 1 in the dirty central tree based on `729cc42549defb5fcf69b432485224cb89da74bc`, within existing review cycle 1 only.

Verdict: **No actionable issue found in the specified repairs by static inspection.** The reported source causes are addressed. Compilation, Clippy acceptance, discovery and model/runtime outcomes remain NOT_EXERCISED by this reviewer.

Fresh source/caller proof:

1. Ledger defining-crate repair:
   - `crates/parking-lot-runtime-tokio/src/ledger.rs:295–296` declares `#[cfg(all(test, parking_lot_loom))] mod loom_ledger;`.
   - `src/ledger/loom_ledger.rs:32` imports `use super::{Ledger, Tally};`. Its complete models retain the original admission/shutdown and finish-before-settle operations and assertions at lines 44–113. The former integration source is deleted; the models no longer path-include ledger.rs into an external test crate.
   - `ledger.rs:51–52` still imports `crate::cancellation::SinkOutcome` and `crate::shutdown::ShutdownReport`; the explicit status match at lines 131–144 and report construction at 211–221 are unchanged. Both types remain defined and non-exhaustive inside this actual runtime library (`cancellation.rs:163–164`, `shutdown.rs:112–113`), so the repaired model route preserves their correct defining-crate construction/exhaustivity rules. No wildcard arm, DTO substitute or public visibility change was introduced.

2. Model selectors:
   - `.github/operational-assurance.json:905–938` selects `cargo test -p prometheus-parking-lot-runtime-tokio --locked --lib ledger::loom_ledger::` for both execution and discovery, with `--cfg parking_lot_loom` and preemption bound 3.
   - `.github/negative-controls.json:1296–1313` identifies library binary `prometheus_parking_lot_runtime_tokio` and both exact names: `ledger::loom_ledger::loom_ledger_submit_shutdown_cannot_both_miss` and `ledger::loom_ledger::loom_ledger_settlement_publishes_finished_before_zero`. Its control source paths include the moved module at lines 32–34.
   - The existing `.github/scripts/tests/negative-controls.py:189–225` routes adapter=loom through native handling, chooses `--lib` when target equals `package.replace("-", "_")`, and requires the exact binary/test identities in discovery before execution.
   - `.github/workflows/ci.yml:221–240,270–283` uses the qualified names and switches the ledger row to `model_args=(--lib ledger::loom_ledger::)`; both identities and exactly two discovered models are required. No stale integration-target selector remains in these inspected routes.

3. Queue Clippy repair:
   - `crates/parking-lot-core/src/dispatch/parked.rs:331–344` retains the expired-key snapshot and then uses `Vec::with_capacity(expired.len())`, `for key in expired`, `self.remove(&key)` and `removed.push(item)`.
   - The immutable entries borrow ends before mutation; traversal order, expiry predicate and absent-key behavior match the former filter_map path. The complete `remove` helper at lines 248–251 still updates the cost index, so removal accounting is preserved.

4. Builder bound and mechanical documentation:
   - `src/builders/pool_builder.rs:55` adds `Q: TaskQueue<P>`, satisfying the unchanged constructor call at lines 81–88 and `ResourcePool::new`'s existing bound in `resource_pool.rs:300–302`.
   - All four direct build_pools callers found at `tests/unit/builders_test.rs:155,179,314,336` provide `InMemoryQueue<String>`. The complete builder function preserves validation-before-factories and durable-backend rejection.
   - The conformance changes in mailbox.rs/task_queue.rs only split documentation paragraphs. The PG mailbox change only simplifies the existing `SchedulerError::Internal` intra-doc link. Their inspected diffs change no executable statements.

Read the relevant implementation handoffs, source/shared patches and receipt manifests. The source review establishes the repair mechanism; it does not establish that the formerly failing gate now passes. Parent must observe successful build/discovery, exactly two ledger-model identities, normal model success, the designated broken-order assertion failure with its positive companion, and the applicable Clippy/native gates.

No Cargo, tests, builds, analyzers, network actions, production/source/index/HEAD/KBD changes or subagents were used. Only this owned review record was created:
`.agent-team/reviews/oa04-runtime/observed-repair1-verification.md`.

