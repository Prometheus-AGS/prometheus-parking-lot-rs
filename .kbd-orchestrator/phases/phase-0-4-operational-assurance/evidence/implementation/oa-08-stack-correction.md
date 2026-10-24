# OA-08 synchronous-discard stack correction

Status: NOT_EXERCISED. OA18 task 2 cumulative correction cycle 1 only. Source authoring and byte hashes were performed; no tests, compilation, Cargo, formatting, syntax validation, analyzer, scan, review, runtime/KBD mutation or external action was performed. This is an implementation handoff, not a passing integration claim.

## Confirmed failure and owned change

A legal Spawn implementation drops each supplied future inline. With one held run and a large parked backlog, dropping that first run entered UnpolledRun::drop -> discard closure -> finish -> Dispatcher::complete -> launch -> Spawn::spawn -> UnpolledRun::drop, one native stack frame chain per successor. Sufficient accepted work aborts the process on stack overflow before its mailbox outcomes and ledger can settle.

The two owned source files were copied from current formatted central before editing. No other central source was copied or edited. The attached exact patch changes only:

- crates/parking-lot-runtime-tokio/src/resource_pool.rs: an Inner-owned mutex contains pending (ScheduledTask, Permit) pairs and sole-drainer ownership. UnpolledRun transfers to discard before releasing its captured Arc. One synchronous drainer pops and settles tasks iteratively; recursive/concurrent drops enqueue and return. Every per-task action retains SettleOnDrop and catch_unwind containment. Module comments describe this scheduling relation.
- crates/parking-lot-runtime-tokio/tests/discarded_spawn_test.rs: legal hold-first/inline-successor spawner; child-process backlog regression and env-gated child helper; existing mailbox-panic fixture now covers three chained discards while retaining its identity and hook/mailbox assertions.

No new dependency, cfg, public interface or Cargo target is needed. Existing OA08 ownership-loss cfg, OA09 admission and OA10 expiry paths remain unchanged.

## Ownership, ordering and bounded stack

1. Construction of UnpolledRun still precedes handing its future to Spawn. First poll takes its task/permit exactly once; an unpolled drop transfers that pair into the worklist exactly once.
2. Enqueue, drainer election, pop and the empty-to-idle transition use the same mutex. Unlock/next lock is the publication edge. A competing enqueue either observes the active drainer, which subsequently sees its item, or sees idle and becomes the drainer. No new atomic ordering or wakeup protocol is introduced.
3. No worklist lock is held across dispatcher operations, Spawn, lifecycle emission, mailbox/audit callbacks or payload destruction. The active caller retains an Arc for the whole drain; queued pairs need no spawner/runtime to make progress.
4. Each item emits its Failed event before returning capacity through dispatcher.complete. complete releases the permit and adopts every newly admitted task through launch before current-task mailbox/audit callbacks. An inline successor drop only enqueues, then returns; it cannot start another drain stack. Cross-task terminal ordering changes from nested unwinding to iterative FIFO settlement, with no cross-task terminal ordering contract added.
5. finish still increments failed and ledger.finish before mailbox delivery; its SettleOnDrop settles after that item's caught action, before the next loop iteration. Existing ledger publication and shutdown notification edges remain unchanged. Every pending item still owns its permit and accepted ledger obligation until processed. Callback panic cannot abandon successors; panic payload forgetting retains the existing protection against a second unwind from its destructor.
6. Pool-induced discard-chain stack depth is independent of backlog length. Worklist storage grows with pending transferred ownership, and no payload is cloned or reused.

## Source-derived fixtures and future gate commands

Target: prometheus-parking-lot-runtime-tokio integration target discarded_spawn_test. Its source now declares seven tests (five incumbent identities plus parent and helper). No command below was run.

New parent fixture: discarded_spawn_large_backlog_is_stack_bounded.
New helper fixture: discarded_spawn_large_backlog_child. It is deliberately a no-op unless the parent sets PARKING_LOT_OA08_BACKLOG_CHILD=1; listing/running that helper alone without the environment is not regression evidence.

The parent launches the same test executable with --exact discarded_spawn_large_backlog_child --nocapture --test-threads=1. The child executes the real public ResourcePool submit/shutdown path on an explicit 2 MiB thread stack, accepts 16,384 mailbox-addressed tasks, holds the first run unpolled, verifies 16,383 are parked, starts shutdown waiting, then drops the first future and every successor inline. It requires zero executor calls, failed_tasks=16,384, completed_tasks=0, running/queued/active_units=0, shutdown (completed,cancelled,unsettled)=(16,384,0,0), exactly one delivery per key, original Failed(Cancelled) reason with no payload, each result consumable once only, and OA-08:discarded-backlog-settled printed after all oracles. This stress leg has no hooks because their bounded channel can legally drop burst events; incumbent hook assertions remain in the smaller fixtures.

