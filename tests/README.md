# Integration Test Suite

This directory contains comprehensive integration tests for pg-replicate-kafka. The tests verify end-to-end functionality, failure scenarios, performance characteristics, and exactly-once semantics.

## Test Categories

### 1. Basic Integration Tests (`integration_test.rs`)
- **End-to-end replication**: Verifies INSERT, UPDATE, DELETE operations are correctly replicated
- **Replicator recovery**: Tests recovery after crashes with checkpoint persistence
- **Checkpoint persistence**: Ensures replication resumes from the correct position

### 2. Failure Scenario Tests (`failure_scenarios_test.rs`)
- **PostgreSQL connection failures**: Tests behavior when database becomes unavailable
- **Kafka connection failures**: Tests behavior when Kafka brokers are unreachable
- **Replicator crash recovery**: Simulates crashes and verifies data integrity
- **Malformed data handling**: Tests edge cases with complex PostgreSQL data types

### 3. Exactly-Once Semantics Tests (`exactly_once_test.rs`)
- **No duplicate messages**: Verifies messages are delivered exactly once despite crashes
- **Checkpoint consistency**: Ensures checkpoints accurately reflect delivered messages
- **Transaction boundaries**: Verifies transactional integrity is maintained

### 4. Performance Tests (`performance_test.rs`)
- **Throughput test**: Measures messages per second (target: >1000 msg/sec)
- **Memory usage test**: Monitors memory consumption over time (target: <256MB)

## Prerequisites

Before running the tests, ensure:

1. PostgreSQL is running on `localhost:5432`
2. Kafka is running on `localhost:9092`
3. PostgreSQL user `postgres` exists with password `postgres`
4. The `postgres` user has replication privileges

## Running Tests

### Run All Tests
```bash
./tests/run_integration_tests.sh
```

### Run Individual Test Categories
```bash
# Basic integration tests
cargo test --test integration_test -- --ignored --nocapture

# Failure scenarios
cargo test --test failure_scenarios_test -- --ignored --nocapture

# Exactly-once semantics
cargo test --test exactly_once_test -- --ignored --nocapture

# Performance tests
cargo test --test performance_test -- --ignored --nocapture
```

### Run Specific Test
```bash
cargo test --test integration_test::test_end_to_end_replication -- --ignored --nocapture
```

## Test Infrastructure

### Test Isolation
- Each test uses unique table names, publication names, and replication slots
- Process ID is incorporated into names to allow parallel test execution
- All test artifacts are cleaned up after each test

### Test Database Setup
Each test:
1. Creates a test table
2. Creates a publication for the table
3. Configures a unique replication slot
4. Runs the test scenario
5. Cleans up all created objects

### Kafka Topic Management
- Topics are automatically created with unique prefixes
- Consumer groups use unique IDs to avoid conflicts
- Messages are consumed from the earliest offset

## Performance Baselines

Based on the performance tests, pg-replicate-kafka should achieve:
- **Throughput**: >1,000 messages/second
- **Latency**: <500ms end-to-end
- **Memory**: <256MB under sustained load
- **CPU**: Efficient async I/O with minimal overhead

## Troubleshooting

### Common Issues

1. **Tests fail with connection errors**
   - Verify PostgreSQL and Kafka are running
   - Check firewall settings
   - Ensure correct credentials

2. **Replication slot already exists**
   - Manually drop stale slots: `SELECT pg_drop_replication_slot('slot_name');`
   - Tests should clean up automatically, but crashes may leave artifacts

3. **Performance tests fail**
   - Ensure system isn't under heavy load
   - Check Kafka broker performance
   - Verify PostgreSQL configuration allows sufficient connections

### Debug Output

Enable detailed logging:
```bash
RUST_LOG=pg_replicate_kafka=debug cargo test --test integration_test -- --ignored --nocapture
```

## Writing New Tests

When adding new integration tests:

1. Use the established helper functions for setup/cleanup
2. Ensure proper test isolation with unique names
3. Always clean up resources in the test teardown
4. Use appropriate timeouts for async operations
5. Verify both positive and negative scenarios
6. Add documentation for complex test scenarios

Example test structure:
```rust
#[tokio::test]
#[ignore] // Run with --ignored flag
async fn test_new_scenario() {
    // Setup
    let (client, config) = setup_test_database().await;
    
    // Test logic
    // ...
    
    // Cleanup
    cleanup_test_database(&client).await;
}
```