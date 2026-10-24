#!/usr/bin/env bash
# OA-18 source entry. No installation, publication or history rewrite.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
export PYTHONDONTWRITEBYTECODE=1 RUSTUP_AUTO_INSTALL=0
exec python3 "$root/.github/scripts/operational-assurance.py" run "$@"
