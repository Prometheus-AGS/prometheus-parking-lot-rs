# Publishing the workspace to crates.io — Playbook

> **Status (2026-10-24):** the seven publishable crates were released manually at **0.3.0**;
> FFI remains unpublished. The **0.4.0 preparation implementation** is now present under
> [ADR 0005](adr/0005-lockstep-release-versioning.md) and `scripts/release/`, but its real-tool
> final-boundary execution and pinned-tool availability are **unverified**. This is not a claim
> that 0.4.0 is certified, signed or published, or that OIDC publisher settings are configured.
> The earlier cr-52 manual procedure below is retained as release history and pre-flight context.

**A publish is permanent.** A version can be yanked, never deleted or overwritten. Nobody runs
`cargo publish` without `--dry-run`, creates the tag, or cuts the GitHub release without the
operator's explicit approval (see section 6). The phase gate runs everything up to and including
`cargo package`; it never publishes.

## 1. What is published

| Crate | Directory | Published |
| --- | --- | --- |
| `prometheus-parking-lot-core` | `crates/parking-lot-core` | yes |
| `prometheus-parking-lot-runtime-tokio` | `crates/parking-lot-runtime-tokio` | yes |
| `prometheus-parking-lot-pg` | `crates/parking-lot-pg` | yes |
| `prometheus-parking-lot-embedded` | `crates/parking-lot-embedded` | yes |
| `prometheus-parking-lot-iggy` | `crates/parking-lot-iggy` | yes |
| `prometheus-parking-lot-wasm` | `crates/parking-lot-wasm` | yes |
| `prometheus_parking_lot` (the facade) | repository root | yes |
| `prometheus-parking-lot-ffi` | `crates/parking-lot-ffi` | no (`publish = false`; an application-side artifact, ADR 0001) |

All versions come from `[workspace.package]`, and every inter-crate dependency in
`[workspace.dependencies]` pins the same version next to its `path`, so a published crate depends on the
published version of its siblings. They are bumped together. [ADR 0005](adr/0005-lockstep-release-versioning.md)
adopts unconditional lockstep for all seven publishable packages, including unchanged packages.
FFI inherits the workspace version but stays outside the release set. The checked-in baseline
remains 0.3.0 until an explicit preparation invocation requests 0.4.0.

### 1.1 Prepare 0.4.0 without publishing

The supported entry point is [scripts/release/prepare.py](../scripts/release/prepare.py).
Its policy lives in [release-plz.toml](../release-plz.toml), [cliff.toml](../cliff.toml)
and [release-tools.toml](../release-tools.toml). Supply **release-plz 0.3.173** and
**git-cliff 2.14.2** as existing local executables. Their official release/source evidence
is recorded in ADR 0005; installation or availability on this host has not been verified.
The script checks exact version output and records binary SHA-256 hashes when it executes.

Run only at the completed implementation boundary (oa-18 task 2), serializing the Cargo
collaborator with other workspace work. Python 3.11 or newer, Git, the pinned Rust toolchain
and the necessary cached dependencies/metadata must already be available. No global install
is part of preparation: Cargo is offline and rustup automatic installation is disabled.
Missing resources produce failed or BLOCKED evidence, never PASS.

From the frozen candidate's repository root, supply full lowercase 40-character commit SHAs
in `CANDIDATE_SHA` and `BASE_SHA`, an explicit ISO `RELEASE_DATE`, absolute executable paths
in `RELEASE_PLZ_BIN` and `GIT_CLIFF_BIN`, and a fresh absolute `PREPARATION_DIR` outside the
source checkout. The base must be an ancestor of the candidate.

```bash
python3 scripts/release/prepare.py check --workspace "$PWD"
python3 scripts/release/prepare.py prepare   --source "$PWD" --candidate "$CANDIDATE_SHA" --base "$BASE_SHA"   --version 0.4.0 --date "$RELEASE_DATE"   --release-plz "$RELEASE_PLZ_BIN" --git-cliff "$GIT_CLIFF_BIN"   --output "$PREPARATION_DIR"
```

