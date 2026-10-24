# OA-12 negative-control boundary runbook

Status: **NOT_EXERCISED**. This runbook describes authored source and future
commands. No matrix, runner, build, discovery, test, browser, Docker operation,
syntax check, scan or review was executed to prepare it. The uncomfortable
limit is that a checked inventory and plausible assertion classifier can still
be wrong until the complete real boundary runs.

The source of truth is [negative-controls.json](../../negative-controls.json),
used by [negative-controls.py](negative-controls.py) and the three additive
OA-12 jobs in [ci.yml](../../workflows/ci.yml). The implementation handoff is
[oa-12.md](../../../.kbd-orchestrator/phases/phase-0-4-operational-assurance/evidence/implementation/oa-12.md).
Execution belongs to OA-18 after all production work and the independent review
boundary. [Operator gates](../../../.kbd-orchestrator/phases/phase-0-4-operational-assurance/operator-gates.md)
continue to apply. Source completion supplies no release/publication approval.

## Inventory and identities

The task-1 manifest contains **33 actual cfg values, 52 execution rows and 99
source-file SHA256 identities**. `source_sites` enumerates actual Rust paths,
including fixture cfg sites, rather than a sequential scenario range. At the
boundary the runner reads committed `*.rs` paths with `git ls-files`, removes
comments while preserving Rust strings, and requires exact cfg/path equality.
It also requires the case control set to equal the discovered cfg set and
checks each of the 99 `source_sha256` entries. A new or removed site, missing
ID, duplicate case ID or stale byte identity fails inventory.

The execution ID is a matrix/artifact/directory key. The `control` field is the
literal value injected into `parking_lot_negative_control`; they are not
interchangeable. In particular, `s-20-at-depth` injects **s-20** into the
at-depth model, whereas execution ID `s-20-depth` injects the distinct
**s-20-depth** variant. `s-20-drain` also injects s-20. PG17/PG18/redb/browser
suffixes distinguish actual collaborators while preserving the cfg value.
`oa-07` changes backend stored-status handling; `oa-07-event` changes runtime
terminal-event reporting with normal backend cfg. Never combine those controls.

The exact 33 values are:

`oa-06`, `oa-07`, `oa-07-event`, `oa-08`, `oa-09`, `oa-10-expiry`, `oa-11-kind-release`, `oa-11-ledger-order`, `oa-13-budget-event`, `oa-14-missing-result`, `oa-16`, `s-1`, `s-10`, `s-11`, `s-16`, `s-17`, `s-18`, `s-19`, `s-2`, `s-20`, `s-20-depth`, `s-20-order`, `s-21`, `s-22`, `s-3`, `s-33`, `s-4`, `s-5`, `s-5-fencing`, `s-6`, `s-7`, `s-8`, `s-9`.

## What must actually execute

Each non-upgrade row runs both `normal` and `mutant` modes. Each mode first
builds the selected real targets, then discovers the exact designated oracle
and unaffected companion identities, then executes each selected test in its
own process. Every normal oracle and companion must pass. In mutant mode each
designated oracle must fail its specified assertion, and every companion must
still pass. All required identities are in the case table below; none may be
omitted. OA-12 is a discriminating control packet; the full unmodified workspace,
backend and platform baseline remains a separate OA-18 requirement.

All Cargo processes within one invocation are sequential. Targets are isolated
at `target/oa12/<case-id>/normal` and `target/oa12/<case-id>/mutant`.
Normal `RUSTFLAGS` is empty except Loom's `--cfg parking_lot_loom`.
Mutants add exactly `--cfg parking_lot_negative_control="<control>"`.
Loom retains `LOOM_MAX_PREEMPTIONS=3`; its bounded schedules do not prove all
possible hardware schedules. The incumbent Loom, OA-11, Node pinned/MSRV and
OA-13 browser jobs remain in CI alongside the stricter OA-12 aggregate.

