#!/usr/bin/env bash
# Supply-chain gates (cr-51). Usage: supply-chain.sh <gate> [args]
#
# Blocking gates (exit 1 on violation):
#   tracked-credentials   no tracked file matches a never-tracked credential path
#   secrets-diff <range>  gitleaks over a git range (PR base..head, push before..after)
#   pre-purge             HEAD must not descend from any root in .github/pre-purge-roots.txt
#   package-allowlist     cargo package --list per crate vs .github/package-allowlist.txt
# Informational gates (always exit 0, print a report; arm after cr-15 purge):
#   secrets-history       gitleaks over the full history
#   path-history          git log --all for the purged credential paths
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
ROOTS_FILE=".github/pre-purge-roots.txt"
ALLOWLIST_FILE=".github/package-allowlist.txt"
PURGED_PATHS=(.codex/auth.json .codex/history.jsonl .codex/sessions .codex/config.toml .codex/version.json)
# Mirrors .kbd-orchestrator/constraints.md no-tracked-tool-credentials, plus env files.
CRED_PATHSPECS=(.codex/auth.json .codex/config.toml .codex/history.jsonl .codex/version.json .codex/sessions .serena
  '*.env' '.env' '.env.*' '*.pem' '*.key')

fail() { echo "::error::$*" >&2; exit 1; }

gate_tracked_credentials() {
  local hits
  hits=$(git ls-files -- "${CRED_PATHSPECS[@]}" | grep -v -E '(^|/)\.env\.example$' || true)
  if [ -n "$hits" ]; then
    echo "$hits" >&2
    fail "tracked credential/tool-state paths found (no-tracked-tool-credentials)"
  fi
  echo "tracked-credentials: ok (no tracked credential paths)"
}

gate_secrets_diff() {
  local range="${1:?range required, e.g. base..head}"
  gitleaks git --no-banner --redact --log-opts="$range" . \
    || fail "gitleaks found secrets in $range"
  echo "secrets-diff: ok ($range)"
}

gate_secrets_history() {
  echo "== pre-purge, informational: gitleaks over full history (cr-15 not yet run) =="
  if gitleaks git --no-banner --redact --log-opts="--all" . ; then
    echo "secrets-history: clean"
  else
    echo "::warning::pre-purge informational: full-history secrets present (expected until cr-15)"
  fi
}

gate_path_history() {
  echo "== pre-purge, informational: purged-path history (must be empty after cr-15) =="
  local out
  out=$(git log --all --oneline -- "${PURGED_PATHS[@]}")
  if [ -n "$out" ]; then
    echo "$out"
    echo "::warning::pre-purge informational: purged paths still in history (expected until cr-15)"
  else
    echo "path-history: empty"
  fi
}

gate_pre_purge() {
  local n=0 root
  while read -r root _; do
    case "$root" in ''|'#'*) continue ;; esac
    n=$((n + 1))
    git cat-file -e "${root}^{commit}" 2>/dev/null || { echo "pre-purge: root $root not in clone (ok, cannot be an ancestor)"; continue; }
    if git merge-base --is-ancestor "$root" HEAD; then
      fail "HEAD descends from pre-purge root $root; rebase onto post-purge main"
    fi
  done < "$ROOTS_FILE"
  echo "pre-purge: ok ($n roots checked; gate is inert while $ROOTS_FILE is empty)"
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
  package-allowlist) gate_package_allowlist ;;
  *) echo "usage: $0 {tracked-credentials|secrets-diff <range>|secrets-history|path-history|pre-purge|package-allowlist}" >&2; exit 2 ;;
esac
