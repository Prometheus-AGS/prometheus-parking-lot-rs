# 0.4.0 merged-candidate certification readiness

0.4.0 is code complete, pending certification. The merged implementation waypoint records70/70 implementation tasks at1129, including the PR94 CI repairs. Evidence-only OA20 tasks do not represent unfinished library implementation. The adopted certification contract remains revision1/e77aa4ac91ad92082a6cef288cd5f9b9636b6101faa65237dbe16116fff64257. Full certification and publication remain separate from code completion.

## Source and observed results

Frozen clean source: ce034cb44d94bb4805ede4583a575f4e5215b728 (merged PR94). Its tree is identical to verified PR headdcfffe1d918d941af2efd7a0c2367497960caedc; historical PR94’s91/91checks are not relabelled as merged-candidate certification. Driver records are authored on codex/release-certification; production verification used the existing clean detached oa04-verification checkout and warm caches. No cold-build claim.

| Actual entry/observation | Observed result |
|---|---|
| operational-assurance-gate.sh --candidate ce034cb… --profile baseline, original public inputs, private pinned tools | First attempt39PASS/1FAIL/2BLOCKED; website browser cache path omitted. Failed receipt retained |
| website npm run verify with VERIFY_SKIP_INSTALL=1 and existing PLAYWRIGHT_BROWSERS_PATH | Exit0; actual browser/sanitizer/link/brand/accessibility checks and12screenshots. Existing Google Chrome for Testing156.0.8078.4; website only, not Linux OA13 browser proof |
| Complete baseline fresh receipt with corrected browser-cache environment | Exit2/BLOCKED:40PASS/0FAIL/2BLOCKED. Missing actual fresh_mirror containment input and public trust_policy/OIDC authority input. Native460passed/119skipped; all16tool identities PASS and original release preparation validates |
| macos profile | Exit0/PASS,5/5 records, actual Tauri build/Clippy/service tests and native check |
| ios profile | Exit0/PASS,4/4 device/simulator/generation/Swift smoke records |
| linux/windows/android/macos-gui/quiet freeze-only | Exit2/NOT_EXERCISED each. Tool/input discovery only; no production, manual GUI or benchmark result claimed |
| Export eight profiles and assemble corrected partial packet --purpose certification | Exports exit0; assembly exit2/BLOCKED.2 scenariosPASS,4BLOCKED,11NOT_EXERCISED; no compact certificate; publication NOT_EXERCISED |
| Current ordinary merged CI | Separate snapshot in merged-ci-observation.json:51successful CI jobs,4active,28queued; Platform passed. Pending CI is not certification provenance |

All production gate commands ran serially under timeouts with stdin closed. Exact argv, timestamps, selected environment, discovery and raw stream hashes remain in producer receipts and private raw artifacts. Original release preparation from72adf5d and its exact thirteen file hashes were preserved; no plan regenerated from prepared0.4 source. The first failing attempt and initial failed partial packet remain retained alongside successful recovery evidence; neither was overwritten. Fresh baseline producer receipt was required because the collector cannot fold a supplemental command result into a sealed profile.

## Existing operator gates

Read-only GitHub readiness found zero registered self-hosted runners, only github-pages environment, no release-certification or release-crates-io environment, main protected=false, no repository rulesets or variable names. Full certification still needs approved environment/main policy, workflow/public trust pins, G02/G03/G04 and independent review/GUI/quiet authority. The accepted workflow requires self-hosted labels; ordinary hosted CI does not supply an adopted amendment or these authorities.

Fresh advertised inventory is136refs:72heads/1tag/63GitHub-owned pull refs, digestb26b813e09f69c210620adf7f3a43a8e1f82c7cfa1fead6c659e11d6fc91f510. StageA proposal/delta and unused private destinations are retained under readiness/. They are proposals only. No mirror, backup, rewrite, remote update, settings mutation, workflow dispatch, private signing-key read, tag, publication or new install occurred. Credential rotation and KBD signer replacement remain explicitly deferred/nonblocking. G05 publication approval has not been requested or inferred.

One batched operator question is pending: retain/approve G02 StageA or explicitly amend/defer its release scope; identify approved self-hosted resources or explicitly amend environments to hosted CI plus attributable manual evidence; provide approved public signing/trust and publisher-owner references. No dependent action or amendment proceeds from silence. The uncomfortable constraint is that code completion and green normal CI leave the accepted certification authority/provenance contract unsatisfied. If containment or an adopted amendment changes the final candidate, these receipts stay bound toce034cb and new final-candidate evidence is required.

## Files and scope

- openspec/changes/oa-20-merged-certification/{proposal.md,plan.md,tasks.md}: two evidence/certification follow-up tasks and explicit model routing; task2 remains pending until prerequisites resolve.
- Phase execution.md and generated KBD/task receipts: authorized continuation and canonical boundaries; no hand-edited runtime projections.
- evidence/oa20/readiness/: current read-only API/ref/source observations and unapproved concrete StageA proposal; unrelated historical PR91 lookup excluded from source evidence.
- evidence/oa20/{baseline-receipts.json (unchanged originals indexed privately),macos-receipt.json,ios-receipt.json,website-recovery.json,packet.json,disposition.json,recovery-export-and-assembly.json,merged-ci-observation.json,artifact-location.json}: actual results, limits and retained raw hashes.
- Append-only .prometheus records: learned environment constraint and session result.

Production source changes: none. New guards: none. Unrequested feature/dependency/configuration changes: none. The only repair was selecting the already provisioned private Playwright cache in the process environment. Remote CI completion, Linux/Windows/Android certification, attributable GUI/quiet evidence and all external authorities remain unverified or unapproved as recorded. Task1 readiness completion must not close task2, Reflect, the phase, a release issue, or certification.

Readiness task1 completed through kbd-apply end-task (exit0) at revision1136. Initial reconciliation found the unobserved planned task2 unmappable; canonical task register recorded its existing plan definition without starting it (revision1137, status pending). Final kbd-apply reconcile exited0: clean=true, drifted=0, errors=[]. Full certification and Execute remain open.
