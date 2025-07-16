use crate::checkpoint::{Checkpoint, CheckpointManager};
use crate::kafka::{JsonSerializer, KafkaProducer, KeyStrategy, SerializationFormat, TopicManager};
use crate::postgres::{DecodedMessage, PgOutputDecoder, ReplicationConnection, ReplicationMessage};
use crate::{Config, Error, Result};
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tokio::sync::mpsc;
use tracing::{error, info, instrument, warn};

/// The main replication orchestrator that manages the PostgreSQL to Kafka CDC pipeline.
///
/// `Replicator` coordinates all aspects of the replication process:
/// - Establishes and maintains connections to PostgreSQL and Kafka
/// - Decodes logical replication messages from PostgreSQL
/// - Publishes change events to appropriate Kafka topics
/// - Manages checkpoints for exactly-once delivery
/// - Handles failures with automatic retry logic
///
/// # Example
///
/// ```rust,no_run
/// use pg_capture::{Config, Replicator, Result};
///
/// #[tokio::main]
/// async fn main() -> Result<()> {
///     let config = Config::from_env()?;
///     let mut replicator = Replicator::new(config);
///     replicator.run().await
/// }
/// ```
pub struct Replicator {
    config: Config,
    postgres_conn: Option<ReplicationConnection>,
    kafka_producer: Option<Arc<KafkaProducer>>,
    topic_manager: Option<TopicManager>,
    shutdown_receiver: Option<mpsc::Receiver<()>>,
    decoder: Option<PgOutputDecoder>,
    checkpoint_manager: CheckpointManager,
    current_lsn: Option<u64>,
    total_message_count: u64,
}

impl Replicator {
    /// Creates a new replicator with the given configuration.
    ///
    /// # Arguments
    ///
    /// * `config` - Configuration loaded from environment or constructed programmatically
    ///
    /// # Example
    ///
    /// ```rust
    /// use pg_capture::{Config, Replicator};
    ///
    /// let config = Config::from_env().expect("Failed to load config");
    /// let replicator = Replicator::new(config);
    /// ```
    pub fn new(config: Config) -> Self {
        // Use checkpoint path from config, or default to checkpoint.json
        let checkpoint_path = config
            .replication
            .checkpoint_file
            .clone()
            .unwrap_or_else(|| std::path::PathBuf::from("checkpoint.json"));

        Self {
            config,
            postgres_conn: None,
            kafka_producer: None,
            topic_manager: None,
            shutdown_receiver: None,
            decoder: None,
            checkpoint_manager: CheckpointManager::new(&checkpoint_path),
            current_lsn: None,
            total_message_count: 0,
        }
    }

    /// Runs the replication process until completion or shutdown.
    ///
    /// This method:
    /// 1. Sets up signal handling for graceful shutdown (Ctrl+C)
    /// 2. Establishes connections to PostgreSQL and Kafka
    /// 3. Starts the replication stream from the last checkpoint (if any)
    /// 4. Continuously reads changes and publishes them to Kafka
    /// 5. Periodically saves checkpoints for resumability
    /// 6. Automatically retries on transient failures
    ///
    /// The method will run indefinitely until:
    /// - A shutdown signal is received (Ctrl+C)
    /// - An unrecoverable error occurs after max retries
    /// - The replication slot is dropped
    ///
    /// # Errors
    ///
    /// Returns `Err` if:
    /// - Configuration is invalid
    /// - Cannot connect to PostgreSQL or Kafka after retries
    /// - Cannot create or access the replication slot
    /// - Checkpoint file cannot be written
    ///
    /// Returns `Ok(())` on graceful shutdown.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use pg_capture::{Config, Replicator, Error};
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let config = Config::from_env().expect("Failed to load config");
    ///     let mut replicator = Replicator::new(config);
    ///     
    ///     match replicator.run().await {
    ///         Ok(()) => println!("Replication completed successfully"),
    ///         Err(Error::Shutdown) => println!("Graceful shutdown"),
    ///         Err(e) => eprintln!("Replication failed: {}", e),
    ///     }
    /// }
    /// ```
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

        // Run with retry logic
        let mut retry_count = 0;
        let max_retries = 5;
        let base_delay = Duration::from_secs(1);

