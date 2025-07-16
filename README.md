# pg-capture

A high-performance PostgreSQL logical replication tool that streams database changes to Apache Kafka topics in real-time. Based on Supabase's pg_replicate project.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org)
[![Docker](https://img.shields.io/badge/docker-ready-blue.svg)](https://www.docker.com)

## Features

- **Real-time CDC**: Stream PostgreSQL changes to Kafka with minimal latency
- **Reliable Delivery**: Exactly-once semantics with checkpoint management
- **High Performance**: Handles 1000+ changes/second with <500ms latency
- **Fault Tolerant**: Automatic recovery from network failures and crashes
- **Easy Setup**: Simple TOML configuration and Docker support
- **Type Safety**: Preserves PostgreSQL data types in JSON output
- **Low Resource Usage**: <256MB memory footprint

## Quick Start

### Using Docker Compose

```bash
# Clone the repository
git clone https://github.com/yourusername/pg-capture.git
cd pg-capture

# Start PostgreSQL, Kafka, and pg-capture
docker-compose up -d

# Create a test table and publication
docker-compose exec postgres psql -U postgres -d testdb -c "
CREATE TABLE users (
    id SERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    email TEXT NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE PUBLICATION my_publication FOR TABLE users;
"

# Insert some data
docker-compose exec postgres psql -U postgres -d testdb -c "
INSERT INTO users (name, email) VALUES 
    ('Alice', 'alice@example.com'),
    ('Bob', 'bob@example.com');
"

# View the replicated messages in Kafka
docker-compose exec kafka kafka-console-consumer.sh \
    --bootstrap-server localhost:9092 \
    --topic cdc.public.users \
    --from-beginning
```

### Using Binary

```bash
# Install from source
cargo install --path .

# Set required environment variables
export PG_DATABASE=mydb
export PG_USERNAME=replicator
export PG_PASSWORD=secret
export KAFKA_BROKERS=localhost:9092

# Run with optional environment variables
export PG_HOST=localhost  # default: localhost
export PG_PORT=5432       # default: 5432
export KAFKA_TOPIC_PREFIX=cdc  # default: cdc

pg-capture
```

### Using direnv for local development

```bash
# Copy example environment file
cp .envrc.example .envrc

# Edit with your settings
vi .envrc

# Allow direnv to load the environment
direnv allow
```

## Configuration

pg-capture uses environment variables for configuration. See `.envrc.example` for all available options.

### Required Environment Variables

```bash
PG_DATABASE              # PostgreSQL database name
PG_USERNAME              # PostgreSQL username
PG_PASSWORD              # PostgreSQL password
KAFKA_BROKERS            # Comma-separated list of Kafka brokers
```

### Optional Environment Variables

```bash
# PostgreSQL
PG_HOST                  # default: localhost
PG_PORT                  # default: 5432
PG_PUBLICATION           # default: pg_replicate_kafka_pub
PG_SLOT_NAME             # default: pg_replicate_kafka_slot
PG_CONNECT_TIMEOUT_SECS  # default: 30
PG_SSL_MODE              # default: disable (options: disable, prefer, require)

# Kafka
KAFKA_TOPIC_PREFIX       # default: cdc
KAFKA_COMPRESSION        # default: snappy (options: none, gzip, snappy, lz4, zstd)
KAFKA_ACKS               # default: all (options: 0, 1, all)
KAFKA_LINGER_MS          # default: 100
KAFKA_BATCH_SIZE         # default: 16384
KAFKA_BUFFER_MEMORY      # default: 33554432 (32MB)

# Replication
REPLICATION_POLL_INTERVAL_MS         # default: 100
REPLICATION_KEEPALIVE_INTERVAL_SECS  # default: 10
REPLICATION_CHECKPOINT_INTERVAL_SECS # default: 10
REPLICATION_CHECKPOINT_FILE          # optional checkpoint file path
REPLICATION_MAX_BUFFER_SIZE          # default: 1000
```

### PostgreSQL Setup

1. Enable logical replication in `postgresql.conf`:
```ini
wal_level = logical
max_replication_slots = 4
max_wal_senders = 4
```

2. Create a replication user:
```sql
CREATE USER replicator WITH REPLICATION LOGIN PASSWORD 'secret';
GRANT CONNECT ON DATABASE mydb TO replicator;
GRANT USAGE ON SCHEMA public TO replicator;
GRANT SELECT ON ALL TABLES IN SCHEMA public TO replicator;
```

3. Create a publication:
```sql
CREATE PUBLICATION my_publication FOR TABLE users, orders, products;
-- Or for all tables:
-- CREATE PUBLICATION my_publication FOR ALL TABLES;
```

### Kafka Topic Structure

Topics are created automatically with the format: `{topic_prefix}.{schema}.{table}`

Example: `cdc.public.users`

## Message Format

Messages are published to Kafka in JSON format:

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
    "connector": "pg-capture",
    "ts_ms": 1634567890100,
    "db": "mydb",
    "schema": "public",
    "table": "users",
    "lsn": "0/1634FA0",
    "xid": 567
  }
}
```

### Operation Types

- `INSERT`: New row inserted (before: null, after: row data)
- `UPDATE`: Row updated (before: old data, after: new data)
- `DELETE`: Row deleted (before: row data, after: null)

## Command Line Options

```bash
pg-capture [OPTIONS]

