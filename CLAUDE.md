# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

pg-replicate-kafka is a PostgreSQL logical replicator that publishes changes to Kafka topics. It's based on Supabase's pg_replicate (now called "etl") project. This is a Rust project currently in initial setup phase.

## Development Commands

Since this is a new Rust project without Cargo.toml yet, these are the standard commands once initialized:

```bash
# Build the project
cargo build
cargo build --release

# Run tests
cargo test
cargo test -- --nocapture  # Show println! output during tests

# Run the application
cargo run
cargo run --release

# Code quality
cargo fmt         # Format code according to Rust standards
cargo clippy      # Lint code for common mistakes and improvements
cargo check       # Fast type-checking without producing binaries

# Documentation
cargo doc --open  # Generate and open documentation
```

## Architecture

The project follows a source-sink architecture pattern for Change Data Capture (CDC):

### Core Components (to be implemented):
1. **PostgreSQL Source**: Connects to PostgreSQL using logical replication protocol
2. **Kafka Sink**: Publishes CDC events to Kafka topics
3. **Configuration Layer**: Manages connection settings and behavior
4. **Error Handling**: Resilient handling of connection failures and retries

### Expected Directory Structure:
```
src/
├── main.rs           # Application entry point
├── lib.rs            # Library exports
├── config/           # Configuration management
├── source/           # Source connectors
│   └── postgres.rs   # PostgreSQL logical replication
└── sink/             # Sink connectors
    └── kafka.rs      # Kafka producer implementation
```

### Key Dependencies (when Cargo.toml is created):
- `tokio`: Async runtime for concurrent operations
- `rdkafka`: Kafka client library for Rust (required for Kafka integration)
- `tokio-postgres` or `postgres`: PostgreSQL client
- `serde` & `serde_json`: Serialization for CDC events
- `tracing`: Structured logging and internal observability metrics (required)
- `config`: Configuration management

## Implementation Guidelines

1. **Async First**: Use tokio for all I/O operations
2. **Error Handling**: Use `thiserror` for custom error types, propagate errors with `?`
3. **Configuration**: Support both environment variables and config files
4. **Testing**: Write integration tests for PostgreSQL → Kafka pipeline
5. **Performance**: Use buffering and batching for Kafka writes

## PostgreSQL Logical Replication

Key concepts for implementing the PostgreSQL source:
- Use `REPLICATION` protocol with `CREATE_REPLICATION_SLOT`
- Handle WAL (Write-Ahead Log) messages
- Parse logical decoding output (likely using `wal2json` or similar)
- Maintain replication slot position for resumability

## Kafka Integration

**Use `rdkafka` for all Kafka operations.** Important considerations:
- Topic naming strategy (e.g., `schema.table` format)
- Message key selection (primary key or custom)
- Serialization format (JSON, Avro, or Protobuf)
- Producer configuration for reliability vs performance
- Schema registry integration (if using Avro/Protobuf)
- Configure rdkafka producer with appropriate settings for CDC workloads

## Logging and Observability

**Use `tracing` for all logging and internal observability metrics.** Guidelines:
- Use structured logging with span context
- Instrument async functions with `#[tracing::instrument]`
- Log at appropriate levels: ERROR for failures, WARN for retries, INFO for state changes, DEBUG for detailed flow
- Export metrics through tracing subscribers (e.g., replication lag, messages processed, errors)
- Consider using `tracing-subscriber` with JSON formatting for production

## Development Setup

To start development:
1. Create `Cargo.toml` with appropriate dependencies (including `rdkafka` and `tracing`)
2. Set up basic CLI argument parsing (consider `clap`)
3. Implement configuration loading
4. Create PostgreSQL connection with replication protocol
5. Implement Kafka producer using `rdkafka` with proper error handling
6. Set up tracing subscriber for logging and metrics