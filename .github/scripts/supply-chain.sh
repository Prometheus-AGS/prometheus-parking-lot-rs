#!/usr/bin/env bash
# Supply-chain gates (cr-51). Usage: supply-chain.sh <gate> [args]
#
# Blocking gates (exit 1 on violation):
#   tracked-credentials   no tracked file matches a never-tracked credential path
#   secrets-diff <range>  gitleaks over a git range (PR base..head, push before..after)
#   release-containment   require armed roots, full history, and all containment gates
#   package-allowlist     cargo package --list per crate vs .github/package-allowlist.txt
# Gates armed by nonempty .github/pre-purge-roots.txt after an approved purge:
#   pre-purge             HEAD must not descend from any recorded pre-purge commit
#   secrets-history       gitleaks over the full history
#   path-history          git log --all for the purged credential paths
# Empty roots preserve informational CI behavior; they cannot certify containment.
set -euo pipefail

# Replacement objects must not hide the ancestry or content being inspected.
export GIT_NO_REPLACE_OBJECTS=1
cd "$(git rev-parse --show-toplevel)"
ROOTS_FILE=".github/pre-purge-roots.txt"
ALLOWLIST_FILE=".github/package-allowlist.txt"
PURGED_PATHS=(.codex/auth.json .codex/history.jsonl .codex/sessions .codex/config.toml .codex/version.json)
# Mirrors .kbd-orchestrator/constraints.md no-tracked-tool-credentials, plus env files.
CRED_PATHSPECS=(.codex/auth.json .codex/config.toml .codex/history.jsonl .codex/version.json .codex/sessions .serena
  '*.env' '.env' '.env.*' '*.pem' '*.key')

fail() { echo "::error::$*" >&2; exit 1; }

load_roots() {
  local line root comment head object
  ROOTS=""
  ROOT_COUNT=0
  [ -r "$ROOTS_FILE" ] || fail "cannot read $ROOTS_FILE"
  head=$(git rev-parse --verify HEAD) || fail "cannot resolve candidate HEAD"
  while IFS= read -r line || [ -n "$line" ]; do
    read -r root comment <<< "$line"
    case "$root" in ''|'#'*) continue ;; esac
    # Require canonical full object IDs, never abbreviations or revision syntax.
    if ! [[ "$root" =~ ^[0-9a-f]+$ ]] || [ "${#root}" -ne "${#head}" ] || [ -z "${root//0/}" ]; then
      fail "malformed pre-purge root in $ROOTS_FILE (full lowercase commit ID required)"
    fi
    case "$comment" in ''|'#'*) ;; *) fail "malformed pre-purge root comment in $ROOTS_FILE" ;; esac
    object=$(printf '%s\n' "$root" | git cat-file --batch-check='%(objectname) %(objecttype)') \
      || fail "cannot inspect recorded pre-purge object"
    case "$object" in
      "$root commit"|"$root missing") ;;
      *) fail "recorded pre-purge root is not a commit" ;;
    esac
    ROOTS+="$root"$'\n'
    ROOT_COUNT=$((ROOT_COUNT + 1))
  done < "$ROOTS_FILE"
}

require_full_history() {
  local shallow grafts
  shallow=$(git rev-parse --is-shallow-repository) || fail "cannot inspect repository depth"
  [ "$shallow" = false ] || fail "full-history inspection required; shallow repositories cannot certify containment"
  grafts=$(git rev-parse --git-path info/grafts) || fail "cannot inspect graft configuration"
  [ ! -s "$grafts" ] || fail "full-history inspection required; legacy grafts hide ancestry"
  git rev-list --all HEAD >/dev/null || fail "full-history traversal failed"
}

gate_tracked_credentials() {
  local hits paths status
  paths=$(git ls-files -- "${CRED_PATHSPECS[@]}") || fail "cannot inspect tracked credential paths"
  if hits=$(printf '%s\n' "$paths" | grep -v -E '(^|/)\.env\.example$'); then
    :
  else
    status=$?
    [ "$status" -eq 1 ] || fail "cannot filter tracked credential paths"
  fi
  if [ -n "$hits" ]; then
    echo "$hits" >&2
    fail "tracked credential/tool-state paths found (no-tracked-tool-credentials)"
  fi
  echo "tracked-credentials: ok (no tracked credential paths)"
}

gate_secrets_diff() {
  local range="${1:?range required, e.g. base..head}"
  # Bind detection to this candidate's policy, ahead of ambient host config.
  gitleaks git --config .gitleaks.toml --no-banner --redact --log-opts="$range" . \
    || fail "gitleaks found secrets in $range"
  echo "secrets-diff: ok ($range)"
}

