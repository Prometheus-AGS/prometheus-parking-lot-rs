Scope: OA18 PG Clippy repair3 only, within existing review/repair cycle 1. Read the supplied exact patch/receipt, full changed functions, direct caller, alternate cfg branches and related public impl boundaries in current central source.

Verdict: no actionable issue in this repair by static inspection. It preserves behavior and adds only the private bound already satisfied by its sole caller. Compiler/Clippy/runtime success remains unverified here.

Fresh proof in crates/parking-lot-pg/src/mailbox/mod.rs:

- Lines 177–184 make both idempotency_scope cfg variants const without changing either body: OA06 still returns the static empty string; normal execution still returns mailbox_key. Both return references without allocation, mutation or destructor work.
- Lines 299–301 make normal duplicate_outcome const while retaining `DeliveryOutcome::Duplicate { status }`, moving the original full status unchanged. The alternative OA07 cfg helper at 305–309 remains non-const and still returns `status: retry.clone()`, preserving its deliberate negative-control behavior.
- Line 257 changes only the private helper impl to `impl<T: Sync> PgResultMailbox<T>`. deliver_keyed at 259–296 borrows `delivery: &Delivery<T>` across begin/query awaits; T: Sync supplies the required Send property for that retained shared reference.
- The sole call site found by source reference search is deliver:211, inside the existing public ResultMailbox impl whose line 189 already requires `T: Serialize + DeserializeOwned + Send + Sync`. No supported caller gains a new requirement. Constructor/name/migration methods remain in unbounded impl<T> at 91–168; the sweeper's incumbent bounds at 364–366 are unchanged.
- The complete deliver_keyed body retains the same begin transaction, keyed insertion bindings (including original TTL and cfg-selected scope), fresh duplicate-status query, full status decode, duplicate_outcome call and commit. No await, query, error propagation, lock lifetime or delivery decision changes appear in the supplied patch. The s-7 consume cfg branches are unchanged.

The PG package inherits workspace Rust edition 2024. The core ResultMailbox<T: Send> contract is unchanged; only the existing PG implementation's already-stronger caller bound is propagated into its private helper.

No tests, Cargo, builds, analyzers, production/source edits, network actions or subagents performed. Only this owned review record was created. Parent must observe the serialized compiler/Clippy and relevant normal/alternate-cfg gates before claiming a passing result.
