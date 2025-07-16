//! # pg-capture
//!
//! A high-performance PostgreSQL to Kafka Change Data Capture (CDC) replicator that uses
//! PostgreSQL's logical replication protocol to stream database changes to Apache Kafka topics
//! in real-time.
//!
//! ## Overview
//!
//! `pg-capture` connects to PostgreSQL as a logical replication client, decodes the replication
//! stream, and publishes changes to Kafka topics. It provides:
//!
//! - **Exactly-once delivery** with checkpoint management
//! - **Automatic failover** and connection retry logic
//! - **High throughput** with batching and compression
//! - **Type-safe** message serialization
//! - **Low latency** streaming (typically < 500ms end-to-end)
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use pg_capture::{Config, Replicator, Result};
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     // Load configuration from environment variables
//!     let config = Config::from_env()?;
//!     
//!     // Create and run the replicator
//!     let mut replicator = Replicator::new(config);
//!     replicator.run().await?;
//!     
//!     Ok(())
//! }
//! ```
//!
//! ## Configuration
//!
//! Configuration is loaded from environment variables. Required variables:
//!
//! - `PG_DATABASE` - PostgreSQL database name
//! - `PG_USERNAME` - PostgreSQL username
//! - `PG_PASSWORD` - PostgreSQL password  
//! - `KAFKA_BROKERS` - Comma-separated list of Kafka brokers
//!
//! See [`Config`] for all available options.
//!
//! ## Message Format
//!
//! Changes are published to Kafka in JSON format:
//!
//! ```json
//! {
//!   "schema": "public",
//!   "table": "users",
//!   "op": "INSERT",
//!   "ts_ms": 1634567890123,
//!   "before": null,
//!   "after": {
//!     "id": 123,
//!     "name": "John Doe",
//!     "email": "john@example.com"
//!   },
//!   "source": {
//!     "version": "0.1.0",
//!     "connector": "pg-capture",
//!     "ts_ms": 1634567890100,
//!     "db": "mydb",
//!     "schema": "public",
//!     "table": "users",
//!     "lsn": "0/1634FA0",
//!     "xid": 567
//!   }
//! }
//! ```
//!
//! ## PostgreSQL Setup
//!
//! 1. Enable logical replication in `postgresql.conf`:
//!    ```ini
//!    wal_level = logical
//!    max_replication_slots = 4
//!    max_wal_senders = 4
//!    ```
//!
//! 2. Create a publication for the tables you want to replicate:
//!    ```sql
//!    CREATE PUBLICATION my_publication FOR TABLE users, orders;
//!    -- Or for all tables:
//!    -- CREATE PUBLICATION my_publication FOR ALL TABLES;
//!    ```
//!
//! 3. Create a replication user:
//!    ```sql
//!    CREATE USER replicator WITH REPLICATION LOGIN PASSWORD 'secret';
//!    GRANT CONNECT ON DATABASE mydb TO replicator;
//!    GRANT USAGE ON SCHEMA public TO replicator;
//!    GRANT SELECT ON ALL TABLES IN SCHEMA public TO replicator;
//!    ```
//!
//! ## Advanced Usage
//!
//! ### Custom Error Handling
//!
//! ```rust,no_run
//! use pg_capture::{Config, Replicator, Result, Error};
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     let config = Config::from_env()?;
//!     let mut replicator = Replicator::new(config);
//!     
//!     match replicator.run().await {
//!         Ok(()) => println!("Replication completed"),
//!         Err(Error::Shutdown) => println!("Graceful shutdown"),
//!         Err(Error::Connection(msg)) => eprintln!("Connection error: {}", msg),
//!         Err(e) => eprintln!("Fatal error: {}", e),
//!     }
//!     
//!     Ok(())
//! }
//! ```
//!
//! ### Programmatic Configuration
//!
//! ```rust
//! use pg_capture::config::{Config, PostgresConfig, KafkaConfig, ReplicationConfig, SslMode};
//!
//! let config = Config {
//!     postgres: PostgresConfig {
//!         host: "localhost".to_string(),
//!         port: 5432,
//!         database: "mydb".to_string(),
//!         username: "replicator".to_string(),
//!         password: "secret".to_string(),
//!         publication: "my_publication".to_string(),
//!         slot_name: "my_slot".to_string(),
//!         connect_timeout_secs: 30,
//!         ssl_mode: SslMode::Prefer,
//!     },
//!     kafka: KafkaConfig {
//!         brokers: vec!["localhost:9092".to_string()],
//!         topic_prefix: "cdc".to_string(),
//!         compression: "snappy".to_string(),
//!         acks: "all".to_string(),
//!         linger_ms: 100,
//!         batch_size: 16384,
//!         buffer_memory: 33_554_432,
//!     },
//!     replication: ReplicationConfig {
//!         poll_interval_ms: 100,
//!         keepalive_interval_secs: 10,
//!         checkpoint_interval_secs: 10,
//!         checkpoint_file: Some("checkpoint.json".into()),
//!         max_buffer_size: 1000,
//!     },
//! };
//! ```
//!
//! ## Architecture
//!
//! The crate is organized into several modules:
//!
//! - [`replicator`] - Main replication orchestrator
//! - [`postgres`] - PostgreSQL connection and protocol handling
//! - [`kafka`] - Kafka producer and message serialization
//! - [`checkpoint`] - Checkpoint management for exactly-once delivery
//! - [`config`] - Configuration structures and parsing
//! - [`error`] - Error types and handling

/// Checkpoint management for exactly-once delivery semantics
pub mod checkpoint;

/// Configuration structures and environment variable parsing
pub mod config;

/// Error types and result handling
pub mod error;

/// Main replication orchestrator that coordinates the CDC pipeline
pub mod replicator;

/// Kafka producer, serialization, and topic management
pub mod kafka;

/// PostgreSQL logical replication connection and protocol handling
pub mod postgres;

pub use config::Config;
pub use error::{Error, Result};
pub use replicator::Replicator;