| Adapter | Build and discovery prerequisites | Required runtime classifier |
|---|---|---|
| Native/redb/PG/Iggy/Loom | Separate nextest `run --no-run` exits 0; `list --message-format json` exits 0; nonempty actual listing includes every exact package/binary/test tuple | Normal/companion: exit 0, named `PASS`, summary one test run/one passed. Mutant oracle: nextest exit **100**, named `FAIL`, summary one test run/one failed, `panicked at`, and all designated diagnostic fragments. Where `diagnostic_any` exists, one listed alternative is also mandatory |
| Browser | Real wasm32 target `test --no-run` exits 0; `-- --list` exits 0 and matches the exact OA-13 target export set (15 pool or 6 events) including the selected name | Actual `Running headless tests in Chrome`, no Node context marker, exactly one selected result. Normal/companion: Cargo exit 0 and `ok`; mutant: Cargo exit **101**, named per-test **FAIL**, summary 0 passed/1 failed, all named diagnostic fragments |
| Upgrade | Both cfg modes separately compile/discover exactly `postgres_upgrade::postgres_upgrade_populated_0_3_to_schema3` before the delegate. The delegate then creates the real run-owned upgraded cluster | Outer owner adapter exits **0** only after its own classification. Normal inner suite exits 0 with real populated upgrade/independent backup restore PASS. Mutant inner nextest exits **100**, exactly one discovered test, its named FAIL/panic and `OA-14:seeded-result`, owner receipt `EXPECTED_FAILURE` |

Native compilation exit 101 is not nextest test-failure exit 100. Browser Cargo
101 is accepted only after the separate successful build and real browser
identity/result/diagnostic checks. An upgrade wrapper's 0 means it classified
its inner outcome; it does not mean the defective upgrade fixture passed.
The owner adapter supplies the upgrade control only after preparing its marked
cluster. OA-12 clears its prebuild injection before delegating, because the
adapter rejects inherited controls/encoded flags.

Compile failures, zero/missing discovery, unsupported flags, setup/backend
errors, arbitrary nonzero exits, a runner watchdog or an unrelated assertion
cannot satisfy an expected negative. The process-group watchdog terminates
only its owned command group and records `TIMEOUT`, which fails the case.
Existing lease-loss assertions have their own bounded observation windows;
the specific source oracle and test identity remain mandatory. A bare deadline
message or generic failed test is insufficient.

## Exact future entry points

These commands have **not been run**. Start from the root of a clean committed
frozen candidate. All new source/fixture/docs paths must be included in that
candidate before the runner's clean-tree check. Use a private fresh evidence
root outside the repository; each case output directory and the aggregate JSON
must not already exist. Store the complete 40-character candidate identity and
adopted contract digest `e77aa4ac91ad92082a6cef288cd5f9b9636b6101faa65237dbe16116fff64257`.
Ordinary PG cases must not inherit `PARKING_LOT_PG_UPGRADE_URL` or
`PARKING_LOT_PG_UPGRADE_RUN_ID`; the upgrade delegate supplies those itself for
its newly owned cluster.

```bash
python3 .github/scripts/tests/negative-controls.py matrix

python3 .github/scripts/tests/negative-controls.py run \
  --case oa-08 --output "$OA12_PACKET/oa-08"

python3 .github/scripts/tests/negative-controls.py run \
  --case s-4-browser --output "$OA12_PACKET/s-4-browser"

python3 .github/scripts/tests/negative-controls.py run \
  --case oa-14-missing-result --output "$OA12_PACKET/oa-14-missing-result" \
  --pg17-image "$APPROVED_PG17_IMAGE_ID" \
  --pg18-image "$APPROVED_PG18_IMAGE_ID" \
  --upgrade-image "$APPROVED_UPGRADE_IMAGE_ID"

python3 .github/scripts/tests/negative-controls.py aggregate \
  --receipts "$OA12_PACKET" --candidate "$FROZEN_CANDIDATE" \
  --output "$OA12_SUMMARY"
```

`OA12_PACKET` must be an absolute private external directory. Populate it with
**all 52 case-ID subdirectories**, not only the three illustrative invocations.
The source-generated CI matrix calls `run` once for each row and serializes
Cargo in that job; matrix `max-parallel` is 4 across separate job checkouts.
For a local complete packet, the OA-18 owner enumerates the same manifest rows
and awaits each invocation; do not run concurrent Cargo commands in one target
or reuse evidence directories. Matrix generation checks source inventory but
is not test or discovery evidence.

The wrapper expands the exact native command forms below for each package and
union of its selected targets. A library target uses `--lib`; integration
binaries use repeated `--test <target>`. Values come from the checked case,
not a substring guessed from an issue number:

