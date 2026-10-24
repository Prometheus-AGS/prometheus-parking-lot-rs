# Runbook: PostgreSQL 18 (and 17) for the parking-lot Postgres backends

Scope: operating `PgLeaseQueue`, `PgResultMailbox` and `PgmqLeaseQueue`
(`crates/parking-lot-pg`) on PostgreSQL 17 and 18. The ordinary CI matrix selects Docker-gated
suites on both majors (`postgres` job in `.github/workflows/ci.yml`, cr-36).
The OA-14 upgrade binary is excluded from those fresh-cluster legs and runs
separately against a marked, physically upgraded cluster (section 7).
Facts about PostgreSQL and the official Docker image are cited in
[Sources](#sources); facts about this repository were checked against the
`parking-lot-pg:17` and `parking-lot-pg:18` images built from `docker/` in the
prior runbook. Those historical observations do not certify the OA-14 source
changes. **OA-14 runtime status: NOT_EXERCISED** until the OA-18 final boundary.
No operator cluster, host package installation or production migration is
authorized by the disposable rehearsal below.

## What differs between 17 and 18

Previously recorded on the project images (`SHOW` / `docker inspect`);
OA-14 must record its own actual versions and image IDs:

| | 17 (17.11) | 18 (18.6) |
|---|---|---|
| `data_checksums` on a fresh cluster | `off` | `on` |
| `PGDATA` in the official image | `/var/lib/postgresql/data` | `/var/lib/postgresql/18/docker` |
| Declared `VOLUME` | `/var/lib/postgresql/data` | `/var/lib/postgresql` |
| `password_encryption` | `scram-sha-256` | `scram-sha-256` |
| `md5_password_warnings` | does not exist | `on` |
| `autovacuum_vacuum_max_threshold` | does not exist | `100000000` |
| `io_method` | does not exist | `worker` |

## 1. Data checksums

PostgreSQL 18 changed `initdb` to enable data checksums by default; the new
`initdb --no-data-checksums` turns them off [S1]. A 17 cluster created with
default options has none (table above).

Check a running server:

```sql
SHOW data_checksums;   -- on | off
```

Consequences:

- A new 18 cluster has checksums. They catch silent page corruption on read at a
  small CPU cost. Keep them for queue data: a corrupted page in `pl_lease_queue`
  is a lost or duplicated task.
- `pg_upgrade` requires the old and new clusters to match [S1]. To upgrade a 17
  cluster without checksums, either initialise the new 18 cluster with
  `--no-data-checksums` (the route [S1] names), or enable checksums on the old
  cluster first with `pg_checksums --enable` while it is shut down [S6].
- In the Docker image, `POSTGRES_INITDB_ARGS` passes extra arguments to
  `initdb` [S3]; for example `POSTGRES_INITDB_ARGS=--no-data-checksums` to match
  an existing non-checksummed 17 cluster being upgraded.

## 2. Docker volume path and `PGDATA`

The official `postgres:18` image changed both [S3]:

- `PGDATA` is version specific: `/var/lib/postgresql/18/docker`.
- The `VOLUME` is `/var/lib/postgresql` (not `.../data`), so that a later
  `pg_upgrade --link` can see the old and new data directories on one mount.

Mount the volume at **`/var/lib/postgresql`** for 18. A mount at
`/var/lib/postgresql/data` is the 17-and-earlier layout; on 18 the server does
not write there, so the data would not be on your volume. [S3] describes the
mirror-image mistake for 17 (a mount at `/var/lib/postgresql` does not persist
data because the declared volume is `.../data`). Verified here with
`docker inspect`: 17 mounts `/var/lib/postgresql/data`, 18 mounts
`/var/lib/postgresql`.

```text
docker run -d --name pg18 -e POSTGRES_PASSWORD=... \
  -v parking-lot-pg18:/var/lib/postgresql parking-lot-pg:18
docker run -d --name pg17 -e POSTGRES_PASSWORD=... \
  -v parking-lot-pg17:/var/lib/postgresql/data parking-lot-pg:17
```

`/docker-entrypoint-initdb.d` scripts (`docker/postgres18/init`, which install
`pgmq` and `pg_stat_statements`) run only against an empty data directory [S3].
An existing volume is not touched; run `CREATE EXTENSION` by hand there
(section 4). The image starts the server with
`-c config_file=/etc/postgresql/postgresql.conf` (the tuned
`docker/postgres18/postgresql.conf`, shared by both majors).

## 3. SCRAM authentication

`password_encryption` defaults to `scram-sha-256` on both majors (observed).
The official image writes `host all all all scram-sha-256` into `pg_hba.conf`
when `POSTGRES_HOST_AUTH_METHOD` is unset (14 and later) [S3]; the project
images show exactly that line, plus `trust` for loopback inside the container
only.

- PostgreSQL 18 deprecates MD5 passwords; `CREATE ROLE`/`ALTER ROLE` warn when
  setting one, controlled by `md5_password_warnings` [S1]. Create application
  roles with SCRAM and do not set `POSTGRES_HOST_AUTH_METHOD=md5` or `trust`.
- A role whose password was stored as MD5 on an old cluster keeps working until
  MD5 is removed; reset it under `password_encryption = 'scram-sha-256'`:
  `ALTER ROLE app PASSWORD '...';`
- sqlx negotiates SCRAM natively; no client change is needed. The connection
  URL is the same on 17 and 18.
- If you override the method, the image notes you may need
  `POSTGRES_INITDB_ARGS=--auth-host=scram-sha-256` for the cluster to initialise
  correctly [S3].

## 4. Installing pgmq

Only `PgmqLeaseQueue` needs pgmq; `PgLeaseQueue` and `PgResultMailbox` use plain
tables. pgmq 1.13.x supports PostgreSQL 14 to 18 [S4].

The project images build it from the pinned tag with PGXS (`make && make
install`, verified against the commit SHA in the Dockerfile) [S4]. On your own
server, install the extension files on the database host, then in each database:

```sql
SELECT name, default_version, installed_version FROM pg_available_extensions WHERE name = 'pgmq';
CREATE EXTENSION IF NOT EXISTS pgmq;
```

Installing needs the host file system; where that is not possible pgmq also has
a SQL-only installation that creates its objects in the `pgmq` schema [S4].
Migration `006_pgmq_lease_queue.sql` runs `CREATE EXTENSION IF NOT EXISTS pgmq`,
so the role that calls `PgmqLeaseQueue::prepare` needs the privilege to create
it, or a superuser creates the extension beforehand. Binary packages must match
the server major: after a 17 to 18 upgrade, install pgmq for 18 before running
`pg_upgrade` (section 7). Upgrading pgmq itself: build the new version, then
`ALTER EXTENSION pgmq UPDATE;` [S5].

## 5. Running migrations

There is no separate migration CLI. Use the backend API; applying the numbered
mailbox SQL files by hand bypasses owner/status resolution and is unsupported.
Queue/audit scripts remain idempotent. Mailbox conversion is an explicit Rust
transaction spanning the ownership and status stages:

| Call | Storage path |
|---|---|
| `PgLeaseQueue::migrate(&pool)` | `crates/parking-lot-pg/migrations/004_lease_queue.sql` |
| `PgResultMailbox::migrate(&pool)` | Metadata-aware conversion below, with empty owner/status evidence |
| `PgResultMailbox::migrate_with_idempotency_owners(&pool, &owners)` | Same conversion with empty status evidence; unresolved consumed status refuses |
| `PgResultMailbox::migrate_with_idempotency_metadata(&pool, &owners, &statuses)` | `mailbox/migration.rs` plus 005/008/009, one transaction to protocol 3 |
| `PgmqLeaseQueue::prepare()` (or `migrate`, then create queues) | `006_pgmq_lease_queue.sql`, then `pgmq.create` and `pl_pgmq_tune_queue` per queue |
| `PostgresAuditSink::migrate(&pool)` | `007_audit_events.sql` |

The facade invokes the empty-evidence mailbox API. It can reopen protocol 3 or
convert a source whose active ownership/status are all provable from retained
rows. It cannot supply missing consumed-generation evidence automatically.
For such a legacy store, stop the applications and use the explicit API before
starting the facade:

```rust
PgResultMailbox::<String>::migrate_with_idempotency_metadata(
    &pool, &owners, &statuses,
).await?;
```

`owners` has type `Vec<(String, LegacyIdempotencyOwner)>`; `statuses` has type
`Vec<(String, LegacyDeliveryStatus)>`. The outer string is the exact
`mailbox_name`. Ownership includes the complete `MailboxKey` and caller key;
status evidence includes that same address/caller and original full
`TaskStatus`, including the complete `TaskError` for `Failed`. The fixed
`fixtures/oa14/metadata.json` is authority for its synthetic fixture only.
Never apply those values to an operator store.

For direct 0.3 to protocol 3, ownership and status resolve before one commit.
For intermediate protocol 2, the scoped address is already known but missing
consumed status still requires proof. Retained-row inference requires the
same namespace/caller, resolved address and native expiry generation
(`expires_at IS NOT DISTINCT FROM forget_at`). Ambiguous supplied evidence
must select a coherent retained owner/status pair. Zero retained candidates
permit separate authoritative owner and status evidence; absent, duplicate,
unknown, expired, wrong-address or contradictory assignments refuse without
committing a partial conversion. In particular, do not commit an owner-only
protocol-2 migration and then attempt the status conversion separately.
See [ADR 0006](../adr/0006-tenant-scoped-result-idempotency.md) and
[ADR 0007](../adr/0007-authoritative-duplicate-status.md).

Results, full statuses, original native TTLs and sequence state are copied;
consume does not erase the active fence's original status. Immutable conversion
proof remains after consume or expiry, but replay does not resurrect an expired
fence. Old 0.3 and intermediate-v2 names remain occupied by guarded tables;
old inserts, updates, deletes and truncates fail. Stop old readers too: their
legacy-table reads would expose the frozen pre-conversion view of results.

A queue/audit script can be applied with `psql -v ON_ERROR_STOP=1
--single-transaction -f <script>` if needed. This does **not** authorize manual
005/008/009 mailbox conversion. Reapplying the supported API also retains
per-table settings; it is not a reason to skip upgrade metadata resolution.

### Locks taken by a re-run

A reopen of an already converted protocol-3 mailbox uses recorded metadata;
queue migration reopens guard catalog-changing statements. A **first mailbox
conversion is different**: it deliberately takes exclusive locks on source
result/fence tables so copy, mapping resolution, write guards and protocol
marker commit together. Perform it in the stopped-reader/writer window.
The migration lock tests in `crates/parking-lot-pg/tests/migrations.rs` remain
part of the real boundary suites; their source is not an OA-14 runtime result.
Four catalog-changing statements need guards even on ordinary reopens:

| Statement | Lock | Effect while held |
|---|---|---|
| `CREATE [UNIQUE] INDEX IF NOT EXISTS` | `SHARE` on the table, taken before the index is looked up | blocks every writer |
| `DROP TRIGGER` / `CREATE TRIGGER` | `ACCESS EXCLUSIVE` | blocks readers too |
| `CREATE OR REPLACE TRIGGER` | `SHARE ROW EXCLUSIVE` | blocks every writer |
| `ALTER TABLE ... SET (...)` | `SHARE UPDATE EXCLUSIVE` | does not block reads or writes |
| `COMMENT ON TABLE / COLUMN` | `SHARE UPDATE EXCLUSIVE` | does not block reads or writes, but conflicts with manual `VACUUM`/`ANALYZE`, anti-wraparound autovacuum, `CREATE INDEX CONCURRENTLY` and `pg_repack`, so under `lock_timeout` a re-run would fail start-up |

A lock request that waits also holds up every request behind it, so a re-run
queued behind one long transaction would stall the queue. The first run on a
fresh database does need these locks (the table is empty and new), and every
script starts with `SET LOCAL lock_timeout = '5s'`: if a lock is not granted in
5 s the migration fails with a `lock timeout` error instead of queueing, and
can be retried. `PgmqLeaseQueue::prepare()` likewise skips `pgmq.create` for a
queue that exists (it would run `CREATE INDEX IF NOT EXISTS` on the pgmq
table). Changing a trigger definition therefore needs a new numbered migration;
editing an existing script does not reach databases that already have the
trigger.

## 6. Autovacuum settings and monitoring bloat

### Settings (migrations 004, 006, 009; pgmq queues via `prepare()`)

| Table | `autovacuum_vacuum_scale_factor` / `_threshold` | `autovacuum_analyze_scale_factor` / `_threshold` |
|---|---|---|
| `pl_lease_queue` | 0 / 1000 | 0 / 1000 |
| `pl_result_mailbox_v3` (active results) | 0 / 1000 | 0 / 1000 |
| `pl_result_mailbox_keys_v3` (active status fences) | 0 / 500 | 0 / 500 |
| `pl_pgmq_aux` | 0 / 500 | 0 / 500 |
| `pgmq.q_<queue>` (each lane and dead-letter queue) | 0 / 1000 | 0 / 1000 |

Why: these tables are insert, update, delete churn with a live set that can
swing from a handful of rows to a large backlog. Autovacuum starts when dead
tuples exceed `threshold + scale_factor * live rows` (on 18, capped by
`autovacuum_vacuum_max_threshold`) [S2]. With the default 20%
(or the conf floor of 5%) a one-million-row backlog waits for 50,000 to 200,000
dead tuples; with scale factor 0 the trigger is the fixed threshold at any size,
so the dead rows that claim-path index scans step over stay bounded. The
thresholds are small enough to act within seconds under load and large enough
not to run a vacuum per transaction: 1000 for the wide tables (JSONB payloads),
500 for the narrow ones. They are starting values, not measured optima: tune from
`pg_stat_user_tables` under your load. The TOAST table of a table inherits the
table's values when it has none of its own [S2], so there are no `toast.*`
overrides. The analyze pair keeps planner statistics current for the partial
claim indexes as the ready/leased/dead mix shifts.

PostgreSQL 18 adds `autovacuum_vacuum_max_threshold`, a cap on the dead-tuple
count that triggers autovacuum [S1]; its default (100,000,000) is far above the
fixed thresholds here, so it does not interact with these settings. It matters
for big tables that still use a scale factor.

Verify:

```sql
SELECT c.relname, c.reloptions
  FROM pg_class c
 WHERE c.relname IN ('pl_lease_queue','pl_result_mailbox_v3','pl_result_mailbox_keys_v3','pl_pgmq_aux')
    OR c.relname LIKE 'q\_%' AND c.relnamespace = 'pgmq'::regnamespace;
```

Cluster-wide, `docker/postgres18/postgresql.conf` sets `autovacuum_naptime =
10s`, a 5% scale factor and a low cost delay for the test and local profile.
Production values belong to the operator.

### Monitoring bloat

Dead tuples and vacuum activity (works on 17 and 18):

```sql
SELECT relname, n_live_tup, n_dead_tup,
       round(100.0 * n_dead_tup / greatest(n_live_tup + n_dead_tup, 1), 1) AS dead_pct,
       last_autovacuum, autovacuum_count, last_autoanalyze
  FROM pg_stat_user_tables
 WHERE relname IN ('pl_lease_queue','pl_result_mailbox_v3','pl_result_mailbox_keys_v3','pl_pgmq_aux')
    OR relname LIKE 'q\_%'
 ORDER BY n_dead_tup DESC;
```

Alert when `n_dead_tup` stays several times above the table's threshold, or
`last_autovacuum` is old on a table that is taking writes: either autovacuum is
blocked or its workers are saturated.

Physical size against live rows (needs the `pgstattuple` contrib extension,
`CREATE EXTENSION pgstattuple`):

```sql
SELECT * FROM pgstattuple('pl_lease_queue');   -- dead_tuple_percent, free_percent
```

Things that stop vacuum from reclaiming space regardless of settings: a long
transaction or an idle-in-transaction session holds the vacuum horizon back (the
tuned conf sets `idle_in_transaction_session_timeout = 60s`), an abandoned
replication slot, or a prepared transaction. Check `pg_stat_activity`
(`xact_start`, `state`), `pg_replication_slots` and `pg_prepared_xacts`.

On 18 only: `pg_stat_all_tables` has `total_vacuum_time`, `total_autovacuum_time`,
`total_analyze_time` and `total_autoanalyze_time`, and vacuum/analyze report
delay time in the server log and in `pg_stat_progress_vacuum` [S1]. Use them to
see whether autovacuum on a queue table is spending its time sleeping on the
cost limit. `pg_stat_io` also reports I/O in bytes [S1].

If a table is already badly bloated, autovacuum marks space reusable but does
not return it to the OS (except by trailing truncation). Rewrite offline with
`VACUUM FULL` (exclusive lock) or online with `pg_repack`; schedule it in a
drain window.

## 7. Populated PG17 to PG18 upgrade rehearsal (OA-14)

**NOT_EXERCISED.** The following is the source procedure handed to OA-18.
Its uncomfortable failure case is a cluster that upgrades physically while a
consumed fence has no provable original owner or status. Physical success is
insufficient: metadata conversion must refuse atomically, and the application
must remain stopped until authoritative evidence or an untouched restore is
available. A fresh PG18 test run does not prove preservation through upgrade.

### Source fixture, pins and prerequisites

The only accepted source is a new, labelled, disposable PG17 volume populated
from `crates/parking-lot-pg/tests/fixtures/oa14`. Fixture identity is
`oa14-populated-0.3-pg17-v1`, bound to a unique run marker. Frozen 004/006 SQL
and the byte-identical OA07 untouched 0.3 mailbox backup create the old layout;
new APIs do not create it. Queue payloads, an expired fenced lease, delayed/dead
entries, retained/consumed mailbox generations, full Failed status, native TTLs,
tenant dedup state, pgmq's four lanes/archive/dead state and sequence counters
are populated. `metadata.json` supplies the synthetic consumed-owner/status
proof. Exact byte hashes, source URLs and argv arrays are in the fixture
`manifest.json`.

Keep the existing Dockerfile pins:

| Input | Source identity |
|---|---|
| PG17 official base | `postgres:17-bookworm@sha256:858bb5e98eed02eb8cafbcdc007f9a65d1b613972028944a8ec2133ccaf80d27` |
| PG18 official base | `postgres:18-bookworm@sha256:bbf83594798e144bff365a9c191c5d779f181fb3135902064ab8ff11918020fc` |
| pgmq | `v1.13.1`, commit `8eab18272e50077ab9f72d9787296655f2849dcc` |
| Dual-binary image PG17 package | `postgresql-17=17.11-1.pgdg12+2`; binary/server assertions require 17.11 and 18.6 |
| Boundary test runner | Preinstalled `cargo-nextest 0.9.149` |

The dual-binary image installs the PG17 package inside the disposable PG18
image and copies PG17 pgmq extension SQL/control files from the project PG17
image. It never installs host PostgreSQL. PG18 `pg_upgrade` needs both major
versions' binaries and extension files [S6]. The runner requires local immutable
`sha256:<64hex>` image IDs, checks source-ID labels and PG18 layer ancestry, and
records inspected IDs/architecture/repository digests and actual tool versions.
The version assertions and digest-built image compatibility remain unverified
until execution; an unavailable exact input is not permission to float a pin.

Required local tools are Docker with a usable daemon, Python 3, Git, the pinned
Rust toolchain and the exact preinstalled nextest. The runner acquires no tools.
The final owner must freeze a full committed candidate SHA and adopted-contract
SHA256. Working and committed bytes of the declared source set, fixtures and
fixture manifest must match. Refresh `candidate_sources` and handoff hashes
after final permitted formatting/source corrections **before** that freeze.
The task1/task2 hashes are source handoff identities, not final certification.

At the authorized final boundary, build the project images, inspect `.Id`, and
replace every angle-bracket placeholder below with those observed values.
`PG17_ID_HEX`/`PG18_ID_HEX` are the 64 hex digits after `sha256:`. The ID-derived
aliases let Dockerfile `FROM` select local images; runtime uses only their IDs.
Do not use an operator image, existing operator volume or host data directory.

```sh
docker build -f docker/postgres17/Dockerfile -t parking-lot-pg:17 docker
docker build -f docker/postgres18/Dockerfile -t parking-lot-pg:18 docker
docker image inspect parking-lot-pg:17 --format '{{.Id}}'
docker image inspect parking-lot-pg:18 --format '{{.Id}}'
docker tag sha256:<PG17_ID_HEX> oa14-local/pg17:<PG17_ID_HEX>
docker tag sha256:<PG18_ID_HEX> oa14-local/pg18:<PG18_ID_HEX>
docker build -f docker/postgres18/upgrade/Dockerfile \
  --build-arg PG17_IMAGE=oa14-local/pg17:<PG17_ID_HEX> \
  --build-arg PG18_IMAGE=oa14-local/pg18:<PG18_ID_HEX> \
  --build-arg PG17_SOURCE_ID=sha256:<PG17_ID_HEX> \
  --build-arg PG18_SOURCE_ID=sha256:<PG18_ID_HEX> \
  -t oa14-local/upgrade:<RUN_ID> docker
docker image inspect oa14-local/upgrade:<RUN_ID> --format '{{.Id}}'
```

### Physical conversion, then one atomic schema conversion

Run the complete positive rehearsal once with a new absolute output directory
and unique lowercase/digit/hyphen run ID (6 to 40 characters):

```sh
bash scripts/rehearse-postgres-upgrade.sh \
  --out <NEW_ABSOLUTE_OUTPUT_DIRECTORY> --run-id <UNIQUE_SAFE_RUN_ID> \
  --candidate <FULL_40_HEX_CANDIDATE> \
  --contract-sha256 <ADOPTED_CONTRACT_64_HEX_SHA256> \
  --pg17-image sha256:<64_HEX_LOCAL_PG17_ID> \
  --pg18-image sha256:<64_HEX_LOCAL_PG18_ID> \
  --upgrade-image sha256:<64_HEX_LOCAL_DUAL_BINARY_ID> --control none
```

The source runner performs these operations in order:

1. Refuse existing resource names. Create owned work and backup volumes; boot
   actual PG17 with checksums on and populate the frozen source in one SQL
   transaction. Persist a full logical baseline of raw rows and sequence state.
2. Stop PG17 cleanly before copying any data. Tar the entire source volume into
   the separate backup volume, record its actual SHA256 and make archive/hash
   files read-only. The backup is mounted read-only for subsequent restoration.
3. In the dual-binary container, initialize checksummed PG18 destination data
   plus two actual incompatible destinations. Run version/checksum preflights
   below before `oa14_upgrade_started` or any `pg_upgrade` call.
4. Run PG18 `pg_upgrade --retain --check`, then `--retain --copy`. Do not use
   `--link`, `--swap` or `--no-sync`. The source stays independent [S6]. Logs
   distinguish `OA-14:cluster-check-pass` and
   `OA-14:physical-upgrade-complete schema=legacy-0.3`.
5. Start actual project PG18 on the copied data and loopback-only published
   port; verify raw legacy rows, pgmq state and counters against the persisted
   baseline. Restore the untouched archive to a third volume and boot actual
   PG17 there; verify the same source baseline independently.
6. On the marked upgraded root database, the ignored
   `postgres_upgrade_populated_0_3_to_schema3` test first invokes owner-only
   migration. Missing consumed status must fail with no protocol/v2/v3 objects
   committed (`OA-14:atomic-schema-refusal`). Then the explicit metadata API in
   section 5 resolves ownership and original status in **one transaction** and
   copies to schema3. `OA-14:atomic-schema3-conversion-complete` is a separate
   result from physical upgrade.
7. Compare legacy and copied schema3 result/status/timestamp/native-TTL rows,
   fence expiry and sequence high-water. Exercise retained/consumed Failed
   provenance, consume-once and independent tenant reuse; lease and ack SQL
   queue rows without resetting the expired lease's fence; exercise actual
   pgmq lanes, delayed/leased/dead depth and archive preservation.
8. Run the existing real PG integration package, including lease kill/recovery,
   concurrent claims, mailbox TTL/status/migration and pgmq tests, on this
   actual upgraded server. Ordinary test helpers require its run marker and
   server18, then create a unique owned database per test; backup/child databases
   also use unique names. The special upgrade oracle alone uses the populated
   root database. It cannot silently fall back to a fresh test container.

The container-side physical argv is:

```sh
/usr/lib/postgresql/18/bin/pg_upgrade \
  --old-bindir=/usr/lib/postgresql/17/bin --new-bindir=/usr/lib/postgresql/18/bin \
  --old-datadir=/upgrade/17 --new-datadir=/upgrade/18 \
  --old-port=50417 --new-port=50418 --socketdir=/upgrade/socket \
  --username=postgres --retain --check
# Same argv, with --copy replacing --check, only after the preflight/check pass.
```

The runner supplies `PARKING_LOT_PG_UPGRADE_URL` (owned loopback PG18 URL),
`PARKING_LOT_PG_UPGRADE_RUN_ID`, `PARKING_LOT_PG_MAJOR=18` and
`CARGO_TERM_COLOR=never` to this exact existing-suite argv:

```sh
cargo nextest run --workspace --all-features --locked --run-ignored all \
  --no-fail-fast --no-tests=fail --test-threads=1 --retries=0 \
  -E 'package(prometheus-parking-lot-pg)'
```

A missing URL/run ID, wrong marker, wrong server/checksum or wrong extension
identity is an error. Nonzero discovered tests and the named upgrade test are
required. Ordinary PG17/18 fresh-cluster success cannot replace this invocation.

### Version, checksum and removed-result controls

Each positive or removed-result rehearsal first constructs real negative
cluster fixtures. Inside the owned upgrade container, `OA14_RUN_ID` must match
`/upgrade/.oa14-owned`; the exact read-only preflight commands are:

```sh
bash /workspace/docker/postgres18/upgrade/preflight.sh /upgrade/17 /upgrade/negative-version
bash /workspace/docker/postgres18/upgrade/preflight.sh /upgrade/17 /upgrade/negative-checksum
bash /workspace/docker/postgres18/upgrade/preflight.sh /upgrade/17 /upgrade/18
```

| Actual fixture | Required result before upgrade starts |
|---|---|
| PG17 source, PG17 destination | exit 42, `OA-14:preflight-version` |
| PG17 checksum 1, PG18 checksum 0 | exit 43, `OA-14:preflight-checksum` |
| PG17 checksum 1, PG18 checksum 1 | exit 0, `OA-14:preflight-ok old=17 new=18 checksums=1` |

Wrong-path/running/malformed fixtures exit 40/41/44. Rejection tests must also
observe no `/upgrade/oa14_upgrade_started` marker. These commands are not host
PGDATA commands; the complete runner creates and owns the required directories.

For the removed-result control, rerun the complete rehearsal command with a
**new** run ID/output directory and `--control missing-result`:

```sh
bash scripts/rehearse-postgres-upgrade.sh \
  --out <NEW_ABSOLUTE_NEGATIVE_OUTPUT_DIRECTORY> --run-id <UNIQUE_NEGATIVE_RUN_ID> \
  --candidate <FULL_40_HEX_CANDIDATE> \
  --contract-sha256 <ADOPTED_CONTRACT_64_HEX_SHA256> \
  --pg17-image sha256:<64_HEX_LOCAL_PG17_ID> \
  --pg18-image sha256:<64_HEX_LOCAL_PG18_ID> \
  --upgrade-image sha256:<64_HEX_LOCAL_DUAL_BINARY_ID> --control missing-result
```

The runner adds
only `--cfg parking_lot_negative_control="oa-14-missing-result"` to `RUSTFLAGS`
and changes the Cargo filter to `-E 'binary(postgres_upgrade)'`; the rest of the
suite argv above is unchanged. It refuses inherited negative-control cfgs and
encoded Rust flags that would mask this injection. The control deletes actual
schema3 result ID 1 after physical upgrade and metadata conversion, then must
fail `OA-14:seeded-result`. It does not omit the upgrade or replace the database
with a mock. Run the normal leg with `--control none` and no control cfg.

| Classifier | Required evidence |
|---|---|
| `PASS`, wrapper exit 0 | Complete positive route, nextest exit 0, nonzero discovery and named upgrade test |
| `EXPECTED_FAILURE`, wrapper exit 0 | Removed-result route, nextest exit 100, exactly one discovered test and `OA-14:seeded-result` |
| `BLOCKED`, wrapper exit 2 | Required local tool/daemon/image or exact source server unavailable; no upgrade certification |
| `FAIL`, wrapper exit 1 | Other failures, including build exit 101, timeout, empty discovery, wrong assertion or cleanup failure |

An expected-negative wrapper exit 0 is not a positive upgrade PASS. Both receipt
status and raw process exit/assertion must be recorded.

### Receipts, retained handles and rollback restrictions

`<OUT>/receipt.json` records candidate/contract identities, fixture manifest hash,
actual image/tool observations, exact argv, timestamps, exits, nonzero discovery
and every resource handle. The effective suite environment is specified above;
retain the generated loopback port/run ID with the final boundary evidence. Numbered `.stdout.txt` and
`.stderr.txt` files retain raw command output; `source-logical-backup.json`
retains the baseline. `<OUT>/cluster/version.*.txt` and `checksum.*.txt` retain
negative preflight logs. `pg_upgrade_output.d` remains under `/work` in the
owned work volume. The backup volume contains `pg17-source.tar` and
`pg17-source.sha256`; the actual archive digest does not exist until execution.

Names are `oa14-<RUN_ID>-work`, `oa14-<RUN_ID>-backup` and
`oa14-<RUN_ID>-restore`; all carry `ai.prometheus.oa14.run=<RUN_ID>`.
Containers use the same prefix, including `-pg17`, `-pg18`, `-restore17` and
registered temporary handles. Cleanup checks the run label, captures logs and
removes only those exact containers. A timeout does not grant permission to
remove unrelated resources. All named volumes remain for inspection; after
retaining evidence and authorizing their disposal, use the exact
`resources.manual_cleanup_argv` in the receipt. For a complete run it is:

```sh
docker volume rm oa14-<RUN_ID>-work oa14-<RUN_ID>-backup oa14-<RUN_ID>-restore
```

Stop all old application **readers and writers** before physical/schema cutover,
and keep them stopped. The database guards old v1/v2 mutations; it does not make
old readers safe or reconcile their frozen result view. Do not serve traffic
from both old and new stores. Copy mode retains source data, but restarting an
old cluster after new writes would expose stale state. Rollback means stopping
new writers and restoring the untouched pre-upgrade archive into a separate
isolated PG17 volume, then reconciling post-cutover writes explicitly. Never
run old binaries on converted data, downgrade schema3 or treat a raw source copy
as a current replica. This fixture restore proves only the pre-upgrade baseline.
Dump/restore or logical replication remain other major-upgrade strategies [S1];
they do not remove the mailbox owner/status proof requirement.

## Running the Postgres tests locally

```text
# build the image once per machine (the helper builds it if missing)
docker build -f docker/postgres18/Dockerfile -t parking-lot-pg:18 docker/
PARKING_LOT_PG_MAJOR=18 cargo nextest run --workspace --all-features --locked \
  --run-ignored all --no-tests=fail \
  -E '(package(prometheus-parking-lot-pg) & not binary(postgres_upgrade)) | test(config_postgres_example_builds_against_pg18)'
```

`PARKING_LOT_PG_MAJOR=17` selects the 17 image. `PARKING_LOT_PG_REBUILD=1`
forces an image rebuild after editing a Dockerfile. Both ordinary PG17 and PG18
ignored legs must exclude `binary(postgres_upgrade)`. The platform-owned CI
patch is `docker/postgres18/upgrade/handoffs/ci-postgres-upgrade-exclusion.patch`;
its held CI base SHA256 is
`c334b530ed7cfdf4603829ed291cb0fb4b5363bd043ce7e4ac8524783dd1f94d`.
OA-18 must apply the same exclusion to its ordinary legs, then explicitly run
the full OA-14 runner with the marked upgraded URL supplied by that runner.

## Sources

Prior runbook sources retrieved 2026-10-23. OA-14 primary-source reads on
2026-10-24 are recorded in the fixture manifest; no runtime outcomes were
measured during this documentation handoff.

- **S1** PostgreSQL 18 release notes, <https://www.postgresql.org/docs/18/release-18.html>
  (E.6.2 Migration to Version 18: initdb checksums default, `--no-data-checksums`,
  pg_upgrade checksum matching, MD5 deprecation and `md5_password_warnings`;
  E.6.3: `autovacuum_vacuum_max_threshold`, `autovacuum_worker_slots`, `total_*vacuum_time`
  columns, `pg_stat_io` bytes, pg_upgrade statistics retention and `--swap`, AIO `io_method`).
- **S2** PostgreSQL 18 `CREATE TABLE`, storage parameters,
  <https://www.postgresql.org/docs/18/sql-createtable.html> (autovacuum per-table
  parameters; `toast.` prefix and TOAST inheritance of the table's values), and
  the autovacuum trigger formula in
  <https://www.postgresql.org/docs/18/routine-vacuuming.html#AUTOVACUUM>.
- **S3** Docker Official Image `postgres`, docs source
  <https://github.com/docker-library/docs/blob/master/postgres/content.md>
  (sections `PGDATA`, `POSTGRES_HOST_AUTH_METHOD`, `POSTGRES_INITDB_ARGS`,
  Initialization scripts) and the 18 Dockerfile
  <https://github.com/docker-library/postgres/blob/master/18/bookworm/Dockerfile>
  (`ENV PGDATA /var/lib/postgresql/18/docker`, `VOLUME /var/lib/postgresql`).
  Read from a clone at commit `c57c402` (2026-10-22).
- **S4** pgmq `INSTALLATION.md` and `README.md` at tag v1.13.1 (commit `8eab182`),
  <https://github.com/pgmq/pgmq/blob/v1.13.1/INSTALLATION.md> (extension from
  source or PGXN, SQL-only alternative, "Supported on Postgres 14-18").
