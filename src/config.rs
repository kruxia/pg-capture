use serde::{Deserialize, Serialize};
use std::env;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub postgres: PostgresConfig,
    pub kafka: KafkaConfig,
    pub replication: ReplicationConfig,
}

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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum SslMode {
    Disable,
    Prefer,
    Require,
}

impl Default for SslMode {
    fn default() -> Self {
        SslMode::Disable
    }
}

impl std::str::FromStr for SslMode {
    type Err = String;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "disable" => Ok(SslMode::Disable),
            "prefer" => Ok(SslMode::Prefer),
            "require" => Ok(SslMode::Require),
            _ => Err(format!("Invalid SSL mode: {}. Valid values: disable, prefer, require", s)),
        }
    }
}

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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReplicationConfig {
    pub poll_interval_ms: u64,
    pub keepalive_interval_secs: u64,
    pub checkpoint_interval_secs: u64,
    pub checkpoint_file: Option<PathBuf>,
    pub max_buffer_size: usize,
}

impl Config {
    /// Load configuration from environment variables
    pub fn from_env() -> Result<Self, String> {
        // PostgreSQL config
        let postgres = PostgresConfig {
            host: env::var("PG_HOST")
                .unwrap_or_else(|_| "localhost".to_string()),
            port: env::var("PG_PORT")
                .unwrap_or_else(|_| "5432".to_string())
                .parse::<u16>()
                .map_err(|_| "PG_PORT must be a valid port number")?,
            database: env::var("PG_DATABASE")
                .map_err(|_| "PG_DATABASE is required")?,
            username: env::var("PG_USERNAME")
                .map_err(|_| "PG_USERNAME is required")?,
            password: env::var("PG_PASSWORD")
                .map_err(|_| "PG_PASSWORD is required")?,
            publication: env::var("PG_PUBLICATION")
                .unwrap_or_else(|_| "pg_capture_pub".to_string()),
            slot_name: env::var("PG_SLOT_NAME")
                .unwrap_or_else(|_| "pg_capture_slot".to_string()),
            connect_timeout_secs: env::var("PG_CONNECT_TIMEOUT_SECS")
                .unwrap_or_else(|_| "30".to_string())
                .parse::<u64>()
                .unwrap_or(30),
            ssl_mode: env::var("PG_SSL_MODE")
                .unwrap_or_else(|_| "disable".to_string())
                .parse::<SslMode>()
                .map_err(|e| e)?,
        };

        // Kafka config
        let brokers = env::var("KAFKA_BROKERS")
            .map_err(|_| "KAFKA_BROKERS is required")?
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        
        if brokers.is_empty() {
            return Err("KAFKA_BROKERS must contain at least one broker".to_string());
        }

        let kafka = KafkaConfig {
            brokers,
            topic_prefix: env::var("KAFKA_TOPIC_PREFIX")
                .unwrap_or_else(|_| "cdc".to_string()),
            compression: env::var("KAFKA_COMPRESSION")
                .unwrap_or_else(|_| "snappy".to_string()),
            acks: env::var("KAFKA_ACKS")
                .unwrap_or_else(|_| "all".to_string()),
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
    
    pub fn postgres_url(&self) -> String {
        format!(
            "postgres://{}:{}@{}:{}/{}?replication=database",
            self.postgres.username,
            self.postgres.password,
            self.postgres.host,
            self.postgres.port,
            self.postgres.database
        )
    }
    
    pub fn kafka_topic_name(&self, table_name: &str) -> String {
        format!("{}.{}", self.kafka.topic_prefix, table_name)
    }
}

impl KafkaConfig {
    pub fn topic_name(&self, schema: &str, table: &str) -> String {
        format!("{}.{}.{}", self.topic_prefix, schema, table)
    }
}