```bash
cargo +1.99.1 nextest run -p "$PACKAGE" --locked --all-features \
  --test "$TARGET" --no-run
cargo +1.99.1 nextest list -p "$PACKAGE" --locked --all-features \
  --test "$TARGET" --message-format json
cargo +1.99.1 nextest run -p "$PACKAGE" --locked --all-features \
  --test "$TARGET" --run-ignored all --no-fail-fast --no-tests=fail \
  --test-threads=1 --retries=0 --status-level all \
  --failure-output immediate --success-output immediate --show-progress none \
  -E "binary(=$TARGET) & test(=$EXACT_TEST_NAME)"
```

Each browser test uses these forms with the checked target/name, once per mode;
there is no invented `--test-threads` flag for wasm-bindgen:

```bash
cargo +1.99.1 test --locked --target wasm32-unknown-unknown \
  -p prometheus-parking-lot-wasm --test "$TARGET" --no-run
cargo +1.99.1 test --locked --target wasm32-unknown-unknown \
  -p prometheus-parking-lot-wasm --test "$TARGET" -- --list
cargo +1.99.1 test --locked --target wasm32-unknown-unknown \
  -p prometheus-parking-lot-wasm --test "$TARGET" -- "$EXACT_TEST_NAME" --nocapture
```

The pinned runner supports substring filters; the classifier requires exactly
one named real result, so an accidental extra match fails. The separate full
OA-13 packet still requires all 15+6 executions, actual host markers and its
five unaffected event peers. Node-only discovery/execution cannot substitute
for any OA-12 browser row. The locked core/WASM no-tokio graph gate also runs.

The upgrade delegate is [rehearse-postgres-upgrade.sh](../../../scripts/rehearse-postgres-upgrade.sh),
with the [OA-14 runbook](../../../docs/runbooks/postgres18.md).
OA-12 invokes it twice with unique `oa12-<candidate-prefix>-<nonce>-normal|mutant`
run IDs and full candidate/contract arguments. Its private output subdirectories
are `upgrade-normal` and `upgrade-mutant`; only mutant adds `--control missing-result`.
The full SHA, not the human-readable run-ID prefix, binds the candidate.
The mutant delegate's actual suite argv is:

```bash
cargo nextest run --workspace --all-features --locked --run-ignored all \
  --no-fail-fast --no-tests=fail --test-threads=1 --retries=0 \
  -E 'binary(postgres_upgrade)'
```

It supplies the freshly upgraded loopback URL, run ID, PG major 18 and exactly
`--cfg parking_lot_negative_control="oa-14-missing-result"`. Normal uses
`-E 'package(prometheus-parking-lot-pg)'` on that real upgraded cluster and
checks the independent restored PG17 backup. Never supply an ordinary PG URL
or hand-create an `EXPECTED_FAILURE` JSON in place of the adapter.

## Prerequisites and failure classification

Use an approved quiet Linux POSIX runner, Git, Python 3.11+, preinstalled Rust
1.99.1 and nextest 0.9.149. WASM rows additionally need the installed
`wasm32-unknown-unknown` target, wasm-bindgen-test-runner 0.2.129 matching
Cargo.lock (test crate 0.3.79), and actual Linux x86_64 Chrome for Testing plus
ChromeDriver 156.0.8078.47. Set `WASM_BROWSER_BIN` and `CHROMEDRIVER` to their
existing absolute executable paths. Browser paths/versions and SHA256 are
captured. `WASM_BINDGEN_USE_BROWSER=1`, local driver and generated
`goog:chromeOptions.binary` capabilities select the actual headless path;
competing WASM/driver overrides and `NO_HEADLESS` are cleared. Native Windows,
workers, remote WebDriver, Safari, Firefox, Edge and Node substitutes do not
provide the supported Chromium receipt. Other native POSIX contexts have not
been certified by this authored Linux matrix.

PG17/PG18 rows need real Docker plus the corresponding project-built
`parking-lot-pg:17` or `:18` image, with unchanged digest-pinned project
Dockerfiles/pgmq sources. CI builds with context `docker`, `load: true`,
`push: false`; local runner image absence is BLOCKED, not an instruction to
install or silently use another database. Iggy needs its real source-pinned
`apache/iggy:0.9.0@sha256:b35fc2841964f2962a5f164d33b3ae3a9de99a66920defd3cd8a1c039470602d`
server and the test helper's SYS_NICE/seccomp/memlock capabilities for its owned
containers. redb cases use actual private temporary files and real process
kill/restart paths, not fake durable queues.

