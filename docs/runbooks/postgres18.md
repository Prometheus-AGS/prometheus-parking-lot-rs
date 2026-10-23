# Runbook: PostgreSQL 18 (and 17) for the parking-lot Postgres backends

Scope: operating `PgLeaseQueue`, `PgResultMailbox` and `PgmqLeaseQueue`
(`crates/parking-lot-pg`) on PostgreSQL 17 and 18. CI runs every Docker-gated
suite on both majors (`postgres` job in `.github/workflows/ci.yml`, cr-36).
Facts about PostgreSQL and the official Docker image are cited in
[Sources](#sources); facts about this repository were checked against the
`parking-lot-pg:17` and `parking-lot-pg:18` images built from `docker/`.

## What differs between 17 and 18

Observed on the project images (`SHOW` / `docker inspect`):

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

There is no separate migration tool. Each type applies its idempotent script
with one multi-statement call (one implicit transaction), safe to run at every
start and from several processes at once (each script takes an advisory lock
first):

| Call | Script |
|---|---|
| `PgLeaseQueue::migrate(&pool)` | `crates/parking-lot-pg/migrations/004_lease_queue.sql` |
| `PgResultMailbox::migrate(&pool)` | `005_result_mailbox.sql` |
| `PgmqLeaseQueue::prepare()` (or `migrate`, then create queues) | `006_pgmq_lease_queue.sql`, then `pgmq.create` and `pl_pgmq_tune_queue` per queue |
| `PostgresAuditSink::migrate(&pool)` | `007_audit_events.sql` |

The facade runs them for a configured Postgres backend. Manual application
(`--single-transaction` keeps the advisory lock and `lock_timeout` in force for
the whole script, as one call does):
`psql -v ON_ERROR_STOP=1 --single-transaction -f crates/parking-lot-pg/migrations/004_lease_queue.sql`
(and 005, 006, 007).

Because the scripts are re-applied, an upgrade of this library that changes
storage parameters (cr-36 did) takes effect the first time the new version
calls `migrate`/`prepare`.

### Locks taken by a re-run

A re-run takes **no table lock stronger than the catalog read it needs**. Three
statements would, even when there is nothing to change, so each is guarded by a
catalog check and runs only when the object is missing or the option differs
(observed on the PG 17 and 18 images by holding an open transaction that had
written to the table, see `crates/parking-lot-pg/tests/migrations.rs`):

| Statement | Lock | Effect while held |
|---|---|---|
| `CREATE [UNIQUE] INDEX IF NOT EXISTS` | `SHARE` on the table, taken before the index is looked up | blocks every writer |
| `DROP TRIGGER` / `CREATE TRIGGER` | `ACCESS EXCLUSIVE` | blocks readers too |
| `CREATE OR REPLACE TRIGGER` | `SHARE ROW EXCLUSIVE` | blocks every writer |
| `ALTER TABLE ... SET (...)` | `SHARE UPDATE EXCLUSIVE` | does not block reads or writes |

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

### Settings (migrations 004, 005, 006; pgmq queues via `prepare()`)

| Table | `autovacuum_vacuum_scale_factor` / `_threshold` | `autovacuum_analyze_scale_factor` / `_threshold` |
|---|---|---|
| `pl_lease_queue` | 0 / 1000 | 0 / 1000 |
| `pl_result_mailbox` | 0 / 1000 | 0 / 1000 |
| `pl_result_mailbox_keys` | 0 / 500 | 0 / 500 |
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
 WHERE c.relname IN ('pl_lease_queue','pl_result_mailbox','pl_result_mailbox_keys','pl_pgmq_aux')
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
 WHERE relname IN ('pl_lease_queue','pl_result_mailbox','pl_result_mailbox_keys','pl_pgmq_aux')
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

## 7. Upgrading 17 to 18

Order, with the library's constraints:

1. **Drain.** Stop dispatchers and wait for leases to expire or be acked. The
   lease queue is at-least-once with fenced leases, so a task in flight at
   cutover is redelivered after `lease_expires_at`; a brief outage loses no
   accepted work, but a redelivered task runs again.
2. **Prepare the new binaries.** Install PostgreSQL 18 and pgmq for 18 (section
   4); extensions the old cluster uses must exist in the new installation.
3. **Checksums.** Make old and new match (section 1).
4. **Check.** `pg_upgrade --check` reads both clusters without changing data
   [S6].
5. **Upgrade.** `pg_upgrade` copies by default; `--link` hard-links (fast, but the
   old cluster is unusable once the new one has started, and both directories must
   be on one file system); 18 adds `--swap`, which can be faster still and may
   destructively modify the old cluster if it fails late [S6]. In Docker, the 18
   volume layout (section 2) exists so that both versions' directories sit under
   one mount for `--link`.
6. **Statistics.** 18's `pg_upgrade` retains optimizer statistics [S1] (extended
   statistics are not preserved), so the post-upgrade `ANALYZE` pass is shorter;
   still run `vacuumdb --all --analyze-in-stages --missing-stats-only` if the
   tool is available.
7. **Re-apply the library schema** by starting the application (it calls
   `migrate`/`prepare`) or with `psql` (section 5). It is a no-op except for
   anything new.
8. **Roles.** Reset any MD5 passwords (section 3).
9. **Smoke.** Enqueue, lease and ack one task; deliver and consume one mailbox
   entry. The CI matrix runs the full suites on 17 and 18 against the project
   images (`PARKING_LOT_PG_MAJOR=17|18 cargo nextest run -p prometheus-parking-lot-pg
   --run-ignored all`).

Rollback: with copy mode, restart the old cluster. With `--link` or `--swap`,
rollback is only possible from a backup taken before the upgrade, so take one
(`pg_basebackup` or a volume snapshot) first.

Dump and restore, or logical replication, are the other supported routes
between majors [S1]; they avoid the checksum and volume constraints at the cost
of a longer cutover.

## Running the Postgres tests locally

```text
# build the image once per machine (the helper builds it if missing)
docker build -f docker/postgres18/Dockerfile -t parking-lot-pg:18 docker/
PARKING_LOT_PG_MAJOR=18 cargo nextest run --workspace --all-features --locked \
  --run-ignored all -E 'package(prometheus-parking-lot-pg) | test(config_postgres_example_builds_against_pg18)'
```

`PARKING_LOT_PG_MAJOR=17` selects the 17 image. `PARKING_LOT_PG_REBUILD=1`
forces an image rebuild after editing a Dockerfile.

## Sources

Retrieved 2026-10-23.

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

All citations above were fetched from the sources named in this session. The
autovacuum trigger on 18 is `min(vacuum max threshold, base threshold + scale
factor * tuples)` (routine-vacuuming page); on 17 there is no max term.
`pg_checksums` requires a cleanly shut-down server (S6). The behaviour of the
settings was confirmed by `autovacuum_reclaims_a_churned_lease_queue`, which
fails (autovacuum never starts) when the `ALTER TABLE` is removed.