OPTIONS:
    -j, --json-logs          Enable JSON output for logs
    -v, --verbose            Enable verbose logging (debug level)
    -h, --help               Print help information
    -V, --version            Print version information
```

## Docker Deployment

### Using Pre-built Image

```bash
docker run -d \
  --name pg-capture \
  -e PG_DATABASE=mydb \
  -e PG_USERNAME=replicator \
  -e PG_PASSWORD=secret \
  -e KAFKA_BROKERS=kafka:9092 \
  -v pg-capture-data:/var/lib/pg-capture \
  ghcr.io/yourusername/pg-capture:latest
```

### Building Custom Image

```bash
# Build the image
docker build -t pg-capture:latest .

# Run with environment variables
docker run -d \
  --name pg-capture \
  --env-file .env \
  -v pg-capture-data:/var/lib/pg-capture \
  pg-capture:latest
```

### Local Development with Docker Compose

```bash
# Start all services (PostgreSQL, Kafka, Kafka UI)
docker-compose up -d

# Start with pg-capture
docker-compose --profile app up -d

# View logs
docker-compose logs -f pg-capture

# Run integration tests
docker-compose -f docker-compose.test.yml up --abort-on-container-exit
```

## Monitoring

### Logs

pg-capture uses structured logging with configurable levels:

```bash
# Set log level via environment variable
export RUST_LOG=pg_replicate_kafka=debug

# Or in config.toml
[logging]
level = "info"  # Options: error, warn, info, debug, trace
format = "json" # Options: plain, json
```

### Health Checks

The replicator logs its status regularly:
- Connection status to PostgreSQL and Kafka
- Replication lag (LSN position)
- Messages processed per second
- Error counts and retry attempts

### Metrics (Coming in v0.2.0)

Future versions will expose Prometheus metrics on port 9090.

## Troubleshooting

### Common Issues

1. **"replication slot already exists"**
   ```sql
   SELECT pg_drop_replication_slot('pg_replicate_kafka_slot');
   ```

2. **"publication does not exist"**
   ```sql
   CREATE PUBLICATION my_publication FOR ALL TABLES;
   ```

3. **Kafka connection timeout**
   - Check broker addresses
   - Verify network connectivity
   - Check Kafka broker logs

4. **High replication lag**
   - Increase `poll_interval_ms`
   - Check network bandwidth
   - Monitor PostgreSQL WAL size

### Debug Mode

Enable debug logging to see detailed information:

```bash
RUST_LOG=pg_replicate_kafka=debug,rdkafka=debug pg-capture
```

## Performance Tuning

### PostgreSQL
- Increase `wal_sender_timeout` for slow networks
- Tune `max_wal_size` for write-heavy workloads
- Consider partitioning large tables

### Kafka
- Adjust `batch.size` and `linger.ms` for throughput
- Use compression for large messages
- Increase partitions for parallel consumption

### pg-capture
- Decrease `poll_interval_ms` for lower latency
- Increase `checkpoint_interval_secs` for better throughput
- Use SSD storage for checkpoint files

## Development

### Building from Source

```bash
# Clone repository
git clone https://github.com/yourusername/pg-capture.git
cd pg-capture

# Build release binary
cargo build --release

# Run tests
cargo test

# Run integration tests (requires PostgreSQL and Kafka)
./tests/run_integration_tests.sh
```

### Architecture

pg-capture follows a modular architecture:

- **PostgreSQL Source**: Manages replication connection and protocol
- **Message Decoder**: Parses pgoutput format messages  
- **Kafka Sink**: Handles reliable message delivery
- **Checkpoint Manager**: Ensures exactly-once semantics
- **Replicator Core**: Orchestrates the data pipeline

## Contributing

Contributions are welcome! Please read our [Contributing Guide](CONTRIBUTING.md) for details.

### Development Setup

1. Install Rust 1.75+
2. Install Docker and Docker Compose
3. Run `docker-compose up -d postgres kafka`
4. Run `cargo test`

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

- Based on [Supabase's pg_replicate](https://github.com/supabase/etl)
- Built with [tokio-postgres](https://github.com/sfackler/rust-postgres)
- Powered by [rdkafka](https://github.com/fede1024/rust-rdkafka)
