#!/usr/bin/env bash
# Site verification gate (`npm run verify`).
# Vendored from the build-branded-docusaurus skill's scripts/verify.sh and
# extended for cr-45: sanitizer negative control, link check over the built
# HTML, light/dark screenshots, brand-token, Flat 2.0, focus and axe checks.
#
# Honours SITE_URL and BASE_URL like the build. VERIFY_SKIP_INSTALL=1 skips the
# frozen install when node_modules is already from `npm ci`.
set -euo pipefail

site="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$site"
test -f package-lock.json

step() { printf '\n== %s\n' "$*"; }

if [ "${VERIFY_SKIP_INSTALL:-0}" != "1" ]; then
  step "frozen install (npm ci)"
  npm ci --no-audit --no-fund
fi

step "sanitizer negative control"
fixture="$(mktemp -d)"
trap 'rm -r "$fixture"' EXIT
# Fragments keep these literals out of this file.
printf 'see /%s/%s/notes.md\n' "Users" "someone" > "$fixture/local-path.md"
printf 'mounted at /%s/%s/repo\n' "Volumes" "disk" > "$fixture/volume-path.md"
printf -- '-----BEGIN %s-----\n' "PRIVATE KEY" > "$fixture/key.md"
printf 'raw: .%s/knowledge/wiki/index.md\n' "prometheus" > "$fixture/wiki.md"
for bad in "$fixture"/*.md; do
  if node scripts/sanitize.mjs "$bad" >/dev/null 2>&1; then
    echo "sanitizer accepted prohibited content in $(basename "$bad")" >&2
    exit 1
  fi
done
echo "sanitizer rejected all $(ls "$fixture" | wc -l | tr -d ' ') prohibited fixtures"

step "production build (sanitizer + docusaurus build; broken links throw)"
npm run build

step "private material scan of build output"
if grep -rEn '/(Users|Volumes)/[A-Za-z0-9._-]+|BEGIN [A-Z ]*PRIVATE KEY|\.prometheus/(knowledge|events)' build; then
  echo "private material found in site output" >&2
  exit 1
fi
echo "no private material in build/"

step "link check, screenshots, brand tokens, Flat 2.0, focus, accessibility"
node scripts/verify-site.mjs

echo
echo "branded Docusaurus verification passed"
