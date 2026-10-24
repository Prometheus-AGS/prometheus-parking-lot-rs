#!/usr/bin/env bash
# OA-01 real Git/gitleaks fixtures. Run only at the completed phase boundary:
#   bash .github/scripts/tests/containment-fixtures.sh
# All repositories, synthetic values, failure shims and logs live in one owned
# temporary directory. This does not inspect or rewrite the caller's history.
set -euo pipefail

source_root=$(cd "$(dirname "$0")/../../.." && pwd)
subject="$source_root/.github/scripts/supply-chain.sh"
real_git=$(command -v git) || { echo "BLOCKED: git is required" >&2; exit 2; }
command -v gitleaks >/dev/null || { echo "BLOCKED: real gitleaks is required" >&2; exit 2; }
fixture_root=$(mktemp -d "${TMPDIR:-/tmp}/oa01-containment.XXXXXX")
trap 'rm -rf -- "$fixture_root"' EXIT

# Isolate fixture identity/configuration from the operator's Git state. Signing,
# hooks and credentials are never used; all clone/fetch sources below are local.
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_OBJECT_DIRECTORY GIT_ALTERNATE_OBJECT_DIRECTORIES
# Preserve ambient scanner overrides: production gates must explicitly select
# the candidate policy. Controlled conflicting overrides are exercised below.
export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null
export GIT_AUTHOR_NAME='Containment fixture' GIT_COMMITTER_NAME='Containment fixture'
export GIT_AUTHOR_EMAIL='fixture@example.invalid' GIT_COMMITTER_EMAIL='fixture@example.invalid'
export GIT_TERMINAL_PROMPT=0
export GIT_TEMPLATE_DIR="$fixture_root/empty-template"
mkdir "$GIT_TEMPLATE_DIR"
checks=0

commit_fixture() {
  git -C "$REPO" -c core.hooksPath=/dev/null -c commit.gpgsign=false commit -qm "$1"
}

new_repo() {
  REPO="$fixture_root/$1"
  git init -q "$REPO"
  git -C "$REPO" symbolic-ref HEAD refs/heads/candidate
  mkdir -p "$REPO/.github"
  # Force the unarmed fixture state even after the production list is populated.
  printf '# Empty fixture roots\n' > "$REPO/.github/pre-purge-roots.txt"
  cp "$source_root/.gitleaks.toml" "$REPO/.gitleaks.toml"
  printf 'Isolated containment fixture: %s\n' "$1" > "$REPO/README.md"
  git -C "$REPO" add .
  commit_fixture 'Clean fixture baseline'
}

arm_repo() {
  printf '%s # isolated legacy fixture\n' "$old_root" > "$REPO/.github/pre-purge-roots.txt"
}

expect_gate() {
  local label="$1" expected="$2" message="$3" actual=0 log
  shift 3
  checks=$((checks + 1))
  log="$fixture_root/check-$checks.txt"
  printf '\nfixture %s: %s; expected exit %s\n' "$checks" "$label" "$expected"
  printf 'command: bash supply-chain.sh'
  printf ' %q' "$@"
  printf '\n'
  (cd "$REPO" && bash "$subject" "$@") >"$log" 2>&1 || actual=$?
  cat "$log"
  if [ "$actual" -ne "$expected" ] || ! grep -Fq -- "$message" "$log"; then
    echo "FAIL: $label (actual exit $actual; required message: $message)" >&2
    exit 1
  fi
  echo "PASS: $label (exit $actual; oracle matched)"
}

new_repo unarmed
expect_gate 'empty roots leave ancestry check unarmed' 0 'pre-purge: unarmed' pre-purge
expect_gate 'empty roots preserve informational path check' 0 'containment is not certified' path-history
expect_gate 'empty roots preserve informational real scan' 0 'containment is not certified' secrets-history
expect_gate 'release command refuses empty roots' 1 'release containment is unarmed' release-containment

