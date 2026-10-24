# OA-11 ledger defining-crate repair

Status: NOT_EXERCISED. Deterministic integration repair 1 under OA18 task 2, not a new review cycle. The parent observed discovery compile exit101 in /tmp/oa04-boundary-729cc42/baseline/loom_ledger-discovery.stderr.txt: E0639 constructing the external non-exhaustive ShutdownReport and E0004 matching the external non-exhaustive SinkOutcome. This worker read that actual stderr; it contains no successful ledger-model execution or ordering evidence. Parent-reported passing core Loom rows do not substitute for ledger coverage.

The owned ledger.rs and integration model were copied from current central before editing. Move tests/loom_ledger.rs to src/ledger/loom_ledger.rs and declare it from actual ledger.rs under cfg(all(test, parking_lot_loom)). The models import super::{Ledger,Tally}. This exercises the one actual private ledger implementation with the actual cancellation::SinkOutcome and shutdown::ShutdownReport defined in its own library crate. No copied/parallel DTOs, wildcard arms, visibility widening, production matching changes, new dependencies or Cargo/lib.rs edits are needed. Normal production cfg excludes the model module. Existing model operations, orderings, preemption bound and assertions are unchanged.

Delete the former integration test target source; do not retain a zero-test target as apparent model evidence. The library filter ledger::loom_ledger:: must discover exactly these two complete identities:

- ledger::loom_ledger::loom_ledger_submit_shutdown_cannot_both_miss
- ledger::loom_ledger::loom_ledger_settlement_publishes_finished_before_zero

Future normal compile/discover/run (one Cargo writer, frozen candidate, isolated target):

```sh
export LOOM_MAX_PREEMPTIONS=3
export CARGO_TARGET_DIR="$OA18_TARGET_ROOT/oa11-ledger-library-normal"
export RUSTFLAGS='--cfg parking_lot_loom'
cargo test -p prometheus-parking-lot-runtime-tokio --locked --lib ledger::loom_ledger:: --no-run
cargo test -p prometheus-parking-lot-runtime-tokio --locked --lib ledger::loom_ledger:: -- --list
cargo test -p prometheus-parking-lot-runtime-tokio --locked --lib ledger::loom_ledger:: -- --nocapture --test-threads=1
```

Future corresponding control, retaining the same real library route:

```sh
export LOOM_MAX_PREEMPTIONS=3
export CARGO_TARGET_DIR="$OA18_TARGET_ROOT/oa11-ledger-library-order-control"
export RUSTFLAGS='--cfg parking_lot_loom --cfg parking_lot_negative_control="oa-11-ledger-order"'
cargo test -p prometheus-parking-lot-runtime-tokio --locked --lib ledger::loom_ledger:: --no-run
cargo test -p prometheus-parking-lot-runtime-tokio --locked --lib ledger::loom_ledger:: -- --list
cargo test -p prometheus-parking-lot-runtime-tokio --locked --lib ledger::loom_ledger:: -- --nocapture --test-threads=1
```

Required results remain unobserved: successful compilation and discovery of exactly the two selected fixtures; normal both pass; control exits101 with only the qualified admission/shutdown oracle FAILED, diagnostic OA-11 ledger admission and shutdown both missed accepted work, and the qualified settlement publication companion ok. A filtered library run may report other unit tests filtered out; it must execute exactly the two model fixtures. Build/zero discovery/timeout/abnormal termination/unrelated failure is inconclusive, not a detected model defect.

Exact source patch and shared owner proposal are separate files. Shared proposal changes only the OA11 CI runtime selector/qualified names; OA12 ledger case library binary target and full names plus its cfg source path/hash entries; OA18 ledger positive/discovery argv; and current OA11 documentation route. Preserve all 52 cases, controls, core model legs and unrelated rows. Parent refreshes final source hashes after other current repair edits. negative-controls.py at inspected SHA f58601b7290655abcb9f23fb90040142e3970f539e581d73d00719eedf0ae954 already routes adapter=loom through native() with cfg instrumentation and selects --lib when target equals package.replace('-', '_'); no helper code change is proposed. The OA12 ledger manifest target is prometheus_parking_lot_runtime_tokio, allowing its existing binary/full-test nextest filters to remain exact.

The source module stays private and normal public behavior is unchanged. No new happens-before claim is added: the original SeqCst admission total-order argument and finish-before-settle release/acquire publication remain the modeled edges; Notify/full runtime/cancellation callbacks and all Relaxed reorderings remain outside this bounded coverage. The uncomfortable limit is still that a repaired build is not evidence of race correctness. Compiler/discovery/model/control behavior must be observed after import before any PASS claim.

Only bounded source reads, source authoring, exact diff generation and SHA256 hashing were performed here. No Cargo/test/build/fmt/syntax validation/analyzer/scan/review, verification-tree edit, KBD mutation, commit or external action ran. There are no unsolicited production changes. Stop at the parent integration handoff.
