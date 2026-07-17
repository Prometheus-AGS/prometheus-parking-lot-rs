#!/bin/bash
# Test script for running Postgres integration tests

set -e

echo "=== Postgres Integration Tests ==="
echo

# Start Docker Compose
echo "Starting Postgres container..."
docker-compose -f docker-compose.test.yml up -d

# Wait for Postgres to be ready
echo "Waiting for Postgres to be ready..."
sleep 5

# Set environment variable for tests
export DATABASE_URL="postgresql://testuser:testpass@localhost:5433/testdb"

# Run integration tests
echo "Running integration tests..."
cargo test --features postgres --test integration -- --test-threads=1

# Capture test result
TEST_RESULT=$?

# Cleanup
echo
echo "Cleaning up..."
docker-compose -f docker-compose.test.yml down

if [ $TEST_RESULT -eq 0 ]; then
    echo
    echo "✓ All tests passed!"
    exit 0
else
    echo
    echo "✗ Tests failed!"
    exit 1
fi
