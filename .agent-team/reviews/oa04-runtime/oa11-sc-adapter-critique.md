# OA11 SC adapter: independent artifact critique

Scope: R2 review of the unapplied `oa11-sc-litmus-adapter-proposal.{md,json,patch}` after the observed admission-model failure at the completed implementation boundary. Current waypoint is `phase-0-4-operational-assurance`, Execute, `oa-18-release-boundary/2`. This is an artifact review of that repair, not another general source review or an execution receipt.

## Exact artifact judged

The freshly read proposal patch adds this helper:

```diff
+#[cfg(not(parking_lot_negative_control = "oa-11-ledger-order"))]
+fn model_admission_sc_boundary() {
+    loom::sync::atomic::fence(Ordering::SeqCst);
+}
```

Its two call-site hunks are:

```diff
                 ledger.accept();
+                #[cfg(not(parking_lot_negative_control = "oa-11-ledger-order"))]
+                model_admission_sc_boundary();
                 if shutdown.load(FLAG_LOAD_ORDER) {
```

```diff
                 shutdown.store(true, FLAG_STORE_ORDER);
+                #[cfg(not(parking_lot_negative_control = "oa-11-ledger-order"))]
+                model_admission_sc_boundary();
                 ledger.in_flight()
```

The helper, both calls and documentation are the entire proposed delta. The original admission assertion, its diagnostic, delayed accepted-work cleanup, and the complete publication fixture remain unchanged.

## Disposition

**ACCEPT this exact patch for implementation and subsequent boundary verification as a narrowly justified model abstraction. No source correction to the proposed patch is required by this review. This is not OA11 PASS, a release certificate, or a waiver of the failed direct model.** The adapter can contribute the ledger admission portion of the adopted OA11 bounded-model evidence without changing the contract or production behavior. OA11 still requires the remaining publication/kind-release evidence and its specified controls. The full boundary remains unverified here.

The acceptance depends on the source mapping below, not merely on a future green Loom run. A fence can trivially suppress a real weak-order defect if that mapping is false. For this frozen source, it is true.

## Independent source and tool checks

Read in full: both functions in `crates/parking-lot-runtime-tokio/src/ledger/loom_ledger.rs`; `Ledger` and its relevant `accept`, `settle`, `in_flight`, `wait_until`, `finish`, `report` paths in `src/ledger.rs`; production `ResourcePool::submit`, `shutdown`, and `shutdown_with` in `src/resource_pool.rs`; OA11 in `adopted-contract.json` and the OpenSpec design. The runtime manifest inherits the workspace's `edition = "2024"`, MSRV `1.95`. No dead-code claim is made, so a dead-symbol usage search is inapplicable.

Production mapping is explicit: `submit` lines 568–569 increments the ledger before its SC flag load; `shutdown` line 521 performs the SC flag store; `shutdown_with` lines 638–645 calls that method before the wait predicate's ledger observation. `Ledger::accept` uses `ADMISSION_ORDER`, and `in_flight` uses `OBSERVE_ORDER`; both select SC outside the named control. The model calls these actual methods. Its flag is a scalar stand-in, not the actual ResourcePool object. Rejection settles the task; accepted work cannot settle until both observations have been joined in this projection. This qualification is essential.

Pinned Loom 0.7.2 README lines 68–79 explicitly distinguishes unsupported SC accesses from supported SC fences and also discloses incomplete memory-model exploration. Independently read `src/rt/synchronize.rs` (`Synchronize`), `src/rt/thread.rs` (`seq_cst`, `seq_cst_fence`), and `src/rt/atomic.rs` fence dispatch, RMW/store/load coherence and store selection. `seq_cst()` is empty; `seq_cst_fence()` joins the active and global causality vectors. The cross-object SC order required by this oracle is absent from direct accesses. This justifies treating the observed failure as unsupported-model evidence rather than, by itself, evidence of a production defect. I did not consult or rely on the proposal's upstream issue discussion.

Fresh SHA256 checks matched the proposal: production ledger `9c3a4a9b7ea82e3ee28fb7153c3be360a0a089f9cfe028fe669b5860dd7ec89e`; current model `6236cf4e778bc0a446cadba139f9cdbe3ad93ce88ec1373e54bc542b336530ca`; proposed patch `99bd0e0c69ec7e871f8aaaaa2597a3e813ead7875a3ca6dfff94c117a1340df2`. The three stated discovery/run log hashes also matched.

