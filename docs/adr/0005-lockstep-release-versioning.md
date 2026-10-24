# ADR 0005: Lockstep versions and nonpublishing release preparation

- Status: Accepted for implementation; final OA-02 execution pending
- Date: 2026-10-24
- Scope: oa-02-release-versioning / #86; seven published crates, unpublished FFI

## Decision

Keep one stable `MAJOR.MINOR.PATCH` in `workspace.package.version` and the same
version requirement on every internal dependency. All seven published packages
participate in every release, including packages with no local changes. The
next planned release is **0.4.0**. The checked-in baseline remains **0.3.0**
until release preparation explicitly requests the new version.

The publish set, in dependency order, is `prometheus-parking-lot-core`,
`prometheus-parking-lot-runtime-tokio`, `prometheus-parking-lot-pg`,
`prometheus-parking-lot-embedded`, `prometheus-parking-lot-iggy`,
`prometheus-parking-lot-wasm`, then the underscore-named facade
`prometheus_parking_lot`. `prometheus-parking-lot-ffi` stays `publish = false`.
It inherits the workspace version as an application artifact; inheritance
does not make it an eighth release package.

`release-plz.toml` selects exactly those seven packages in one `version_group`.
Its defaults disable publishing, tag creation and GitHub releases, and set
`release_always = false`. The later approval-gated workflow owns publication.
This configuration is preparation policy, not authorization to run a release.
`cliff.toml` renders local commit messages and full commit IDs in a fixed order,
without remote enrichment, current timestamps or executable preprocessors.

The uncomfortable limitation: release-plz's `version_group` only affects changed
packages. It does not independently establish the unconditional lockstep policy.
The supported path therefore runs `release-plz set-version <version>` explicitly
against the workspace, then rejects any inconsistent package/dependency/lockfile
version. Running `release-plz update` alone is not a certified preparation path.

## Supported production path

`scripts/release/prepare.py` requires Python 3.11 or newer (standard-library
`tomllib`), a local Git checkout containing the entire selected history, full
base and candidate commit SHAs, a stable release version, an explicit ISO date,
and locally supplied pinned executables. It installs nothing. `prepare` clones
the candidate into a separate output directory, detaches HEAD and removes its
origin. Source manifests and the source changelog are never changed.

Before either release tool is invoked, the checker rejects package membership,
version inheritance, internal dependency requirements/paths, lockfile versions,
FFI publish eligibility or release configuration that disagrees with this policy.
This makes the required mismatched-dependency control fail instead of silently
repairing it. It accepts the incumbent path-only self dev-dependency used for
embedded fault-injection tests, which Cargo omits when packaging.

Preparation then checks exact tool version output and records executable SHA-256
hashes. It generates one root changelog section containing exactly seven package
subsections from the immutable `base..candidate` history. Each package includes
its own directory and the shared root manifest. Unchanged packages get an
explicit lockstep entry. Existing historical changelog prose is retained from
the candidate. The date is supplied, never read from the clock.

The pinned release-plz `set-version` implementation visits **all** inheriting
members, even `release = false` members, and requires a readable changelog for
each. All eight configuration entries therefore point at the shared changelog;
release-plz deduplicates that path. The wrapper seeds its latest heading with
the current metadata version, then `set-version` renames that heading to the
requested version. No FFI changelog section is generated.

An existing output is accepted only for the identical source/base/candidate/
version/date tuple. The same preparation is executed again, rather than returning
early, so idempotence means the actual second tool run leaves the prepared
manifests, lockfile, changelog and deterministic `release-plan.json` unchanged.
Raw collaborator argv, cwd, exit, stdout and stderr are saved in `commands.json`;
copy that log between invocations if both runs' raw logs are needed. A failed
rerun removes the old success plan before checking, preventing stale receipt use.
Unexpected tracked writes outside root Cargo.toml, Cargo.lock and CHANGELOG.md
are rejected. Interrupted outputs can be retained for diagnosis; use a fresh
output directory rather than resetting an unknown directory.

