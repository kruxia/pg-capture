use pg_capture::config::{Config, KafkaConfig, PostgresConfig, ReplicationConfig, SslMode};
use std::env;

/// Get test configuration from environment variables
pub fn get_test_config() -> Config {
    // Use TEST_ prefix for test environment variables
    let postgres = PostgresConfig {
        host: env::var("TEST_PG_HOST").unwrap_or_else(|_| "localhost".to_string()),
        port: env::var("TEST_PG_PORT")
            .unwrap_or_else(|_| "5432".to_string())
            .parse()
            .unwrap_or(5432),
        database: env::var("TEST_PG_DATABASE").unwrap_or_else(|_| "postgres".to_string()),
        username: env::var("TEST_PG_USERNAME").unwrap_or_else(|_| "postgres".to_string()),
        password: env::var("TEST_PG_PASSWORD").unwrap_or_else(|_| "postgres".to_string()),
        publication: format!("test_publication_{}", std::process::id()),
        slot_name: format!("test_slot_{}", std::process::id()),
        connect_timeout_secs: 30,
        ssl_mode: SslMode::Disable,
    };

    let kafka = KafkaConfig {
        brokers: env::var("TEST_KAFKA_BROKERS")
            .unwrap_or_else(|_| "localhost:9092".to_string())
            .split(',')
            .map(|s| s.trim().to_string())
            .collect(),
        topic_prefix: format!("test_{}", std::process::id()),
        compression: "none".to_string(), // No compression for tests
        acks: "all".to_string(),
        linger_ms: 0, // Immediate sending for tests
        batch_size: 1, // Small batches for tests
        buffer_memory: 1_048_576, // 1MB for tests
    };

    let replication = ReplicationConfig {
        poll_interval_ms: 100,
        keepalive_interval_secs: 10,
        checkpoint_interval_secs: 2, // Frequent checkpoints for tests
        checkpoint_file: None, // Will be set per test if needed
        max_buffer_size: 100,
    };

    Config {
        postgres,
        kafka,
        replication,
    }
}