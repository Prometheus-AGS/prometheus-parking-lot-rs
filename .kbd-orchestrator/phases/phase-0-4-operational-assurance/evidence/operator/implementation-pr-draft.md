# Proposed PR: Implement 0.4.0 operational assurance

Target: `Prometheus-AGS/prometheus-parking-lot-rs`, branch `codex/operational-assurance` against `main`, draft PR. This file proposes ordinary branch delivery only and is not push/PR approval. The operator owns merge.

## Proposed description

Durable outcomes and scheduler accounting need discriminating regression coverage, and the0.4 release needs evidence tied to one source candidate. This change implements the accepted operational-assurance source plan, including mailbox status/fence migrations, runtime accounting and permit fixes, ordering models, upgrade/browser/desktop/Iggy fixtures, benchmarks, and candidate-bound release verification. Seven publishable crates are prepared as0.4.0; FFI remains excluded from publication.

The release machinery separates preparation, certification and publication. Missing environments or authority produce BLOCKED evidence. The implementation handoff contains the complete664-file source inventory and retained artifact pointers.

### Observed validation

Tested source: `180a2c612030648902567ccce0823bc0a369658c`, contract `e77aa4ac91ad92082a6cef288cd5f9b9636b6101faa65237dbe16116fff64257`.

- Native baseline:40 PASS,0 FAIL,2 BLOCKED;460 tests passed,119 skipped.
- macOS profile:5/5 PASS, including actual Tauri shell build, Clippy and service tests.
- iOS profile:4/4 PASS, including device/simulator builds and Swift smoke.
- Local negative controls:27/27 producer receipts verified; remaining25 require their real collaborators/environments.
- Additional old-recursion control: designated stack-overflow oracle detected, ordinary companion passed.
- Candidate/staged/diff secret scans passed for the tested source. Independent cumulative source reviews and targeted repair confirmations are retained.

### Remaining certification and publication work

The local packet is BLOCKED:2 full OA scenarios PASS,15 BLOCKED. Required Linux/backend/browser, Windows, Android, manual macOS GUI and quiet-performance evidence remain absent, as do approved history-containment, public signing/trust, trusted-publisher and remote-provenance authorities. A merged/rebased candidate needs applicable fresh certification under its own identity. Actual publication requires separate G05 approval and registry/tag/install receipts. Credential rotation and KBD signer replacement remain explicitly deferred/nonblocking.

The unchanged website dependency audit reports37 vulnerabilities, including16 critical. The earlier reviewer Ruff run reported935 diagnostics; no present-candidate Ruff PASS is asserted. These observed limits are documented rather than hidden behind successful Rust/platform checks.

Related scope: #57, #64, #65, #66, #67, #68, #71, #78, #79, #80, #81, #82, #85, #86, #87, #88, #89. This description does not automatically close external certification/containment issues.
