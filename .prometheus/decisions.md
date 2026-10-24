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
