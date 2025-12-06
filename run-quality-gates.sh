#!/bin/bash
set -e

echo "======================================"
echo "🚦 QUALITY GATES - Once/OnceCell"
echo "======================================"
echo ""

cd /Users/gqadonis/Projects/prometheus/prometheus-parking-lot

echo "📋 Gate 1: cargo check"
echo "--------------------------------------"
cargo check
echo "✅ Gate 1 PASSED"
echo ""

echo "📋 Gate 2: cargo clippy -- -D warnings"
echo "--------------------------------------"
cargo clippy -- -D warnings
echo "✅ Gate 2 PASSED (0 warnings)"
echo ""

echo "📋 Gate 3: cargo test"
echo "--------------------------------------"
cargo test
echo "✅ Gate 3 PASSED"
echo ""

echo "📋 Gate 4: cargo doc --no-deps"
echo "--------------------------------------"
cargo doc --no-deps
echo "✅ Gate 4 PASSED"
echo ""

echo "📋 Gate 5: cargo fmt --check"
echo "--------------------------------------"
cargo fmt --check
echo "✅ Gate 5 PASSED"
echo ""

echo "======================================"
echo "✅ ALL QUALITY GATES PASSED!"
echo "======================================"
echo ""
echo "Summary:"
echo "  - Compilation: ✅ PASS"
echo "  - Linting: ✅ PASS (0 warnings)"
echo "  - Tests: ✅ PASS (10 tests)"
echo "  - Documentation: ✅ PASS"
echo "  - Formatting: ✅ PASS"
