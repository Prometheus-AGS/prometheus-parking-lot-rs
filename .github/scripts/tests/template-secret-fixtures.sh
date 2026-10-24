#!/usr/bin/env bash
# OA-05 real Git/gitleaks fixtures. Run only at the completed phase boundary:
#   bash .github/scripts/tests/template-secret-fixtures.sh
# Uses the repository's complete rules and actual example documents. Canaries
# are nonfunctional synthetic values assembled only inside an isolated fixture.
# CLI flags follow the pinned v8.30.1 upstream cmd/{detect,git}.go and README:
# https://github.com/gitleaks/gitleaks/tree/v8.30.1
set -euo pipefail

source_root=$(cd "$(dirname "$0")/../../.." && pwd)
command -v git >/dev/null || { echo "BLOCKED: real Git is required" >&2; exit 2; }
command -v gitleaks >/dev/null || { echo "BLOCKED: real gitleaks is required" >&2; exit 2; }
fixture_root=$(mktemp -d "${TMPDIR:-/tmp}/oa05-template-secrets.XXXXXX")
trap 'rm -rf -- "$fixture_root"' EXIT

unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_OBJECT_DIRECTORY GIT_ALTERNATE_OBJECT_DIRECTORIES
unset GITLEAKS_CONFIG GITLEAKS_CONFIG_TOML
export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null
export GIT_AUTHOR_NAME='Template fixture' GIT_COMMITTER_NAME='Template fixture'
export GIT_AUTHOR_EMAIL='fixture@example.invalid' GIT_COMMITTER_EMAIL='fixture@example.invalid'
export GIT_TEMPLATE_DIR="$fixture_root/empty-template" GIT_TERMINAL_PROMPT=0
mkdir "$GIT_TEMPLATE_DIR"
repo="$fixture_root/repo"
git init -q "$repo"
cp "$source_root/.gitleaks.toml" "$repo/.gitleaks.toml"
printf 'Isolated template scan fixture\n' > "$repo/README.md"
git -C "$repo" add .
git -C "$repo" -c core.hooksPath=/dev/null -c commit.gpgsign=false commit -qm 'Fixture baseline'
baseline=$(git -C "$repo" rev-parse HEAD)
checks=0
sk_canary=""
default_canary=""

fail() { echo "FAIL: $*" >&2; exit 1; }

assert_redacted() {
  local file value status
  for file in "$@"; do
    [ -f "$file" ] || fail "missing scanner output artifact"
    for value in "$sk_canary" "$default_canary"; do
      [ -n "$value" ] || continue
      if grep -Fq -- "$value" "$file"; then
        # Never print the offending artifact or the synthetic value on failure.
        fail "scanner output contains an unredacted synthetic canary"
      else
        status=$?
        [ "$status" -eq 1 ] || fail "cannot inspect scanner output redaction"
      fi
    done
  done
}

expect_scan() {
  local label="$1" expected="$2" rule="$3" path="$4" actual=0 log report
  shift 4
  checks=$((checks + 1))
  log="$fixture_root/scan-$checks.txt"
  report="$fixture_root/scan-$checks.json"
  printf '\nfixture %s: %s; expected exit %s\n' "$checks" "$label" "$expected"
  printf 'cwd: %s\n' "$repo"
  printf 'command: gitleaks'
  printf ' %q' "$@"
  printf ' %q' --no-banner --no-color --redact=100 --exit-code=10 \
    --config "$repo/.gitleaks.toml" --report-format=json --report-path "$report"
  printf '\n'
  (cd "$repo" && gitleaks "$@" --no-banner --no-color --redact=100 --exit-code=10 \
    --config "$repo/.gitleaks.toml" --report-format=json --report-path "$report") > "$log" 2>&1 || actual=$?
  # Check before emitting any raw logs or report metadata to the boundary packet.
  assert_redacted "$log"
  if [ -f "$report" ]; then
    assert_redacted "$report"
  fi
  cat "$log"
  [ "$actual" -eq "$expected" ] || fail "$label (actual exit $actual; expected $expected)"
  if [ -n "$rule" ]; then
    [ -s "$report" ] || fail "$label produced no finding report"
    grep -Eq '"RuleID"[[:space:]]*:[[:space:]]*"'"$rule"'"' "$report" \
      || fail "$label did not detect the required rule"
    grep -Fq -- "$path" "$report" || fail "$label did not identify the expected fixture path"
    cat "$report"
  fi
  echo "PASS: $label (exit $actual; required finding/redaction oracle matched)"
}

