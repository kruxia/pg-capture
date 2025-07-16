# Changelog

All notable changes to pg-replicate-kafka will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2024-01-XX

### Added
- Initial release of pg-replicate-kafka
- PostgreSQL to Kafka CDC replication using logical replication
- Support for INSERT, UPDATE, DELETE operations
- JSON message format with before/after states
- Automatic Kafka topic creation
- Checkpoint management for exactly-once semantics
- Automatic recovery from connection failures
- Configurable via TOML files
- Docker support with minimal Alpine-based image (<50MB)
- Comprehensive test suite including:
  - Unit tests for all components
  - Integration tests for end-to-end scenarios
  - Performance tests validating >1000 msg/sec throughput
  - Failure scenario tests
- Support for PostgreSQL 14+ and Kafka 2.8+
- Structured logging with configurable levels
- Command-line interface with helpful options

### Performance
- Achieves >1,000 messages/second throughput
- End-to-end latency <500ms
- Memory usage <256MB under sustained load

### Known Limitations
- Single publication/table support only (multi-table coming in v0.2.0)
- JSON format only (Avro/Protobuf coming in v0.2.0)
- No initial snapshot support (coming in v0.2.0)
- No metrics endpoint (coming in v0.2.0)

[0.1.0]: https://github.com/yourusername/pg-replicate-kafka/releases/tag/v0.1.0