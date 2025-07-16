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
