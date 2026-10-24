# Operational assurance 0.4.0 implementation handoff

The planned production code, regression fixtures and runbooks are delivered. Release certification is **BLOCKED** and publication is **NOT_EXERCISED**. Completing implementation does not certify the release. Credential rotation and signer replacement remain operator-deferred and nonblocking; no exposure-neutralization claim is made.

## Candidate and evidence

- Tested source: `180a2c612030648902567ccce0823bc0a369658c`.
- Adopted contract: `e77aa4ac91ad92082a6cef288cd5f9b9636b6101faa65237dbe16116fff64257`, plan revision1.
- Base: merged PR93, `92da0d3d28f7896ddc67062fa49ba40ea48d74a3`.
- [Artifact location and verified inventories](evidence/boundary-attempt-5/artifact-location.json).
- [Complete changed-file inventory](evidence/boundary-attempt-5/changed-files.txt): 664 paths/status records from the exact base/candidate Git diff. Later closeout documentation and KBD projections are separate records.

Raw evidence is retained privately outside Git at `/Users/gqadonis/.local/state/prometheus/operational-assurance/180a2c612030648902567ccce0823bc0a369658c/`. The `export/` folders are the checked packet inputs; original profile directories additionally preserve scratch preparation trees. Export, packet, control and driver inventories bind retained bytes. These are local evidence, not independently authenticated remote provenance.

## Observed local boundary

| Check | Actual outcome |
|---|---|
| Baseline driver | Exit2; 40 PASS, 0 FAIL, 2 BLOCKED |
| Native suite | `cargo nextest run --workspace --all-features --locked --no-tests=fail`; exit0; 460 passed,119 skipped |
| macOS profile | Exit0; all5 checks PASS: real Tauri shell build, formatting, Clippy,2 service tests, native check |
| iOS profile | Exit0; all4 checks PASS: device build, simulator build, generation, Swift smoke |
| Local control subset | Exit0; 27/27 checked producer receipts PASS:9 Loom,12 native,6 redb |
| Additional old-recursion proof | Compile0, discovery0, designated stack-overflow oracle101, ordinary-spawner companion0; classifier PASS |
| Local packet assembly | Exit2, BLOCKED; only OA02 and OA05 are complete scenario PASS under the full contract |

Every profile's `job-receipt.json` records exact argv, environment, exits, discovery and raw stream hashes. `drivers/` retains launchers and the reviewed subset driver. Every control compiled and discovered its named tests, ran the normal oracle/companion, detected its designated runtime fault, and passed the unaffected mutant companion. Compilation errors, watchdogs or missing tests were not accepted as controls. Compiled caches were reused; no cold-build claim is made. The additional recursion proof is separate from the52-case manifest.

The baseline also passed its MSRV, documentation, dependency-policy, seven-crate packaging, candidate/staged/diff secret scans, template/containment fixtures, release preparation, model, example, website/browser-verification and WASM checks. Native skipped tests remain unexecuted. Website Playwright verification is not the Linux WASM browser assurance scenario.

All99 negative-control source hashes and46 upgrade-fixture source hashes match. The original13-file release-preparation identity still matches; seven publishable crates are prepared at0.4.0, FFI excluded. The pinned ledger and model source hashes remain the independently reviewed ones. The Loom adapter proves only the documented four-event projection; it supplies no general C11/SC equivalence claim.

Earlier failures remain attached to their original candidates under `evidence/boundary-attempt-1/` through `boundary-attempt-4/`. The interrupted68bc local subset remains SUPERSEDED_INCOMPLETE. No old receipt was relabelled as this candidate.

## Last source repairs and review

- `bindings/tauri-desktop/Cargo.lock`: three local path-package versions0.3.0 to0.4.0; all434 registry records/checksums/dependency arrays unchanged. Observed locked fetch101 became0 after repair.
- `bindings/tauri-desktop/src-tauri/src/assurance.rs`: the four observed rustfmt reflows, including three optional trailing commas; behavior preserved. Final macOS formatting passed.
- Phase evidence, reviewer reports and append-only project memory retain the diagnostics, dispositions and learned constraints. No dependency upgrade, runtime guard or unrelated feature was added by these repairs.

