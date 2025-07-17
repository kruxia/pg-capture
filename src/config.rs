//! Configuration module for pg-capture.
//!
//! This module provides configuration structures and utilities for loading
//! settings from environment variables. All configuration follows the 12-factor
//! app methodology.
//!
//! # Example
//!
//! ```rust,no_run
//! use pg_capture::Config;
//!
//! // Load from environment variables
//! let config = Config::from_env().expect("Failed to load config");
//!
//! // Access configuration values
//! println!("Connecting to PostgreSQL at {}:{}",
//!          config.postgres.host, config.postgres.port);
//! println!("Publishing to Kafka brokers: {:?}",
//!          config.kafka.brokers);
//! ```

use crate::Error;
use serde::{Deserialize, Serialize};
use std::env;
use std::path::PathBuf;

/// Main configuration structure containing all settings for pg-capture.
///
/// Configuration is organized into three sections:
/// - `postgres` - PostgreSQL connection and replication settings
/// - `kafka` - Kafka connection and producer settings
/// - `replication` - Replication behavior and tuning parameters
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub postgres: PostgresConfig,
    pub kafka: KafkaConfig,
    pub replication: ReplicationConfig,
}

/// PostgreSQL connection and replication configuration.
///
/// Contains all settings needed to establish a logical replication connection
/// to PostgreSQL and manage the replication slot and publication.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PostgresConfig {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password: String,
    pub publication: String,
    pub slot_name: String,
    pub connect_timeout_secs: u64,
    pub ssl_mode: SslMode,
}

/// SSL/TLS connection mode for PostgreSQL.
///
/// Controls whether and how SSL/TLS is used when connecting to PostgreSQL.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub enum SslMode {
    #[default]
    Disable,
    Prefer,
    Require,
}

impl std::str::FromStr for SslMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "disable" => Ok(SslMode::Disable),
            "prefer" => Ok(SslMode::Prefer),
            "require" => Ok(SslMode::Require),
            _ => Err(format!(
                "Invalid SSL mode: {s}. Valid values: disable, prefer, require"
            )),
        }
    }
}

/// Kafka connection and producer configuration.
///
/// Contains all settings for connecting to Kafka brokers and tuning
/// the producer for optimal CDC performance.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct KafkaConfig {
    pub brokers: Vec<String>,
    pub topic_prefix: String,
    pub compression: String,
    pub acks: String,
    pub linger_ms: u32,
    pub batch_size: usize,
    pub buffer_memory: usize,
}

/// Replication behavior and tuning configuration.
///
/// Controls various aspects of the replication process including
/// polling intervals, checkpointing, and buffer sizes.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReplicationConfig {
    pub poll_interval_ms: u64,
    pub keepalive_interval_secs: u64,
    pub checkpoint_interval_secs: u64,
    pub checkpoint_file: Option<PathBuf>,
    pub max_buffer_size: usize,
}

