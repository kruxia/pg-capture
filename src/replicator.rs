use crate::{Config, Result, Error};
use crate::postgres::{ReplicationConnection, DecodedMessage, PgOutputDecoder};
use crate::kafka::{KafkaProducer, JsonSerializer, KeyStrategy, SerializationFormat, TopicManager};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::signal;
use tracing::{info, warn, error, instrument};

pub struct Replicator {
    config: Config,
    postgres_conn: Option<ReplicationConnection>,
    kafka_producer: Option<Arc<KafkaProducer>>,
    topic_manager: Option<TopicManager>,
    shutdown_receiver: Option<mpsc::Receiver<()>>,
    decoder: Option<PgOutputDecoder>,
}

impl Replicator {
    pub fn new(config: Config) -> Self {
        Self { 
            config,
            postgres_conn: None,
            kafka_producer: None,
            topic_manager: None,
            shutdown_receiver: None,
            decoder: None,
        }
    }
    
    #[instrument(skip(self))]
    pub async fn run(&mut self) -> Result<()> {
        info!("Replicator starting");
        
        // Set up shutdown handling
        let (shutdown_tx, shutdown_rx) = mpsc::channel(1);
        self.shutdown_receiver = Some(shutdown_rx);
        
        // Spawn task to handle shutdown signals
        tokio::spawn(async move {
            let _ = signal::ctrl_c().await;
            info!("Shutdown signal received");
            let _ = shutdown_tx.send(()).await;
        });
        
        // Initialize connections
        self.initialize_connections().await?;
        
        // Run the main replication loop
        match self.replication_loop().await {
            Ok(()) => {
                info!("Replication completed successfully");
                Ok(())
            }
            Err(Error::Shutdown) => {
                info!("Replication stopped due to shutdown signal");
                Ok(())
            }
            Err(e) => {
                error!("Replication failed: {}", e);
                Err(e)
            }
        }
    }
    
    async fn initialize_connections(&mut self) -> Result<()> {
        info!("Initializing PostgreSQL connection");
        
        // Create PostgreSQL replication connection
        let mut pg_conn = ReplicationConnection::new(
            &self.config.postgres_url(),
            self.config.postgres.slot_name.clone(),
            self.config.postgres.publication.clone(),
        ).await?;
        
        // Get system info and create slot if needed
        let system_info = pg_conn.identify_system().await?;
        info!("Connected to PostgreSQL: {:?}", system_info);
        
        // Create replication slot if it doesn't exist
        match pg_conn.create_replication_slot().await {
            Ok(_) => info!("Created replication slot '{}'", self.config.postgres.slot_name),
            Err(e) => {
                if e.to_string().contains("already exists") {
                    info!("Replication slot '{}' already exists", self.config.postgres.slot_name);
                } else {
                    return Err(e);
                }
            }
        }
        
        self.postgres_conn = Some(pg_conn);
        
        info!("Initializing Kafka producer");
        
        // Create Kafka producer
        let kafka_producer = Arc::new(
            KafkaProducer::new(&self.config.kafka.brokers, &self.config.kafka)?
        );
        self.kafka_producer = Some(kafka_producer);
        
        // Create topic manager
        let topic_manager = TopicManager::new(
            &self.config.kafka.brokers,
            1, // Default partitions
            1, // Default replication factor
        )?;
        self.topic_manager = Some(topic_manager);
        
        // Create decoder
        let decoder = PgOutputDecoder::new(self.config.postgres.database.clone());
        self.decoder = Some(decoder);
        
        info!("Connections initialized successfully");
        Ok(())
    }
    