Preparation writes only to `PREPARATION_DIR`: a detached local clone at `workspace/`,
with its origin removed, plus a deterministic `release-plan.json` and raw collaborator
commands/results in `commands.json`. It rejects inconsistent versions before running the
release tools. It then generates seven package changelog subsections from the frozen commit
range, sets the shared version and sibling requirements with `release-plz set-version`,
and checks the resulting manifests and lockfile. It creates no commit, tag, upload or release.
The source checkout stays at its original version; integrating a prepared release candidate
is a separate driver step, and requires the final gate before publication.

To exercise the adopted OA-02 oracle, the final boundary uses the real regression entry point
instead of treating a standalone preparation or static inspection as a PASS. Supply a fresh
absolute `EVIDENCE_DIR`; the fixture preserves its prepared output at `oa-02/prepared/`:

```bash
python3 scripts/release/test-preparation.py   --source "$PWD" --candidate "$CANDIDATE_SHA" --base "$BASE_SHA"   --version 0.4.0 --date "$RELEASE_DATE"   --release-plz "$RELEASE_PLZ_BIN" --git-cliff "$GIT_CLIFF_BIN"   --output "$EVIDENCE_DIR/oa-02"
```

The fixture runs production preparation twice, compares output hashes and the release plan,
and injects mismatched dependency, package, lockfile, FFI-publish and version-group controls.
Each control must fail at its named policy diagnostic. It retains first/second tool logs.
These commands are instructions for future boundary execution, not observed results.
`version_group` alone is insufficient: release-plz synchronizes only changed packages, so
running `release-plz update` alone does not establish this unconditional lockstep policy.

**Publish order.** A crate must be on the registry before a crate that depends on it can be published:

1. `prometheus-parking-lot-core`
2. `prometheus-parking-lot-runtime-tokio`, `-pg`, `-embedded`, `-iggy`, `-wasm` (each depends on core and
   on no other workspace crate, so any order among them)
3. `prometheus_parking_lot` (the facade; depends on all of the above)

`cargo publish --workspace --exclude prometheus-parking-lot-ffi` does this ordering itself (cargo 1.99
supports `--workspace`; see `cargo publish --help`): it sorts the packages by dependency, publishes each and
waits for the registry index to show a crate before publishing its dependents. Prefer it. The explicit
per-crate commands in section 5 are the fallback.

## 2. One-time setup

1. The publishing account has a verified crates.io email.
2. **Trusted publishing (oa-04, separate operator gate G-04).** The seven 0.3.0 crates
   already exist following manual publication. The operator must establish and provide evidence
   of their GitHub/crates.io trusted publisher settings and repository environment permissions
   before the 0.4.0 workflow can publish using short-lived OIDC credentials. Preparation does
   not configure those settings or prove that they exist. Credential rotation and exposed KBD
   signer replacement are explicitly **deferred and nonblocking** under G-01; this guide makes
   no claim that a token was retired, revoked or rotated. Existing exposure remains.
3. Never put a token in a command line, a workflow file or the repository.

## 3. Pre-flight (every release)

Run from a clean checkout of the release commit, with the pinned toolchain (`rust-toolchain.toml`, 1.99.1).
Do not publish on red.

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo nextest run --workspace --all-features --locked --no-tests=fail
cargo test --workspace --all-features --locked --doc
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --locked
cargo +1.95 check --workspace --all-targets --all-features --locked     # MSRV (workspace.package.rust-version)
cargo deny --workspace --all-features --locked check                    # licenses, advisories, bans
cargo check --target wasm32-unknown-unknown -p prometheus_parking_lot --no-default-features --locked
bash .github/scripts/semver-check.sh                                    # section 4
```

CI runs the same set (`.github/workflows/ci.yml`, `platform.yml`). Also confirm:

- [ ] `workspace.package.version` and every workspace-crate `version =` in `[workspace.dependencies]` agree.
- [ ] `CHANGELOG.md` has the release section; cr-52 replaces `Unreleased` with the date.
- [ ] `README.md`, `docs/API.md` and the crate READMEs describe the code at this commit.
- [ ] `Cargo.lock` is committed and `--locked` passes.

### Package inspection

```bash
# Every publishable crate: does it build from its packaged sources, and what ships?
cargo package --workspace --locked --exclude prometheus-parking-lot-ffi
for c in core runtime-tokio pg embedded iggy wasm; do
  cargo package --list --locked -p prometheus-parking-lot-$c
