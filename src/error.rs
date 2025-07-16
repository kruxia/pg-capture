use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Configuration error: {0}")]
    Config(#[from] config::ConfigError),
    
    #[error("PostgreSQL error: {0}")]
    Postgres(#[from] tokio_postgres::Error),
    
    #[error("Kafka error: {0}")]
    Kafka(#[from] rdkafka::error::KafkaError),
    
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Connection error: {message}")]
    Connection { message: String },
    
    #[error("Replication error: {message}")]
    Replication { message: String },
    
    #[error("Invalid message format: {message}")]
    InvalidMessage { message: String },
    
    #[error("Timeout error: {message}")]
    Timeout { message: String },
    
    #[error("Shutdown requested")]
    Shutdown,
}

pub type Result<T> = std::result::Result<T, Error>;