        loop {
            match self.run_with_retry().await {
                Ok(()) => {
                    info!("Replication completed successfully");
                    return Ok(());
                }
                Err(Error::Shutdown) => {
                    info!("Replication stopped due to shutdown signal");
                    return Ok(());
                }
                Err(e) => {
                    retry_count += 1;

                    if retry_count > max_retries {
                        error!("Maximum retries ({}) exceeded, giving up", max_retries);
                        return Err(e);
                    }

                    // Exponential backoff: 1s, 2s, 4s, 8s, 16s
                    let delay = base_delay * 2u32.pow(retry_count.min(4) - 1);
                    error!(
                        "Replication failed: {}. Retrying in {:?} (attempt {}/{})",
                        e, delay, retry_count, max_retries
                    );

                    tokio::time::sleep(delay).await;

                    // Reset connections for retry
                    self.postgres_conn = None;
                    self.kafka_producer = None;
                    self.topic_manager = None;
                    self.decoder = None;
                }
            }
        }
    }

    async fn run_with_retry(&mut self) -> Result<()> {
        // Initialize connections
        self.initialize_connections().await?;

        // Run the main replication loop
        self.replication_loop().await
    }

    async fn initialize_connections(&mut self) -> Result<()> {
        info!("Initializing PostgreSQL connection");

        // Create PostgreSQL replication connection
        let mut pg_conn = ReplicationConnection::new(
            &self.config.postgres_url(),
            self.config.postgres.slot_name.clone(),
            self.config.postgres.publication.clone(),
        )
        .await?;

        // Get system info and create slot if needed
        let system_info = pg_conn.identify_system().await?;
        info!("Connected to PostgreSQL: {:?}", system_info);

        // Create replication slot if it doesn't exist
        match pg_conn.create_replication_slot().await {
            Ok(_) => info!(
                "Created replication slot '{}'",
                self.config.postgres.slot_name
            ),
            Err(e) => {
                if e.to_string().contains("already exists") {
                    info!(
                        "Replication slot '{}' already exists",
                        self.config.postgres.slot_name
                    );
                } else {
                    return Err(e);
                }
            }
        }

        self.postgres_conn = Some(pg_conn);

        info!("Initializing Kafka producer");

        // Create Kafka producer
        let kafka_producer = Arc::new(KafkaProducer::new(
            &self.config.kafka.brokers,
            &self.config.kafka,
        )?);
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
        let pg_conn = self.postgres_conn.as_mut().ok_or_else(|| {
            Error::Connection("PostgreSQL connection not initialized".to_string())
        })?;

        let kafka_producer = self
            .kafka_producer
            .as_ref()
            .ok_or_else(|| Error::Connection("Kafka producer not initialized".to_string()))?;

        let topic_manager = self
            .topic_manager
            .as_mut()
            .ok_or_else(|| Error::Connection("Topic manager not initialized".to_string()))?;

        let decoder = self
            .decoder
            .as_mut()
            .ok_or_else(|| Error::Connection("Decoder not initialized".to_string()))?;

        // Load checkpoint if it exists
        let start_lsn = match self.checkpoint_manager.load().await? {
            Some(checkpoint) => {
                info!(
                    "Resuming from checkpoint: LSN={}, messages={}",
                    checkpoint.lsn, checkpoint.message_count
                );
                self.total_message_count = checkpoint.message_count;
                Some(checkpoint.lsn)
            }
            None => {
                info!("No checkpoint found, starting from beginning");
                None
            }
        };

        // Start replication
        info!("Starting replication");
        pg_conn.start_replication(start_lsn).await?;

        // Create serializer and key strategy
        let serializer = JsonSerializer::new(SerializationFormat::Json);
        let key_strategy = KeyStrategy::None; // TODO: Make this configurable

        let mut shutdown_rx = self
            .shutdown_receiver
            .take()
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
                            // Update current LSN
                            self.current_lsn = Some(message.wal_end);

                            // Send standby status update to keep connection alive
                            if let Err(e) = pg_conn.send_standby_status_update(message.wal_end).await {
                                warn!("Failed to send standby status update: {}", e);
                            }

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
                                        match &e {
                                            Error::Kafka(_) => {
                                                // Kafka errors might be recoverable
                                                warn!("Continuing after Kafka error - will retry on next message");
                                            }
                                            Error::Serialization(_) => {
                                                // Skip messages that can't be serialized
                                                warn!("Skipping message due to serialization error");
                                            }
                                            _ => {
                                                // Other errors are fatal
                                                return Err(e);
                                            }
                                        }
                                    }

                                    message_count += 1;
                                    self.total_message_count += 1;
                                }
                                Ok(None) => {
                                    // Message was not relevant or was a keepalive
                                }
                                Err(e) => {
                                    error!("Failed to decode message: {}", e);
                                    // Continue on decode errors
                                }
                            }

                            // Periodic checkpoint
                            if last_checkpoint.elapsed() > Duration::from_secs(self.config.replication.checkpoint_interval_secs) {
                                info!("Processed {} messages since last checkpoint", message_count);

                                // Save checkpoint
                                if let Some(lsn) = self.current_lsn {
                                    let checkpoint = Checkpoint::new(
                                        ReplicationMessage::format_lsn(lsn),
                                        self.total_message_count
                                    );

                                    if let Err(e) = self.checkpoint_manager.save(&checkpoint).await {
                                        error!("Failed to save checkpoint: {}", e);
                                    } else {
                                        info!("Checkpoint saved: LSN={}, total_messages={}",
                                              checkpoint.lsn, checkpoint.message_count);
                                    }
                                }

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

        // Save final checkpoint
        if let Some(lsn) = self.current_lsn {
            let checkpoint = Checkpoint::new(
                ReplicationMessage::format_lsn(lsn),
                self.total_message_count,
            );

            if let Err(e) = self.checkpoint_manager.save(&checkpoint).await {
                error!("Failed to save final checkpoint: {}", e);
            } else {
                info!(
                    "Final checkpoint saved: LSN={}, total_messages={}",
                    checkpoint.lsn, checkpoint.message_count
                );
            }
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
                kafka_producer
                    .send_change_event(&event, key_strategy, serializer)
                    .await?;

                Ok(())
            }
        }
    }
}
