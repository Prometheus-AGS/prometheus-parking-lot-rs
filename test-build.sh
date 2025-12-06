#!/bin/bash
set -e

cd /Users/gqadonis/Projects/prometheus/prometheus-parking-lot

echo "==================================="
echo "  PROMETHEUS PARKING LOT BUILD"
echo "==================================="
echo ""

echo "📦 Gate 1: cargo check"
cargo check
echo "✅ PASSED: cargo check"
echo ""

echo "🔍 Gate 2: cargo clippy"
cargo clippy -- -D warnings
echo "✅ PASSED: cargo clippy (0 warnings)"
echo ""

echo "🧪 Gate 3: cargo test"
cargo test
echo "✅ PASSED: cargo test"
echo ""

echo "📚 Gate 4: cargo doc"
cargo doc --no-deps
echo "✅ PASSED: cargo doc"
echo ""

echo "✨ Gate 5: cargo fmt --check"
cargo fmt --check
echo "✅ PASSED: cargo fmt"
echo ""

echo "==================================="
echo "  ✅ ALL QUALITY GATES PASSED!"
echo "==================================="
