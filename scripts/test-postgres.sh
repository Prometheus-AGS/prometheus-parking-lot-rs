#!/usr/bin/env bash
# Run the Postgres substrate tests (CR-43) against real PG 18 and PG 17 images.
# The testcontainers helper builds parking-lot-pg:18 / :17 on first use.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo test -p prometheus-parking-lot-pg -- --ignored --test-threads=1