## Restricted proof and distinguishing observations

Let A be the acceptance RMW, B the submit flag load, C the shutdown flag store, and D the shutdown counter load. For the normal source's all-SC four-event projection, A precedes B and C precedes D in the SC total order. If accepted is true, B read initial false, hence B precedes C. No rejection decrement occurs on that branch. Accepted work remains counted, so D reading initial zero would put D before A. The required cycle A < B < C < D < A is impossible. Neither a cross-object synchronizes-with edge nor join-based repair is needed for this argument.

For the proposed Loom adapter, put Fs after A/before B and Fd after C/before D. If Fs is first, its causality contains A and the later Fd transfers that causality to the D observer; Loom's write/read coherence excludes initial zero while accepted work remains counted. If Fd is first, it transfers C before B and excludes initial false. Thus the adapter excludes the same forbidden pair. These are model causality edges; they must not be described as production synchronizes-with edges.

Exclusion alone would be insufficient. In this isolated model the complete `(accepted, seen)` observation space is boolean × `{0,1}`: one increment, at most its rejection decrement, no other work, no wraparound. The three legal pairs all remain attainable with the adapter and with SC source events:

| Observation | Adapter event witness (rejection decrement follows B where needed) |
| --- | --- |
| `(true, 1)` | A, Fs, B, C, Fd, D |
| `(false, 0)` | C, Fd, D, A, Fs, B |
| `(false, 1)` | A, Fs, C, Fd, D, B |

These witnesses require at most two worker-to-worker switches, within bound 3. The fourth pair `(true,0)` is excluded by both arguments. This establishes equality of this tiny projected outcome set, not equality of event traces, arbitrary C11 programs, or the complete runtime. The adapters' additional synchronization cannot justify any other shared or Relaxed data. In particular, no adapter reaches the publication companion.

## Negative control and evidence obligations

The control weakens actual ledger A/D order constants and modeled C/B orders and omits both representation fences. Omitting the adapter in the mutant is necessary: leaving it present would preserve the very SC constraint that the mutant is supposed to remove. The normal/control pair is therefore discriminating at the represented ordering constraint, subject to observing the required runs. Removing model fences alone is not evidence of a production failure; nor does the future positive run independently establish that production accesses remain SC.

The uncomfortable limitation: if production order constants were later weakened while these unconditional normal-model fences stayed, the normal fixture could still pass. Each frozen candidate must therefore retain an explicit static check of the four actual orderings and their source-to-model mapping. This review verifies that premise only for the hashes above. Do not market the model as an automatic detector for every production ordering regression. The adopted contract's phrase “named happens-before edge” must not be used to claim a cross-object production HB edge here; the removed source premise is its SC total-order constraint, represented by model fence causality.

Required next evidence, already required by the proposal: freeze the applied source/candidate and mapping; preserve the old direct-model FAIL; compile/discover/run both exact ledger fixture identities in the normal and isolated control builds with the adopted bound; observe both normal fixtures passing; observe the control failing specifically at the unchanged both-missed admission assertion while publication passes. Missing discovery, compilation errors, timeouts and unrelated panics are invalid-control results, never successful negatives. Preserve exact argv, exits, versions, source digests and raw logs. The accepted abstraction and its limits must accompany the resulting evidence; no contract amendment or weakened oracle is authorized.

Observed baseline logs state exactly `2 tests, 0 benchmarks`; run output reports publication `ok`, admission `FAILED`, `1 passed; 1 failed`, and the original diagnostic `OA-11 ledger admission and shutdown both missed accepted work`. Stderr reports `error: test failed`. No adapted execution has been observed by this reviewer.

## Verification and isolation limits

Used source/document/log reads and SHA256 checks only. No Cargo/check/clippy/fmt/test, builds, analyzers, network writes, source/index/HEAD/KBD changes, or child agents ran. The standard executable review gates are deliberately unperformed in this artifact-only assignment and remain the boundary owner's responsibility; no green CI, resolved-merge, compile, lint, runtime or model-pass claim is made. The requested `memory-model` skill was unavailable at its listed canonical path; review used the loaded verification protocol/Rust review guidance and installed primary Loom sources without inventing missing instructions.

Review was fresh-context native isolation in the same model family. It is independent of generation history but is not verified cross-model or cross-harness certification. Only `.agent-team/reviews/oa04-runtime/oa11-sc-adapter-critique.md` was written. No unsolicited source changes or guards were added.