# A controlled tool error must never count as a successful armed scan. The
# successful scans and finding controls elsewhere use the installed real tool.
scanner_shim="$fixture_root/scanner-error-bin"
mkdir "$scanner_shim"
cat > "$scanner_shim/gitleaks" <<'SHIM'
#!/usr/bin/env bash
echo 'controlled scanner failure' >&2
exit 42
SHIM
chmod +x "$scanner_shim/gitleaks"
PATH="$scanner_shim:$PATH" expect_gate 'unarmed scanner error stays informational' 0 'failed (exit 42)' secrets-history

# Keep the old ancestry in a separate repository so the clean candidate can be
# cloned with no old refs or objects, as it would be after an approved purge.
new_repo legacy
legacy_repo="$REPO"
mkdir "$REPO/.codex"
printf 'Non-secret synthetic tool state\n' > "$REPO/.codex/history.jsonl"
git -C "$REPO" add .codex/history.jsonl
commit_fixture 'Legacy tool-state path'
old_root=$(git -C "$REPO" rev-parse HEAD)
git -C "$REPO" rm -q .codex/history.jsonl
commit_fixture 'Remove legacy tool-state path from tip'
expect_gate 'pre-purge historical path finding stays informational' 0 'purged paths still in history' path-history
arm_repo
git -C "$REPO" add .github/pre-purge-roots.txt
commit_fixture 'Record isolated pre-purge root'
legacy_tip=$(git -C "$REPO" rev-parse HEAD)
expect_gate 'scratch old-ancestry branch is rejected' 1 'HEAD descends from pre-purge root' pre-purge
expect_gate 'removed historical credential path still blocks armed check' 1 'purged credential paths remain in history' path-history
expect_gate 'release command rejects old ancestry' 1 'HEAD descends from pre-purge root' release-containment

# Create an orphan candidate without deleting the legacy fixture branch. The
# clean clone below fetches only the candidate, with its entire ancestry.
git -C "$REPO" branch legacy "$legacy_tip"
git -C "$REPO" checkout -q --orphan clean-candidate
git -C "$REPO" rm -qrf .
mkdir -p "$REPO/.github"
cp "$source_root/.gitleaks.toml" "$REPO/.gitleaks.toml"
arm_repo
printf 'Clean orphan candidate\n' > "$REPO/README.md"
git -C "$REPO" add .
commit_fixture 'Clean orphan candidate'
printf 'Second clean commit\n' >> "$REPO/README.md"
git -C "$REPO" add README.md
commit_fixture 'Clean complete ancestry'
clean_source="$REPO"
REPO="$fixture_root/clean-clone"
git clone -q --no-local --single-branch --branch clean-candidate "$clean_source" "$REPO"
expect_gate 'fresh full clone accepts clean orphan candidate' 0 'release-containment: clean-history checks passed' release-containment
PATH="$scanner_shim:$PATH" expect_gate 'armed scanner error blocks' 1 'failed (exit 42)' secrets-history
PATH="$scanner_shim:$PATH" expect_gate 'release command propagates scanner error' 1 'failed (exit 42)' release-containment

# Fetching a real old ref must fail even while HEAD stays on the clean candidate.
git -C "$REPO" fetch -q "$legacy_repo" refs/heads/legacy:refs/heads/old-scratch
expect_gate 'all-ref path scan rejects retained legacy scratch ref' 1 'purged credential paths remain in history' path-history
git -C "$REPO" checkout -q old-scratch
expect_gate 'checked-out fetched scratch ancestry is rejected' 1 'HEAD descends from pre-purge root' pre-purge
git -C "$REPO" checkout -q clean-candidate
git -C "$REPO" branch -qD old-scratch

