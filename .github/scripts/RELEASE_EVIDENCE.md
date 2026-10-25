# OA-04 / OA-18 shared release evidence interface

This is implementation protocol documentation, not an executed receipt. No
workflow, registry operation, key action or real rehearsal was run to author it.

The shared [Python module](release_evidence.py) and
[JSON schema](../release-boundary.schema.json) are the one boundary receipt
protocol. OA-18 owns `.github/workflows/operational-assurance.yml` and its actual
scenario runner. Do not create a second release certificate schema or accept a
caller JSON file claiming `approved` or `PASS` as provenance.

## Producer and artifact

The operator approves the full source digest of the boundary workflow, protected
`main` branch and `release-certification` environment/reviewers. A successful
first-attempt `workflow_dispatch` run at the frozen candidate uploads an
immutable artifact named `operational-assurance-boundary`, with exactly these
two root members and no private material:

- `boundary-receipt.json`, conforming to the shared schema and validator.
- `release-plan.json`, the exact bytes produced by OA-02's
  `scripts/release/prepare.py`, never reconstructed from a crate-name list.

Raw logs and individual receipts belong in the separately retained boundary
packet. The trusted producer must inspect those real artifacts and compute
their digests before assigning PASS. Do not let workflow input JSON determine
the scenario results. The operator-pinned workflow digest is the authority for
the code that constructs the packet; GitHub's authenticated run/artifact APIs,
archive SHA256 and actual environment review history establish its provenance.

`fetch_boundary(api, artifact_id, policy, candidate)` is the only consumer path
establishing remote provenance. It returns `(receipt, preparation_plan,
provenance)`. Production exposes no local-receipt argument or arbitrary API URL.
`validate_receipt(...)` validates content only; invoking it on local data does
not authorize that data. OA-18 can import this module to validate its authored
packet before upload, after the real production gates.

The top-level receipt fields are exactly `schema_version`, `purpose`, `candidate`,
`contract_digest`, `tag`, `tag_object`, `version`, `manifest_sha256`,
`preparation_plan_sha256`, `tag_verification`, `scenarios`, `baseline`, `review`,
`operator_gates` and `status`. Version 1 is fixed to the accepted 0.4.0 phase.
`candidate` and `tag_object` are full lowercase SHA-1 IDs for this repository.
`manifest_sha256` names the checked boundary command/environment manifest;
`preparation_plan_sha256` hashes the exact uploaded preparation JSON bytes.

Each mandatory `OA-01` through `OA-17` occurs exactly once. Each scenario,
baseline, independent review and G-02/G-03/G-04 record has `status`; a PASS also
requires the digest of its actual observed receipt in `receipt_sha256`.
Scenario records additionally have `id`. G-01 is explicitly deferred/non-blocking;
it must not be smuggled into mandatory gate keys. G-05 is independently checked
on the current publication workflow/environment, not a receipt boolean.

Two purposes share this schema:

| Purpose | Required global status | Per-oracle status | Effect |
|---|---|---|---|
| `rehearsal` | `NOT_EXERCISED` | `PASS` with a real receipt digest, or `NOT_EXERCISED` | Preliminary tooling path only; never publication authority |
| `certification` | `PASS` | Every scenario, baseline, review and operator gate actually PASS with receipt digests | Eligible for the later current-run publication authorization |

This distinction avoids circularity: an approved preliminary run can upload a
rehearsal packet before OA-04 itself has a final result. A later completed run
may upload certification only after actual complete gates. The consumer does
not promote a rehearsal packet or let the requested publication mode fall back
to rehearsal. FAIL/BLOCKED observed outcomes must remain in the full packet and
cannot be encoded as a certification PASS. Missing external authority may leave
implementation complete with certification pending.

## Preparation and tag binding

`validate_preparation(workspace, plan, candidate, version)` reuses OA-02's real
manifest/lockfile checker without Cargo. It requires exactly the seven packages
in `PACKAGES`, matching name/path/version records and FFI exclusion. The existing
plan's `candidate` is the **preparation source** before its version bump; that
commit must be an ancestor of the later integrated frozen candidate. Do not
rewrite it to the final candidate. Every `files_sha256` entry must match the
integrated final manifests, lockfile, changelog and release-policy files.

`verify_candidate_tag(workspace, policy, candidate, tag)` calls the OA-03 verifier
against the real tag and returns `(PASS_JSON, public_trust_digest)`. The receipt's
`tag_verification` must equal that JSON plus `public_trust_sha256`; its tag object
must also match the receipt's top-level tag object. Thus a status string alone
cannot substitute for actual signature/candidate authorization. The signed
candidate is checked before loading its OA-02 Python checker. Trust/tooling comes
from the workflow's `github.workflow_sha` checkout, separately from candidate data.

Public trust digests use `sha256(canonical(public))`, where `canonical` is UTF-8
JSON with sorted keys, compact separators and one trailing newline. For SSH,
`public` has `format`, `fingerprint`, `identity`, `allowed_signers_sha256` and
`revocations_sha256` (null when absent). Include the effective configured
revocation file if no explicit policy file overrides it. For OpenPGP it has
`format`, uppercase primary `fingerprint` and `public_keyring_sha256`. Only
`pubring.kbx` or `pubring.gpg` directly in the existing GnuPG home is digested;
keyboxd-only homes are currently unsupported. Never hash/export the whole home
or any private-key file to produce this binding.

