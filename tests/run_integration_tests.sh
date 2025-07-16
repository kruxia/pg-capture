#!/bin/bash
# Script to run integration tests for pg-replicate-kafka

set -e

echo "=== pg-replicate-kafka Integration Test Suite ==="
echo "================================================"
echo ""
echo "Prerequisites:"
echo "- PostgreSQL must be running on localhost:5432"
echo "- Kafka must be running on localhost:9092"
echo "- User 'postgres' with password 'postgres' must exist"
echo ""
echo "Press Enter to continue or Ctrl+C to cancel..."
read

# Colors for output
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Function to run a test and report results
run_test() {
    local test_name=$1
    local test_path=$2
    
    echo -n "Running $test_name... "
    
    if cargo test --test $test_path -- --ignored --nocapture --test-threads=1 2>&1 | tee /tmp/test_output.log | grep -q "test result: ok"; then
        echo -e "${GREEN}PASSED${NC}"
        return 0
    else
        echo -e "${RED}FAILED${NC}"
        echo "See /tmp/test_output.log for details"
        return 1
    fi
}

# Check if services are running
echo "Checking services..."
if ! pg_isready -h localhost -p 5432 -U postgres &>/dev/null; then
    echo -e "${RED}PostgreSQL is not running on localhost:5432${NC}"
    exit 1
fi

if ! nc -z localhost 9092 &>/dev/null; then
    echo -e "${RED}Kafka is not running on localhost:9092${NC}"
    exit 1
fi

echo -e "${GREEN}Services are ready${NC}"
echo ""

# Build the project first
echo "Building project..."
cargo build --release
echo ""

# Track test results
PASSED=0
FAILED=0
TOTAL=0

# Run integration tests
echo "=== Running Integration Tests ==="
echo ""

# Basic integration tests
if run_test "End-to-End Replication" "integration_test::test_end_to_end_replication"; then
    ((PASSED++))
else
    ((FAILED++))
fi
((TOTAL++))

if run_test "Replicator Recovery" "integration_test::test_replicator_recovery"; then
    ((PASSED++))
else
    ((FAILED++))
fi
((TOTAL++))

if run_test "Checkpoint Persistence" "integration_test::test_checkpoint_persistence"; then
    ((PASSED++))
else
    ((FAILED++))
fi
((TOTAL++))

# Failure scenario tests
echo ""
echo "=== Running Failure Scenario Tests ==="
echo ""

if run_test "PostgreSQL Connection Failure" "failure_scenarios_test::test_postgres_connection_failure"; then
    ((PASSED++))
else
    ((FAILED++))
fi
((TOTAL++))

if run_test "Kafka Connection Failure" "failure_scenarios_test::test_kafka_connection_failure"; then
    ((PASSED++))
else
    ((FAILED++))
fi
((TOTAL++))

if run_test "Replicator Crash Recovery" "failure_scenarios_test::test_replicator_crash_recovery"; then
    ((PASSED++))
else
    ((FAILED++))
fi
((TOTAL++))

if run_test "Malformed Replication Data" "failure_scenarios_test::test_malformed_replication_data"; then
    ((PASSED++))
else
    ((FAILED++))
fi
((TOTAL++))

# Exactly-once semantics tests
echo ""
echo "=== Running Exactly-Once Semantics Tests ==="
echo ""

if run_test "No Duplicate Messages" "exactly_once_test::test_no_duplicate_messages"; then
    ((PASSED++))
else
    ((FAILED++))
fi
((TOTAL++))

if run_test "Checkpoint Consistency" "exactly_once_test::test_checkpoint_consistency"; then
    ((PASSED++))
else
    ((FAILED++))
fi
((TOTAL++))

if run_test "Transaction Boundaries" "exactly_once_test::test_transaction_boundaries"; then
    ((PASSED++))
else
    ((FAILED++))
fi
((TOTAL++))

# Performance tests (optional)
echo ""
echo -e "${YELLOW}=== Performance Tests (Optional) ===${NC}"
echo "Run performance tests? (y/N)"
read -r response
if [[ "$response" =~ ^([yY][eE][sS]|[yY])$ ]]; then
    if run_test "Throughput Test" "performance_test::test_throughput"; then
        ((PASSED++))
    else
        ((FAILED++))
    fi
    ((TOTAL++))
    
    if run_test "Memory Usage Test" "performance_test::test_memory_usage"; then
        ((PASSED++))
    else
        ((FAILED++))
    fi
    ((TOTAL++))
fi

# Summary
echo ""
echo "========================================"
echo "Test Summary:"
echo "========================================"
echo -e "Total Tests: $TOTAL"
echo -e "Passed: ${GREEN}$PASSED${NC}"
echo -e "Failed: ${RED}$FAILED${NC}"

if [ $FAILED -eq 0 ]; then
    echo ""
    echo -e "${GREEN}All tests passed! 🎉${NC}"
    exit 0
else
    echo ""
    echo -e "${RED}Some tests failed. Please check the logs.${NC}"
    exit 1
fi