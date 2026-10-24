# Observed integration repair 1 — narrow source verification

Scope: the listed dirty repair paths against `729cc42549defb5fcf69b432485224cb89da74bc`, their immediate callers/selectors and new source-scope proof controls. This is targeted verification within correction cycle 1, not a second general R2 review. No fresh unrelated findings were sought. The initial two R2 findings are outside this batch and remain resolved by the earlier confirmation.

Observed context: retained first-attempt stderr reports the conformance Clippy diagnostics, `build_pools` E0277, ledger-model E0639/E0004 and bindgen-deny's unexpected `--locked` argument. These logs establish the repair triggers, not successful execution of the repaired bytes.

## Source observations

- **Core documentation and expiry removal: acceptable targeted repairs.** The first paragraphs now end before a blank doc line in `conformance/mailbox.rs:173–176`, `:339–343` and `conformance/task_queue.rs:43–46`; substantive address/status/deadline requirements remain. `dispatch/parked.rs:332–337` still snapshots exactly the keys whose tickets satisfy `is_expired(now_ms)`. The repaired removal is:

  ```rust
  let mut removed = Vec::with_capacity(expired.len());
  for key in expired {
      if let Some(item) = self.remove(&key) {
          removed.push(item);
      }
  }
  Ok(removed)
  ```

  (`parked.rs:338–344`). Reading the complete PriorityQueue implementation confirms that the same `remove` helper updates the cost index. Ordering, absent-key handling and ownership of returned items match the replaced filter/map collection; the immutable entries borrow ends before removals.

- **Facade constructor bound: acceptable propagation.** `src/builders/pool_builder.rs:10` imports the existing `TaskQueue` reexport, and `:55` adds exactly `Q: TaskQueue<P>`. The complete `build_pools` function still calls `ResourcePool::<P, T, Q, M, E, S>::new` at `:82`; that constructor's existing where-clause requires `Q: TaskQueue<P>` in `resource_pool.rs:302`. Configuration validation, factories and iteration are unchanged. The bound expresses a requirement already imposed by the called constructor.

- **Postgres rustdoc link: acceptable syntax repair.** `crates/parking-lot-pg/src/mailbox/mod.rs:137` now contains `/// [\`SchedulerError::Internal\`] on invalid or`; the redundant explicit destination is removed. The migration forwarding method and runtime behavior are unchanged.

- **Ledger defining-crate model route: coherent source and selector repair.** `ledger.rs:295–296` declares:

  ```rust
  #[cfg(all(test, parking_lot_loom))]
  mod loom_ledger;
  ```

  The new `src/ledger/loom_ledger.rs:32` uses `super::{Ledger, Tally}` instead of compiling ledger.rs as an external integration-crate copy. The actual defining crate's `SinkOutcome` and `ShutdownReport` now retain their internal construction/matching rules; no wildcard or parallel DTO was introduced. Both model bodies retain their ordering/control constants and assertions, including `!accepted || seen > 0` (`:71–74`) and `ledger.report(Tally::default()).completed == 1` (`:100–105`). The old integration target is deleted. CI now sets `model_args=(--lib ledger::loom_ledger::)` (`ci.yml:270–271`) for build/list/run while retaining exact two-name discovery and separate outcome classification. OA18 run/discovery argv uses the same library filter (`operational-assurance.json:911–912`, `:935–936`). OA12 names the underscore library binary and complete qualified oracle/companion names (`negative-controls.json:1302–1312`); its existing `native` adapter selects `--lib` for `package.replace("-", "_")`, checks actual discovery identities and then runs exact binary/test filters. The runtime manifest's cfg-gated Loom dependency and workspace edition 2024 remain unchanged.

- **cargo-deny argv: consistent with the observed CLI repair and incumbent CI scope.** Root OA18 argv is now `cargo deny --workspace --all-features --locked check` (`operational-assurance.json:280–285`), matching the root CI workspace/all-feature/locked arguments (`ci.yml:783–786`). Bindgen argv places `--manifest-path`, `--locked` and `--config` before `check` (`operational-assurance.json:2864–2871`), removing the observed passing of `--locked` to the check subcommand. No policy file or exclusion changed. Actual installed-CLI acceptance remains for the parent to run.

- **Candidate-tree scan integration/proof controls: no integration issue found.** The OA18 secrets-tree row now invokes the fixture with the frozen `{candidate}` and `{workspace}` and retains OA05/profile/timeout identity (`operational-assurance.json:312–332`). The fixture requires the actual candidate's nonempty successful source scan before its controls (`candidate-tree-fixtures.py:70–75`). Its added source-scope oracles distinguish ignored generated artifacts from the same tracked canary, including an export-ignore path (`:114–129`), and request the original candidate again after later HEAD/worktree changes (`:132–134`). Success is returned only after those controls (`:136–141`). This checks the intended frozen-source scope without a scanner exemption. Python scanner-helper internals/security are assigned to the separate reviewer; this report does not replace that review.

## Findings and limits

No actionable issue found in this repair batch. Only the owned report file was written. No production source, KBD state, index or HEAD was edited. No Cargo, tests, builds, analyzers, scanner invocation, network operation or subagent dispatch ran. Source/manifest inspection cannot establish compiler acceptance, actual model discovery, normal/control outcomes, scan results or a passing integration packet. The parent owns final source-hash refresh, candidate preparation/freeze and serialized real verification; prior candidate receipts do not verify these dirty bytes.

## Review Summary

| Severity | Count | Status |
|----------|-------|--------|
| CRITICAL | 0 | pass |
| HIGH | 0 | pass |
| MEDIUM | 0 | pass |
| LOW | 0 | pass |

Verdict: APPROVE the inspected targeted source repairs for the parent's next verification boundary. No runtime or release-certification PASS is asserted.