gate_secrets_history() {
  local status
  load_roots
  if [ "$ROOT_COUNT" -gt 0 ]; then
    require_full_history
    echo "== armed: blocking full-history secret scan =="
  else
    echo "== pre-purge, informational: full-history secret scan; containment is not certified =="
  fi
  if gitleaks git --config .gitleaks.toml --no-banner --redact --log-opts="--all HEAD --full-history" . ; then
    echo "secrets-history: no findings in inspected history"
  else
    status=$?
    if [ "$ROOT_COUNT" -gt 0 ]; then
      fail "full-history secret scan found secrets or failed (exit $status)"
    fi
    echo "::warning::pre-purge informational: full-history secret scan found secrets or failed (exit $status); containment is not certified"
  fi
}

gate_path_history() {
  local out
  load_roots
  if [ "$ROOT_COUNT" -gt 0 ]; then
    require_full_history
    echo "== armed: blocking purged-path history check =="
  else
    echo "== pre-purge, informational: purged-path history; containment is not certified =="
  fi
  # Emit commit IDs only, never historical credential content or commit subjects.
  if out=$(git log --all HEAD --full-history --format=%H -- "${PURGED_PATHS[@]}"); then
    :
  else
    [ "$ROOT_COUNT" -eq 0 ] || fail "purged-path history inspection failed"
    echo "::warning::pre-purge informational: purged-path history inspection failed; containment is not certified"
    return
  fi
  if [ -n "$out" ]; then
    echo "$out"
    [ "$ROOT_COUNT" -eq 0 ] || fail "purged credential paths remain in history"
    echo "::warning::pre-purge informational: purged paths still in history; containment is not certified"
  else
    echo "path-history: empty"
  fi
}

gate_pre_purge() {
  local root ancestry
  load_roots
  if [ "$ROOT_COUNT" -eq 0 ]; then
    echo "pre-purge: unarmed ($ROOTS_FILE has no roots); containment is not certified"
    return
  fi
  require_full_history
  ancestry=$(git rev-list HEAD) || fail "cannot inspect candidate ancestry"
  # A fresh post-purge clone need not contain the old objects. Compare complete
  # ancestry IDs instead of interpreting cat-file/merge-base errors as absence.
  while IFS= read -r root; do
    [ -n "$root" ] || continue
    if [[ $'\n'"$ancestry"$'\n' == *$'\n'"$root"$'\n'* ]]; then
      fail "HEAD descends from pre-purge root $root; rebase onto post-purge main"
    fi
  done <<< "$ROOTS"
  echo "pre-purge: ok ($ROOT_COUNT roots checked against full candidate ancestry)"
}

gate_release_containment() {
  load_roots
  [ "$ROOT_COUNT" -gt 0 ] || fail "release containment is unarmed: $ROOTS_FILE has no approved pre-purge roots"
  require_full_history
  gate_tracked_credentials
  gate_pre_purge
  gate_path_history
  gate_secrets_history
  echo "release-containment: clean-history checks passed; operator purge/cache evidence is required separately"
}

gate_package_allowlist() {
  local bad=0 crate pkg regexes list line
  # Lines: "<package-name> <ERE>"; '#' comments allowed.
  for pkg in $(grep -v -E '^\s*(#|$)' "$ALLOWLIST_FILE" | awk '{print $1}' | sort -u); do
    regexes=$(grep -E "^$pkg[[:space:]]" "$ALLOWLIST_FILE" | awk '{print $2}' | paste -sd'|' -)
    list=$(cargo package -p "$pkg" --list --locked --allow-dirty 2>/dev/null) \
      || fail "cargo package --list failed for $pkg"
    while IFS= read -r line; do
      if ! printf '%s\n' "$line" | grep -q -E "^($regexes)$"; then
        echo "::error::$pkg would package non-allowlisted path: $line" >&2
        bad=1
      fi
    done <<< "$list"
    echo "package-allowlist: $pkg $(printf '%s\n' "$list" | wc -l | tr -d ' ') files checked"
  done
  [ "$bad" -eq 0 ] || fail "package allowlist violated"
}

case "${1:-}" in
  tracked-credentials) gate_tracked_credentials ;;
  secrets-diff) shift; gate_secrets_diff "$@" ;;
  secrets-history) gate_secrets_history ;;
  path-history) gate_path_history ;;
  pre-purge) gate_pre_purge ;;
  release-containment) gate_release_containment ;;
  package-allowlist) gate_package_allowlist ;;
  *) echo "usage: $0 {tracked-credentials|secrets-diff <range>|secrets-history|path-history|pre-purge|release-containment|package-allowlist}" >&2; exit 2 ;;
esac
