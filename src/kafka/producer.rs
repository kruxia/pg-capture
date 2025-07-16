use crate::{Error, Result, config::KafkaConfig, postgres::ChangeEvent};
use rdkafka::producer::{FutureProducer, FutureRecord, Producer};
use rdkafka::ClientConfig;
use std::time::Duration;
use tracing::{info, warn, error, instrument};
use tokio::time::timeout;
use super::{KeyStrategy, EventSerializer};

pub struct KafkaProducer {
    producer: FutureProducer,
    config: KafkaConfig,
    delivery_timeout: Duration,
}

impl KafkaProducer {
    #[instrument(skip_all, fields(brokers = ?brokers))]
    pub fn new(brokers: &[String], config: &KafkaConfig) -> Result<Self> {
        info!("Creating Kafka producer with brokers: {:?}", brokers);
        
        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", brokers.join(","))
            .set("compression.type", &config.compression)
            .set("acks", &config.acks)
            .set("linger.ms", config.linger_ms.to_string())
            .set("batch.size", config.batch_size.to_string())
            .set("buffer.memory", config.buffer_memory.to_string())
            .set("message.timeout.ms", "30000")
            .set("request.timeout.ms", "20000")
            .set("retries", "3")
            .set("retry.backoff.ms", "100")
            .set("enable.idempotence", "true")
            .create()
            .map_err(|e| {
                error!("Failed to create Kafka producer: {}", e);
                Error::Kafka(e)
            })?;
        
        info!("Kafka producer created successfully");
        
        Ok(Self { 
            producer,
            config: config.clone(),
            delivery_timeout: Duration::from_secs(30),
        })
    }
    
    #[instrument(skip(self, payload), fields(topic = %topic, key = ?key, payload_len = payload.len()))]
    pub async fn send(&self, topic: &str, key: Option<&str>, payload: &str) -> Result<()> {
        let mut record = FutureRecord::to(topic)
            .payload(payload);
        
        if let Some(k) = key {
            record = record.key(k);
        }
        
        let start = std::time::Instant::now();
        
        match timeout(
            self.delivery_timeout,
            self.producer.send(record, rdkafka::util::Timeout::Never)
        ).await {
            Ok(Ok((partition, offset))) => {
                let duration = start.elapsed();
                info!(
                    "Message delivered to topic '{}' partition {} offset {} in {:?}",
                    topic, partition, offset, duration
                );
                Ok(())
            }
            Ok(Err((e, _msg))) => {
                error!(
                    "Failed to deliver message to topic '{}': {}",
                    topic, e
                );
                Err(Error::Kafka(e))
            }
            Err(_) => {
                error!(
                    "Message delivery to topic '{}' timed out after {:?}",
                    topic, self.delivery_timeout
                );
                Err(Error::Kafka(rdkafka::error::KafkaError::MessageProduction(
                    rdkafka::types::RDKafkaErrorCode::MessageTimedOut
                )))
            }
        }
    }
    
    #[instrument(skip(self, payloads), fields(topic = %topic, batch_size = payloads.len()))]
    pub async fn send_batch(&self, topic: &str, payloads: Vec<(Option<String>, String)>) -> Result<Vec<std::result::Result<(i32, i64), Error>>> {
        info!("Sending batch of {} messages to topic '{}'", payloads.len(), topic);
        
        let mut results = Vec::with_capacity(payloads.len());
        let start = std::time::Instant::now();
        
        for (key, payload) in payloads {
            let mut record = FutureRecord::to(topic)
                .payload(&payload);
            
            if let Some(k) = &key {
                record = record.key(k);
            }
            
            match self.producer.send_result(record) {
                Ok(delivery_future) => {
                    results.push(delivery_future);
                }
                Err((e, _)) => {
                    warn!("Failed to queue message for batch delivery: {}", e);
                    return Err(Error::Kafka(e));
                }
            }
        }
        
        let mut delivery_results = Vec::with_capacity(results.len());
        let mut success_count = 0;
        let mut failure_count = 0;
        
        for (idx, future) in results.into_iter().enumerate() {
            match timeout(self.delivery_timeout, future).await {
                Ok(Ok(result)) => {
                    match result {
                        Ok((partition, offset)) => {
                            success_count += 1;
                            delivery_results.push(Ok((partition, offset)));
                        }
                        Err((e, _)) => {
                            failure_count += 1;
                            warn!("Failed to deliver message {} in batch: {}", idx, e);
                            delivery_results.push(Err(Error::Kafka(e)));
                        }
                    }
                }
                Ok(Err(_)) => {
                    failure_count += 1;
                    warn!("Message {} in batch was cancelled", idx);
                    delivery_results.push(Err(Error::Kafka(rdkafka::error::KafkaError::MessageProduction(
                        rdkafka::types::RDKafkaErrorCode::MessageTimedOut
                    ))));
                }
                Err(_) => {
                    failure_count += 1;
                    warn!("Message {} in batch timed out", idx);
                    delivery_results.push(Err(Error::Kafka(rdkafka::error::KafkaError::MessageProduction(
                        rdkafka::types::RDKafkaErrorCode::MessageTimedOut
                    ))));
                }
            }
        }
        
        let duration = start.elapsed();
        info!(
            "Batch delivery completed in {:?}: {} succeeded, {} failed",
            duration, success_count, failure_count
        );
        
        Ok(delivery_results)
    }
    
    pub async fn flush(&self, timeout: Duration) -> Result<()> {
        info!("Flushing Kafka producer queue");
        self.producer.flush(timeout).map_err(|e| {
            error!("Failed to flush producer: {}", e);
            Error::Kafka(e)
        })
    }
    
    #[instrument(skip(self, event, serializer), fields(table = %event.table, op = ?event.op))]
    pub async fn send_change_event(
        &self,
        event: &ChangeEvent,
        key_strategy: &KeyStrategy,
        serializer: &impl EventSerializer,
    ) -> Result<()> {
        let topic = self.config.topic_name(&event.schema, &event.table);
        let key = key_strategy.extract_key(event);
        let payload = serializer.serialize(event)?;
        
        info!(
            "Sending change event for {}.{} with key: {:?}",
            event.schema, event.table, key
        );
        
        self.send(&topic, key.as_deref(), &payload).await
    }
    
    #[instrument(skip(self, events, serializer), fields(count = events.len()))]
    pub async fn send_change_events_batch(
        &self,
        events: &[ChangeEvent],
        key_strategy: &KeyStrategy,
        serializer: &impl EventSerializer,
    ) -> Result<Vec<std::result::Result<(i32, i64), Error>>> {
        if events.is_empty() {
            return Ok(vec![]);
        }
        
        let table = &events[0].table;
        let schema = &events[0].schema;
        let topic = self.config.topic_name(schema, table);
        
        let mut payloads = Vec::with_capacity(events.len());
        
        for event in events {
            let key = key_strategy.extract_key(event);
            let payload = serializer.serialize(event)?;
            payloads.push((key, payload));
        }
        
        info!(
            "Sending batch of {} events for {}.{}",
            events.len(), schema, table
        );
        
        self.send_batch(&topic, payloads).await
    }
}