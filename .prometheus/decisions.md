# decisions

Append-only. Dated entries. Mark superseded entries; do not delete them.

## 2026-10-22
- Initialized by prometheus-context-bootstrap.

## 2026-10-23
- **Operator decision: raise the MSRV to what dependencies need.** The operator directed "upgrade the version of Rust to the latest version needed" while cr-32 (Iggy) was in progress. The pinned toolchain stays 1.99.1, which is the latest stable (released 2026-10-15). The MSRV (1.95, set 2026-10-22) may rise to the minimum rustc that the chosen iggy client crate requires under `--locked`, and no higher than 1.99.1. Rationale: correctness and features over compatibility (operator decision 2026-10-22 (3)); the crate has not been published yet. Supersedes the "MSRV 1.95" part of the 2026-10-22 toolchain decision once cr-32 records the new value; `.kbd-orchestrator/constraints.md` is updated to match.
- **Outcome (cr-32):** no MSRV change needed. `iggy` 0.11.0 declares `rust-version = "1.95"` and its tree builds on 1.95 under `--locked`. The MSRV stays 1.95 and the toolchain 1.99.1. The decision above stands for any future dependency that needs more.
- **Operator approval: round 16 (cr-52, release 0.3.0).** On 2026-10-23 the operator wrote "I approve Round 16": versioning, crates.io publication, the signed tag, the GitHub release and the docs deploy for 0.3.0. This is the explicit operator approval that publication requires. The other standing precondition is unchanged: cr-52 runs only after rounds 14–15 merge **and** the phase-boundary integration gate and independent review PASS. A boundary BLOCK suspends this approval until the findings are fixed and the gate re-runs to PASS.
- **Operator decision: boundary-gate amendment (2026-10-23).** For the 0.3.0 release gate:
  - (1) The history-purge check (mirror clone must contain no `.codex/*` credential files) is dropped from this release's boundary packet. `.codex/auth.json` and related files are in public history (`ec6b4b4`, removed from tracking in `926174e`). Rotating the leaked credentials is the operator's responsibility and is what neutralizes the leak. Rewriting history (cr-15: `git filter-repo` plus force-push) stays deferred follow-up work.
  - (2) Stale commands are corrected:
    - The Postgres step runs the Docker-gated suites with `--run-ignored all` under `PARKING_LOT_PG_MAJOR=17` and `=18` (the tests do not read `POSTGRES_IMAGE`).
    - The loom step adds `loom_skip_ahead`.
    - The wasm step runs the CI wasm-bindgen tests, with the headless Chrome run as best-effort (issue #65).
    - The run adds the guide examples.
  - (3) The operator approved the gate run itself. Nothing is published, tagged or pushed by the gate. The original `adopted-contract.json` is left unchanged, and this entry is the amendment record.
- **Operator decision: fix everything the boundary review raised before 0.3.0 (2026-10-23).** The four-part independent review found 6 CRITICAL, about 27 WARNING and about 21 SUGGESTION items. The operator chose to fix all of them before the release, rather than only the blockers. A suggestion an agent judges unsafe to change late goes to the 0.4.0 milestone with a stated reason. Rationale given to the operator: the review load was deferred to the phase boundary by design, so it surfaced all at once.
- **Operator decision: exit criteria for 0.3.0 (2026-10-24).** The round-2 review-fix PR is the last fix PR, and no further review rounds run. One final gate runs on a quiet machine. A real product defect it finds is fixed alone, and only the failed step reruns. A load- or timing-only test failure is fixed in the test. Then cr-52 releases 0.3.0 under the approval already given. Everything else found from now on goes to 0.4.0 issues, except a critical data-loss or security defect, which still blocks the release. Operator: "Execute your strategy and get 0.3.0 shipped as soon as possible."

## 2026-10-24 — Final gate step 13 corrected to the adopted secret-scan scope

- Final gate on `a3e517f` (main after #83): steps 01–12 passed. Step 13 ran `gitleaks detect --log-opts="--all"`, a full-history scan, and found 5 leaks. All are in the two root commits: `.codex/auth.json` ×3 (`ec6b4b4`, the known cr-15 leak), a made-up UUID in the redaction-test example in `docs/coding-standards/README.md` (`ec6b4b4`), and a placeholder key in `.e2b-template/SETUP.md` (`9399671`). None is in any `cargo package --list`.
- The full-history scan contradicts the 2026-10-23 gate amendment (1), and `.github/scripts/supply-chain.sh` already classifies `secrets-history` as informational until the cr-15 purge. Classification: gate-script defect, not a product defect.
- Step 13 replaced with the script's blocking checks: `tracked-credentials`, `secrets-diff bbd2b96^..HEAD` (this phase's range) and `package-allowlist`. Per the exit criteria, only that step was rerun, then the gate continued at step 14.
- Uncomfortable part: the two template files still sit in the working tree and will trip any full-tree scan. Allowlisting or removing them is a 0.4.0 item alongside cr-15.

## 2026-10-24 — 0.3.0 released

- Published all seven crates at 0.3.0 from `4b74735` (main after #84) with `cargo publish --workspace --exclude prometheus-parking-lot-ffi --locked`. crates.io rate-limited new crates (429) after five; `-wasm` and the facade published on retry within ~14 minutes. No yank needed.
- Annotated tag `v0.3.0` (unsigned: no key configured); GitHub release with the CHANGELOG 0.3.0 section.
- Post-release verification: fresh projects resolving from crates.io build for default, postgres, embedded, iggy, all three, and wasm32 with no default features. docs.rs `doc_status=true` for all seven crates.
- Deferred to 0.4.0: #85 trusted publishing + release workflow, #86 release-plz/cliff/versioning ADR, #87 signed tags, #88 cr-15 history purge, #89 template files flagged by full-tree scans.
- Uncomfortable part: the leaked `.codex` credentials remain in public history; only rotation (operator) neutralises them until #88 lands.

## 2026-10-24 — Operator-approved phase closure with deferred carry-over

- Operator in Codex: "Record both deferrals and close the phase."
- Canonical decision: `phase-correctness-and-durability-close-deferrals-20261024`.
- cr-52 tasks 1.1–1.3 close as deferred scope, carried to 0.4.0 #85/#86. Signed tags remain #87. The published v0.3.0 tag is annotated and unsigned; publication used the operator's local token, not OIDC.
- cr-15 closes as deferred carry-over under #88. No credential rotation, history rewrite, GitHub Support purge or post-purge fresh-mirror verification is asserted. The operator-only actions remain outstanding outside this phase.
- The receiving Codex driver records the completed cumulative reviews from PRs #74/#75/#76/#83 and the final gate summary at `a3e517f`, validates and archives OpenSpec artifacts, then closes Execute and writes Reflect. Administrative completion of deferred task records records their disposition, not implementation.
- Uncomfortable part: archived scope does not neutralise the historical credential leak or supply the deferred release controls. The signing key provided to resume this journal was also delivered through a publicly reachable IPFS URL; that authority must be treated as exposed and replaced through the runtime's supported operator-authorized key process. No credential or enrollment changes are performed by this closure.

## 2026-10-24 — 0.4.0 operational-assurance scope and operator gates

- Operator requested assessment and planning of `phase-0-4-operational-assurance`, with explicit release blockers/exit criteria and one batched operator-gate question before Execute.
- Operator answered: “Defer credential rotation for now and accept your recommendations on the rest. We need stuff to get finished before we start blocking things and making it hard to finish. We will handle keys and stuff later.”
- Credential rotation and replacement of the exposed KBD signer are explicitly deferred, non-blocking for planned implementation and this release contract. Existing exposure is disclosed, not neutralized or certified away.
- Accepted scope: #57, #64–#68, #71, #78–#82, #85–#89 (17 issues); 13 other milestone issues remain open outside this phase. Eighteen OpenSpec changes include one final boundary-assembly change.
- History rewrite/cache work and real release publication remain separate scoped approvals. Operator merges PRs. No rewrite, credential/account change, publication or Execute stage is authorized by the planning artifacts alone.
- Separate implementation completion from evidence, certification and publication: missing external authorization does not reopen finished code. Required absent/stale scenario evidence cannot be PASS.

## 2026-10-24 — OA-06 legacy consumed idempotency migration

Source inspection by tech-lead found that PostgreSQL0.3 idempotency tombstones retain mailbox_name/caller-key/expiry but no MailboxKey after consume, while redb retains caller-key/expiry only. Owner attribution can be irrecoverable. Implement conservative atomic refusal unless each unexpired legacy tombstone has one proven retained-result owner with matching generation/expiry or an explicit authoritative mapping. Preserve all data and TTL; do not discard unknown fences or silently Duplicate an unrelated tenant. Fixtures have known authoritative mappings and test refusal/success. Actual production migration/mapping collection remains an operator rollout action; no production stores are being changed. This implements the accepted preservation requirement and is unrelated to deferred credential rotation.

## 2026-10-24 — OA-09 rejected dispatcher entries

Scheduler source inspection found Dispatched only returns started/expired entries and ResourcePool::new initializes an empty accepted-work ledger for a prepopulated backend. Tech-lead approved scheduler-owned public Dispatched.rejected: Vec<(T,SchedulerError)> plus defaults/is_empty/literals/merging and required consumer handling; ResourcePool::new method-level Q:TaskQueue<P> bound seeds accepted count from initial queue.len before draining. Required by the accepted impossible-prefill oracle and prevention of observed source-level accounting underflow. Terminal callbacks remain outside dispatcher lock. Other owners receive exact diffs for out-of-role consumers; no parallel trait or new error taxonomy. Production verification deferred to phase boundary.

## 2026-10-24 — OA-07 authoritative duplicate status

Preserve the existing generic ResultMailbox acceptance of Queued and Running. DeliveryOutcome::Duplicate will carry the exact stored TaskStatus, including errors, retained past consume; no destructive consume or retry-status fallback. A durable consumer encountering a nonterminal duplicate diagnoses and uses its existing requeue path, without fabricating a terminal event or acknowledging the lease. A permanent nonterminal fence can retry indefinitely until authoritative remediation; an original TTL can expire normally. This is a guard at the existing public mailbox/durable boundary, observed during source inspection, rather than new terminal-only validation. Legacy owner and status conversions must share one transaction, including direct 0.3 and intermediate schema-2 upgrades; unknown historical metadata refuses atomically. No live upgrade or runtime evidence is implied.

## 2026-10-24 — OA17 memory measurement scope

Decision: measure and explicitly label per-sample benchmark-client/pool process peak RSS, normalizing Unix rusage units, and retain machine/Docker VM/resource/image identity. PostgreSQL server/daemon RSS is outside that process and is not claimed. Rationale: frozen issue71 asks Docker-backed throughput/latency and contention/memory use; neither it nor the adopted peak-RSS oracle requires database-process RSS. Adding that separate measurement would expand scope. Client samples include runtime/migration/warmup/teardown; no cgroup or Docker stats are mislabeled as RSS. No measurement has run.

## 2026-10-24 — Resolve existing exact compiler without silent acquisition

Read-only installed stable channel manifest records rustc1.99.1 even though no numeric1.99.1 directory exists. Do not infer absence from directory naming. Final native checks may use explicitly selected existing bytes only after actual binary version/hash verification; nested +1.99.1 wrappers still require a verified isolated numeric view or BLOCKED. Official Rustup docs confirm +selector precedence and RUSTUP_AUTO_INSTALL=0; freeze automatic installation off, preserve global configuration, and record every resolution input. No compiler/tool command or installation ran to make this decision.

## 2026-10-24T14:57:28.526385+00:00 — Final OA18 correction boundary
Five fresh native R2 reviewers found five unique source issues; original reviewers confirmed all five resolved in one cumulative correction cycle. Same model family isolation limitation remains explicit. Native reports are retained under .agent-team/reviews/oa04-*/. Use separate clean committed verification worktree so KBD/review-report writes cannot alter the candidate during the gate. Baseline against729cc425 is active, not certified. Credential rotation/replacement remains operator-deferred/nonblocking; no GitHub settings/ref or publication action is authorized by local completion.