# Git's default path simplification can hide a side branch discarded by an
# ours merge. No side ref is retained; full-history traversal must still find it.
new_repo merged-path
arm_repo
git -C "$REPO" checkout -qb discarded-path
mkdir "$REPO/.codex"
printf 'Non-secret discarded tool state\n' > "$REPO/.codex/history.jsonl"
git -C "$REPO" add .codex/history.jsonl
commit_fixture 'Discarded side-branch path'
git -C "$REPO" checkout -q candidate
git -C "$REPO" -c core.hooksPath=/dev/null -c commit.gpgsign=false merge -q --no-ff -s ours discarded-path -m 'Discard side-branch tree'
git -C "$REPO" branch -qD discarded-path
expect_gate 'full path history includes discarded merge ancestry' 1 'purged credential paths remain in history' path-history

# Generate a nonfunctional catch-all-rule canary at runtime, never as a literal
# secret-like string in tracked sources. Remove it from HEAD to test history.
new_repo secret-history
arm_repo
before_secret=$(git -C "$REPO" rev-parse HEAD)
printf 'fixture_key = "%s%s%s"\n' 's' 'k-' "$(printf '%08x' 324508639 610839776 1437226410 252645135)" > "$REPO/synthetic-fixture.txt"
git -C "$REPO" add synthetic-fixture.txt .github/pre-purge-roots.txt
commit_fixture 'Synthetic detector fixture'
git -C "$REPO" rm -q synthetic-fixture.txt
commit_fixture 'Remove synthetic value from tip'
expect_gate 'real scanner rejects secret retained only in history' 1 'full-history secret scan found secrets or failed (exit 1)' secrets-history
expect_gate 'real diff scanner still rejects new secrets' 1 'gitleaks found secrets' secrets-diff "$before_secret..HEAD"

# A permissive host policy must not replace the candidate's default+sk rules.
# First prove this valid policy really permits the same synthetic history;
# configuration/parser failures cannot stand in for the ambient-policy control.
ambient_config="$fixture_root/permissive-ambient.toml"
cat > "$ambient_config" <<'TOML'
title = "Permissive ambient fixture policy"
[[rules]]
id = "ambient-fixture-only"
description = "Matches only a marker absent from the synthetic repository"
regex = '''OA01_AMBIENT_ONLY_NOT_PRESENT'''
TOML
ambient_toml=$(cat "$ambient_config")
ambient_status=0
(cd "$REPO" && gitleaks git --config "$ambient_config" --no-banner --redact --log-opts="$before_secret..HEAD" .) \
  > "$fixture_root/ambient-policy-baseline.txt" 2>&1 || ambient_status=$?
cat "$fixture_root/ambient-policy-baseline.txt"
[ "$ambient_status" -eq 0 ] || { echo "FAIL: permissive ambient policy precondition (exit $ambient_status)" >&2; exit 1; }
echo 'PASS: valid permissive ambient policy permits the real synthetic-history scan (exit 0)'
GITLEAKS_CONFIG="$ambient_config" GITLEAKS_CONFIG_TOML= expect_gate 'candidate history policy overrides ambient config file' 1 'full-history secret scan found secrets or failed (exit 1)' secrets-history
GITLEAKS_CONFIG="$ambient_config" GITLEAKS_CONFIG_TOML= expect_gate 'candidate diff policy overrides ambient config file' 1 'gitleaks found secrets' secrets-diff "$before_secret..HEAD"
GITLEAKS_CONFIG= GITLEAKS_CONFIG_TOML="$ambient_toml" expect_gate 'candidate history policy overrides ambient inline TOML' 1 'full-history secret scan found secrets or failed (exit 1)' secrets-history
GITLEAKS_CONFIG= GITLEAKS_CONFIG_TOML="$ambient_toml" expect_gate 'candidate diff policy overrides ambient inline TOML' 1 'gitleaks found secrets' secrets-diff "$before_secret..HEAD"
GITLEAKS_CONFIG="$ambient_config" GITLEAKS_CONFIG_TOML="$ambient_toml" expect_gate 'release containment rejects synthetic history despite both ambient overrides' 1 'full-history secret scan found secrets or failed (exit 1)' release-containment
printf '# Empty roots\n' > "$REPO/.github/pre-purge-roots.txt"
expect_gate 'real scanner findings stay informational before arming' 0 'failed (exit 1)' secrets-history

