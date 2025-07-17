//! Error types and result handling for pg-capture.
//!
//! This module defines the main error type [`Error`] and a convenience
//! [`Result`] type alias used throughout the crate.
//!
//! # Example
//!
//! ```rust
//! use pg_capture::{Error, Result};
//!
//! fn connect_to_database() -> Result<()> {
//!     // Simulating a connection error
//!     Err(Error::Connection("Failed to connect".to_string()))
//! }
//!
//! match connect_to_database() {
//!     Ok(()) => println!("Connected"),
//!     Err(Error::Connection(msg)) => eprintln!("Connection error: {}", msg),
//!     Err(e) => eprintln!("Other error: {}", e),
//! }
//! ```

use thiserror::Error;

/// The main error type for pg-capture operations.
///
/// This enum represents all possible errors that can occur during
/// replication, from configuration issues to runtime failures.
#[derive(Error, Debug)]
pub enum Error {
    /// Configuration error, typically from invalid environment variables.
    #[error("Configuration error: {0}")]
    Config(String),

    /// PostgreSQL client or protocol error.
    #[error("PostgreSQL error: {0}")]
    Postgres(#[from] tokio_postgres::Error),

    /// Kafka client or producer error.
    #[error("Kafka error: {0}")]
    Kafka(#[from] rdkafka::error::KafkaError),

    /// JSON serialization error when encoding messages.
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// I/O error, typically from checkpoint file operations.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Generic connection error not covered by specific types.
    #[error("Connection error: {0}")]
    Connection(String),

    /// Authentication failure with PostgreSQL or Kafka.
    #[error("Authentication error: {0}")]
    Authentication(String),

    /// Protocol-level error in replication stream.
    #[error("Protocol error: {0}")]
    Protocol(String),

    /// Replication-specific error.
    #[error("Replication error: {message}")]
    Replication {
        /// Description of the replication error
        message: String,
    },

    /// Invalid or malformed replication message.
    #[error("Invalid message format: {message}")]
    InvalidMessage {
        /// Description of what was invalid
        message: String,
    },

    /// Operation timeout.
    #[error("Timeout error: {message}")]
    Timeout {
        /// Description of what timed out
        message: String,
    },

    /// Graceful shutdown was requested (e.g., via Ctrl+C).
    ///
    /// This is not really an error but uses the error mechanism
    /// to cleanly exit the replication loop.
    #[error("Shutdown requested")]
    Shutdown,
}

/// A convenient Result type alias for pg-capture operations.
///
/// This is equivalent to `std::result::Result<T, pg_capture::Error>`.
///
/// # Example
///
/// ```rust
/// use pg_capture::Result;
///
/// fn do_something() -> Result<String> {
///     Ok("Success".to_string())
/// }
/// ```
pub type Result<T> = std::result::Result<T, Error>;