done
cargo package --list --locked -p prometheus_parking_lot
```

`cargo package --workspace` verifies each crate by building it from its packaged tarball, resolving sibling
crates through a local overlay, so it works before anything is on the registry. Each list must contain
`Cargo.toml`, `README.md`, `LICENSE-MIT` and `src/`, and nothing sensitive (no `.env`, keys,
`.kbd-orchestrator/`, `.prometheus/`, `target/`). Every manifest gates the file list with an `include`
allowlist.

Pitfalls that have broken this step:

- A dev-dependency that names a workspace crate with `workspace = true` keeps its `version` and must
  resolve on crates.io. A crate's dev-dependency on **itself** (to turn on a test feature) is therefore
  written `{ path = ".", features = [...] }`: cargo strips a dependency that has no version when it
  packages.
- A `readme` or license file named in `include` that does not exist is silently omitted, not an error.
  Check the `--list` output.

## 4. Semver gate

`.github/scripts/semver-check.sh` runs `cargo semver-checks --workspace --all-features` against the newest
release tag `v<MAJOR>.<MINOR>.<PATCH>`. Before the manual 0.3.0 release there was no registry
baseline; that historical absence is not a 0.4.0 verification result. At the final boundary,
record the actual baseline tag/commit and command output. A skipped no-baseline check is not
semver compatibility evidence. A breaking pre-1.0 change requires a minor version increment;
0.4.0 preparation uses section 1.1's explicit workspace-version path.

## 5. Dry run, publish, tag

### 5.1 Dry run (mandatory)

```bash
cargo publish --workspace --exclude prometheus-parking-lot-ffi --dry-run --locked
```

`--dry-run` packages and verifies every crate and stops each one before the upload. With `--workspace`
cargo resolves the sibling crates through a local overlay, so the whole set dry-runs cleanly before anything
is on the registry (checked 2026-10-23: all seven crates reach "aborting upload due to dry run").

The per-crate form does not: `cargo publish -p prometheus-parking-lot-runtime-tokio --dry-run` fails with
"no matching package named `prometheus-parking-lot-core` found" until core is really uploaded, because a
single-package verify resolves its dependencies from the registry. Only `-p prometheus-parking-lot-core`
dry-runs on its own. That is expected for the first release; use the `--workspace` form for the dry run.

### 5.2 Publish (operator approval required)

**0.4.0 trusted release tooling (oa-04).** The authored
[release workflow](../.github/workflows/release.yml) has separate `prepare`, default
`rehearse`, and explicitly selected `publish` operations. Nothing has been dispatched,
exchanged through OIDC or published by this implementation. G-04 settings and G-05
publication approval remain operator-controlled and unverified.

The shared [release evidence protocol](../.github/scripts/RELEASE_EVIDENCE.md),
[schema](../.github/release-boundary.schema.json) and
[consumer](../.github/scripts/trusted-release.py) require a numeric GitHub artifact ID,
not caller JSON declaring approval. The reserved OA-18 producer is
`operational-assurance.yml`; artifact `operational-assurance-boundary` contains exactly
`boundary-receipt.json` and the original OA-02 `release-plan.json`. The consumer verifies
server archive digest, repository/candidate, operator-pinned workflow source digest,
successful first-attempt run and actual environment review history through GitHub's API.
The first-attempt restriction prevents reuse of approval history that the API does not bind
to a rerun. Stale, expired, unapproved or mismatched evidence fails before any publish call.

`purpose=rehearsal` packets retain `NOT_EXERCISED` status and cannot authorize publication.
Only `purpose=certification` with all seventeen scenarios, baseline, independent review and
G-02/G-03/G-04 observed receipts PASS is publishable. The gate re-verifies the real signed tag,
full frozen candidate and public trust digest; checks exactly seven 0.4.0 crates, FFI exclusion
and the integrated prepared-file hashes; and checks the current publication run's independent
`release-crates-io` approval. A requested publish never falls back to rehearsal.

The operator must approve/configure the public `RELEASE_TRUST_POLICY_JSON` environment
setting using the intentionally unarmed [policy example](../.github/release-trust-policy.example.json),
including actual source digests, environment/reviewer IDs and existing runner-local public
signing trust. Protect main and the named environments before using this workflow. Configure
all seven crates' trusted publisher entries for `release.yml` and `release-crates-io` only
through the separately authorized G-04 operation. No setup, tool install or key import occurs
automatically. Existing preparation executables remain the pinned OA-02 inputs.

Only the publication job has `id-token: write` (with contents/actions read for verification).
It first validates certification, then uses the official Rust authentication action pinned at
`3268eb413133e2960fa4ae0e963ca76380c54cd9` (v1.0.6). Its temporary output is the sole registry
credential. The final gate revalidates everything before `cargo publish`; it uses a fresh Cargo
home and has no long-lived token secret, cached credential or token fallback. This does not
retire, rotate or revoke existing credentials; G-01 remains deferred and non-blocking.

At the completed-phase boundary, provide a real approved remote rehearsal artifact and a
clean exact candidate, capture raw logs/exit and run the real rehearsal fixtures:

```bash
python3 .github/scripts/tests/trusted-publishing-fixtures.py --workspace "$PWD" \
  --policy "$APPROVED_PUBLIC_POLICY" --artifact-id "$BOUNDARY_ARTIFACT_ID" \
  --contract .kbd-orchestrator/phases/phase-0-4-operational-assurance/adopted-contract.json \
  --output "$FIXTURE_OUT"
