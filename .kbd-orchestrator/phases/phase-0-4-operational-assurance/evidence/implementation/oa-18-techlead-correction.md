# OA18 cumulative correction cycle 1 — techlead source handoff

Task: oa-18-release-boundary:2. Runtime disposition: NOT_EXERCISED.
Only the two assigned high findings are corrected. The five owned sources were
copied from the current central checkout before authoring; the accompanying
unified patch records those exact bases. No shared certificate schema/module,
production prepare.py, workflow, backend, runtime or supply-chain source changed.

## Preparation source and final commit

The outer job/packet identity remains the clean final candidate commit at0.4.0.
New baseline input `preparation_source` is the full original pre-bump commit SHA;
`preparation_plan` supplies original plan bytes. Base/date/source must equal the
plan. The incumbent validate_preparation verifies the original source ancestry,
seven-package/FFI policy and exact metadata hashes against the final candidate.
The original bytes and SHA are frozen before product commands; absence blocks
OA02 alone, allowing other available baseline commands to run. Plan mutation
between freeze/run refuses; collection revalidates original bytes. Compact
assembly requires the same original-plan digest as the baseline freeze/export.
No plan is forged, rewritten or replaced with a reproduced plan.

The checked OA02 row now passes this exact argument structure:

```text
python3 scripts/release/test-preparation.py --source {workspace} --candidate {preparation_source} --base {base} --version 0.4.0 --date {release_date} --release-plz {release_plz} --git-cliff {git_cliff} --output {out}/oa02 --final-candidate {candidate} --preparation-plan {preparation_plan}
```

The actual fixture still invokes production prepare.py twice, cloning/checking
out the ancestor into an external scratch workspace. It compares all portable
plan fields, pinned/observed tool metadata and executable hashes, and exact
prepared-file hashes with the original plan and actual final candidate files.
The reproduced plan must itself be byte-identical across its two invocations.
Original source/tool/argv scratch paths are invocation provenance and may differ;
those fields are retained honestly rather than normalized into forged original
bytes. The collector copies only the exact original plan into the incumbent
compact artifact. Existing standalone pre-bump fixture argv remains supported;
final mode requires both new flags together and cannot use final SHA as ancestor.

Two new real-fixture controls alter the claimed Cargo.toml hash: one must fail
compare_final_preparation with OA-02:final-preparation-mismatch; the other must
fail incumbent validate_preparation with content-differs-from-candidate. They
retain separate observed diagnostic JSON only when actually run. The final
binding file records final candidate, preparation source, original/reproduced
plan digests and output file digests. None has run in this source handoff.

## Three distinct native desktop builds

`desktop-linux` now belongs to `linux`, whose required OS is Linux. Mac and
Windows remain separate required macos/windows rows. All three pin child receipt
`os` and `case_id` to the actual OA15 protocol's Linux/OA15-BUILD-LINUX,
Darwin/OA15-BUILD-MACOS and Windows/OA15-BUILD-WINDOWS pairs. The execution path
and collector both call validate_child_receipt; collection uses the checked
model's identity requirements and refuses an omitted required child receipt.
Existing PASS/candidate/contract binding remains required as well.

Boundary fixtures cover each correct pair and reject wrong OS alone, wrong
build case alone and another OS's complete pair even when status/candidate/
contract all match. A definition fixture requires three exact profile/OS/case
rows. These are authored controls, not evidence that an OS build or GUI ran.

## Deferred execution and limits

Proposed execution uses the existing gate/profile interface from the runbook,
with new baseline preparation_source/preparation_plan inputs. The existing
`python3 .github/scripts/tests/boundary-packet.py` command covers added desktop
predicate fixtures; the checked OA02 row covers real preparation/mismatch paths.
These commands were NOT executed. No tests, Cargo, builds, syntax checks,
formatters, scans, analyzers, independent review or runtime mutations occurred.
Only targeted reads, source authoring, unified-diff generation and exact file
SHA hashing were performed. The next source freeze/hash refresh and authorized
verification belong to the driver. No unrelated additions were made. Both new
guards address the named source-observed high findings, not hypothetical risk.
Actual pins/tool health, reproducibility, platform executability and all new
fixtures remain unverified; G02/G03/G04 authority remains pending/nonfabricated.
