# Phase 1 Implementation Plan: pg-replicate-kafka MVP

## Overview

Phase 1 focuses on delivering a minimal viable product that can reliably replicate a single PostgreSQL table to a Kafka topic using JSON format. This establishes the core architecture and proves the fundamental concept before expanding to more complex features.

## Goals

1. **Establish Core Pipeline**: PostgreSQL → pg-replicate-kafka → Kafka
2. **Prove Reliability**: Demonstrate stable replication with error recovery
3. **Validate Performance**: Achieve reasonable throughput for single table
4. **Create Foundation**: Build extensible architecture for future phases

## Scope

### In Scope
- Single table replication via PostgreSQL publication
- JSON message format only
- Basic error handling and reconnection logic
- Essential CLI interface
- Minimal configuration via TOML file
- Basic structured logging with tracing
- Docker container support

### Out of Scope (Future Phases)
- Multiple tables or complex publications
- Avro/Protobuf formats
- Schema registry integration
- Web UI or REST API
- Advanced transformations
- Metrics endpoints
- Initial snapshot support

## Technical Architecture

### Component Structure

```
pg-replicate-kafka/
├── Cargo.toml              # Dependencies and metadata
├── src/
│   ├── main.rs            # CLI entry point
│   ├── lib.rs             # Library exports
│   ├── config.rs          # Configuration structs
│   ├── error.rs           # Error types
│   ├── replicator.rs      # Main orchestration logic
│   ├── postgres/
│   │   ├── mod.rs         # PostgreSQL module
│   │   ├── connection.rs  # Replication connection
│   │   ├── decoder.rs     # Logical decoding parser
│   │   └── types.rs       # PostgreSQL types
│   └── kafka/
│       ├── mod.rs         # Kafka module
│       ├── producer.rs    # Kafka producer wrapper
│       └── serializer.rs  # JSON serialization
├── config/
│   └── example.toml       # Example configuration
└── Dockerfile             # Container image
```

### Data Flow

```
PostgreSQL Table
    ↓
[PUBLICATION]
    ↓
Logical Replication Slot (pgoutput)
    ↓
[pg-replicate-kafka]
    ├─→ Replication Connection
    ├─→ Message Decoder
    ├─→ JSON Serializer
    └─→ Kafka Producer
         ↓
    Kafka Topic
```

## Implementation Steps

### Step 1: Project Setup (Day 1-2)

**Tasks:**
1. Create Cargo.toml with core dependencies
2. Set up basic project structure
3. Implement error types using thiserror
4. Create configuration structs with serde
5. Set up tracing subscriber for logging

**Deliverables:**
- Compilable project skeleton
- Basic CLI that loads configuration
- Structured logging setup

### Step 2: PostgreSQL Connection (Day 3-5)

**Tasks:**
1. Implement replication connection using tokio-postgres
2. Create replication slot management
3. Handle IDENTIFY_SYSTEM and START_REPLICATION
4. Parse pgoutput format messages
5. Implement basic keepalive handling

**Deliverables:**
- Working connection to PostgreSQL replication
- Ability to receive and decode WAL messages
- Test harness for PostgreSQL integration

### Step 3: Message Parsing (Day 6-8)

**Tasks:**
1. Implement pgoutput protocol decoder
2. Parse BEGIN, COMMIT, INSERT, UPDATE, DELETE messages
3. Handle RELATION messages for table metadata
4. Convert PostgreSQL types to JSON values
5. Create structured change events

**Deliverables:**
- Complete pgoutput parser
- JSON representation of change events
- Unit tests for message parsing

### Step 4: Kafka Integration (Day 9-11)

**Tasks:**
1. Set up rdkafka producer with proper configuration
2. Implement JSON serialization for change events
3. Configure producer for reliability (acks, retries)
4. Handle producer errors and backpressure
5. Implement graceful shutdown

**Deliverables:**
- Working Kafka producer integration
- JSON messages published to topics
- Error handling for producer failures

### Step 5: Core Replicator Loop (Day 12-14)

**Tasks:**
1. Implement main replication loop
2. Coordinate PostgreSQL reading and Kafka writing
3. Add checkpoint management (LSN tracking)
4. Implement graceful error recovery
5. Handle connection failures on both ends

**Deliverables:**
- End-to-end replication working
- Automatic recovery from failures
- Checkpoint persistence

### Step 6: Testing & Hardening (Day 15-17)