The existing discarded_spawn_contains_mailbox_callback_panic_and_settles_once now stores then panics on all three chained tasks. It requires one result/event per task, failed_tasks=3 and no active/queued work, establishing continued drain after each caught callback panic.

At the authorized final gate, with an actual isolated target root and frozen candidate identity, run compile/discover/full normal execution serially:

```sh
CARGO_TARGET_DIR="$OA18_TARGET_ROOT/oa08-stack-normal" cargo test -p prometheus-parking-lot-runtime-tokio --locked --test discarded_spawn_test --no-run
CARGO_TARGET_DIR="$OA18_TARGET_ROOT/oa08-stack-normal" cargo test -p prometheus-parking-lot-runtime-tokio --locked --test discarded_spawn_test -- --list
CARGO_TARGET_DIR="$OA18_TARGET_ROOT/oa08-stack-normal" cargo test -p prometheus-parking-lot-runtime-tokio --locked --test discarded_spawn_test -- --nocapture --test-threads=1
```

The mandatory real regression identity can be selected explicitly without treating it as whole-phase completion evidence:

```sh
CARGO_TARGET_DIR="$OA18_TARGET_ROOT/oa08-stack-normal" cargo test -p prometheus-parking-lot-runtime-tokio --locked --test discarded_spawn_test discarded_spawn_large_backlog_is_stack_bounded -- --exact --nocapture --test-threads=1
```

Source-derived discriminating control: oa-08-stack-regression-control.patch restores only resource_pool.rs to the recorded pre-correction central bytes in a disposable copy of the final candidate, retaining the new tests and all other collaborators. It introduces no new cfg and must never be applied to the normal candidate. Record the control patch digest, complete candidate/control source identities, command/exit and raw stdout/stderr. Compile/discover with the analogous separate target directory before:

```sh
CARGO_TARGET_DIR="$OA18_TARGET_ROOT/oa08-stack-old-recursion" cargo test -p prometheus-parking-lot-runtime-tokio --locked --test discarded_spawn_test discarded_spawn_large_backlog_is_stack_bounded -- --exact --nocapture --test-threads=1
CARGO_TARGET_DIR="$OA18_TARGET_ROOT/oa08-stack-old-recursion" cargo test -p prometheus-parking-lot-runtime-tokio --locked --test discarded_spawn_test discarded_spawn_ordinary_spawner_preserves_success_panic_and_timeout_once -- --exact --nocapture --test-threads=1
```

Expected classifier (unobserved): parent test exits normally through libtest failure (101), with designated OA-08:discarded-backlog: child did not settle diagnostic and child stack-overflow/abnormal-exit evidence. The ordinary TokioSpawner companion remains a required success. The separate OA-08:discarded-backlog-watchdog diagnostic is an infrastructure/liveness failure and does not establish the stack-overflow control. Compile failure, test absence, arbitrary child failure or timeout cannot classify this control as detected. The incumbent oa-08 ownership-loss control remains a separate leg with its existing discarded-terminal assertion; it is not the old-recursion control.

Shared control-manifest owner handoff: no new cfg or Cargo registration; account for the two additional discovery identities in this target if whole-target counts are frozen. The child helper is not an independently sufficient positive. Any new old-recursion leg requires the exact production-only reverse patch and dedicated target directory above, not enabling oa-08 (which changes ownership and would confound this oracle). Parent/platform own shared runner/manifest edits.

## Limits and remaining evidence

No performance claim or model proof is added. This fixes the named pool-induced synchronous discard recursion; arbitrary collaborator recursion/blocking, panicking clocks/queue backends, allocation failure, aborting destructors and process abort remain outside existing containment guarantees. A collaborator retaining an unpolled future may still retain its work indefinitely. Hook delivery remains bounded and lossy under backpressure. Mailbox callback containment does not promise storage before a callback actually stores. The new mutex adds one enqueue/pop critical section to unpolled discards; normal polled execution does not take it. All compiled behavior, actual child outcomes, classifiers, counter/ledger assertions, callback continuation and normal companions remain unverified until the parent's final cumulative gate. No unsolicited production changes were added.
