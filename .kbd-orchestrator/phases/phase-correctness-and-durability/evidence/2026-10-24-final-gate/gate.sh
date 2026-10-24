#!/usr/bin/env bash
# Phase-boundary integration gate (amended 2026-10-23). Stops at the first failure.
set -u
cd /Volumes/T7/Projects/prometheus/prometheus-parking-lot.worktrees/gate
OUT=$1; mkdir -p "$OUT"; [ -n "${START:-}" ] || : > "$OUT/summary.txt"
WBG=/private/tmp/claude-501/-Volumes-T7-Projects-prometheus-prometheus-parking-lot/d281c3f1-466c-41cb-b006-ce00f1aa43b6/scratchpad/wbg/bin
step() { local name=$1; shift; local start=$(date +%s)
  if [[ "$name" < "${START:-00}" ]]; then return 0; fi
  echo "== $name" ; ( eval "$*" ) > "$OUT/$name.log" 2>&1 < /dev/null; local rc=$?
  echo "$name rc=$rc $(( $(date +%s) - start ))s :: $*" >> "$OUT/summary.txt"
  if [ $rc -ne 0 ]; then echo "STOP at $name (rc=$rc)" >> "$OUT/summary.txt"; exit $rc; fi; }
echo "commit $(git rev-parse HEAD) rustc $(rustc --version) date $(date -u +%FT%TZ)" >> "$OUT/summary.txt"
step 01-fmt 'cargo fmt --all -- --check'
step 02-clippy 'cargo clippy --workspace --all-targets --all-features --locked -- -D warnings'
step 03-clippy-wasm32 'cargo clippy --locked --target wasm32-unknown-unknown -p prometheus-parking-lot-core -p prometheus-parking-lot-wasm --all-targets --all-features -- -D warnings'
step 04-nextest 'cargo nextest run --workspace --all-features --locked --no-tests=fail'
step 05-docker-pg18 'PARKING_LOT_PG_MAJOR=18 cargo nextest run --workspace --all-features --locked --run-ignored all --no-fail-fast --no-tests=fail'
step 06-docker-pg17 'PARKING_LOT_PG_MAJOR=17 cargo nextest run --workspace --all-features --locked --run-ignored all --no-fail-fast --no-tests=fail -E "package(prometheus-parking-lot-pg) | test(config_postgres_) | test(config_pgmq_) | test(pool_sigkill_) | test(shutdown_durable_pg_)"'
step 07-msrv 'cargo +1.95 check --workspace --all-targets --all-features --locked'
step 08-loom 'RUSTFLAGS="--cfg parking_lot_loom" LOOM_MAX_PREEMPTIONS=3 CARGO_TARGET_DIR=$PWD/target-gate-loom cargo test -p prometheus-parking-lot-core --test loom_capacity --test loom_wakeup --test loom_vector --test loom_skip_ahead --test loom_at_depth --locked'
step 09-doctest 'cargo test --doc --workspace --all-features --locked'
step 10-rustdoc 'RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --locked'
step 11-deny 'cargo deny --workspace --all-features --locked check'
step 12-package 'cargo package --workspace --locked --exclude prometheus-parking-lot-ffi'
step 13-supply-chain 'bash .github/scripts/supply-chain.sh tracked-credentials && bash .github/scripts/supply-chain.sh secrets-diff bbd2b96^..HEAD && bash .github/scripts/supply-chain.sh package-allowlist'
step 14-core-graph 'bash .github/scripts/check-wasm-graph.sh && ! cargo tree -p prometheus-parking-lot-core -e normal --locked | grep -E " (tokio|sqlx|sea-orm|iggy|redb|reqwest) v" && ! cargo tree -p prometheus-parking-lot-core --target wasm32-unknown-unknown -e normal --locked | grep -E " tokio v"'
step 15-wasm-node "PATH=$WBG:\$PATH cargo test --locked --target wasm32-unknown-unknown -p prometheus-parking-lot-wasm"
step 16-android 'cargo ndk -t arm64-v8a check -p prometheus-parking-lot-ffi --locked'
step 17-ios-sim 'cargo check -p prometheus-parking-lot-ffi --target aarch64-apple-ios-sim --locked'
step 18-examples-build 'cargo build --examples --workspace --all-features --locked && /usr/bin/python3 .github/scripts/check-doc-snippets.py && /usr/bin/python3 .github/scripts/check-orphan-tests.py'
step 19-site 'npm --prefix website ci && npm --prefix website run build && npm --prefix website run verify'
echo "ALL PASS" >> "$OUT/summary.txt"
