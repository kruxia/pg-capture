use serde::{Deserialize, Serialize};
use std::path::Path;

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
    #[serde(default = "default_connect_timeout")]
    pub connect_timeout_secs: u64,
    #[serde(default)]
    pub ssl_mode: SslMode,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SslMode {
    #[default]
    Disable,
    Prefer,
    Require,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct KafkaConfig {
    pub brokers: Vec<String>,
    pub topic_prefix: String,
    #[serde(default = "default_compression")]
    pub compression: String,
    #[serde(default = "default_acks")]
    pub acks: String,
    #[serde(default = "default_linger_ms")]
    pub linger_ms: u32,
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    #[serde(default = "default_buffer_memory")]
    pub buffer_memory: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReplicationConfig {
    #[serde(default = "default_poll_interval_ms")]
    pub poll_interval_ms: u64,
    #[serde(default = "default_keepalive_interval_secs")]
    pub keepalive_interval_secs: u64,
    #[serde(default = "default_checkpoint_interval_secs")]
    pub checkpoint_interval_secs: u64,
    #[serde(default = "default_max_buffer_size")]
    pub max_buffer_size: usize,
}

impl Config {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, config::ConfigError> {
        let settings = config::Config::builder()
            .add_source(config::File::from(path.as_ref()))
            .add_source(
                config::Environment::with_prefix("PG_REPLICATE")
                    .prefix_separator("_")
                    .separator("__"),
            )
            .build()?;
        
        settings.try_deserialize()
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

fn default_connect_timeout() -> u64 {
    30
}

fn default_compression() -> String {
    "snappy".to_string()
}

fn default_acks() -> String {
    "all".to_string()
}

fn default_linger_ms() -> u32 {
    100
}

fn default_batch_size() -> usize {
    16384
}

fn default_buffer_memory() -> usize {
    33_554_432 // 32MB
}

fn default_poll_interval_ms() -> u64 {
    100
}

fn default_keepalive_interval_secs() -> u64 {
    10
}

fn default_checkpoint_interval_secs() -> u64 {
    10
}

fn default_max_buffer_size() -> usize {
    1000
}