The upgrade row requires approved prebuilt local `sha256:<64 hex>` IDs for
project PG17, project PG18 and the dual-binary image. They must satisfy OA-14's
source labels, exact base ancestry and version/checksum assertions (PG17.11,
PG18.6, pgmq 1.13.1). The delegate retains owned backup/data volumes and records
cleanup handles. No global install, host tuning, arbitrary volume deletion or
operator database mutation is authorized by this runbook.

CI inputs are `WASM_BROWSER_RUNNER`, `WASM_CHROME_BIN`,
`WASM_CHROMEDRIVER_BIN`, `PG_UPGRADE_RUNNER`, `PG_UPGRADE_PG17_IMAGE_ID`,
`PG_UPGRADE_PG18_IMAGE_ID`, and `PG_UPGRADE_IMAGE_ID`. Image IDs can instead be
passed to the wrapper explicitly or via `OA12_PG17_IMAGE`, `OA12_PG18_IMAGE`,
`OA12_UPGRADE_IMAGE`. Declared CI tool/install/image steps have not executed;
this documentation does not prove runner access or authorize dispatch.

Explicit missing prerequisites return runner **2 / BLOCKED**. Examples are a
missing tool, unavailable Docker daemon/project image, missing browser path or
absent approved upgrade image IDs. Observed source/hash/version mismatches,
compile/discovery/runtime/classifier failures and timeouts return **1 / FAIL**.
Some lower-level unexpected OS/file errors are also FAIL; do not relabel them
as an expected control. Neither BLOCKED nor FAIL satisfies OA-12. A successful
case/aggregate exits 0 only after its actual required checks.

## Evidence, aggregation and verifier controls

Each case output retains numbered raw `.stdout.txt` and `.stderr.txt`,
`commands.json`, and `receipt.json`. Commands bind exact argv, selected
non-secret environment (`RUSTFLAGS`, target directory, toolchain, Loom bound,
PG major and browser selection), actual exit, and both log SHA256 values.
The environment is not a full dump or a claim of complete hermeticity. Receipts
also bind candidate, contract, runner/manifest hashes, actual OS/machine,
Python/tool identities and case proofs. Image inspections retain ID/digests/
labels without dumping image environment. Upgrade proofs hash their delegated
receipt and raw evidence files. Preserve private originals; if redaction is
needed, retain separately labelled derivatives without replacing hashed logs.

The aggregate requires exactly all 52 case directories, matching source/runner/
manifest/candidate/contract identities, successful build/discovery proofs,
actual named normal/mutant outcomes and unchanged raw logs. It replays runtime
classifiers against those logs and checks exact isolated injection flags.
A single passing row or authored `execution` field cannot certify the matrix.
Trusted CI/environment/artifact provenance is separately established by the
OA-18/OA-04 boundary producer; local JSON alone does not authorize publication.

Two discriminating verifier controls execute only after the complete actual
packet is accepted. Omission removes one row from the actual receipt map and
requires the same `complete()` predicate to raise `OA-12:omission`. PASS
substitution takes an actual **normal PASS of the same designated native
oracle**, preserving its real exit 0 and raw logs, and feeds it into the same
expected-mutant classifier. That must raise `OA-12:exit`; no fabricated exit
code or synthetic passing-test log is used. This tests rejection of a passing
expected control; it does not claim a mutant was actually executed with an
invented outcome. The aggregate records both as `EXPECTED_FAILURE`, including
the substitution's source case, oracle, actual exit and raw-log hash.

## Final repairs and historical identities

The original task-1 source manifest binds 99 pre-boundary source byte hashes.
Final formatting, approved source repairs or shared-manifest changes can make
those identities stale. Preserve the original task-1 handoff and its hashes as
historical authored evidence. After the complete repair set, the authorized
owner must record a source diff/reason, refresh only affected `source_sha256`
entries from the actual approved bytes, update cfg/sites/cases if behavior or
identities changed, and record the revised manifest/runner/CI identities before
freezing the final committed candidate. Do not remove the hash checks, retain
old bytes under a new claim, or edit historical receipts to make a candidate
appear unchanged. OA-14 has its own nested source/fixture identity manifest;
coordinate any affected refresh with that owner as well.

The wrapper intentionally refuses dirty/untracked candidates and stale source
identities. Complete the new candidate freeze before rerunning the whole
required real boundary. A formatter alone supplies no control result. No hash
refresh, formatter, execution or candidate commit was performed by this docs
task; the tables below describe the original authored task-1 package.

