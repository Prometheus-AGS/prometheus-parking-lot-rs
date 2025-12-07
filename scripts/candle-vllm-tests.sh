#!/bin/bash
set -e

echo "=== Running candle-vllm Dedicated Test Suite ==="

mkdir -p reports

# Run only candle-vllm tests
cargo test --test candle_vllm_suite -- --nocapture 2>&1 | tee reports/candle-vllm-test-report.txt

echo "" >> reports/candle-vllm-test-report.txt
echo "=== CANDLE-VLLM TEST SUMMARY ===" >> reports/candle-vllm-test-report.txt
echo "Generated at: $(date)" >> reports/candle-vllm-test-report.txt

echo ""
echo "=== candle-vllm Test Report Generated ==="
echo "Report: reports/candle-vllm-test-report.txt"
