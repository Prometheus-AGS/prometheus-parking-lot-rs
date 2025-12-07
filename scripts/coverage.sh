#!/bin/bash
set -e

echo "=== Generating Coverage Reports ==="

# Create output directories
mkdir -p coverage/llvm-cov
mkdir -p reports

# LLVM-cov coverage (HTML + terminal)
echo "Running cargo-llvm-cov..."
cargo llvm-cov --html --output-dir coverage/llvm-cov
cargo llvm-cov report > reports/llvm-cov-report.txt

echo ""
echo "=== Coverage Reports Generated ==="
echo "LLVM-cov HTML: coverage/llvm-cov/html/index.html"
echo "LLVM-cov Text: reports/llvm-cov-report.txt"
