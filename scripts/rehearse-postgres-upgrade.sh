#!/usr/bin/env bash
# Source-only until OA-18. All Docker/Cargo execution is owned by the final gate.
set -euo pipefail
repo_dir=$(cd "$(dirname "$0")/.." && pwd)
exec python3 "$repo_dir/docker/postgres18/upgrade/rehearse.py" "$@"
