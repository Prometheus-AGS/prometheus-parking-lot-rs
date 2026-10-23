#!/usr/bin/env bash
# Semver gate (cr-39). Usage: semver-check.sh
#
# Baseline: the newest release tag `v<MAJOR>.<MINOR>.<PATCH>` (cr-52 creates v0.3.0).
# Nothing from this workspace has ever been published to crates.io (checked
# 2026-10-23), so there is no registry baseline and the 0.2.0 sources are not a
# release: the intended breaking changes of 0.3.0 are not violations. Until the
# first release tag exists the gate reports that and passes; from v0.3.0 on, a
# breaking change fails unless the crate version in Cargo.toml has been bumped
# accordingly (release-plz does that in the release PR).
#
# Exit codes: 0 pass or no baseline yet; cargo-semver-checks' own 100 on a violation.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

tag=$(git tag --list 'v[0-9]*.[0-9]*.[0-9]*' --sort=-v:refname | head -n 1)
if [ -z "$tag" ]; then
  echo "semver: no release tag (v*.*.*) in this clone; no baseline to compare against. Passing."
  echo "::notice::semver gate inactive until the first release tag exists (cr-52, v0.3.0)"
  exit 0
fi
echo "semver: baseline $tag ($(git rev-parse --short "$tag"))"
# Unpublished crates (publish = false: the FFI crate) are skipped by the tool itself.
exec cargo semver-checks --workspace --all-features --baseline-rev "$tag"