```

The fixture exercises real Git/signature and approved GitHub provenance paths. Its Cargo shim
records/rejects any unexpected invocation; local mutation controls supplement the real path.
No fake local approval/API positive fixture substitutes for actual authority. Missing approved
API/environment access is BLOCKED. The source and [official API/action pins](../.github/release-publishing-sources.json)
are authored; no fixture, scan, syntax check, build or independent review has run. Preparation,
rehearsal and implementation closure are not publication receipts.


For 0.4.0, the approval-gated workflow is owned by oa-04 and requires independently
verified OIDC settings (G-04), the frozen candidate's boundary PASS receipt and explicit
publication approval (G-05). The preparation implementation does not satisfy those gates.
Release signing authority and exact candidate/tag verification are separate oa-03/G-03 work.

Manual fallback, in order, waiting for each crate to appear in the index before the next:

```bash
cargo publish --locked -p prometheus-parking-lot-core
cargo publish --locked -p prometheus-parking-lot-runtime-tokio
cargo publish --locked -p prometheus-parking-lot-pg
cargo publish --locked -p prometheus-parking-lot-embedded
cargo publish --locked -p prometheus-parking-lot-iggy
cargo publish --locked -p prometheus-parking-lot-wasm
cargo publish --locked -p prometheus_parking_lot
```

Do not publish the facade with `--all-features`: the verify step only builds the package being published,
and the feature matrix is checked in section 3.

If publication stops midway, record which versions reached the registry. Any fix-forward
version plan must preserve the seven-crate lockstep policy; do not independently bump only
the failed crate and its dependents. A yank still requires separate operator approval.

### 5.3 Tag and verify

**0.4.0 verification tooling (oa-03, G-03).**
[The verifier](../.github/scripts/verify-release-tag.py) requires an existing annotated
signed tag, its exact short name, the full frozen candidate commit ID and an independently
operator-approved signing identity. It rejects lightweight/unsigned tags, renamed tag objects,
a different direct commit target and invalid signatures. It verifies the captured tag object
and rejects a tag ref that moves during inspection. It creates no tag and performs no publish.

Select one operator-approved trust mode. Do not derive the expected identity from the tag being
checked, its message, a key comment, an email alone or a hosting-provider verification badge.
The following commands are future boundary instructions, not observed results:

```bash
python3 .github/scripts/verify-release-tag.py \
  --tag "$RELEASE_TAG" --candidate "$RELEASE_CANDIDATE" --format ssh \
  --identity "$RELEASE_SSH_IDENTITY" --fingerprint "$RELEASE_SSH_FINGERPRINT" \
  --allowed-signers "$RELEASE_SSH_ALLOWED_SIGNERS_FILE"

python3 .github/scripts/verify-release-tag.py \
  --tag "$RELEASE_TAG" --candidate "$RELEASE_CANDIDATE" --format openpgp \
  --fingerprint "$RELEASE_GPG_FINGERPRINT" --gnupg-home "$RELEASE_GPG_HOME"