The cumulative independent contract, runtime, storage, security and Python reviews and their correction confirmations are under `.agent-team/reviews/oa04-*/`. Prior retained source findings were resolved. Narrow observed repairs received separate static confirmations, including the final lock/format corrections. Review isolation used fresh contexts within the available model family; no stronger cross-family result or remote authority is claimed. Local static review does not satisfy the independently pinned workflow-authority receipt by itself.

## Release blockers and exits

The accepted contract remains unchanged. These external gaps do not reopen delivered implementation:

1. G02: separately approved history/cache containment, untouched backup/exact ref plan, fresh post-purge mirror and actual removal evidence. No rewrite, force push or Support submission is authorized.
2. G03/G04: approved public signing/trust identity, attributable repository/crates.io trusted-publisher settings and independent authority receipts. No private signing material is requested.
3. Environments: Linux backend/browser runs, PG17/18 populated upgrade/restore and real Iggy controls, Windows native build, required Linux Android profile, attributable manual macOS GUI restart, and quiet performance measurements. The remaining25 manifest controls are3 browser,18 PG,3 Iggy and1 upgrade. They are not locally certified.
4. Provenance: protected `release-certification` and appropriate runners/labels, approved workflow digests, fresh review authority and real candidate-bound remote artifacts. Read-only GitHub discovery found only `github-pages` and zero registered self-hosted runners; availability is not inferred from admin permissions.

Certification exits only when OA01–OA17 and the complete regression packet pass on one frozen candidate with required authorities and provenance. The two blocked baseline inputs are the actual post-purge mirror and public trust policy. Local assembly reports2 scenario PASS/15 BLOCKED, not a full release PASS. A future merged/rebased source identity needs its own applicable candidate-bound certification; these receipts retain their original SHA.

Publication has a separate G05 exit: explicit approval naming the exact candidate/tag and seven-crate set, verified operator-signed v0.4.0 tag, actual registry publication/fresh install and GitHub release receipts. Publication, account settings, pushes and PR creation have not been authorized by Execute. The operator merges PRs. [Prepared operator scope draft](evidence/operator/oa18-next-actions-draft.md) is a draft only, not approval.

## Other observed limits

The separate website production audit exited1 with37 dependency vulnerabilities:16 critical,18 moderate,3 low. Both website dependency files are byte-unchanged from the merged base. Findings are retained in `evidence/environment-preparation/website-production-audit.json`; dependency upgrades were not added to the adopted phase or silently turned into a new gate.

The earlier reviewer Ruff invocation exited1 with935 diagnostics, predominantly formatting. Its exact command and limits remain in `.agent-team/reviews/oa04-python/confirmation.md`; no present-candidate Ruff PASS is claimed. Required Rust Clippy and formatting gates did pass. Aggregate child tool-call accounting was not available; no invented total or budget-compliance certificate is supplied.

No unrequested production feature or guard was added in the last repairs. Existing phase guards trace to the adopted OA failure scenarios and release/fixture trust boundaries. What remains unverified is explicitly listed above; unavailable evidence is BLOCKED/NOT_EXERCISED, never PASS.

## Canonical closeout record

After the independent document critique, the timeout-bounded kbd-apply end-task driver exited0. It recorded task2 completion at revision1120 and change18/18 completion at revision1121. Canonical status confirms all18 changes/36 tasks complete. Read-only reconciliation exited0: clean true, drifted0, drift[], errors[]. [Reconciliation artifact](evidence/boundary-attempt-5/reconciliation.json). The phase and Execute stage remain in progress; no archive, Reflect, phase-completion or release-certification transition was performed. Later closeout records are documentation/runtime state, not a replacement source-candidate test receipt.