- **S5** pgmq `UPDATING.md` at v1.13.1 (`ALTER EXTENSION pgmq UPDATE`).
- **S6** PostgreSQL 18 `pg_upgrade`, <https://www.postgresql.org/docs/18/pgupgrade.html>
  (`--check`, `--link`, `--swap`); `pg_checksums`,
  <https://www.postgresql.org/docs/18/app-pgchecksums.html>.

The prior runbook records the source retrievals above. The
autovacuum trigger on 18 is `min(vacuum max threshold, base threshold + scale
factor * tuples)` (routine-vacuuming page); on 17 there is no max term.
`pg_checksums` requires a cleanly shut-down server (S6). The existing
`autovacuum_reclaims_a_churned_lease_queue` regression and its source control
remain in the boundary suite; this OA-14 handoff does not claim a new run.

OA-14 source references (read 2026-10-24):

- PG17 and PG18 `initdb`: <https://www.postgresql.org/docs/17/app-initdb.html>,
  <https://www.postgresql.org/docs/18/app-initdb.html>.
- `pg_controldata`: <https://www.postgresql.org/docs/18/app-pgcontroldata.html>.
- Official image package definitions:
  <https://raw.githubusercontent.com/docker-library/postgres/master/17/bookworm/Dockerfile>,
  <https://raw.githubusercontent.com/docker-library/postgres/master/18/bookworm/Dockerfile>.
- pgmq 1.13.1 extension metadata/PGXS installation:
  <https://raw.githubusercontent.com/pgmq/pgmq/v1.13.1/pgmq-extension/pgmq.control>,
  <https://raw.githubusercontent.com/pgmq/pgmq/v1.13.1/pgmq-extension/Makefile>.
- nextest flags and process exit classifications:
  <https://nexte.st/docs/running/>,
  <https://docs.rs/nextest-metadata/latest/nextest_metadata/enum.NextestExitCode.html>.