# Copy the actual complete documents, not reconstructed snippets or regex mocks.
mkdir -p "$repo/docs/coding-standards" "$repo/.e2b-template"
cp "$source_root/docs/coding-standards/README.md" "$repo/docs/coding-standards/README.md"
cp "$source_root/.e2b-template/SETUP.md" "$repo/.e2b-template/SETUP.md"
expect_scan 'actual examples pass the full-tree production scan' 0 '' '' detect --no-git --source .
git -C "$repo" add docs .e2b-template
expect_scan 'actual examples pass the real staged scan' 0 '' '' git --pre-commit --staged .
git -C "$repo" -c core.hooksPath=/dev/null -c commit.gpgsign=false commit -qm 'Sanitized actual examples'
expect_scan 'actual examples pass the real commit-diff scan' 0 '' '' git --log-opts="$baseline..HEAD" .

# Hash non-secret fixture labels at runtime. No complete secret-like canary is
# present in this tracked source. The generic control has no custom-rule prefix,
# so its finding independently proves inherited default rules still operate.
sk_canary=$(printf '%s%s%s' 's' 'k-' "$(printf 'OA05 synthetic prefix detector fixture' | git hash-object --stdin)")
default_canary="$(printf 'OA05 synthetic default detector fixture one' | git hash-object --stdin)$(printf 'OA05 synthetic default detector fixture two' | git hash-object --stdin)"
mkdir "$repo/production"
printf 'api_key = "%s"\n' "$sk_canary" > "$repo/production/prefix-control.txt"
expect_scan 'custom rule detects a canary outside example paths' 10 'sk-prefixed-api-key' 'production/prefix-control.txt' detect --no-git --source .
rm "$repo/production/prefix-control.txt"
printf 'api_key = "%s"\n' "$default_canary" > "$repo/production/default-control.txt"
expect_scan 'inherited default rule detects a separate canary' 10 'generic-api-key' 'production/default-control.txt' detect --no-git --source .
rm "$repo/production/default-control.txt"

# The same canary must also be detected inside each exact example path. These
# controls reject a future blanket docs/template exception that hides secrets.
for path in docs/coding-standards/README.md .e2b-template/SETUP.md; do
  printf '\napi_key = "%s"\n' "$sk_canary" >> "$repo/$path"
  expect_scan "canary remains detectable within $path" 10 'sk-prefixed-api-key' "$path" detect --no-git --source .
  cp "$source_root/$path" "$repo/$path"
done

# A staged secret stays detectable even when the worktree copy is sanitized.
# The subsequent commit/range checks exercise real Git patch scanning as in CI.
printf 'api_key = "%s"\n' "$sk_canary" > "$repo/production/staged-control.txt"
git -C "$repo" add production/staged-control.txt
printf 'api_key = "example"\n' > "$repo/production/staged-control.txt"
expect_scan 'clean worktree does not expose staged-only canary to full-tree scan' 0 '' '' detect --no-git --source .
expect_scan 'staged canary is detected despite sanitized worktree' 10 'sk-prefixed-api-key' 'production/staged-control.txt' git --pre-commit --staged .
git -C "$repo" -c core.hooksPath=/dev/null -c commit.gpgsign=false commit -qm 'Runtime synthetic staged control'
expect_scan 'committed canary is detected in CI-style range scan' 10 'sk-prefixed-api-key' 'production/staged-control.txt' git --log-opts="$baseline..HEAD" .

printf '\nPASS: OA-05 template fixture oracles matched (%s checks; real Git/gitleaks, no allowlist additions)\n' "$checks"
