//! PostgreSQL logical replication client implementation.
//!
//! This module provides all the functionality needed to connect to PostgreSQL
//! as a logical replication client, decode the replication stream, and handle
//! the logical replication protocol.
//!
//! # Architecture
//!
//! The module is organized into three main components:
//!
//! - [`connection`] - Manages the replication connection and protocol
//! - [`decoder`] - Decodes pgoutput format messages into structured data
//! - [`types`] - Common types for change events and operations
//!
//! # Example
//!
//! ```rust,no_run
//! use pg_capture::postgres::{ReplicationConnection, PgOutputDecoder};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create replication connection
//!     let mut conn = ReplicationConnection::new(
//!         "postgres://replicator:secret@localhost/mydb?replication=database",
//!         "my_slot".to_string(),
//!         "my_publication".to_string(),
//!     ).await?;
//!
//!     // Create decoder
//!     let mut decoder = PgOutputDecoder::new("mydb".to_string());
//!
//!     // Start replication
//!     conn.start_replication(None).await?;
//!
//!     // Read and decode messages
//!     while let Some(msg) = conn.recv_replication_message().await? {
//!         if let Some(decoded) = decoder.decode(&msg.data)? {
//!             println!("Received change: {:?}", decoded);
//!         }
//!     }
//!
//!     Ok(())
//! }
//! ```

pub mod connection;
pub mod decoder;
pub mod types;

#[cfg(test)]
mod decoder_tests;

#[cfg(test)]
pub mod test_utils;

#[cfg(test)]
mod type_parser_tests;

pub use connection::{ReplicationConnection, ReplicationMessage, SystemInfo};
pub use decoder::{ColumnInfo, DecodedMessage, PgOutputDecoder, RelationInfo};
pub use types::*;