```

SSH mode pins both the exact principal and full SHA256 fingerprint and uses the operator's
public allowed-signers file. Optional `--revocations` supplies an existing approved revocation
file; configured revocations still apply when this option is omitted. OpenPGP mode pins a full
primary fingerprint, including when a signing subkey made the signature. It uses the existing
approved absolute GnuPG home and the verification-only wrapper, disables automatic key discovery/agent
startup and rejects bad, expired or revoked signature/key status. UID text and local ownertrust
alone are not release authority. No production key generation, import, private-key copying or
host/global trust configuration is performed by this tooling.

[The read-only reusable workflow](../.github/workflows/verify-release-tag.yml) accepts
`candidate`, `tag` and an existing approved `runner` label. The operator must protect the default
branch and the `release-signature-verification` environment before use. Environment variables
`RELEASE_SIGNATURE_FORMAT`, `RELEASE_SSH_IDENTITY`, `RELEASE_SSH_FINGERPRINT` and
`RELEASE_SSH_ALLOWED_SIGNERS` select SSH trust; the last is **public allowed-signers text**,
never a private key. Optional `RELEASE_SSH_REVOCATIONS_FILE` names an existing runner-local
file. OpenPGP uses `RELEASE_GPG_FINGERPRINT` and `RELEASE_GPG_HOME`; the existing approved
runner must already have that public verification home. Missing trust/tooling fails; there is
no key import, runner acquisition or fallback to merely accepting any valid signature.
These environment settings, signing authority and runner availability remain unverified.

At the final completed-phase boundary, capture exact argv/exit and raw public-metadata logs:

```bash
bash .github/scripts/tests/release-tag-fixtures.sh
```

The real Git/SSH/OpenPGP fixtures generate only throwaway identities inside their owned
private temporary directory at that future invocation, without using host signing agents or
copying private keys. They include authorized SSH/OpenPGP tags plus separate unsigned,
wrong-identity, wrong-commit, renamed-object, tampered-signature and missing-trust controls.
Their source is present; no fixture, verifier, syntax check or review has run. Temporary key
material is never an evidence artifact. Bind the actual verifier PASS JSON to candidate,
tag object, identity, approved trust/configuration digest and the final contract packet.
Fixture success establishes the tooling path, not the operator's real G-03 release authority.
The actual release tag and all publication actions still require G-05.


The following is the historical 0.3.0 signing command from the manual playbook;
it is not evidence that a signature was created or verified, and must not be rerun
as a 0.4.0 instruction:

```text
git tag -s v0.3.0 -m "v0.3.0" && git push origin v0.3.0
git tag -v v0.3.0
```

For 0.4.0, oa-03/G-03 must verify the operator-authorized signing identity and
exact candidate commit. Actual tag creation/push and the GitHub release remain
separately subject to G-05; neither occurs during preparation. The 0.3.0 checks
below are retained as historical playbook context, not a new release receipt.

- [ ] A fresh project runs `cargo add prometheus_parking_lot@0.3.0 --features postgres,embedded` and builds.
- [ ] Each crate page renders on crates.io and its docs.rs build succeeded (`https://docs.rs/<crate>`;
  `prometheus-parking-lot-wasm` builds for `wasm32-unknown-unknown`).
- [ ] The GitHub release lists the 0.3.0 changelog; the docs site is deployed for the tag.

## 6. Operator approval

These steps always wait for an explicit operator instruction; no agent and no scheduled job runs them on its
own authority:

- the first publish of any crate, and every later `cargo publish` without `--dry-run`;
- creating or pushing the `v*` tag and the GitHub release;
- configuring or changing crates.io trusted publishers and API tokens;
- yanking a version.

Agents implement preparation and fixtures first. The final completed-phase boundary runs
the dry run/pre-flight and records actual results; task completion alone is not certification.
The authoritative [operator gates](../.kbd-orchestrator/phases/phase-0-4-operational-assurance/operator-gates.md)
keep signing identity (G-03), OIDC/account setup (G-04) and actual publication (G-05)
separate. G-01 explicitly defers credential rotation and KBD signer replacement without
blocking this implementation or release contract. No credential or signer change is implied.

## 7. Post-release

- [ ] Add crate owners if needed: `cargo owner --add <github-user-or-team> <crate>`.
- [ ] Open the next `Unreleased` section of `CHANGELOG.md`.
- [ ] Broken release: `cargo yank --version X.Y.Z <crate>` (`--undo` reverses it). Yanking blocks new
  dependents but does not break existing lockfiles.
