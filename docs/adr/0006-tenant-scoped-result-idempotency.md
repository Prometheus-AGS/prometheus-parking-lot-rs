# ADR 0006: Address-scoped result idempotency and explicit legacy migration

- Status: Accepted implementation contract; execution unverified
- Date: 2026-10-24
- Change: oa-06-tenant-idempotency (#80)
- Owners: tech-lead contracts; postgres-engineer PG; events-engineer redb

## Final 0.4 protocol: schema 3 supersedes the intermediate layout

OA-07 is now source integrated. Final 0.4 uses PG protocol 3 and redb schema 3,
not the intermediate OA-06 version 2 described in the historical layout section.
[ADR 0007](0007-authoritative-duplicate-status.md) is the current owner/status
conversion runbook. A direct 0.3 -> 3 conversion resolves owner and status in one
transaction; intermediate 2 -> 3 resolves missing status without committing a
partial upgrade. Ordinary embedded open creates/reopens 3 and refuses 1/2.
Owner-only APIs remain available but supply no missing historical status.
`DeliveryOutcome::Duplicate { status }` now returns original full status and
loses `Copy`/`Hash`; mailbox method signatures and address scope remain intact.
All execution remains `NOT_EXERCISED`.

## Decision and exact shared interface

Within a backend storage namespace, a result's idempotency identity is the
pair `(complete MailboxKey, caller idempotency key)`. Every tenant, user and
session field participates; optional `None` and `Some("")` remain distinct.
Two addresses reusing one caller key each retain their own first result.
Consume does not forget a fence. The fence retains the original result's
expiry, or remains permanent without a TTL. Expiry/eviction at one address
must not forget an equal caller key at another address. Unkeyed delivery is
unchanged. This is a deliberate correction to the 0.3 global-key semantics.

`Delivery`, `MailboxEntry` and `ResultMailbox<T>` method signatures are unchanged.
OA-06 originally left `DeliveryOutcome` unchanged; OA-07 extends its duplicate
variant as described above. In particular:

```rust
fn deliver(
    &self,
    delivery: Delivery<T>,
) -> impl Future<Output = Result<DeliveryOutcome, SchedulerError>> + Send;
```

The shared core additionally exports `LegacyIdempotencyOwner` from
`contracts::result_mailbox`, `contracts`, and the core crate root. It is a
pure data type with public `idempotency_key: String` and `key: MailboxKey`
fields, serde serialization/deserialization, `deny_unknown_fields`, and
`LegacyIdempotencyOwner::new(idempotency_key, key)`. It adds no IO or backend
dependency. A record serializes as:

```json
{
  "idempotency_key": "consumed-without-ttl",
  "key": {"tenant": "tenant-a", "user_id": "user", "session_id": "session"}
}
```

The backend migration entry contracts are:

- Postgres: `PgResultMailbox::<T>::migrate_with_idempotency_owners(pool,
  owners: &[(String, LegacyIdempotencyOwner)])`, async, returning
  `Result<(), SchedulerError>`. The existing pool argument remains `&PgPool`;
  each outer string is the exact `mailbox_name`. The existing `migrate(pool)`
  delegates with no assignments and refuses if safe inference is insufficient.
- redb: `EmbeddedStore::open_with_idempotency_owners(path,
  owners: &[LegacyIdempotencyOwner]) -> Result<Self, SchedulerError>` and
  `open_with_clock_and_idempotency_owners(path, clock,
  owners: &[LegacyIdempotencyOwner]) -> Result<Self, SchedulerError>`.
  Paths/clocks retain the existing `impl AsRef<Path>` / `impl Clock + 'static`
  forms. These owner-only conversion APIs now delegate with empty status evidence.
  Ordinary `open` and `open_with_clock` create/reopen schema 3 and refuse 1/2.
  Use `open_with_idempotency_metadata` for missing original status; a schema-3
  reopen requires no mapping and a replay cannot extend any fence lifetime.

Do not invent a second mapping type or another mailbox trait. Mapping ownership
evidence is authoritative external input; a caller-provided arbitrary tenant
string is not a provenance discovery mechanism. A migration cannot identify a
live production store's owner merely from the task id or queued work.

## Why automatic tombstone reassignment is unsafe

Postgres 0.3 `pl_result_mailbox_keys` stores only `mailbox_name`,
`idempotency_key`, and `forget_at`. redb 0.3 `mb_idem` stores only caller key
and forget-at milliseconds (`u64::MAX` means permanent). Both delete the result
on consume while retaining that fence, so its address is then absent. Neither
store can reconstruct an owner for a consumed permanent tombstone.

The uncomfortable tradeoff is that such a store may need external ownership
evidence to upgrade at all. The approved default is **atomic refusal**, not a
global compatibility fence, a guessed tenant or a reset that can redeliver a
consumed result. This is an implementation policy, not permission to touch a
live store and not an additional release-wide operator gate.

## Conversion rules for backend implementers

1. Stop all old readers/writers; take an untouched backup and record its digest,
   source schema, native expiry precision, store/queue identity and fixture
   manifest. Work on an isolated copy. No production migration is authorized.
2. Inspect a stable locked snapshot of the legacy entries and fences before
   changing schema/data. PG holds the required table/advisory locks for its
   transaction; redb uses one write transaction including the schema version.
3. For each **unexpired** remembered fence, infer the owner only if exactly one
   retained result matches its storage namespace, caller key and **original
   expiry**. PG requires `forget_at IS NOT DISTINCT FROM expires_at` at native
   timestamp precision. redb requires the same original expiry, treating a
   result's `None` as the fence's permanent sentinel. A retained expired row
   from an earlier reuse of the caller key is not owner evidence.
4. Otherwise require an explicit authoritative owner assignment. Reject missing,
   duplicate, unknown or conflicting assignments. If a retained result proves
   one owner, a conflicting supplied mapping fails. Do not arbitrarily choose
   between multiple matching rows or silently ignore ambiguous legacy data.
5. Preserve every existing result, task id, status, payload, delivery order,
   delivered timestamp and original TTL/expiry. Preserve consumed fences and
   never restart their TTL. Preserve all legacy fence records, including
   expired ownerless rows, as immutable migration provenance. Expired rows
   have no active dedup effect; no owner is fabricated for them. New scoped
   fences go into a separate versioned table keyed by address and caller key.
6. Refusal rolls back all logical table/schema/identity changes. On success,
   atomically install scoped fences and the new schema/protocol marker. Keep
   queue contents, counters and stable queue identity intact. New delivery
   never reads old global fences as active suppression of another tenant.
7. Enforce old-writer rejection after conversion. The intermediate redb schema 2 caused the
   unmodified 0.3 open to reject. Final schema 3 also rejects the intermediate
   schema-2 open protocol. PG must reject the actual legacy SQL mutation
   path at the database boundary, for example with write-protected retired
   legacy tables and separate versioned replacements. An application-only
   convention that old binaries do not check is insufficient. Old readers
   must remain stopped even if archived tables can still be inspected.

The scoped encoding must preserve structure. PG can use canonical existing
MailboxKey JSON plus caller key columns; redb can use the tuple
`(canonical MailboxKey JSON, caller key)`. Do not concatenate unescaped tenant,
user, session or caller strings with a delimiter.

Rollback uses the **untouched pre-upgrade backup** while every new writer is
stopped. Do not downgrade or point an old binary at a migrated live store;
restoring the backup loses subsequent new writes unless those are separately
reconciled. A refused redb transaction guarantees unchanged logical content,
schema and identity, not identical physical file bytes; record backup byte
digests separately from logical migration assertions.

## Required populated 0.3 fixtures and discriminating cases

Each backend owner must add a real legacy-layout fixture, with a manifest and
digest-recorded untouched backup. Populate a retained keyed result with known
owner, a consumed finite-TTL fence, a consumed permanent fence, an unkeyed
result, and an expired old result followed by reuse of the same caller key.
Include an expired ownerless fence retained as provenance. Preserve native
expiry precision. The fixture's known owner mapping is test authority, never
a fabricated production mapping.

Without the consumed-fence mapping, explicit migration must fail with all
logical results/fences/versions/identity unchanged. With its correct mapping,
migration succeeds, original results remain consumable once and retries of
consumed keys remain duplicate at the original address. Other addresses may
independently reuse the key. An incorrect owner contradicting retained proof,
duplicate assignments and unknown keys fail. The reused-key case must bind
only to the matching expiry generation. Reopening/retrying migration must not
extend TTL, drop legacy provenance, or reset sequence/queue identity. Old
writer SQL / schema-1 open must be rejected after successful conversion.

Run the shared `result_mailbox_conformance!` cases on fresh PG17, PG18 and
redb stores. New shared cases are
`idempotency_is_scoped_to_mailbox_key` and
`idempotency_expiry_is_scoped_to_mailbox_key`. Both are registered in the macro
and `run_all`; `first_write_wins` now asserts same-address suppression.
The deliberately mailbox-global negative control must fail the cross-address
oracle. Durable owners additionally exercise cross-tenant reuse, same-address
first-write-wins after consume/restart and original-TTL expiry across restart.

## Deferred verification and authoritative status extension

OA-06 and OA-07 source/fixtures are integrated; no tests, builds, formatters,
analyzers or review ran here. Final verification belongs to OA-18 task2.
Consumed 0.3 fences lack owner and status; consumed intermediate fences lack
status. `LegacyIdempotencyOwner` asserts owner only. The current
`LegacyDeliveryStatus` mapping and atomic refusal rules are in ADR 0007;
never invent Completed or use a retry/destructive consume as historical proof.

## Historical intermediate OA-06 layouts and replay contract

The following records the intermediate OA-06 source format for migration and
fixture interpretation only. These are not final 0.4 deployment instructions.
Final active/provenance tables and schema 3 are specified in ADR 0007. All
execution evidence remains `NOT_EXERCISED`.

| Backend | Active data | Immutable migration provenance | Protocol boundary |
| --- | --- | --- | --- |
| PG17 / PG18 | `pl_result_mailbox_v2`; `pl_result_mailbox_keys_v2` keyed by `(mailbox_name, mailbox_key, idempotency_key)` | Original `pl_result_mailbox` and `pl_result_mailbox_keys`; resolved `(mailbox_name, idempotency_key) -> mailbox_key` in `pl_result_mailbox_owners_v2` | `pl_result_mailbox_protocol` singleton version 2; retired-table statement triggers reject legacy writes |
| redb | Existing `mb_entries`, `mb_meta`; scoped `(mailbox JSON, caller key) -> original forget-at` in `mb_idem_v2` | Original `mb_idem`; resolved `caller key -> canonical owner JSON` in `mb_legacy_owners_v2` | `q_meta.schema_version = 2`, committed atomically with conversion |

PG leaves original result rows, fence rows and old identity-sequence state as
provenance, copies every result to the v2 table and transfers the original
identity-sequence high-water mark. Its `pl_result_mailbox_retired` trigger
function raises SQLSTATE `55000` for legacy `INSERT`, `UPDATE`, `DELETE` and
`TRUNCATE`, including statements affecting no rows. The old names remain
occupied, so running the frozen 005 startup SQL cannot recreate a parallel
writable mailbox. Startup SQL may itself return successfully; attempted old
mutations must still be rejected. The resolved owners table gains its insert
guard after initial population and rejects later mutations as well. Fresh PG
databases also install the version-2 protocol and guarded old-name tables.

redb preserves all raw mailbox entry bytes, mailbox sequence and queue tables,
counters and store identity. Its legacy fence table is no longer consulted
or changed by v2 delivery/eviction. Owner provenance is recorded in the same
conversion transaction as the new scoped fences and schema marker. The old
schema-1 open protocol rejects version 2 before committing a write.

After successful conversion, an ordinary reopen/migration with an empty
mapping is valid. An explicit nonempty replay validates each supplied owner
against the immutable resolved-owner table. It must not reconstruct active
fences from the legacy table, reset their TTL, or resurrect fences that have
expired or been evicted. A previously recorded mapping remains replayable
after its fence expires; an unknown/conflicting replay fails. During initial
conversion, by contrast, an assignment for an already expired legacy fence
is rejected as unnecessary rather than silently ignored. Duplicate supplied
identities fail in both phases.

If more than one retained row matches an active caller key and exact expiry,
an authoritative mapping is mandatory and must select one of those candidate
addresses. It cannot contradict retained candidate evidence. A consumed fence
with no candidate can be assigned only from authoritative external evidence.

## Operational runbook: isolated rehearsal and future store conversion

Use the [current schema-3 runbook in ADR 0007](0007-authoritative-duplicate-status.md#operational-runbook-for-final-schema-3).
There is no owner/status mapping CLI or configuration-builder evidence field.
No live store upgrade is authorized by this implementation.

1. Quiesce old readers/writers; preserve an untouched consistent backup and
   SHA-256 receipt, namespace, original schema, native expiry precision and
   queue/store identity. Rehearse only on an isolated writable copy.
2. Resolve every active fence's complete address and original full status from
   the same exact-expiry retained generation or authoritative original evidence.
   A consumed permanent fence with unknown metadata cannot safely upgrade by
   waiting. Preserve unmatched/ambiguous source data and refuse atomically.
3. Build core `LegacyIdempotencyOwner` assignments for unresolved schema-1
   owners and `LegacyDeliveryStatus` assignments for unresolved status in either
   source version. Bind authority to the exact backup and namespace. Synthetic
   fixture assignments do not identify any live production owner or outcome.
4. Invoke `PgResultMailbox::<T>::migrate_with_idempotency_metadata(&pool,
   &owners, &statuses).await` or `EmbeddedStore::open_with_idempotency_metadata(
   &working_copy, &owners, &statuses)`; use the clock-controlled embedded
   equivalent when appropriate. Do not execute migrations 008/009 by hand or
   stage a committed 1 -> 2 upgrade before status validation.
5. At the final boundary, establish logical refusal/backup preservation,
   successful known-evidence conversion, once-only results, consumed-status
   retention, original TTL, immutable replay, identity/order and rejection of
   both old protocols. After conversion use only schema-3 readers/writers.
6. Rollback stops all new writers and restores the untouched source backup to
   a separate target. Do not downgrade converted stores. Reconcile subsequent
   writes separately. Redb refusal guarantees logical rollback, not unchanged
   physical housekeeping; preserve independent backup bytes/digests.

## Fixture identity and compatibility-evidence limits

PG's frozen schema/data/backup and legacy keyed-delivery SQL are under
`crates/parking-lot-pg/tests/fixtures/oa06/`; `manifest.json` declares source
revision `92da0d3d28f7896ddc67062fa49ba40ea48d74a3`, SHA-256 identities,
synthetic owners and test/control classifiers. `mailbox_migration.rs` starts
real isolated PG17/PG18 containers when exercised. Its backup-read and
old-writer cases execute frozen 0.3 SQL against the corresponding schemas;
they do **not** execute a published 0.3 binary. The frozen backup is an SQL
fixture, not a newly observed operator backup or `pg_dump` receipt.

redb's `tests/support/legacy_v03.rs` independently writes real schema-1 files
using the frozen 0.3 table/open protocol from the same source revision. It
retains `working.redb`, `untouched-v03.redb` and `fixture-manifest.json` in an
owned temporary directory. Its manifest records source hashes, redb 4.4.0,
fixed clock `1700000000000`, queue identity
`redb:00000000000000110000000000000022`, owner mapping and logical snapshot.
Its FNV-1a fingerprint is explicitly **noncryptographic**; the final packet
must compute actual SHA-256 identities for generated working/backup files.
The old-open probe implements the frozen schema-1 protocol, not a downloaded
or published old binary. Fixture retention paths are printed when generated;
no such files or execution receipts exist merely because source is present.

Both backends include `parking_lot_negative_control="oa-06"`, which uses an
empty address only for the scoped fence lookup/claim while retaining the
real delivery address. The corresponding cross-address assertion must fail
with its named `OA-06:address-scope` oracle on each real backend. Compilation,
Docker failure, zero discovery, timeout or unrelated migration failure is not
an accepted negative-control result. The source-derived invocation/classifier
handoff is recorded in phase evidence `implementation/oa-06.md`.
