# Phase Reflection: phase-correctness-and-durability

## Delta

1. Containment remains incomplete. cr-15 is administratively closed as deferred carry-over under #88, not implemented. No credential rotation, GitHub Support request, history purge or post-purge mirror check is asserted. The operator explicitly chose “Record both deferrals and close the phase”; canonical decision `phase-correctness-and-durability-close-deferrals-20261024` records that scope disposition.
2. The release shipped manually. Seven crates reached crates.io at 0.3.0, FFI remained unpublished, and annotated tag `v0.3.0` points to `4b747350c6883497813bed33623a9937ad7a5894`. The tag is unsigned. Versioning ADR/release tooling, OIDC trusted publishing, the gated release workflow and signed tags remain #85–#87. cr-52 tasks 1.1–1.3 close as deferred, with explicit task summaries; they are not implementation evidence.
3. Review needed explicit exit criteria. The cumulative independent review found six critical items; a focused second round found another critical regression. Findings were corrected in merged PRs #74/#75/#76/#83. The operator stopped further review rounds and accepted one final quiet-machine gate. This closure records those reviews and does not claim a third code review, a new cross-family review, or a fresh PASS verdict on all original requirements.
4. Gate execution under load produced timing-only failures. The final gate summary at `a3e517f` records all 19 adopted steps passing after step 13 was corrected to the approved secret-scan scope. Full-history containment was explicitly deferred. This checkout has the gate README, script and summary, but not raw per-step logs. Sender-side test runs are historical evidence, not local reruns.
5. Cross-machine restoration was incomplete without signing authority. The copied journal and registration correctly returned revision 627/event `9cb19a43-6764-4b64-9cd9-c750bc8b83d2`; the first write still failed because the enrolled private key was absent. After the operator supplied it, task boundaries succeeded. The supplied key was delivered through a publicly reachable IPFS URL and must be treated as exposed; no key rotation or enrollment change was performed here.
6. Bookkeeping needed correction. Original task-display-ID mismatches left stale blockers. During closure, redundant change-after evaluations after successful `end-task` calls created two more diagnostic blockers; they were cleared with the successful earlier receipts retained. An overlapping archive pass caused a read-only OpenSpec listing to be refused by its global operation lock. Operations then ran sequentially. No journal, receipt or projection was rewritten by hand.
7. The published behavior retains documented gaps. Open 0.4.0 issues include dropped supervise futures (#81), pre-populated queue admission (#82), stored-outcome event consistency (#78), expiry events (#57), browser execution (#65), desktop GUI coverage (#67), and further backend/performance coverage. Closing this phase does not establish the three promises without their documented operating limits.

## Root Cause

1. Security/account actions and release automation were not delivered before the publication deadline; the operator accepted carry-over. Administrative counters cannot distinguish that disposition from implementation without explicit summaries and decisions.
2. Deferring all reviews to the boundary concentrated findings near release. Without a stopping rule, each repair round could grow scope indefinitely. Same-family isolated review is weaker than cross-family review; receiving Codex bookkeeping does not change that historical limit.
3. Timing assumptions were tested on a heavily loaded host. The quiet-machine lesson recurred after earlier failures; the final gate and revised tests addressed the observed scheduling sensitivity.
4. A journal archive carries signed state and public identities, not macOS Keychain secrets. Read-only restoration success did not prove write capability. Reusing a consumed boundary receipt and overlapping a globally locked CLI were closure-driver mistakes, not product defects.
5. New-crate publication hit crates.io rate limits after five crates. The remaining crates published on retry; that operational limit was distinct from package correctness.

## Corrective Actions

1. Complete operator-led credential containment and replace exposed journal signing authority through the supported authorized key process. Finish #88 and #89 before claiming clean full-history or full-tree scans.
2. Deliver #85–#87: OIDC trusted publishing, approval/boundary-gated release automation, a versioning ADR, release tooling and signed future tags. Preserve the actual unsigned v0.3.0 history.
3. Prioritize the accepted correctness follow-ups #78/#80/#81/#82/#57 and negative-control coverage #79/#64. Define a bounded cumulative review and integration exit before the next release starts.
4. Keep tests on quiet hosts, record available resources, and make time-based assertions tolerate real scheduling. Keep source builds, packaged-artifact checks and production-service evidence distinct.
5. Transfer canonical state and signer access through separate private channels; prove the intended signer is available before task mutations. Serialize all managed OpenSpec operations and let `end-task` consume each after-boundary once.
6. Preserve raw gate logs alongside summaries in a dated evidence bundle. Plan first-publication retries without treating a registry throttle as a product failure.

## Recalled Lessons

The Reflect-before hook refreshed `prior-context.md` with zero lessons on this machine. These evaluations use the preserved Execute-stage context at `evidence/2026-10-24-codex-closure/prior-context-execute.md`, recovered from the unchanged receiving Git HEAD, rather than claiming that fresh recall returned those lessons.

- Applied: “External integrations require at least one real-service test” (`prior-context-execute.md`, learning `ddd2db0fb37f6232`) — the sender's adopted gate includes Docker PG 17/18 and real backend paths; review PRs record Iggy and crash/restart tests.
- Applied with a limit: “Boundary gates must launch the packaged artifact” (`prior-context-execute.md`, learning `391671bab251836b`) — the sender packaged and verify-built the seven publishable crates and recorded fresh crates.io install builds; this receiver did not repeat those builds. Browser and GUI execution remain explicit gaps.
- Applied: “Preserve production evidence in dated review folders” (`prior-context-execute.md`, learning `0d46bd8a48d92ee1`) — original gate evidence is retained and a separate dated Codex closure audit is added. Missing raw logs remain disclosed.
- Recurred: “Discover test environment state instead of encoding checkout assumptions” (`prior-context-execute.md`, learning `6c1c504c9400e396`) — sender paths and credential-store assumptions did not transfer to this machine. The supplement, project ID and checksum prevented rebuilding authority from stale projections.
- Applied: “Ask once at phase boundary for operator-only actions” (`prior-context-execute.md`, learning `29cb1718c3df4502`) — the two deferrals were presented together; no merge, purge, publication or successor phase was performed by this closure.
- Applied: “Operator controls PR merges” (`prior-context-execute.md`, learning `c217c3a296f8ed4f`) — this closure neither pushed nor merged anything.

## Goals

Assessment is against the adopted phase scope and its recorded amendments. Five of seven goals are MET (71.4% fully met); two remain PARTIAL. This is not a measured reliability percentage. The administrative scope is settled at 37/37 changes and 132/132 tasks, including seven explicitly deferred task dispositions.

| Goal | Status | Evidence and limits |
|---|---|---|
| Three promises: capacity, accepted-work retention, disconnected retrieval | PARTIAL | The adopted gate covers weighted/vector admission, panic/budget handling and PG/redb restart/mailbox paths. Durable guarantees apply to durable ResourcePool, not every pool; accepted follow-ups #81/#82 and event/idempotency gaps remain. |
| M0 containment, truthful docs, corrected prior record | PARTIAL | Docs and KBD correction delivered; package allowlists and adopted secret checks passed. cr-15 containment remains #88; exposed signing authority is an additional unresolved operational risk. |
| M1 core rework | MET | Shared dispatcher, weighted/vector semaphore, FIFO/skip-ahead, panic containment, async result slots and loom models have historical gate/review evidence. |
| M2 durable backends | MET | Async lease/mailbox contracts, raw-sqlx PG/pgmq, redb, durable ResourcePool and shutdown/cancellation paths are delivered; the adopted real PG 17/18 gate passed. This does not assert durable native WorkerPool (#59). |
| M3 events and composition | MET | HookBus, Iggy hook/notifier, storage-first composition and the stretch replay change were delivered. Backpressure/expiry/event-consistency limitations remain #57/#62/#78. |
| M4 platforms, configuration, CI and benchmarks | MET | Passed adopted native/WASM Node, MSRV, Android/iOS check, PG matrix, packaging and docs-site steps; browser tests, desktop GUI and expanded performance coverage remain #65/#67/#71. |
| Delivery by parking-lot-core with independent context review | MET | Sender role implementations and cumulative isolated reviews are recorded in merged PRs. Review independence remained same-family; Codex only closed records and reconciled specs. |

## Delivered Changes

Implementation and release work was performed by the sending Claude Code team. The receiving Codex driver recorded review outcomes, approved deferrals, validation, archival and reflection. All archive entries retain their authored proposals; dated disposition notes override deferred claims.

| Change | Disposition | Artifact |
|---|---|---|
| `cr-15-containment` | deferred carry-over #88 | [archive](../../../openspec/changes/archive/2026-10-24-cr-15-containment/) |
| `cr-41-toolchain-baseline` | delivered | [archive](../../../openspec/changes/archive/2026-10-24-cr-41-toolchain-baseline/) |
| `cr-17-kbd-correction` | delivered | [archive](../../../openspec/changes/archive/2026-10-24-cr-17-kbd-correction/) |
| `cr-48-workspace-split` | delivered | [archive](../../../openspec/changes/archive/2026-10-24-cr-48-workspace-split/) |
| `cr-42-ci-rebuild` | delivered | [archive](../../../openspec/changes/archive/2026-10-24-cr-42-ci-rebuild/) |
| `cr-45-docs-site` | delivered | [archive](../../../openspec/changes/archive/2026-10-24-cr-45-docs-site/) |
| `cr-18-core-contracts` | delivered | [archive](../../../openspec/changes/archive/2026-10-24-cr-18-core-contracts/) |
| `cr-51-supply-chain-gates` | delivered | [archive](../../../openspec/changes/archive/2026-10-24-cr-51-supply-chain-gates/) |
| `cr-43-postgres18-image` | delivered | [archive](../../../openspec/changes/archive/2026-10-24-cr-43-postgres18-image/) |
| `cr-25-backend-contracts` | delivered | [archive](../../../openspec/changes/archive/2026-10-24-cr-25-backend-contracts/) |
| `cr-19-dispatcher-semaphore` | delivered | [archive](../../../openspec/changes/archive/2026-10-24-cr-19-dispatcher-semaphore/) |
| `cr-46-docs-theory` | delivered | [archive](../../../openspec/changes/archive/2026-10-24-cr-46-docs-theory/) |
| `cr-35-test-targets` | delivered | [archive](../../../openspec/changes/archive/2026-10-24-cr-35-test-targets/) |
| `cr-16-docs-truth` | delivered | [archive](../../../openspec/changes/archive/2026-10-24-cr-16-docs-truth/) |
| `cr-21-panic-containment` | delivered | [archive](../../../openspec/changes/archive/2026-10-24-cr-21-panic-containment/) |
| `cr-28-embedded-redb-queue` | delivered | [archive](../../../openspec/changes/archive/2026-10-24-cr-28-embedded-redb-queue/) |
| `cr-22-result-slots` | delivered | [archive](../../../openspec/changes/archive/2026-10-24-cr-22-result-slots/) |
| `cr-26-pg-lease-queue` | delivered | [archive](../../../openspec/changes/archive/2026-10-24-cr-26-pg-lease-queue/) |
| `cr-38-wasm-target` | delivered | [archive](../../../openspec/changes/archive/2026-10-24-cr-38-wasm-target/) |
| `cr-27-pg-mailbox` | delivered | [archive](../../../openspec/changes/archive/2026-10-24-cr-27-pg-mailbox/) |
| `cr-37-config-schema` | delivered | [archive](../../../openspec/changes/archive/2026-10-24-cr-37-config-schema/) |
| `cr-30-pgmq-sql-backend` | delivered | [archive](../../../openspec/changes/archive/2026-10-24-cr-30-pgmq-sql-backend/) |
| `cr-34-mailbox-composition` | delivered | [archive](../../../openspec/changes/archive/2026-10-24-cr-34-mailbox-composition/) |
| `cr-44-mobile-desktop-bindings` | delivered | [archive](../../../openspec/changes/archive/2026-10-24-cr-44-mobile-desktop-bindings/) |
| `cr-49-durable-dispatch` | delivered | [archive](../../../openspec/changes/archive/2026-10-24-cr-49-durable-dispatch/) |
| `cr-36-pg-matrix` | delivered | [archive](../../../openspec/changes/archive/2026-10-24-cr-36-pg-matrix/) |
| `cr-29-shutdown-cancellation` | delivered | [archive](../../../openspec/changes/archive/2026-10-24-cr-29-shutdown-cancellation/) |
| `cr-31-hook-emission` | delivered | [archive](../../../openspec/changes/archive/2026-10-24-cr-31-hook-emission/) |
| `cr-20-skip-ahead` | delivered | [archive](../../../openspec/changes/archive/2026-10-24-cr-20-skip-ahead/) |
| `cr-32-iggy-events` | delivered | [archive](../../../openspec/changes/archive/2026-10-24-cr-32-iggy-events/) |
| `cr-23-multi-resource` | delivered | [archive](../../../openspec/changes/archive/2026-10-24-cr-23-multi-resource/) |
| `cr-33-stream-replay` | delivered | [archive](../../../openspec/changes/archive/2026-10-24-cr-33-stream-replay/) |
| `cr-39-dependency-hygiene` | delivered | [archive](../../../openspec/changes/archive/2026-10-24-cr-39-dependency-hygiene/) |
| `cr-40-benchmarks` | delivered | [archive](../../../openspec/changes/archive/2026-10-24-cr-40-benchmarks/) |
| `cr-47-docs-guides` | delivered | [archive](../../../openspec/changes/archive/2026-10-24-cr-47-docs-guides/) |
| `cr-53-wasm-parity` | delivered | [archive](../../../openspec/changes/archive/2026-10-24-cr-53-wasm-parity/) |
| `cr-52-release` | released; tasks 1.1–1.3 deferred to #85/#86; unsigned tag #87 | [archive](../../../openspec/changes/archive/2026-10-24-cr-52-release/) |

## Artifact Quality Summary

| Metric | Observed evidence |
|---|---|
| Dedicated per-change artifact-refiner logs | Not present in this checkout; no pass rate is asserted |
| Cumulative code review | Two recorded rounds; findings/corrections in #74/#75/#76/#83; no additional round under operator exit criteria |
| Adopted final gate | Historical summary reports 19/19 steps passing after approved step-13 correction |
| Raw final-gate logs | Not transferred; README/script/summary available |
| OpenSpec validation and archival | 37 changes validated and archived; final canonical-spec validation reports 30 passed / 0 failed with no warnings after completing generated Purpose sections. This validates artifacts, not deferred production behavior |
| Scope reconciliation | Canonical/backend reconciliation is recorded separately in the closure evidence |

## Technical Debt

- Security: original leaked `.codex` history and operator-only rotation/purge remain #88. Template scan findings remain #89. The journal signing key supplied through a public gateway must be replaced under authorized runtime procedures.
- Release: long-lived-token retirement, OIDC, workflow gates, versioning automation and tag signing remain #85–#87.
- Runtime/config/events: #52–#62, #78, #80–#82 capture the accepted functional gaps and operating limits.
- Verification/performance/platforms: #63–#71 and #79 capture deeper loom/negative-control, browser, GUI, upgrade and benchmark coverage.
- No new production code, fallback, retry, timeout or guard was introduced by the receiving closure. Temporary orchestration timeouts bounded CLI calls as the handoff required.

## Architecture Integrity

- The sender's gate reports core dependency separation, supply-chain/package checks, MSRV, native and platform checks passing. This is evidence within the amended scope, not a new source audit.
- Historical credential containment is an approved exception, not a satisfied safety invariant. Deferred delta specs were reconciled before promotion so canonical specs do not claim a purged history, OIDC release, or signed v0.3.0 tag.
- All canonical state changes used typed runtime commands and `kbd-apply`; projections and checkbox state were generated by their owners. No kernel write path or production architecture was changed.

## Cross-Tool Coordination Notes

- Progress tracking: GAPS FOUND. The restored journal preserved revision 627 accurately, but review tasks remained pending despite merged implementations, original alias errors remained as blockers, and the private signer was absent. Closure used the authoritative journal, not migration or checkbox editing.
- Handoff quality: the supplement correctly superseded stale sender paths and separated shipped work from bookkeeping. Issue #92 supplied a checksum-verified journal archive. Private-key transfer instructions were missing from that archive procedure, and raw gate logs were not carried.
- The first duplicate guard and the overlapping OpenSpec pass were receiving-driver mistakes; both are disclosed in `execution.md` and `gotchas.md`. Their diagnostic resolutions do not claim product fixes.
- Memory hooks queued progress in the local outbox when the memory write path was unavailable. Remote memory delivery is not asserted. Unrelated global promotion/skill candidates were left untouched.

## Lessons Learned

- A successful KBD restore/status read proves journal identity, not signing capability; check both before task mutations and transfer keys privately.
- A completed after-boundary consumes its start receipt. Inspect that receipt rather than firing the same boundary again as a status query.
- Serialize all managed OpenSpec commands because task listing and archival share a global operation lock.
- Keep deferred dispositions explicit in canonical task summaries, archived task notes and promoted specs; never interpret checked boxes as proof that deferred production behavior shipped.
- Preserve raw gate output with its summary and label historical evidence separately from fresh verification.
- Set bounded review exit criteria before late release repairs and run timing-sensitive integration gates on quiet machines.
- First crates.io publication can be throttled even when packages are correct; preserve the dependency order and distinguish retries from defects.

## Next Phase Seed

`phase-0-4-operational-assurance` is a recommendation only; no successor phase is started.

1. Containment and release authority: #85–#89 plus replacement of exposed journal signing authority, with operator-only actions kept explicit.
2. Accepted correctness gaps: #78/#80/#81/#82/#57, followed by the negative-control and concurrency evidence in #79/#64. Scope invariants and release blockers before execution.
3. Runtime/platform assurance and configuration: #52–#71, especially browser execution #65, desktop GUI #67, PG upgrade #66, and reproducible backend/contention benchmarks #71.

### Open 0.4.0 milestone inventory

The receiver read the following 30 open milestone issues. Listing them seeds planning; it does not add them to this completed phase or authorize implementation.

| Issue | Title |
|---|---|
| [#52](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/issues/52) | Config-built durable pools cannot take a notifier |
| [#53](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/issues/53) | Config-built durable pools: result TTL, heartbeat and poll intervals are not configurable |
| [#54](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/issues/54) | Durable pools should wake on Postgres LISTEN/NOTIFY instead of polling |
| [#55](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/issues/55) | Allow a SchedulerConfig with no pools (events-only configuration) |
| [#56](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/issues/56) | Optional result_ttl_ms for WorkerPool result slots |
| [#57](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/issues/57) | Emit a terminal lifecycle event for tasks pruned as expired |
| [#58](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/issues/58) | Skip-ahead for DurableResourcePool |
| [#59](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/issues/59) | Durable mode for the native WorkerPool |
| [#60](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/issues/60) | Decide and test pgmq-backed durable pools |
| [#61](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/issues/61) | Attach IggyHook to pools from configuration |
| [#62](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/issues/62) | Iggy: a restarted publisher can store duplicate messages |
| [#63](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/issues/63) | Single-dimension dispatcher is 2–4% slower after multi-resource admission |
| [#64](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/issues/64) | Loom models for the shutdown ledger and kind-limited permit release |
| [#65](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/issues/65) | Run the WASM tests in a headless browser |
| [#66](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/issues/66) | Test the PostgreSQL 17 to 18 upgrade path in the runbook |
| [#67](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/issues/67) | Run the Tauri desktop example's GUI and build it on Linux and Windows |
| [#68](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/issues/68) | Docker test for IggyStreamSink writer poisoning |
| [#69](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/issues/69) | Remove the stale llm_inference_test and its reqwest dev-dependency |
| [#70](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/issues/70) | Mark superseded ADRs and fix the stale WASM CI step name |
| [#71](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/issues/71) | Benchmark the Postgres backends, contention and memory |
| [#78](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/issues/78) | Emit a durable run's terminal event from the stored outcome |
| [#79](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/issues/79) | CI job that runs every negative control |
| [#80](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/issues/80) | Scope mailbox idempotency keys per tenant |
| [#81](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/issues/81) | A task is lost when a spawner discards ResourcePool's supervise future |
| [#82](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/issues/82) | Tasks in a pre-populated TaskQueueAdapter backend skip check_capacity |
| [#85](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/issues/85) | Release automation: crates.io trusted publishing (OIDC) and a release workflow |
| [#86](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/issues/86) | Release tooling: release-plz, git-cliff and a versioning ADR |
| [#87](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/issues/87) | Sign release tags |
| [#88](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/issues/88) | cr-15: purge leaked .codex credentials from git history |
| [#89](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/issues/89) | Template files trip full-tree secret scans |

## Codify as Skill?

- Update cross-machine KBD handoff guidance to include private signer transfer, a write-capability preflight, one-shot boundary consumption and global OpenSpec serialization. This is a proposal; no skill was edited or candidate accepted here.

## Context for Next Phase

Use this reflection, the canonical deferral decision, `execution.md`, the adopted final-gate summary and the separate Codex closure evidence as assessment inputs. Do not inherit administrative completion as evidence that #85–#89 were implemented. No raw secret values belong in those records.