**Tasks:**
1. Create integration test suite
2. Test failure scenarios (network, crashes)
3. Verify exactly-once semantics
4. Performance testing and optimization
5. Memory leak detection

**Deliverables:**
- Comprehensive test suite
- Performance baseline metrics
- Bug fixes and optimizations

### Step 7: Packaging & Documentation (Day 18-20)

**Tasks:**
1. Create Dockerfile with minimal image
2. Write user documentation
3. Create example configurations
4. Set up CI/CD pipeline
5. Prepare release artifacts

**Deliverables:**
- Docker image
- User guide
- Configuration examples
- v0.1.0 release

## Configuration Schema (Phase 1)

```toml
# config.toml
[postgres]
host = "localhost"
port = 5432
database = "mydb"
username = "replicator"
password = "secret"
publication = "my_publication"
slot_name = "pg_replicate_kafka_slot"

[kafka]
brokers = ["localhost:9092"]
topic_prefix = "cdc"
compression = "snappy"
acks = "all"

[replication]
poll_interval_ms = 100
keepalive_interval_secs = 10
checkpoint_interval_secs = 10
```

## JSON Message Format

```json
{
  "schema": "public",
  "table": "users",
  "op": "INSERT",
  "ts_ms": 1634567890123,
  "before": null,
  "after": {
    "id": 123,
    "name": "John Doe",
    "email": "john@example.com",
    "created_at": "2023-10-15T10:30:00Z"
  },
  "source": {
    "version": "0.1.0",
    "connector": "pg-replicate-kafka",
    "ts_ms": 1634567890100,
    "db": "mydb",
    "schema": "public",
    "table": "users",
    "lsn": "0/1634FA0",
    "xid": 567
  }
}
```

## Dependencies

```toml
[dependencies]
tokio = { version = "1.40", features = ["full"] }
tokio-postgres = { version = "0.7", features = ["runtime", "with-chrono-0_4"] }
rdkafka = { version = "0.36", features = ["tokio"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
config = "0.14"
thiserror = "1.0"
clap = { version = "4.5", features = ["derive"] }
chrono = { version = "0.4", features = ["serde"] }
bytes = "1.5"
futures = "0.3"
```

## Success Criteria

Phase 1 is complete when:

1. **Functional Requirements**
   - Single table replication works reliably
   - JSON messages are correctly formatted
   - Automatic recovery from failures
   - No data loss during normal operation

2. **Performance Requirements**
   - Handles 1,000 changes/second minimum
   - Memory usage stays under 256MB
   - Latency under 500ms end-to-end

3. **Operational Requirements**
   - Docker image under 50MB
   - Starts up in under 5 seconds
   - Graceful shutdown on SIGTERM
   - Clear error messages in logs

## Risk Mitigation

### Technical Risks
- **pgoutput complexity**: Start with minimal decoder, expand incrementally
- **Kafka producer tuning**: Use conservative defaults, document tuning
- **Type conversions**: Focus on common types, document limitations

### Schedule Risks
- **Dependency issues**: Use stable versions, avoid experimental features
- **Testing complexity**: Automate with docker-compose environments
- **Unknown unknowns**: Budget 20% time for unexpected issues

## Next Steps (Phase 2 Preview)

After Phase 1 success:
- Add support for multiple tables
- Implement initial snapshot capability
- Add Avro format with schema registry
- Create metrics endpoint
- Support more PostgreSQL types
- Add transformation capabilities

## Team Requirements

### Core Development
- 1 Rust developer (senior level)
- PostgreSQL and Kafka experience required
- 20 days full-time commitment

### Support Needs
- PostgreSQL DBA for testing/validation (part-time)
- DevOps for CI/CD setup (2-3 days)
- Technical writer for documentation (3-5 days)

## Definition of Done

Phase 1 is complete when:
- [ ] All code merged to main branch
- [ ] Integration tests passing
- [ ] Docker image published
- [ ] Documentation complete
- [ ] Example configuration working
- [ ] Performance benchmarks documented
- [ ] v0.1.0 tagged and released

## Monitoring Progress

Weekly checkpoints:
- Week 1: PostgreSQL connection working
- Week 2: End-to-end pipeline functional
- Week 3: Hardened and documented
- Week 4: Released and deployed

Daily updates:
- Stand-up on progress
- Blocker identification
- Scope adjustment if needed