impl Config {
    /// Loads configuration from environment variables.
    ///
    /// Required environment variables:
    /// - `PG_DATABASE` - PostgreSQL database name
    /// - `PG_USERNAME` - PostgreSQL username
    /// - `PG_PASSWORD` - PostgreSQL password
    /// - `KAFKA_BROKERS` - Comma-separated list of Kafka brokers
    ///
    /// Optional variables have sensible defaults. See the struct fields
    /// for documentation of all available options.
    ///
    /// # Errors
    ///
    /// Returns `Err` if:
    /// - Required environment variables are missing
    /// - Values cannot be parsed (e.g., invalid port number)
    /// - Values are invalid (e.g., empty broker list)
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use pg_capture::Config;
    /// use std::env;
    ///
    /// // Set required environment variables
    /// env::set_var("PG_DATABASE", "mydb");
    /// env::set_var("PG_USERNAME", "replicator");
    /// env::set_var("PG_PASSWORD", "secret");
    /// env::set_var("KAFKA_BROKERS", "localhost:9092");
    ///
    /// let config = Config::from_env().expect("Failed to load config");
    /// ```
    pub fn from_env() -> crate::Result<Self> {
        // PostgreSQL config
        let postgres = PostgresConfig {
            host: env::var("PG_HOST").unwrap_or_else(|_| "localhost".to_string()),
            port: env::var("PG_PORT")
                .unwrap_or_else(|_| "5432".to_string())
                .parse::<u16>()
                .map_err(|_| Error::Config("PG_PORT must be a valid port number".to_string()))?,
            database: env::var("PG_DATABASE")
                .map_err(|_| Error::Config("PG_DATABASE is required".to_string()))?,
            username: env::var("PG_USERNAME")
                .map_err(|_| Error::Config("PG_USERNAME is required".to_string()))?,
            password: env::var("PG_PASSWORD")
                .map_err(|_| Error::Config("PG_PASSWORD is required".to_string()))?,
            publication: env::var("PG_PUBLICATION")
                .unwrap_or_else(|_| "pg_capture_pub".to_string()),
            slot_name: env::var("PG_SLOT_NAME").unwrap_or_else(|_| "pg_capture_slot".to_string()),
            connect_timeout_secs: env::var("PG_CONNECT_TIMEOUT_SECS")
                .unwrap_or_else(|_| "30".to_string())
                .parse::<u64>()
                .unwrap_or(30),
            ssl_mode: env::var("PG_SSL_MODE")
                .unwrap_or_else(|_| "disable".to_string())
                .parse::<SslMode>()
                .map_err(Error::Config)?,
        };

        // Kafka config
        let brokers = env::var("KAFKA_BROKERS")
            .map_err(|_| Error::Config("KAFKA_BROKERS is required".to_string()))?
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();

        if brokers.is_empty() {
            return Err(Error::Config(
                "KAFKA_BROKERS must contain at least one broker".to_string(),
            ));
        }

        let kafka = KafkaConfig {
            brokers,
            topic_prefix: env::var("KAFKA_TOPIC_PREFIX").unwrap_or_else(|_| "cdc".to_string()),
            compression: env::var("KAFKA_COMPRESSION").unwrap_or_else(|_| "snappy".to_string()),
            acks: env::var("KAFKA_ACKS").unwrap_or_else(|_| "all".to_string()),
            linger_ms: env::var("KAFKA_LINGER_MS")
                .unwrap_or_else(|_| "100".to_string())
                .parse::<u32>()
                .unwrap_or(100),
            batch_size: env::var("KAFKA_BATCH_SIZE")
                .unwrap_or_else(|_| "16384".to_string())
                .parse::<usize>()
                .unwrap_or(16384),
            buffer_memory: env::var("KAFKA_BUFFER_MEMORY")
                .unwrap_or_else(|_| "33554432".to_string())
                .parse::<usize>()
                .unwrap_or(33_554_432), // 32MB
        };

        // Replication config
        let replication = ReplicationConfig {
            poll_interval_ms: env::var("REPLICATION_POLL_INTERVAL_MS")
                .unwrap_or_else(|_| "100".to_string())
                .parse::<u64>()
                .unwrap_or(100),
            keepalive_interval_secs: env::var("REPLICATION_KEEPALIVE_INTERVAL_SECS")
                .unwrap_or_else(|_| "10".to_string())
                .parse::<u64>()
                .unwrap_or(10),
            checkpoint_interval_secs: env::var("REPLICATION_CHECKPOINT_INTERVAL_SECS")
                .unwrap_or_else(|_| "10".to_string())
                .parse::<u64>()
                .unwrap_or(10),
            checkpoint_file: env::var("REPLICATION_CHECKPOINT_FILE")
                .ok()
                .map(PathBuf::from),
            max_buffer_size: env::var("REPLICATION_MAX_BUFFER_SIZE")
                .unwrap_or_else(|_| "1000".to_string())
                .parse::<usize>()
                .unwrap_or(1000),
        };

        Ok(Config {
            postgres,
            kafka,
            replication,
        })
    }

    /// Constructs a PostgreSQL connection URL.
    ///
    /// This URL is used for regular connections. The replication parameter
    /// will be added separately when needed for replication protocol.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use pg_capture::Config;
    /// # let config = Config::from_env().unwrap();
    /// let url = config.postgres_url();
    /// // Returns: "postgres://user:pass@host:5432/db"
    /// ```
    pub fn postgres_url(&self) -> String {
        format!(
            "postgres://{}:{}@{}:{}/{}",
            self.postgres.username,
            self.postgres.password,
            self.postgres.host,
            self.postgres.port,
            self.postgres.database
        )
    }

    /// Constructs a Kafka topic name for a table.
    ///
    /// **Deprecated**: Use `kafka.topic_name()` instead which properly
    /// handles schema names.
    ///
    /// # Arguments
    ///
    /// * `table_name` - Name of the table (without schema)
    ///
    /// # Example
    ///
    /// ```rust
    /// # use pg_capture::Config;
    /// # let config = Config::from_env().unwrap();
    /// let topic = config.kafka_topic_name("users");
    /// // Returns: "cdc.users" (if topic_prefix is "cdc")
    /// ```
    #[deprecated(since = "0.1.0", note = "Use kafka.topic_name() instead")]
    pub fn kafka_topic_name(&self, table_name: &str) -> String {
        format!("{}.{table_name}", self.kafka.topic_prefix)
    }
}

impl KafkaConfig {
    /// Constructs a Kafka topic name for a schema and table.
    ///
    /// Topic names follow the pattern: `{prefix}.{schema}.{table}`
    ///
    /// # Arguments
    ///
    /// * `schema` - PostgreSQL schema name (e.g., "public")
    /// * `table` - PostgreSQL table name
    ///
    /// # Example
    ///
    /// ```rust
    /// use pg_capture::config::KafkaConfig;
    ///
    /// let kafka_config = KafkaConfig {
    ///     topic_prefix: "cdc".to_string(),
    ///     // ... other fields
    /// # brokers: vec![],
    /// # compression: String::new(),
    /// # acks: String::new(),
    /// # linger_ms: 0,
    /// # batch_size: 0,
    /// # buffer_memory: 0,
    /// };
    ///
    /// let topic = kafka_config.topic_name("public", "users");
    /// assert_eq!(topic, "cdc.public.users");
    /// ```
    pub fn topic_name(&self, schema: &str, table: &str) -> String {
        format!("{}.{schema}.{table}", self.topic_prefix)
    }
}
