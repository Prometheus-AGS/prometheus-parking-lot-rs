#!/usr/bin/env bash
# agent-worktree.sh — one git worktree per agent-team role (docs/agent-team.md).
#
#   scripts/agent-worktree.sh add <role-id> <CR-id>   create worktree + branch team/<role>/<CR>
#   scripts/agent-worktree.sh remove <role-id>        remove a clean worktree
#   scripts/agent-worktree.sh list                    list team worktrees
#
# Worktrees live under one parent so build-dirs and sccache base dirs are easy to manage.
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
PARENT="${AGENT_WORKTREE_PARENT:-${ROOT}.worktrees}"
TEAM="$ROOT/.agent-team/parking-lot-core/team.json"

die() { printf 'agent-worktree: %s\n' "$1" >&2; exit 1; }

known_role() {
  command -v jq >/dev/null || return 0
  jq -e --arg r "$1" '.roles[] | select(.id == $r)' "$TEAM" >/dev/null 2>&1
}

case "${1:-}" in
  add)
    role="${2:?role id required}"; cr="${3:?CR id required (e.g. CR-19)}"
    known_role "$role" || die "unknown role '$role' (see $TEAM)"
    dir="$PARENT/$role"
    [ -e "$dir" ] && die "$dir already exists"
    mkdir -p "$PARENT"
    git -C "$ROOT" fetch -q origin main
    git -C "$ROOT" worktree add -b "team/$role/$cr" "$dir" origin/main
    echo "worktree: $dir (branch team/$role/$cr)"
    ;;
  remove)
    role="${2:?role id required}"; dir="$PARENT/$role"
    [ -d "$dir" ] || die "no worktree at $dir"
    git -C "$ROOT" worktree remove "$dir"
    echo "removed $dir. Its build-dir under /Volumes/T7/rust/cargo-build is not removed;"
    echo "see docs/build-performance.md (Disk hygiene)."
    ;;
  list)
    git -C "$ROOT" worktree list | grep -F "$PARENT" || echo "no team worktrees under $PARENT"
    ;;
  *)
    sed -n '2,8p' "$0"; exit 1
    ;;
esac
