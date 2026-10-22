#!/usr/bin/env bash
# install-agent-skills.sh — install the third-party Rust/systems agent skills
# routed in AGENTS.md and docs/skill-routing.md, globally for every harness.
#
# Canonical copy: ~/.agents/skills/<name>  (Codex, Zed, OpenCode)
# Symlinks:       ~/.claude/skills, ~/.kimi/skills, ~/.minimax/skills
#
# Sources are pinned to commits. Each skill is validated with `skills-ref`
# (agentskills.io reference validator) before install. Existing skills are
# never overwritten. Usage: scripts/install-agent-skills.sh [--dry-run]
set -euo pipefail

DRY_RUN=0
[ "${1:-}" = "--dry-run" ] && DRY_RUN=1

CANONICAL="$HOME/.agents/skills"
LINK_DIRS=("$HOME/.claude/skills" "$HOME/.kimi/skills" "$HOME/.minimax/skills")

BEAGLE_REPO="https://github.com/existential-birds/beagle.git"
BEAGLE_SHA="d1a74899fbfec74974d1818e4cac7c3d54d44b65"
LOWLEVEL_REPO="https://github.com/mohitmishra786/low-level-dev-skills.git"
LOWLEVEL_SHA="bdc58472fa9f309ed1b3f7d985a0d8e9bd8f4608"

# repo-key|path-in-repo|license
SKILLS=(
  "beagle|plugins/beagle-rust/skills/rust-code-review|Apache-2.0"
  "beagle|plugins/beagle-rust/skills/tokio-async-code-review|Apache-2.0"
  "beagle|plugins/beagle-rust/skills/rust-testing-code-review|Apache-2.0"
  "beagle|plugins/beagle-rust/skills/serde-code-review|Apache-2.0"
  "beagle|plugins/beagle-rust/skills/sqlx-code-review|Apache-2.0"
  "lowlevel|skills/rust/rust-async-internals|MIT"
  "lowlevel|skills/rust/rust-sanitizers-miri|MIT"
  "lowlevel|skills/rust/rust-profiling|MIT"
  "lowlevel|skills/rust/rust-debugging|MIT"
  "lowlevel|skills/low-level-programming/memory-model|MIT"
  "lowlevel|skills/debuggers/concurrency-debugging|MIT"
  "lowlevel|skills/profilers/flamegraphs|MIT"
)

die() { printf 'install-agent-skills: %s\n' "$1" >&2; exit 1; }
command -v git >/dev/null || die "git is required"
command -v npx >/dev/null || die "npx is required (for skills-ref validate)"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

fetch() { # <dir> <repo> <sha>
  git init -q "$1"
  git -C "$1" fetch -q --depth 1 "$2" "$3"
  git -C "$1" checkout -q FETCH_HEAD
}
fetch "$WORK/beagle" "$BEAGLE_REPO" "$BEAGLE_SHA"
fetch "$WORK/lowlevel" "$LOWLEVEL_REPO" "$LOWLEVEL_SHA"

mkdir -p "$CANONICAL"
installed=0; skipped=0
for entry in "${SKILLS[@]}"; do
  IFS='|' read -r key rel license <<<"$entry"
  name="$(basename "$rel")"
  src="$WORK/$key/$rel"
  case "$key" in
    beagle) url="$BEAGLE_REPO@$BEAGLE_SHA" ;;
    lowlevel) url="$LOWLEVEL_REPO@$LOWLEVEL_SHA" ;;
  esac

  [ -f "$src/SKILL.md" ] || die "$name: SKILL.md missing at pinned commit"
  if find "$src" -type f \( -perm -u+x -o -name '*.sh' -o -name '*.py' -o -name '*.js' \) | grep -q .; then
    die "$name: contains executable files; review manually before installing"
  fi
  npx --yes skills-ref validate "$src" >/dev/null || die "$name: fails agentskills.io validation"

  if [ -e "$CANONICAL/$name" ]; then
    echo "SKIP    $name (exists in $CANONICAL)"; skipped=$((skipped + 1))
  elif [ "$DRY_RUN" = 1 ]; then
    echo "WOULD   $name <- $url/$rel ($license)"
  else
    cp -R "$src" "$CANONICAL/$name"
    printf 'source: %s/%s\nlicense: %s\ninstalled: %s\nvalidated: skills-ref validate\n' \
      "$url" "$rel" "$license" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" > "$CANONICAL/$name/.provenance"
    echo "INSTALL $name ($license)"; installed=$((installed + 1))
  fi

  for dir in "${LINK_DIRS[@]}"; do
    [ -d "$dir" ] || continue
    [ -e "$dir/$name" ] || [ -L "$dir/$name" ] && continue
    [ "$DRY_RUN" = 1 ] || ln -s "$CANONICAL/$name" "$dir/$name"
  done
done

echo "done: $installed installed, $skipped already present$([ "$DRY_RUN" = 1 ] && echo ' (dry run)')"
