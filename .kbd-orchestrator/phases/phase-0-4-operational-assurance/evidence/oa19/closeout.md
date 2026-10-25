# PR94 CI repair closeout

Phase Execute; oa-19-ci-repairs backend task1 complete through kbd-apply at revision1128, change complete1129. Reconciliation exit0: clean true, drifted0, errors[]. Implementation70/70; phase Execute remains IN_PROGRESS. This does not close release certification.

Source head2e65341989263a39b9dbe2500478646d9140a6be passed all91 GitHub checks. CI40125561998, Platform40125562008, Docs examples and all source-derived52control cases passed. GitHub checks test synthetic merge candidate942e3612d2f8604474edd667ef39d569546fc4ec. OA12 summary binds that exact candidate and contract e77aa4ac91ad92082a6cef288cd5f9b9636b6101faa65237dbe16116fff64257. The normal physical upgrade receipt is PASS; its seeded-result mutant is EXPECTED_FAILURE. No skipped or timed-out control is counted as a pass. Earlier failed attempts remain in their source-specific records.

## File-by-file source changes

| File | Change and observed reason |
|---|---|
| .github/workflows/ci.yml | Explicit default-hosted pinned Chromium and physical-upgrade image setup before strict validators; missing executable paths/imageIDs blocked actual jobs |
| .github/workflows/platform.yml | Explicit default-hosted Linux Tauri prerequisites; actual pkg-config lacked webkit2gtk-4.1 |
| .github/scripts/bootstrap-hosted-browser.py | Reads existing exact browser pins, verifies official metadata/version/URLs, extracts temporary executables and exports paths; guards protect downloaded-executable and configured-runner boundaries |
| .github/wasm-browser-assurance.json | Provisioning prose only; pins and oracles unchanged |
| .github/negative-controls.json | Ten stale pg module names corrected to postgres across four real discovered cases; current source hashes refreshed; still52cases |
| crates/parking-lot-pg/tests/mailbox_migration.rs | Explicit last_value/log_cnt/is_called snapshot instead of unsupported whole-sequence row JSON; observed SQL42809 |
| crates/parking-lot-pg/tests/mailbox_status.rs | Same sequence snapshot repair; all preservation assertions unchanged |
| crates/parking-lot-pg/tests/fixtures/oa14/manifest.json | Current source hashes refreshed before OA12; new bootstrap source included |
| .github/scripts/tests/browser-assurance.py | Requires actual pinned Linux WASM runner exit1 for designated failure; retains exact test, diagnostic, context and complete peer outcomes |
| .github/scripts/tests/negative-controls.py | Same browser-only exit semantics in producer and receipt verifier; native values unchanged; stable scheduling puts upgrade first without changing52case set or concurrency |
| .github/scripts/tests/README-negative-controls.md | Documents verified pinned browser exit semantics |

OpenSpec repair proposal/plan/task, generated KBD projections/task receipts, execution log, this evidence folder and append-only .prometheus records document the authorized follow-up. No unrelated feature, dependency upgrade, credential/settings/history mutation or publication was added. The sole scheduling adjustment avoids the observed late upgrade feedback while preserving every check. No speculative runtime guard was added.

## Verification and limits

Commands actually run: cargo fmt --all -- --check exit0 under private pinned Rust1.99.1; git diff --check exit0; PythonAST and workflow YAML parsing exit0; bash -n exact upgrade setup exit0; negative-controls.py matrix exit0 and52unique unchanged IDs;47/100 current source SHA256 maps and original13release-plan hashes verified. Actual browser artifact replay accepts normal0/companion0/designated1 and rejects101/100/124/-15. Fresh narrow source reviews found no concrete issues. Staged and whole-PR-range Gitleaks scans before each source push exited0 with no leaks (last range12commits/5.59MB). GitHub gh pr view94 snapshot observed91COMPLETED/SUCCESS and MERGEABLE. ci-checks.json and oa12-summary.json retain job/receipt evidence; reconciliation.json retains canonical agreement.

The following commit contains closeout records only and must receive its own latest-head CI verification before PR readiness is claimed. Keep original180a local-packet receipts and original release preparation identities intact. Green normal PR CI does not certify the full release packet: attributable manual GUI/quiet-performance observations, containment/public signing/trust/publisher/provenance authorities and publication remain separate. Credential rotation and KBD signer replacement stay explicitly deferred/nonblocking. No merge or release action is authorized here; the operator merges PR94.

Fresh artifact-only critic checked all retained closeout records, the11-file source list,91/52counts, source/merge identities, canonical1128/1129completion and70/70waypoint; no concrete factual discrepancies. Suitable for records-only delivery; final-head CI still pending. Same-family review is not release authority.
