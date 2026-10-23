#!/usr/bin/env bash
# Web-target dependency gate (cr-38). Usage: check-wasm-graph.sh
#
# The core and wasm crates must build for wasm32-unknown-unknown without an async
# runtime, a database client or an HTTP client (ADR 0001):
#
#   - core graph: none of tokio, sqlx, sea-orm, redb, reqwest, iggy.
#   - wasm graph: the same, plus no getrandom. getrandom is not in either graph today;
#     if a dependency ever pulls it in, wasm32-unknown-unknown needs an explicit
#     backend (getrandom 0.3: the `wasm_js` feature plus
#     RUSTFLAGS='--cfg getrandom_backend="wasm_js"'; 0.2: the `js` feature), so the
#     gate fails until that choice is made on purpose.
#
# `cargo tree` runs with --locked against the committed Cargo.lock. A failure of
# cargo tree itself fails the gate (an empty graph must not read as "clean").
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

# `--prefix none` prints "name vX.Y.Z" at column 0, so the name is anchored on ^.
FORBIDDEN='^(tokio|sqlx|sea-orm|redb|reqwest|iggy)(-[A-Za-z0-9_-]+)? v[0-9]'
TARGET=wasm32-unknown-unknown
status=0

check() {
  local package="$1" pattern="$2" what="$3" graph
  graph=$(cargo tree --locked -p "$package" --target "$TARGET" -e normal --prefix none)
  if [ "$(printf '%s\n' "$graph" | grep -c .)" -lt 2 ]; then
    echo "::error::cargo tree printed no dependency graph for $package" >&2
    exit 2
  fi
  if hits=$(printf '%s\n' "$graph" | grep -E "$pattern"); then
    echo "::error::$package has $what in its $TARGET graph:" >&2
    printf '%s\n' "$hits" | sort -u >&2
    status=1
  else
    echo "wasm-graph: $package: no $what ($(printf '%s\n' "$graph" | sort -u | wc -l | tr -d ' ') distinct crates)"
  fi
}

check prometheus-parking-lot-core "$FORBIDDEN" "runtime/database/network crate"
check prometheus-parking-lot-wasm "$FORBIDDEN" "runtime/database/network crate"
check prometheus-parking-lot-wasm '^getrandom v[0-9]' "getrandom (needs an explicit wasm backend)"
exit "$status"