    #[instrument(skip(self))]
    async fn replication_loop(&mut self) -> Result<()> {
        let pg_conn = self.postgres_conn.as_mut()
            .ok_or_else(|| Error::Connection("PostgreSQL connection not initialized".to_string()))?;
        
        let kafka_producer = self.kafka_producer.as_ref()
            .ok_or_else(|| Error::Connection("Kafka producer not initialized".to_string()))?;
        
        let topic_manager = self.topic_manager.as_mut()
            .ok_or_else(|| Error::Connection("Topic manager not initialized".to_string()))?;
        
        let decoder = self.decoder.as_mut()
            .ok_or_else(|| Error::Connection("Decoder not initialized".to_string()))?;
        
        // Start replication
        info!("Starting replication");
        pg_conn.start_replication(None).await?;
        
        // Create serializer and key strategy
        let serializer = JsonSerializer::new(SerializationFormat::Json);
        let key_strategy = KeyStrategy::None; // TODO: Make this configurable
        
        let mut shutdown_rx = self.shutdown_receiver.take()
            .ok_or_else(|| Error::Connection("Shutdown receiver not initialized".to_string()))?;
        
        let mut message_count = 0;
        let mut last_checkpoint = std::time::Instant::now();
        
        loop {
            tokio::select! {
                // Check for shutdown signal
                _ = shutdown_rx.recv() => {
                    info!("Received shutdown signal");
                    break;
                }
                
                // Receive replication messages with timeout
                result = tokio::time::timeout(
                    Duration::from_millis(self.config.replication.poll_interval_ms),
                    pg_conn.recv_replication_message()
                ) => {
                    match result {
                        Ok(Ok(Some(message))) => {
                            // Decode the replication message
                            match decoder.decode(&message.data) {
                                Ok(Some(decoded_msg)) => {
                                    if let Err(e) = Self::process_message(
                                        &self.config,
                                        decoded_msg,
                                        kafka_producer,
                                        topic_manager,
                                        &serializer,
                                        &key_strategy
                                    ).await {
                                        error!("Failed to process message: {}", e);
                                        // Decide whether to continue or fail based on error type
                                        if matches!(e, Error::Kafka(_)) {
                                            // Kafka errors might be recoverable
                                            warn!("Continuing after Kafka error");
                                        } else {
                                            return Err(e);
                                        }
                                    }
                                }
                                Ok(None) => {
                                    // Message was not relevant or was a keepalive
                                }
                                Err(e) => {
                                    error!("Failed to decode message: {}", e);
                                    // Continue on decode errors
                                }
                            }
                            
                            message_count += 1;
                            
                            // Periodic checkpoint
                            if last_checkpoint.elapsed() > Duration::from_secs(self.config.replication.checkpoint_interval_secs) {
                                info!("Processed {} messages since last checkpoint", message_count);
                                message_count = 0;
                                last_checkpoint = std::time::Instant::now();
                                
                                // Flush Kafka producer
                                if let Err(e) = kafka_producer.flush(Duration::from_secs(5)).await {
                                    warn!("Failed to flush Kafka producer: {}", e);
                                }
                            }
                        }
                        Ok(Ok(None)) => {
                            // No message available, continue
                        }
                        Ok(Err(e)) => {
                            error!("Error receiving message: {}", e);
                            return Err(e);
                        }
                        Err(_) => {
                            // Timeout is normal, continue
                        }
                    }
                }
            }
        }
        
        // Graceful shutdown
        info!("Shutting down replicator");
        
        // Flush any pending Kafka messages
        if let Err(e) = kafka_producer.flush(Duration::from_secs(10)).await {
            warn!("Failed to flush Kafka producer during shutdown: {}", e);
        }
        
        Err(Error::Shutdown)
    }
    
    async fn process_message(
        config: &Config,
        message: DecodedMessage,
        kafka_producer: &Arc<KafkaProducer>,
        topic_manager: &mut TopicManager,
        serializer: &JsonSerializer,
        key_strategy: &KeyStrategy,
    ) -> Result<()> {
        match message {
            DecodedMessage::Begin { xid } => {
                info!("Transaction begin: xid={}", xid);
                Ok(())
            }
            DecodedMessage::Commit => {
                info!("Transaction commit");
                Ok(())
            }
            DecodedMessage::Change(event) => {
                // Ensure topic exists
                let topic = config.kafka.topic_name(&event.schema, &event.table);
                topic_manager.ensure_topic_exists(&topic).await?;
                
                // Send to Kafka
                kafka_producer.send_change_event(&event, key_strategy, serializer).await?;
                
                Ok(())
            }
        }
    }
}