//! Kafka producer implementation for publishing CDC events.
//!
//! This module provides all the functionality needed to publish PostgreSQL
//! change events to Apache Kafka topics with configurable serialization,
//! partitioning strategies, and delivery guarantees.
//!
//! # Architecture
//!
//! The module is organized into four main components:
//!
//! - [`producer`] - High-level Kafka producer with CDC-specific features
//! - [`serializer`] - Message serialization (currently JSON, with Avro planned)
//! - [`key_strategy`] - Partitioning strategies for Kafka message keys
//! - [`topic_manager`] - Automatic topic creation and management
//!
//! # Example
//!
//! ```rust,no_run
//! use pg_capture::kafka::{KafkaProducer, JsonSerializer, KeyStrategy, SerializationFormat};
//! use pg_capture::postgres::ChangeEvent;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create producer
//!     let producer = KafkaProducer::new(
//!         &["localhost:9092".to_string()],
//!         &Default::default(),
//!     )?;
//!
//!     // Create serializer
//!     let serializer = JsonSerializer::new(SerializationFormat::Json);
//!
//!     // Send a change event
//!     let event = ChangeEvent {
//!         // ... populate event fields
//! #       schema: String::new(),
//! #       table: String::new(),
//! #       op: pg_capture::postgres::ChangeOperation::Insert,
//! #       ts_ms: 0,
//! #       before: None,
//! #       after: None,
//! #       source: pg_capture::postgres::SourceMetadata::new(
//! #           String::new(), String::new(), String::new(), String::new()
//! #       ),
//!     };
//!
//!     producer.send_change_event(
//!         &event,
//!         &KeyStrategy::None,
//!         &serializer,
//!     ).await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! # Features
//!
//! - **Automatic retries** with exponential backoff
//! - **Batching** for improved throughput
//! - **Compression** support (snappy, gzip, lz4, zstd)
//! - **Configurable delivery guarantees** (at-least-once or at-most-once)
//! - **Automatic topic creation** with configurable partitions

pub mod key_strategy;
pub mod producer;
pub mod serializer;
pub mod topic_manager;

#[cfg(test)]
mod tests;

pub use key_strategy::KeyStrategy;
pub use producer::KafkaProducer;
pub use serializer::{EventSerializer, JsonSerializer, SerializationFormat};
pub use topic_manager::TopicManager;
