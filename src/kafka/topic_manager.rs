use crate::{Error, Result};
use rdkafka::admin::{AdminClient, AdminOptions, NewTopic, TopicReplication};
use rdkafka::client::DefaultClientContext;
use rdkafka::ClientConfig;
use std::collections::HashSet;
use std::time::Duration;
use tracing::{info, warn, debug, instrument};

pub struct TopicManager {
    admin_client: AdminClient<DefaultClientContext>,
    default_partitions: i32,
    default_replication_factor: i32,
    created_topics: HashSet<String>,
}

impl TopicManager {
    pub fn new(brokers: &[String], partitions: i32, replication_factor: i32) -> Result<Self> {
        let admin_client: AdminClient<_> = ClientConfig::new()
            .set("bootstrap.servers", brokers.join(","))
            .create()
            .map_err(|e| Error::Kafka(e))?;
        
        Ok(Self {
            admin_client,
            default_partitions: partitions,
            default_replication_factor: replication_factor,
            created_topics: HashSet::new(),
        })
    }
    
    #[instrument(skip(self), fields(topic = %topic_name))]
    pub async fn ensure_topic_exists(&mut self, topic_name: &str) -> Result<()> {
        if self.created_topics.contains(topic_name) {
            debug!("Topic '{}' already verified to exist", topic_name);
            return Ok(());
        }
        
        match self.topic_exists(topic_name).await {
            Ok(true) => {
                info!("Topic '{}' already exists", topic_name);
                self.created_topics.insert(topic_name.to_string());
                Ok(())
            }
            Ok(false) => {
                info!("Creating topic '{}'", topic_name);
                self.create_topic(topic_name).await?;
                self.created_topics.insert(topic_name.to_string());
                Ok(())
            }
            Err(e) => {
                warn!("Failed to check if topic '{}' exists: {}", topic_name, e);
                Err(e)
            }
        }
    }
    
    async fn topic_exists(&self, topic_name: &str) -> Result<bool> {
        let metadata = self.admin_client
            .inner()
            .fetch_metadata(Some(topic_name), Duration::from_secs(5))
            .map_err(|e| Error::Kafka(e))?;
        
        Ok(metadata.topics().iter().any(|topic| topic.name() == topic_name))
    }
    
    async fn create_topic(&self, topic_name: &str) -> Result<()> {
        let new_topic = NewTopic::new(
            topic_name,
            self.default_partitions,
            TopicReplication::Fixed(self.default_replication_factor),
        )
        .set("cleanup.policy", "delete")
        .set("retention.ms", "604800000") // 7 days
        .set("compression.type", "snappy");
        
        let opts = AdminOptions::new()
            .operation_timeout(Some(Duration::from_secs(30)));
        
        let results = self.admin_client
            .create_topics(&[new_topic], &opts)
            .await
            .map_err(|e| Error::Kafka(e))?;
        
        for result in results {
            match result {
                Ok(topic) => {
                    info!("Successfully created topic: {}", topic);
                }
                Err((_topic, error)) => {
                    return Err(Error::Kafka(rdkafka::error::KafkaError::AdminOp(error)));
                }
            }
        }
        
        Ok(())
    }
    
    pub async fn delete_topic(&self, topic_name: &str) -> Result<()> {
        let opts = AdminOptions::new()
            .operation_timeout(Some(Duration::from_secs(30)));
        
        let results = self.admin_client
            .delete_topics(&[topic_name], &opts)
            .await
            .map_err(|e| Error::Kafka(e))?;
        
        for result in results {
            match result {
                Ok(topic) => {
                    info!("Successfully deleted topic: {}", topic);
                }
                Err((_topic, error)) => {
                    return Err(Error::Kafka(rdkafka::error::KafkaError::AdminOp(error)));
                }
            }
        }
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    #[ignore] // Requires running Kafka
    async fn test_topic_creation() {
        let manager = TopicManager::new(
            &["localhost:9092".to_string()],
            3,
            1,
        ).unwrap();
        
        let topic_name = "test-topic-creation";
        
        // Clean up if exists
        let _ = manager.delete_topic(topic_name).await;
        
        // Create topic
        let mut manager = manager;
        manager.ensure_topic_exists(topic_name).await.unwrap();
        
        // Verify it exists
        assert!(manager.topic_exists(topic_name).await.unwrap());
        
        // Clean up
        manager.delete_topic(topic_name).await.unwrap();
    }
}