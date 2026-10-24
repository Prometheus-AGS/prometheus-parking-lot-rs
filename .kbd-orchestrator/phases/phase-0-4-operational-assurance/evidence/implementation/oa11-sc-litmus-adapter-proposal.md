# OA11 direct-SC diagnosis and exact litmus-adapter proposal

PROPOSAL ONLY; not applied. Neither synced Rust file has changed. No Cargo, tests, experimental model, compiler, formatter, analyzer, scan or review ran here. The parent requested independent R2 review before any implementation. No central/shared-map/runtime/KBD mutation was performed.

## Observed evidence, preserved as failure

The parent's actual completed-candidate run in /tmp/oa04-boundary-2c18c9a/baseline/loom_ledger.stdout.txt executed two tests: settlement publication passed, admission/shutdown failed at its existing both-missed-work assertion, with exit101 in stderr. The discovery log lists exactly those two qualified fixtures. Keep this direct-model failure and its raw logs; this proposal is neither an observed success nor permission to relabel that run PASS.

Exact central sources were copied first: ledger.rs SHA9c3a4a9b7ea82e3ee28fb7153c3be360a0a089f9cfe028fe669b5860dd7ec89e and src/ledger/loom_ledger.rs SHA6236cf4e778bc0a446cadba139f9cdbe3ad93ce88ec1373e54bc542b336530ca. All four normal modeled accesses select SeqCst, so the problem is not an accidentally weakened cfg in normal source. The model flag intentionally mirrors ResourcePool's operation order, rather than running the full pool.

Pinned primary sources:

- /Users/gqadonis/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/loom-0.7.2/README.md:68-74 explicitly documents unsupported SeqCst accesses, acquire/release treatment, possible false alarms, and supported SeqCst fences.
- src/rt/synchronize.rs:24-53 implements a SeqCst load as acquire plus threads.seq_cst(), and a SeqCst store as release plus threads.seq_cst().
- src/rt/thread.rs:336-348 leaves seq_cst() empty and implements seq_cst_fence() by joining active/global causality. src/rt/atomic.rs:171-209 routes SeqCst fence through acquire/release fence handling and seq_cst_fence().
- [Upstream issue180](https://github.com/tokio-rs/loom/issues/180), read directly, records the same two-thread store/load false alarm. Its contributor comment dated August22,2021 explains using supported SC fences between access pairs for RAW consistency. That discussion supports this narrow model representation, not a general equivalence claim for arbitrary programs.

These sources explain the missing semantic premise of the normal oracle. This observed model failure alone does not establish a production scheduling or admission defect.

## Actual production order and four-event argument

ResourcePool::submit calls ledger.accept() before loading inner.shutdown SeqCst. Ledger::accept performs in_flight.fetch_add(1,SeqCst). ResourcePool::shutdown stores shutdown=true SeqCst; shutdown_with invokes it before Ledger::wait_until checks in_flight via its SeqCst load. Normal source has no fences at these boundaries.

Restrict the argument to the exact modeled single submission/single shutdown observation, initially counter0/flagfalse. Define A=accept RMW, B=submit flag load, C=shutdown flag store, D=shutdown counter load. Program order requires A<B and C<D in the real SC total order. The assertion matters only if B reads false (accepted=true). Then no rejection decrement occurs; cleanup is delayed until both observers join. With no intervening writes, B reading initial false requires B<C; D reading initial zero requires D<A. Thus both missed observations would require A<B<C<D<A, impossible in a total order. If the submission instead observes shutdown, it settles and returns accepted=false, so the assertion is already satisfied even when the counter returns to zero. Joining does not retroactively repair historical B or D snapshots.

## Proposed source delta (model file only)

The accompanying exact patch adds a named model_admission_sc_boundary helper that calls loom::sync::atomic::fence(SeqCst). It is compiled only when the oa-11-ledger-order control is absent. The first model calls it once immediately after actual ledger.accept() and before its flag load, and once immediately after the flag store and before actual ledger.in_flight(). Both helper calls, the helper definition and their extra synchronization are absent under the ordering control. Actual Ledger methods, their instrumentation and order constants remain unchanged. No production fence or lock, guard, trait, dependency, cfg ID, public interface or release ordering changes.

The original assertion, cleanup, exact two fixture identities and named diagnostic remain unchanged. The settlement publication companion is byte-for-byte unchanged and receives no helper calls or new synchronization. The control still weakens the actual ledger acceptance/observation constants and modeled flag operations; it also removes the model SC representation, so it continues to represent the intended weaker two-pair ordering rather than a fenced version of that mutant.

## Why this represents this observation, and what differs

Let Fsubmit be the added model fence after A and Fshutdown the fence after C. Loom implements a global order of these supported fences. If Fsubmit precedes Fshutdown, the first fence publishes A through the second fence before D. Per-location coherence then prevents D observing an older initial counter value when accepted=true and no decrement has occurred. In the opposite fence order, C is published before B, so B cannot read the older false flag. Therefore this adapter excludes the same both-missed observation as the four-event SC cycle argument.

The adaptation changes the model's synchronization, explicitly: real SC operations impose a global order without necessarily establishing cross-object synchronizes-with; the model fences introduce supported fence/causality edges. For this isolated predicate, all pre-assertion observable state is the two scalar snapshots and the accepted flag; accepted work is retained until both are recorded. We rely only on the exclusion of that one impossible snapshot pair, not on the extra edges to justify unrelated relaxed data or callbacks. Legal distinguishing observation cases remain representable: A,Fsubmit,B,C,Fshutdown,D yields accepted=true/seen1; C,Fshutdown,D,A,Fsubmit,B yields rejected/seen0; A,Fsubmit,C,Fshutdown,D,B yields rejected/seen1 before its rejection settlement. No claim is made that every underlying event trace or every C11 program is equivalent after this replacement.

This is a justified litmus abstraction proposal, not direct execution of the unmodified production synchronization semantics in Loom. Independent R2 must assess that restricted mapping before implementation. If it is not accepted, retain the honest unsupported-SC coverage gap rather than adding production ordering or weakening the oracle. The raw direct-model normal run remains FAIL regardless of that decision.

## Execution and evidence after an accepted proposal

Keep the existing normal/control commands, isolated target directories and LOOM_MAX_PREEMPTIONS=3. Both still use --lib ledger::loom_ledger:: with the two exact qualified fixture names. A newly frozen candidate and source-map digest must bind any accepted patch. Preserve this proposal, review disposition, pinned-tool evidence, old unsupported direct-run logs, compile/discovery/run stdout+stderr and exact exits. Normal must run/pass both fixtures; the separate ordering control must fail only the original admission assertion with its original diagnostic while publication passes. Compiler failure, timeout, no discovery or arbitrary panic cannot classify as success. None of these proposed outcomes has been observed.

Even an accepted and executed adapter supplies bounded evidence for the exact admission observation only. It does not prove the complete shutdown/Notify protocol, ResourcePool execution, callbacks, cancellation, relaxed-memory correctness or general SeqCst implementation. Pinned Loom's unsupported access semantics and incomplete relaxed exploration must remain explicit in current model documentation/evidence. No API or performance claim changes.
