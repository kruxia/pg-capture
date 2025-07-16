use crate::{Error, Result, config::KafkaConfig};
use rdkafka::producer::{FutureProducer, FutureRecord};
use rdkafka::ClientConfig;

pub struct KafkaProducer {
    producer: FutureProducer,
}

impl KafkaProducer {
    pub fn new(brokers: &[String], config: &KafkaConfig) -> Result<Self> {
        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", brokers.join(","))
            .set("compression.type", &config.compression)
            .set("acks", &config.acks)
            .set("linger.ms", config.linger_ms.to_string())
            .set("batch.size", config.batch_size.to_string())
            .set("buffer.memory", config.buffer_memory.to_string())
            .create()
            .map_err(|e| Error::Kafka(e))?;
        
        Ok(Self { producer })
    }
    
    pub async fn send(&self, topic: &str, key: Option<&str>, payload: &str) -> Result<()> {
        let record = FutureRecord::to(topic)
            .payload(payload)
            .key(key.unwrap_or(""));
        
        self.producer
            .send(record, rdkafka::util::Timeout::Never)
            .await
            .map_err(|(e, _)| Error::Kafka(e))?;
        
        Ok(())
    }
}