The wrapper uses Cargo's offline mode and disables rustup automatic installation
with `RUSTUP_AUTO_INSTALL=0`, as documented in the official
[rustup environment reference](https://rust-lang.github.io/rustup/environment-variables.html).
It clears an ambient toolchain override so the candidate's pinned toolchain file
applies. `set-version` internally calls Cargo
metadata and refreshes workspace entries in Cargo.lock. The pinned toolchain,
cached metadata and dependencies must already be available at the final boundary;
missing resources produce failure/BLOCKED evidence, never a claimed pass.
No publishing command, signing command, account mutation or remote push exists
in this path. Package build/upload rehearsals belong to the final release gate.

## Verified official tool evidence

Checked 2026-10-24; these are selected pins, not assertions of local installation:

- **release-plz 0.3.173**, official
  [release-plz-v0.3.173](https://github.com/release-plz/release-plz/releases/tag/release-plz-v0.3.173),
  source commit `907af6d031bbb69d392354803c04cd75bb6e6fa0`.
  The official [configuration reference](https://release-plz.dev/docs/config)
  documents `version_group`, `release`, `release_always`, publication/tag flags
  and root-relative `changelog_path`. The official
  [set-version reference](https://release-plz.dev/docs/usage/set-version) and
  [tagged implementation](https://github.com/release-plz/release-plz/blob/release-plz-v0.3.173/crates/release_plz_core/src/command/set_version.rs)
  establish workspace inheritance, local dependency and lockfile updates,
  shared changelog deduplication and the inherited FFI behavior. The tagged
  [CLI arguments](https://github.com/release-plz/release-plz/blob/release-plz-v0.3.173/crates/release_plz/src/args/set_version.rs)
  and [config path arguments](https://github.com/release-plz/release-plz/blob/release-plz-v0.3.173/crates/release_plz/src/args/config_path.rs)
  establish `set-version <version> --manifest-path <path> --config <path>`.
- **git-cliff 2.14.2**, official
  [v2.14.2 release](https://github.com/orhun/git-cliff/releases/tag/v2.14.2)
  and [tagged package manifest](https://github.com/orhun/git-cliff/blob/v2.14.2/git-cliff/Cargo.toml).
  Its [Git configuration](https://git-cliff.org/docs/configuration/git/) and
  [tagged CLI arguments](https://github.com/orhun/git-cliff/blob/v2.14.2/git-cliff/src/args.rs)
  establish explicit ranges, include paths, sort order, `--offline` and `--no-exec`.
  `ignore_tags = ".*"` keeps that range in one output rather than splitting it
  into historical tagged releases. No remote integration is configured.

Exact pins also live in `release-tools.toml`; actual binary hashes and version
responses must be captured by the final execution. No global installation or
host/toolchain modification is part of this task.

## Regression and final-boundary handoff

At oa-18 task 2, supply absolute paths to existing pinned executables and the
frozen candidate and base SHAs. Run one fixture entry point:

```bash
python3 scripts/release/test-preparation.py \
  --source "$PWD" --candidate "$CANDIDATE_SHA" --base "$BASE_SHA" \
  --version 0.4.0 --date "$RELEASE_DATE" \
  --release-plz "$RELEASE_PLZ_BIN" --git-cliff "$GIT_CLIFF_BIN" \
  --output "$EVIDENCE_DIR/oa-02"
```

The fixture calls the production `prepare.py prepare` twice with those inputs,
retains the prepared workspace and plan, checks seven changelog subsections and
unchanged output hashes, then injects isolated dependency, package, lockfile,
FFI-eligibility and version-group mismatches. Each must fail with its precise
policy diagnostic before modifying the broken file. A missing tool, syntax
error or Cargo failure cannot stand in for a discriminating rejection.

Standalone production invocations use the same arguments:

```bash
python3 scripts/release/prepare.py check --workspace "$PWD"
python3 scripts/release/prepare.py prepare \
  --source "$PWD" --candidate "$CANDIDATE_SHA" --base "$BASE_SHA" \
  --version 0.4.0 --date "$RELEASE_DATE" \
  --release-plz "$RELEASE_PLZ_BIN" --git-cliff "$GIT_CLIFF_BIN" \
  --output "$PREPARATION_DIR"
```

No invocation above has run during implementation. They are final-boundary
instructions, not execution evidence. Integration certification, account setup,
signer verification, tag creation and actual publication remain separate gates.

## Alternatives and tradeoffs

Independent package versions would reduce unchanged-package releases but would
require a new compatibility policy and depart from the uniform 0.3.0 baseline.
Lockstep makes seven release artifacts necessary even for one-crate fixes; that
cost is intentional. Keep normal Cargo-compatible version requirements rather
than changing consumers to exact `=version` pins. The checker compares the
declared sibling requirement strings to the shared version; it does not change
third-party dependency ranges or perform a general dependency upgrade.