## Operator-controlled public policy

[The example](../release-trust-policy.example.json) is intentionally unarmed.
The operator fills real public source digests, environment IDs/reviewer user IDs,
signature identity/configuration and a freshness limit, then installs the
approved JSON as `RELEASE_TRUST_POLICY_JSON` in the relevant protected workflow
environments. This setting is not exposed as a dispatch input and must never
come from the boundary artifact itself. No environment setting was configured
by this implementation.

SSH signature policy keys are shown in the example. OpenPGP instead uses
`format: openpgp`, full `fingerprint`, absolute existing `gnupg_home`, absolute
`public_keyring_file`, and `public_trust_sha256`. SSH public trust files must
already exist at the approved runner-local paths; GnuPG uses an existing public
verification home. No production job imports or generates keys, downloads trust
from the candidate, or acquires a runner/tool. A missing trust file, approved
policy or installed tool fails/blocks the affected evidence.

Both source workflows must have operator-pinned SHA256 values. The consumer
also checks repository identity, fork exclusion, main branch, workflow path,
event, full candidate, successful completed source run, server artifact digest,
freshness and exact review environment/user IDs. GitHub's current review schema
contains no review timestamp or attempt binding. Therefore first-attempt runs
and unambiguous approved review histories are required; reruns need a fresh
independently approved dispatch rather than reused approval history. A rejected
or pending history is not silently treated as approved.

## Consumer commands at the final boundary

From a clean exact candidate checkout, use a fresh external output directory:

```bash
python3 .github/scripts/trusted-release.py --workspace "$PWD" \
  --candidate "$CANDIDATE_SHA" --tag "$RELEASE_TAG" --artifact-id "$BOUNDARY_ARTIFACT_ID" \
  --policy "$APPROVED_PUBLIC_POLICY" \
  --contract .kbd-orchestrator/phases/phase-0-4-operational-assurance/adopted-contract.json \
  --output "$REHEARSAL_OUT"

python3 .github/scripts/tests/trusted-publishing-fixtures.py --workspace "$PWD" \
  --policy "$APPROVED_PUBLIC_POLICY" --artifact-id "$BOUNDARY_ARTIFACT_ID" \
  --contract .kbd-orchestrator/phases/phase-0-4-operational-assurance/adopted-contract.json \
  --output "$FIXTURE_OUT"
```

These commands are unexecuted instructions. The fixture requires real approved
remote provenance and a real authorized candidate tag; there is no local fake
approval/API-server positive path. It exercises the production rehearsal, then
named CLI and shared-validator negative controls. Its Cargo shim records/rejects
every invocation; any such call fails the suite. Synthetic mutations used to
isolate baseline/review/operator failure controls stay in memory, are never
uploaded or passed as trusted provenance, and are not executed PASS receipts.
The authored suite contains eighteen cases. Capture raw stdout/stderr, exact
argv/exit, all expanded command logs, `release-gate.json` and the actual fixture
summary. Missing API/authority is BLOCKED; no receipt is manufactured to avoid it.

The CLI defaults to `rehearse`, which invokes no Cargo command. `authorize` also
invokes no Cargo, but additionally requires certification, exact version tag and
the current active first-attempt main-branch release run's independently approved
`release-crates-io` environment. `publish` repeats every validation and requires
the OIDC action's per-step temporary output. The supported publication path is
the protected workflow, never a local JSON claiming that an operator approved it.

## Publication and current limits

[release.yml](../workflows/release.yml) exposes explicit `prepare`, default
`rehearse`, and separately approved `publish` operations. Preparation uses
OA-02's already installed pinned tools in a separate directory and uploads only
its plan/log. Rehearsal has read permissions. Only the protected publication job
has `id-token: write`, alongside contents/actions read for necessary inspection.
Its pre-OIDC step rejects incomplete/unapproved certificates. The pinned
official Rust OIDC action supplies the sole credential to the final step.
Publication uses a fresh Cargo home, an allowlisted environment and the exact
seven-crate workspace/FFI exclusion with `--locked --registry crates-io`.
There is no registry-token secret, cached credential or long-lived fallback.

The workflow records a gate result before Cargo and marks `publish_invoked`
before that irreversible call. A partial failure needs an operator registry
inspection and fix-forward decision, never an automatic rerun. Actual crate
installation verification and publication receipts still belong to the later
G-05-authorized boundary/publication process.

[Source/API pins](../release-publishing-sources.json) record official reads and
the full OIDC action commit. All execution remains NOT_EXERCISED: no workflow
dispatch, trusted artifact, review history, live trust configuration, OIDC
exchange, publication, syntax check, build, scan, fixture or review was performed.
G-04's seven actual publisher/environment settings and G-05 approval remain
operator-controlled and unverified. G-01 remains deferred/non-blocking. This
protocol prepares those operations without claiming they happened.
