# ADR 0007: Authoritative status on duplicate result delivery

- Status: Accepted implementation contract; execution unverified
- Date: 2026-10-24
- Change: oa-07-stored-terminal-outcome (#78), after OA-06
- Owners: tech-lead contracts; PG/events backend migrations; scheduler runtime/crash fixtures

## Decision and exact shared interface

`DeliveryOutcome` becomes:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub enum DeliveryOutcome {
    Stored,
    Duplicate { status: TaskStatus },
}
```

`Stored` is still a unit variant and means the attempted delivery was stored.
`Duplicate` means nothing was stored or consumed and returns the **original**
full `TaskStatus`, including the complete `TaskError` of a failure. That status
is retained atomically with the first idempotency fence, for its original
lifetime, including after result consumption. It cannot come from the retry.
No destructive `consume`, payload read, or optional metadata fallback is used
to discover it. Missing/undecodable authoritative metadata is an error, never
a metadata-free successful duplicate. `ResultMailbox` method signatures and
the OA-06 full-address/caller-key scope remain unchanged.

The enum loses `Copy` and `Hash`, because `TaskStatus` has neither. Callers must
match `Duplicate { status }`, use `Duplicate { .. }` when the metadata is
intentionally irrelevant, or clone the outcome. Existing `Stored` comparisons
in storage/notifier composition remain valid: notify only a newly stored
delivery and pass duplicate metadata through unchanged.

The in-memory reference retains `{ status, expires_at }` in its remembered
address/caller-key map, independently of the consumable result queue. Expiry
forgets both the original fence and status; subsequent reuse establishes a
new generation. TTL is never extended by a duplicate.

## Legacy metadata evidence and exact backend API handoff

Core exports `LegacyDeliveryStatus` alongside `LegacyIdempotencyOwner`, from
`contracts::result_mailbox`, `contracts` and the core crate root. The new pure
data type has public `idempotency_key: String`, `key: MailboxKey`, and
`status: TaskStatus` fields, serde with unknown-field rejection, and
`LegacyDeliveryStatus::new(caller_key, key, status)`. It adds no IO/dependencies.
Unlike the global legacy owner identity, status mapping identity includes the
complete address: intermediate OA-06 stores can remember one caller key at
multiple addresses. `LegacyIdempotencyOwner` remains unchanged and makes no
status claim.

The integrated backends expose these explicit metadata conversion APIs:

- `PgResultMailbox::<T>::migrate_with_idempotency_metadata(pool: &PgPool,
  owners: &[(String, LegacyIdempotencyOwner)],
  statuses: &[(String, LegacyDeliveryStatus)]) -> Result<(), SchedulerError>`,
  async. Each outer string is the exact Postgres mailbox name.
- `EmbeddedStore::open_with_idempotency_metadata(path,
  owners: &[LegacyIdempotencyOwner], statuses: &[LegacyDeliveryStatus])
  -> Result<Self, SchedulerError>` and
  `open_with_clock_and_idempotency_metadata(path, clock, owners, statuses)`.
  Paths/clocks retain the existing `impl AsRef<Path>` / `impl Clock + 'static`
  forms.

Existing PG `migrate` and owner-only APIs delegate with empty status evidence.
They can infer metadata only when uniquely proved; unresolved active status
refuses. Embedded ordinary opens create/reopen schema 3, refusing 1 and 2;
existing explicit owner-only opens delegate with empty status evidence and
can convert only when both required owner and status facts are provable.
Converted protocol-3/schema-3 reopens validate supplied mappings against
immutable provenance and never reconstruct active fences.

For direct 0.3 conversion, an unknown original owner still requires the
existing owner assignment; an unknown original status separately requires a
status assignment whose full key agrees with that resolved owner. Supplying
status evidence does not silently override ownership proof. Intermediate
schema-2 conversion already knows the address and needs only missing status
evidence; a repeated caller key at another address is an independent record.

## Direct 0.3 and intermediate OA-06 migration policy

The uncomfortable data loss is provenance loss in the old format: a consumed
0.3 tombstone has neither owner nor status; a consumed OA-06 tombstone has
owner but still no status. The stored retry outcome cannot restore either
historical fact. In particular an unknown original status is never inferred
as `Completed` merely because the retry succeeded.

1. Stop all old readers/writers and retain an untouched, SHA-256-bound backup.
   Rehearse on an isolated copy; this ADR authorizes no live upgrade.
2. Resolve every unexpired source fence's owner **and** status under one
   backend transaction, including schema/protocol transition. Direct 0.3 -> 3
   must not commit OA-06's intermediate version 2 and then refuse missing
   status. A refusal leaves logical source tables/schema/identity unchanged.
3. Infer only from retained rows matching namespace, resolved complete address,
   caller key and exact **original expiry generation** (native PG timestamp
   precision, or original redb milliseconds/permanent sentinel). Owner and
   status proof must come from the same retained generation. Multiple matching
   rows with the same address/status can prove one metadata value; differing
   owner/status candidates require authoritative evidence selecting a coherent
   retained `(address, status)` pair, never mixing facts from different rows.
4. With no retained proof, require authoritative status evidence; missing,
   unknown, expired, duplicate or contradictory initial assignments refuse.
   A uniquely proved status rejects contradictory supplied evidence. Keep all
   source results, original timestamps/TTL, consumed/expired legacy provenance,
   queue state/counters and stable queue identity. Do not synthesize a result.
5. Record resolved metadata as immutable migration provenance separate from
   active fences. A valid evidence replay after consume/expiry/eviction only
   validates that original proof; it cannot rebuild or extend a fence. Empty
   evidence reopen is allowed after successful conversion. Conflicting or
   unknown supplied replay fails.
6. Use a new backend protocol/schema version that rejects both old 0.3 and
   intermediate OA-06 writers. PG cannot merely add a nullable/default status
   column that lets an old v2 writer create statusless active fences. Versioned
   replacements plus retired-write guards must cover old v1 **and v2** SQL.
   redb commits schema 3 atomically so both frozen old open protocols refuse.
   Existing live handles must be quiesced before conversion.
7. Rollback restores the untouched pre-upgrade backup with all new writers
   stopped. Do not downgrade converted data or erase provenance; reconcile
   subsequent new writes separately. Refused redb transactions promise logical
   rollback, not identical physical file housekeeping; preserve backup bytes
   independently as in ADR 0006.

Backend implementations must return duplicate decision/status consistently
under concurrent delivery, consume and eviction. PG's current expired-claim
`ON CONFLICT ... DO UPDATE ... WHERE` locks an existing key even when it
returns no inserted row: the owner may hold that lock in a short transaction
and obtain original metadata with a fresh-snapshot SELECT before commit.
A same-statement fallback SELECT at READ COMMITTED can miss a concurrent
winner and is not a metadata fallback. redb uses one write transaction.

## Generic current statuses and the durable boundary

`Delivery` already supports current statuses `Queued` and `Running`. OA-07
preserves that behavior: all mailbox backends and migrations retain/return
those values exactly. No terminal-only validation or new error taxonomy is
added.

A durable pool must use duplicate metadata to choose its event and lease
resolution; a local successful retry cannot emit `Completed` or ack over an
original stored `Failed`. Preserve delivery-before-resolution ordering and
exactly one consumable result, including if the first result was already
consumed before the crash/retry.

If the authoritative duplicate is `Queued` or `Running`, the durable consumer
logs the existing-boundary conflict and uses its existing requeue/hand-back
path. It emits the ordinary `Requeued` run ending, never a fabricated terminal
result, acknowledgement or dead-letter merely to conceal missing terminal
state. A finite original fence eventually expires; a permanent nonterminal
fence can force retries indefinitely until separate explicit data remediation.
That availability cost follows from the existing public mailbox semantics.
Do not overwrite the first result or silently change its lifetime to escape it.

The scheduler owner must handle all supported terminal stored statuses
explicitly rather than retaining a wildcard `Completed` fallback. Stored
`Completed`, `Failed(error)`, `Expired` and `Dropped(reason)` must not be
misreported according to the local run's branch. The stored drop reason does
not reconstruct a historical typed capacity error; no such error may be
invented. Use existing event categories/reasons with a truthful stored-status
diagnostic where richer historical classification is unavailable, or surface
an actual event-taxonomy conflict to the driver before implementation.

## Required fixtures and handoffs

Shared conformance adds `duplicate_returns_original_status` (all existing
TaskStatus variants, before and after consume, without destructive lookup) and
`duplicate_status_expires_with_original_fence` (new generation after expiry).
Existing OA-06 first-write-wins assertions now check authoritative duplicate
status. A deliberately broken bool-only wrapper returns the retry's status
and must fail `OA-07:stored-status` / `OA-07:consumed-status` oracles.

PG17, PG18 and redb owners extend their real persisted fixtures for direct
0.3 -> 3 and independent intermediate 2 -> 3 conversion. Keep frozen OA-06
source/backup assets intact. New synthetic metadata evidence identifies its
source fixture and original status explicitly. Both paths must refuse missing
consumed-status evidence atomically, succeed with known evidence, preserve
native TTL/results/identity, retain status after consume/reopen and reject
both old writer protocols. Intermediate fixtures must actually use the old
layout, not create the new schema and relabel its marker.

The scheduler-owned production crash harness is
`tests/scheduler/durable_sigkill.rs`, with the existing child phase markers
`EXECUTING`, `DELIVERING`, `DELIVERED`, real PG/redb parents, restarted/renamed
pool and once-only consumption. Extend it rather than adding a parallel fake
crash harness: kill after a stored Failed delivery and before lease resolution,
then retry with local success and require a Failed event carrying the original
TaskError plus one original result. Include the consumed-before-retry case.
The old bool-only runtime control must preserve the real backend execution
and fail the event/result-status consistency assertion, not compilation,
timeout or empty discovery. Delivery-before-resolution retains its existing
S-16 discriminating scenario.

Storage/notifier composition must pass status through and notify only `Stored`.
FFI's existing two-way delivery label can intentionally ignore duplicate
metadata with `Duplicate { .. }`; it must not fabricate or expose the retry's
status as the original. Backend/runtime/FFI owners update their own call sites
after the driver imports the contract. No other role's source is edited here.

No Cargo, tests, formatting, analyzers, scans or reviews ran while authoring
this contract. Compilation, migrations, crash/restart behavior and all oracle
results remain unverified until the completed production phase boundary.

## Integrated final schema 3 and settlement

The driver integrated contract, PG, redb/consumer, FFI and scheduler packages;
import receipts in phase evidence identify exact source hashes. This is source
integration, not compilation, migration or process-kill execution evidence.

| Backend | Active schema | Immutable migration provenance / old-writer boundary |
| --- | --- | --- |
| PG17/18 | `pl_result_mailbox_v3`; `pl_result_mailbox_keys_v3` keyed by `(mailbox_name, mailbox_key, idempotency_key)`, retaining `status JSONB NOT NULL` and native `forget_at` | `pl_result_mailbox_metadata_v3` retains original conversion status/expiry; `pl_result_mailbox_owners_v2` retains legacy ownership; original and v2 tables/sequence state remain guarded provenance; `pl_result_mailbox_protocol.version = 3`. |
| redb | Existing `mb_entries`/`mb_meta`; `mb_idem_v3` maps structured `(canonical mailbox JSON, caller)` to original status/forget-at JSON | `mb_idem`, `mb_idem_v2`, `mb_legacy_owners_v2` and `mb_legacy_metadata_v3` remain provenance; `q_meta.schema_version = 3` rejects both older open protocols. Queue state/counters/identity remain intact. |

PG conversion holds an advisory transaction lock and required table locks,
resolves all metadata, copies original rows and sequence high-water, installs
v3 fences/proof and guards, then commits version 3 together. Retired v1/v2
INSERT/UPDATE/DELETE/TRUNCATE fail at the database boundary (SQLSTATE 55000),
including empty mutations. Old startup cannot recreate writable old names;
startup itself may succeed, which is not write compatibility. Proof rows are
write guarded after population. Final delivery holds its key claim row lock
through a fresh-statement status SELECT and transaction commit, avoiding the
READ COMMITTED same-statement concurrent-winner visibility gap.

Redb direct conversion and status conversion run in one store write transaction;
refusal cannot commit an intermediate schema 2. Existing raw result bytes and
original timestamps are retained. Ordinary open creates/reopens 3; explicit
metadata APIs convert 1/2. The older tables are never active global/scoped
suppression in new delivery. Malformed authoritative metadata is a backend
error, not a retry-status fallback.

The integrated durable `settle` preserves `Option<DeliveryOutcome>` through
its retrying delivery helper. `Stored` and no-address settlements retain local
classification. An authoritative duplicate selects both event and resolution:

| Stored status | Event / lease resolution |
| --- | --- |
| `Completed` | `Completed`, then ack. |
| `Failed(error)` | `Failed` with original error's string, then dead-letter with that reason. |
| `Expired` | `Expired`, then deadline dead-letter. |
| `Dropped(reason)` | `Rejected(Custom(reason))`, then dead-letter with original reason; no invented historical capacity error. |
| `Queued` / `Running` / unsupported future status | Warning and existing `Requeued` hand-back; no ack or dead-letter based on fabricated terminal truth. |

Delivery precedes terminal emission and lease resolution. Reported-run tracking,
heartbeat stop, resolution retries and held-work accounting retain their
incumbent order; exhausted delivery attempts hand back as before. Executor
attempt counters still describe local execution. Shutdown/cancellation,
oversize thresholds, dispatch rejection and expiry behavior are preserved.
Permanent nonterminal metadata may retry indefinitely; finite metadata retains
its original TTL. This implementation does not authorize remediation writes.

## Operational runbook for final schema 3

1. Identify source schema/protocol 1 or 2, candidate and source commits,
   backend major/library/image, namespace, native time/expiry precision and
   queue identity. Quiesce every old handle before consistent backup. Retain
   an untouched backup and SHA-256 receipt outside the writable working copy.
2. Bind original owner/status evidence to that backup. Schema 1 consumed
   fences may need both mappings; schema 2 already knows each complete
   address but may lack status. A key reused at two addresses requires two
   status records. Exact-generation retained candidates constrain supplied
   authority; an assignment cannot choose an unrelated owner/status pair.
   Missing permanent consumed metadata requires atomic refusal, preserving
   source data; neither waiting nor local retry success recovers that fact.
3. Invoke only the explicit Rust metadata entry on an isolated copy. Example
   below assumes `original_status` was already obtained authoritatively and
   bound to the source generation; it does not manufacture an outcome.

```rust,ignore
use prometheus_parking_lot_core::{LegacyDeliveryStatus, LegacyIdempotencyOwner};
use prometheus_parking_lot_pg::PgResultMailbox;

let owners = vec![(namespace.clone(),
    LegacyIdempotencyOwner::new(caller.clone(), original_address.clone()))];
let statuses = vec![(namespace,
    LegacyDeliveryStatus::new(caller, original_address, original_status))];
PgResultMailbox::<String>::migrate_with_idempotency_metadata(
    &pool, &owners, &statuses,
).await?;
```

For intermediate schema 2, owners can be empty; original legacy owner replay
is optional but must match immutable evidence if supplied. Embedded takes the
same core record values without PG's outer namespace strings:

```rust,ignore
use prometheus_parking_lot_embedded::EmbeddedStore;

let store = EmbeddedStore::open_with_idempotency_metadata(
    &working_copy, &owners, &statuses,
)?;
```

Here embedded `owners` is `Vec<LegacyIdempotencyOwner>` and `statuses` is
`Vec<LegacyDeliveryStatus>`, not the PG tuple vectors. Its clock-controlled
entry is `open_with_clock_and_idempotency_metadata(path, clock, owners,
statuses)`. No backend automatically loads an evidence JSON file. Core types
are available through `prometheus_parking_lot_core`, not a new facade export.

4. At the final production boundary run the real refusal/conversion/replay,
   concurrent duplicate, old-protocol and process-kill fixtures recorded in
   phase `implementation/oa-07.md`. Capture actual generated manifests,
   source/backup/working hashes, images, commands, exits and logs. No successful
   result is implied by authored tests or task closure.
5. On successful conversion permit only new readers/writers. Empty evidence
   reopen is valid; nonempty replay validates immutable provenance without
   rebuilding/refreshing active fences. Unknown/conflicting replay refuses.
   Builders supply no missing owner/status authority and embedded ordinary
   opens do not migrate 1/2 automatically.
6. To roll back, stop every new writer and restore the untouched pre-upgrade
   backup to a separate target. Never downgrade or strip provenance from a
   converted store. Reconcile later writes separately. PG archived-table
   readability is inspection only; old readers stay stopped. Redb refusal
   compares logical state/marker/identity, with backup bytes checked separately.

The concrete persisted fixtures, backup identities, exact process-kill case
IDs, backend `oa-07` and independent runtime `oa-07-event` control classifiers
and final-boundary argv are in phase `implementation/oa-07.md`. Frozen old
SQL/open-protocol probes are not published-binary executions. No live upgrade,
credential retirement, signing, OIDC or publication action is authorized.
