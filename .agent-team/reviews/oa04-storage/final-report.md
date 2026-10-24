Verdict: **No critical or important defect found in the reviewed storage/OA14 artifacts. Static approval only; runtime acceptance remains unverified.**

Fresh artifact anchors:

- `crates/parking-lot-pg/src/mailbox/sql.rs:85`: `ON CONFLICT (mailbox_name, mailbox_key, idempotency_key) DO UPDATE`
- `crates/parking-lot-pg/src/mailbox/mod.rs:287`: `.fetch_one(&mut *tx)`; line 294 commits only after decoding duplicate status.
- `crates/parking-lot-pg/src/mailbox/migration.rs:85`: `let resolved = resolve_statuses(&mut tx, supplied_statuses).await?;`; line 88 commits the complete conversion.
- `crates/parking-lot-embedded/src/store.rs:228`: `mailbox::migrate_metadata(&txn, now, statuses)?;`; line 229 updates the version within the same transaction.
- `crates/parking-lot-embedded/src/mailbox.rs:461`: `status: duplicate_status(remembered.status, &stored.status),`
- `docker/postgres18/upgrade/rehearse.py:271`: `if code != 100 or "OA-14:seeded-result" not in combined or int(counts[-1]) != 1:`

Strengths:

- Both backends scope active fences by complete mailbox address and retain original status after consumption. PG additionally scopes by mailbox name.
- PG duplicate lookup uses a fresh statement snapshot while retaining the conflicting claim lock. This addresses a concurrent winner becoming visible after the claim waits.
- Direct 0.3→3 and intermediate 2→3 conversion resolve ownership and exact-expiry-generation status before committing. Unknown, conflicting, expired, and incoherent evidence is rejected.
- Immutable conversion proof supports replay without renewing expired fences or overwriting newer generations. Legacy PG tables reject mutation through statement triggers; embedded schema markers reject older open protocols.
- OA14 exercises physical `pg_upgrade --check` followed by `--copy`, incompatible version/checksum fixtures, independent backup restore, populated schema conversion, and APIs on the upgraded server.
- Regression artifacts cover consumed Failed outcomes, tenant isolation, reconnect/expiry, concurrent status winners, old-writer rejection, atomic refusal, and real SIGKILL delivery-before-resolution paths.

Critical/important issues and required fixes: **None retained.** I rejected style-only concerns and did not infer runtime success from fixture definitions.

Limitations:

- Read-only inspection covered the tracked diff from `92da0d3d28f7896ddc67062fa49ba40ea48d74a3`, new source files, callers, migrations, manifests, fixtures, and relevant cfg branches. Both reviewed crates inherit workspace edition `"2024"`.
- No Cargo, Docker, build, or test execution occurred, per the parent’s serialized acceptance assignment. Compiler/Clippy/fmt/test health, actual image/package availability, PG17/18 behavior, backup recovery, and OA14 receipts remain unverified here.
- `git diff --check` reported whitespace in retained handoff patch files; no correctness finding follows from those patch-context lines.
- Merge/release approval assumes green CI, resolved conflicts, and successful real-backend acceptance against the frozen candidate.
- No files, guards, configuration, or runtime state were changed.
