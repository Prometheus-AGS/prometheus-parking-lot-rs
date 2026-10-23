# Publishing the workspace to crates.io — Playbook

> **Status (2026-10-23):** nothing from this workspace has ever been published; crates.io returns 404
> for every crate name below. 0.3.0 is the first publication. The release automation (release-plz,
> git-cliff, the release workflow, crates.io trusted publishing) is change **cr-52**
> (`openspec/changes/cr-52-release/`) and is not built yet; this playbook is the manual procedure it
> automates and the pre-flight it must keep passing.

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
published version of its siblings. They are bumped together.

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
2. **Trusted publishing (planned in cr-52, task 1.2).** Per crate, the operator adds the GitHub repository
   `Prometheus-AGS/prometheus-parking-lot-rs` and the release workflow as a trusted publisher in the
   crates.io crate settings, so CI publishes with a short-lived OIDC token and no long-lived secret. A
   crate must exist before its trusted publisher can be configured, so the **first** publish of each crate
   is made by the operator with a scoped, short-lived token (`publish-new`); configure trusted publishing
   straight after and revoke the token. The `CARGO_REGISTRY_TOKEN` in the operator's shell profile is
   retired (cr-52, task 1.2).
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
release tag `v<MAJOR>.<MINOR>.<PATCH>`. There is no tag until cr-52 creates `v0.3.0`, and no crates.io
baseline either, so until then the script reports that it has no baseline and passes. From `v0.3.0` on, a
breaking change fails unless the version has been bumped accordingly (pre-1.0: a breaking change bumps the
minor). The release PR (release-plz) does the bump.

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

Preferred, once cr-52's workflow exists: the release workflow publishes with trusted publishing, behind a
GitHub environment that requires the operator's approval and the recorded boundary-gate PASS receipt.

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

If a middle crate fails, fix forward with a patch version of the failed crate and its dependents; do not
leave a mixed set. Yank only a version that is broken for users.

### 5.3 Tag and verify

The release workflow creates the signed, annotated tag after the last crate is published:

```bash
git tag -s v0.3.0 -m "v0.3.0" && git push origin v0.3.0
git tag -v v0.3.0
```

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

Agents prepare the package, run the dry run and the pre-flight, and report the output.

## 7. Post-release

- [ ] Add crate owners if needed: `cargo owner --add <github-user-or-team> <crate>`.
- [ ] Open the next `Unreleased` section of `CHANGELOG.md`.
- [ ] Broken release: `cargo yank --version X.Y.Z <crate>` (`--undo` reverses it). Yanking blocks new
  dependents but does not break existing lockfiles.
