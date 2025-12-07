#!/usr/bin/env bash
# Run the LLM inference test with OpenAI API key from environment

set -e

if [ -z "$OPENAI_API_KEY" ]; then
    echo "Error: OPENAI_API_KEY environment variable is not set"
    echo "Please set it with: export OPENAI_API_KEY=your-api-key"
    exit 1
fi

echo "=================================="
echo "LLM Inference Integration Test"
echo "=================================="
echo ""
echo "This test validates the parking lot scheduler with:"
echo "  ✓ Resource pool with max_units=3 (3 tokio worker threads)"
echo "  ✓ 15 concurrent LLM inference tasks submitted"
echo "  ✓ Parking/queuing when capacity exceeded"
echo "  ✓ Real OpenAI streaming API calls"
echo "  ✓ Channel-based stream collection"
echo "  ✓ Concurrency verification (peak ≤ 3)"
echo ""
echo "Starting test..."
echo ""

cargo test --features tokio-runtime --test llm_inference_test -- --nocapture --test-threads=1