new_repo tracked-paths
arm_repo
mkdir "$REPO/.codex"
printf '{}\n' > "$REPO/.codex/auth.json"
git -C "$REPO" add .codex/auth.json
expect_gate 'tracked tool credential negative control' 1 'tracked credential/tool-state paths found' tracked-credentials
expect_gate 'release command enforces tracked credentials' 1 'tracked credential/tool-state paths found' release-containment
git -C "$REPO" rm -qf .codex/auth.json
printf 'FIXTURE_ONLY=example\n' > "$REPO/.env"
git -C "$REPO" add .env
expect_gate 'tracked env credential negative control' 1 'tracked credential/tool-state paths found' tracked-credentials
git -C "$REPO" rm -qf .env
printf 'FIXTURE_ONLY=example\n' > "$REPO/.env.example"
git -C "$REPO" add .env.example
expect_gate 'documented env example exception still works' 0 'tracked-credentials: ok' tracked-credentials

new_repo malformed-roots
printf '%s\n' "${old_root:0:7}" > "$REPO/.github/pre-purge-roots.txt"
expect_gate 'abbreviated root cannot certify clean' 1 'malformed pre-purge root' release-containment
printf '%s unexpected-field\n' "$old_root" > "$REPO/.github/pre-purge-roots.txt"
expect_gate 'malformed trailing root field blocks' 1 'malformed pre-purge root comment' pre-purge
printf '%040d\n' 0 > "$REPO/.github/pre-purge-roots.txt"
expect_gate 'null root cannot arm containment' 1 'malformed pre-purge root' secrets-history
blob=$(git -C "$REPO" rev-parse HEAD:README.md)
printf '%s\n' "$blob" > "$REPO/.github/pre-purge-roots.txt"
expect_gate 'existing non-commit root cannot arm containment' 1 'recorded pre-purge root is not a commit' path-history
printf '%s' "$old_root" > "$REPO/.github/pre-purge-roots.txt"
expect_gate 'last root without newline is still armed' 0 'pre-purge: ok (1 roots checked' pre-purge
rm "$REPO/.github/pre-purge-roots.txt"
expect_gate 'missing root list cannot certify clean' 1 'cannot read .github/pre-purge-roots.txt' release-containment

REPO="$fixture_root/shallow-clone"
git clone -q --no-local --depth 1 --single-branch --branch clean-candidate "$clean_source" "$REPO"
for gate in pre-purge secrets-history path-history release-containment; do
  expect_gate "shallow $gate inspection cannot certify clean" 1 'full-history inspection required' "$gate"
done

# Exercise real Git error propagation with one controlled failing Git subcommand
# at a time. All other invocations go to the installed Git, on real repositories.
new_repo git-errors
arm_repo
git_shim="$fixture_root/git-error-bin"
mkdir "$git_shim"
cat > "$git_shim/git" <<'SHIM'
#!/usr/bin/env bash
if [ "$1" = "$OA01_FAIL_GIT_COMMAND" ]; then
  echo 'controlled Git inspection failure' >&2
  exit 43
fi
exec "$OA01_REAL_GIT" "$@"
SHIM
chmod +x "$git_shim/git"
export OA01_REAL_GIT="$real_git"
OA01_FAIL_GIT_COMMAND=log PATH="$git_shim:$PATH" expect_gate 'armed path-history tool error blocks' 1 'purged-path history inspection failed' path-history
OA01_FAIL_GIT_COMMAND=rev-list PATH="$git_shim:$PATH" expect_gate 'incomplete history traversal blocks ancestry inspection' 1 'full-history traversal failed' pre-purge
OA01_FAIL_GIT_COMMAND=ls-files PATH="$git_shim:$PATH" expect_gate 'tracked-path inspection error cannot pass' 1 'cannot inspect tracked credential paths' tracked-credentials

printf '\nPASS: OA-01 containment fixture oracles matched (%s checks; real Git and gitleaks, with explicit error shims)\n' "$checks"