## Exact authored cases and source classifiers

Package abbreviations: C = core, R = runtime-tokio, F = facade, P = PG,
E = embedded/redb, I = Iggy, W = WASM. The JSON records full package names.
Every `target::test` below is literal; diagnostics separated by `AND` are all
required. An `OR` group is the manifest's explicit `diagnostic_any`, not generic
panic acceptance. The table is a static rendering of the authored manifest,
not observed discovery or execution.

| Case ID | cfg / adapter / package | Designated oracle identity and required diagnostic | Unaffected mutant companion |
|---|---|---|---|
| `s-1` | `s-1` / loom / C | `loom_capacity::loom_capacity_never_exceeded_and_no_job_lost` → `capacity invariant violated:` | `unit::util_test::priority_ordering` |
| `s-2` | `s-2` / loom / C | `loom_wakeup::loom_wakeup_completion_racing_a_park_is_not_lost` → `lost wakeup: a job is parked` | `unit::util_test::priority_ordering` |
| `s-3` | `s-3` / native / R | `panic_test::panic_resource_pool_one_in_100_is_contained` → `OA12:S3: executor panic escaped the ResourcePool spawned future` | `budget_test::budget_resource_pool_in_budget_task_is_not_cancelled` |
| `s-4-resource` | `s-4` / native / R | `execution_timeout_test::work_exceeding_the_budget_is_cancelled` → `the body must have been CANCELLED` | `execution_timeout_test::work_within_the_budget_completes_normally` |
| `s-4-worker` | `s-4` / native / R | `budget_test::budget_worker_pool_wedged_executor_is_cancelled` → `OA12:S4:native-worker-budget` | `budget_test::budget_worker_pool_in_budget_task_is_not_cancelled` |
| `s-4-browser` | `s-4` / browser / W | `wasm_pool::budget_cancels_a_wedged_executor` → `OA12:S4:wasm-budget: executor future was not dropped` | `wasm_pool::smoke_submit_and_retrieve` |
| `s-5-pg17` | `s-5` / pg 17 / P | `lease_queue::sigkill_redelivery` → `the task was never redelivered after the dead consumer's lease expired` | `lease_queue::undecodable_entry_does_not_wedge_the_queue` |
| `s-5-fencing-pg17` | `s-5-fencing` / pg 17 / P | `lease_queue::sigkill_redelivery` → `queue.ack(killed.token).await` AND `SchedulerError::LeaseLost` | `lease_queue::notify_wakes_an_idle_dispatcher` |
| `s-6-pg17` | `s-6` / pg 17 / P | `lease_queue::concurrent_dispatchers` → `a task was leased twice` | `lease_queue::sigkill_redelivery` |
| `s-7-pg17` | `s-7` / pg 17 / P | `mailbox::mailbox_consume_once_race` → `consumers received the one entry` | `mailbox::mailbox_first_write_wins_race` |
| `s-18-pg17` | `s-18` / pg 17 / P | `pgmq::pgmq_priority_order` → `right: ["high-1", "high-2", "normal-1", "low-1", "low-2"]` | `pgmq::pgmq_stale_token_is_rejected` |
| `oa-06-pg17` | `oa-06` / pg 17 / P | `mailbox_migration::mailbox_scoped_fences_survive_reconnect_and_expire_independently` → `OA-06:address-scope oracle` | `mailbox::mailbox_consume_once_race` |
| `oa-07-pg17` | `oa-07` / pg 17 / P | `mailbox_status::mailbox_status_direct_and_v2_refuse_atomically_then_preserve_failed_provenance` → `OA-07:stored-status` | `mailbox::mailbox_consume_once_race` |
| `s-5-pg18` | `s-5` / pg 18 / P | `lease_queue::sigkill_redelivery` → `the task was never redelivered after the dead consumer's lease expired` | `lease_queue::undecodable_entry_does_not_wedge_the_queue` |
| `s-5-fencing-pg18` | `s-5-fencing` / pg 18 / P | `lease_queue::sigkill_redelivery` → `queue.ack(killed.token).await` AND `SchedulerError::LeaseLost` | `lease_queue::notify_wakes_an_idle_dispatcher` |
| `s-6-pg18` | `s-6` / pg 18 / P | `lease_queue::concurrent_dispatchers` → `a task was leased twice` | `lease_queue::sigkill_redelivery` |
| `s-7-pg18` | `s-7` / pg 18 / P | `mailbox::mailbox_consume_once_race` → `consumers received the one entry` | `mailbox::mailbox_first_write_wins_race` |
| `s-18-pg18` | `s-18` / pg 18 / P | `pgmq::pgmq_priority_order` → `right: ["high-1", "high-2", "normal-1", "low-1", "low-2"]` | `pgmq::pgmq_stale_token_is_rejected` |
| `oa-06-pg18` | `oa-06` / pg 18 / P | `mailbox_migration::mailbox_scoped_fences_survive_reconnect_and_expire_independently` → `OA-06:address-scope oracle` | `mailbox::mailbox_consume_once_race` |
| `oa-07-pg18` | `oa-07` / pg 18 / P | `mailbox_status::mailbox_status_direct_and_v2_refuse_atomically_then_preserve_failed_provenance` → `OA-07:stored-status` | `mailbox::mailbox_consume_once_race` |
| `s-8` | `s-8` / redb / E | `crash_points::crash_point_between_enqueue_a_and_enqueue_b` → `between:ENQUEUED a: ACCEPTED TASK LOST: a was enqueued, never acked, yet is gone` | `restart::second_open_of_the_same_file_is_an_error` |
| `s-9` | `s-9` / native / R | `shutdown_test::shutdown_worker_pool_drain_accounts_for_every_task` → `tasks with neither an outcome nor a capture (lost):` | `shutdown_test::shutdown_worker_pool_cancel_immediately_accounts_for_every_task` |
| `s-10` | `s-10` / native / C | `prometheus_parking_lot_core::mailbox::storage_tests::storage_mailbox_notifier_failure_every_call_still_stores_and_consumes` → `a failing notifier must not fail delivery` | `prometheus_parking_lot_core::mailbox::storage_tests::storage_mailbox_storage_only_has_no_notifier_cost` |
| `s-11` | `s-11` / iggy / I | `iggy_events::iggy_same_idempotency_key_yields_one_message_id` → `one message id for one idempotency key:`<br>`iggy_events::iggy_paused_server_does_not_slow_dispatch` → `on_event took` AND `with the server paused` | `iggy_events::iggy_retention_option_sets_the_topic_expiry` |
| `s-16-redb` | `s-16` / redb / F | `scheduler::durable_sigkill::redb::pool_sigkill_redb_between_execution_and_delivery` → `no result after restart: the accepted task was lost` | `scheduler::durable_sigkill::redb::pool_sigkill_redb_mid_execution` |
| `oa-07-event-redb` | `oa-07-event` / redb / F | `scheduler::durable_sigkill::redb::pool_sigkill_redb_failed_delivery_retained_after_restart` → `OA-07:terminal-event`<br>`scheduler::durable_sigkill::redb::pool_sigkill_redb_failed_delivery_consumed_before_restart` → `OA-07:terminal-event` | `scheduler::durable_sigkill::redb::pool_sigkill_redb_between_delivery_and_ack` |
| `s-16-pg17` | `s-16` / pg 17 / F | `scheduler::durable_sigkill::pg::pool_sigkill_pg_between_execution_and_delivery` → `no result after restart: the accepted task was lost` | `scheduler::durable_sigkill::pg::pool_sigkill_pg_mid_execution` |
| `oa-07-event-pg17` | `oa-07-event` / pg 17 / F | `scheduler::durable_sigkill::pg::pool_sigkill_pg_failed_delivery_retained_after_restart` → `OA-07:terminal-event`<br>`scheduler::durable_sigkill::pg::pool_sigkill_pg_failed_delivery_consumed_before_restart` → `OA-07:terminal-event` | `scheduler::durable_sigkill::pg::pool_sigkill_pg_between_delivery_and_ack` |
| `s-16-pg18` | `s-16` / pg 18 / F | `scheduler::durable_sigkill::pg::pool_sigkill_pg_between_execution_and_delivery` → `no result after restart: the accepted task was lost` | `scheduler::durable_sigkill::pg::pool_sigkill_pg_mid_execution` |
| `oa-07-event-pg18` | `oa-07-event` / pg 18 / F | `scheduler::durable_sigkill::pg::pool_sigkill_pg_failed_delivery_retained_after_restart` → `OA-07:terminal-event`<br>`scheduler::durable_sigkill::pg::pool_sigkill_pg_failed_delivery_consumed_before_restart` → `OA-07:terminal-event` | `scheduler::durable_sigkill::pg::pool_sigkill_pg_between_delivery_and_ack` |
| `s-16-ordering` | `s-16` / native / F | `scheduler::durable_dispatch::durable_ack_after_delivery_orders_every_outcome` → `lease was resolved before its outcome was delivered` | `unit::config_test::config_pool_limits_are_validated` |
| `s-17` | `s-17` / native / R | `result_slots_test::waiters_no_blocking_threads` → `OA12:S17: pending retrieve_async started a blocking thread` | `result_slots_test::waiters_no_blocking_threads_unretrieved_result_expires_after_ttl` |
| `s-19` | `s-19` / loom / C | `loom_vector::loom_vector_no_schedule_exceeds_any_dimension` → `capacity violated in` | `unit::util_test::priority_ordering` |
| `s-20-drain` | `s-20` / loom / C | `loom_skip_ahead::loom_skip_ahead_races_completion_and_keeps_every_invariant` → (`starvation bound violated:` OR `the guard did not engage`) | `unit::util_test::priority_ordering` |
| `s-20-at-depth` | `s-20` / loom / C | `loom_at_depth::loom_at_depth_skip_races_completion_and_keeps_every_invariant` → `starvation bound violated:` | `unit::util_test::priority_ordering` |
| `s-20-depth` | `s-20-depth` / loom / C | `loom_at_depth::loom_at_depth_skip_races_completion_and_keeps_every_invariant` → `starvation bound violated:` | `unit::util_test::priority_ordering` |
| `s-20-order` | `s-20-order` / loom / C | `loom_at_depth::loom_at_depth_leaves_a_fitting_head_its_capacity` → `priority order violated: a Low job started ahead of the High head` | `unit::util_test::priority_ordering` |
| `s-21-native` | `s-21` / native / R | `lifecycle_events_test::lifecycle_events_resource_pool_cancelled_under_every_policy` → `tasks broke the lifecycle order:` AND `no terminal event`<br>`lifecycle_events_test::lifecycle_events_worker_pool_cancelled_under_every_policy` → `tasks broke the lifecycle order:` AND `no terminal event`<br>`lifecycle_events_test::lifecycle_events_durable_pool_requeued_at_shutdown` → `tasks broke the lifecycle order:` AND `no terminal event` | `lifecycle_events_test::lifecycle_events_rejected_capacity_names_the_exceeded_kind` |
| `s-21-browser` | `s-21` / browser / W | `wasm_events::lifecycle_events_cancelled_at_shutdown` → `["submitted", "queued", "cancelled"]` | `wasm_events::lifecycle_events_completed_failed_and_timed_out` |
| `s-22` | `s-22` / native / F | `unit::config_test::config_misspelled_pool_field_is_rejected_naming_it` → `expected a parse error, loaded`<br>`unit::config_test::config_misspelled_top_level_field_is_rejected_naming_it` → `expected a parse error, loaded`<br>`unit::config_test::config_misspelled_postgres_backend_field_is_rejected_naming_it` → `expected a parse error, loaded`<br>`unit::config_test::config_misspelled_redb_backend_field_is_rejected_naming_it` → `expected a parse error, loaded`<br>`unit::config_test::config_misspelled_worker_pool_field_is_rejected_naming_it` → `a misspelled field must not load` | `unit::config_test::config_worker_pool_config_loads_every_field_on_every_target` |
| `s-33` | `s-33` / iggy / I | `iggy_stream::iggy_stream_reconnect_midstream_receives_remaining_chunks_once` → `tokens 51-200 then the end marker, no gap, no duplicate`<br>`iggy_stream::iggy_stream_missing_chunk_is_reported_as_a_gap` → `left: None` AND `right: Some(Gap { expected: 3, got: 4 })` | `iggy_stream::iggy_stream_uses_the_topics_real_partition_count` |
| `oa-06-redb` | `oa-06` / redb / E | `mailbox_migration::mailbox_scope_distinguishes_optional_and_delimited_addresses` → `OA-06:address-scope: another complete address suppressed` | `conformance::result_mailbox::deliveries_without_key_all_stored` |
| `oa-07-redb` | `oa-07` / redb / E | `mailbox_status::duplicate_status_survives_consume_restart_and_original_expiry` → `OA-07:stored-status: retry hid durable failure` | `mailbox_status::malformed_authoritative_status_is_an_error_without_consuming_result` |
| `oa-07-event-nonterminal` | `oa-07-event` / redb / F | `scheduler::durable_sigkill::redb::pool_duplicate_redb_nonterminal_status_requeues_without_false_terminal` → `OA-07:nonterminal-event` | `scheduler::durable_sigkill::redb::pool_sigkill_redb_between_delivery_and_ack` |
| `oa-08` | `oa-08` / native / R | `discarded_spawn_test::discarded_spawn_reports_once_and_returns_capacity_without_polling` → `OA-08:discarded-terminal: each discarded run reports failure` | `discarded_spawn_test::discarded_spawn_ordinary_spawner_preserves_success_panic_and_timeout_once` |
| `oa-09` | `oa-09` / native / C | `backend_admission_test::backend_admission_rejects_total_and_kind_heads_once_before_fifo_followers` → `both impossible heads leave once`<br>`backend_admission_test::backend_admission_rejected_only_batch_requires_terminal_reporting` → `rejected-only batches cannot be ignored` | `backend_admission_test::backend_admission_temporary_total_or_kind_pressure_is_not_rejection`<br>`backend_admission_test::backend_admission_direct_submit_keeps_its_capacity_error` |
| `oa-10-expiry` | `oa-10-expiry` / native / R | `prune_expiry_test::prune_expiry_reports_each_front_and_backend_task_once_without_execution` → `task 1` AND `["submitted", "queued", "expired"]` | `prune_expiry_test::prune_expiry_live_deadline_control_still_completes` |
| `oa-11-kind-release` | `oa-11-kind-release` / loom / C | `loom_kind_release::loom_kind_release_parallel_returns_preserve_all_kind_units` → `OA-11 kind release lost a GpuVram subtraction` | `loom_kind_release::loom_kind_release_reacquire_publishes_previous_holder_work` |
| `oa-11-ledger-order` | `oa-11-ledger-order` / loom / R | `loom_ledger::loom_ledger_submit_shutdown_cannot_both_miss` → `OA-11 ledger admission and shutdown both missed accepted work` | `loom_ledger::loom_ledger_settlement_publishes_finished_before_zero` |
| `oa-13-budget-event` | `oa-13-budget-event` / browser / W | `wasm_events::lifecycle_events_completed_failed_and_timed_out` → `OA-13 execution-budget terminal event missing` | `wasm_events::lifecycle_events_rejected_at_submit` |
| `oa-14-missing-result` | `oa-14-missing-result` / upgrade / P | `postgres_upgrade::postgres_upgrade_populated_0_3_to_schema3` → `OA-14:seeded-result` (owned real upgrade delegate) | real populated PG17-to-PG18 copy plus backup restore and full PG suites, control absent |
| `oa-16` | `oa-16` / iggy / I | `iggy_stream::iggy_stream_abandoned_chunk_poisons_writer_and_preserves_prefix` → `OA-16:no-publication-past-hole: expected only seqs 1-2, got` AND `Ok(Err(Gap { expected: 3, got: 4 }))` | `iggy_stream::iggy_stream_reconnect_midstream_receives_remaining_chunks_once` |

## Primary source interfaces

The checked manifest pins nextest 0.9.149 and wasm-bindgen 0.2.129. API evidence
consulted during source authoring: [nextest JSON list format](https://nexte.st/docs/machine-readable/list/),
[exact filter matchers](https://nexte.st/docs/filtersets/reference/),
[reporting/status/output options](https://nexte.st/docs/reporting/),
[pinned exit-code definitions](https://raw.githubusercontent.com/nextest-rs/nextest/cargo-nextest-0.9.149/nextest-metadata/src/exit_codes.rs),
[pinned wasm-bindgen runner CLI](https://raw.githubusercontent.com/wasm-bindgen/wasm-bindgen/0.2.129/crates/cli/src/wasm_bindgen_test_runner.rs),
[headless browser guidance](https://wasm-bindgen.github.io/wasm-bindgen/wasm-bindgen-test/browsers.html),
[download-artifact v8 inputs/default directory behavior](https://raw.githubusercontent.com/actions/download-artifact/v8/action.yml),
and [rust-cache v2 inputs](https://raw.githubusercontent.com/Swatinem/rust-cache/v2/action.yml).
These establish the authored interfaces; they do not establish successful local
execution or current operator environment access.
