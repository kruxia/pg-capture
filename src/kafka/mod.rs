pub mod producer;
pub mod serializer;
pub mod key_strategy;
pub mod topic_manager;

#[cfg(test)]
mod tests;

pub use producer::KafkaProducer;
pub use serializer::{JsonSerializer, SerializationFormat, EventSerializer};
pub use key_strategy::KeyStrategy;
pub use topic_manager::TopicManager;