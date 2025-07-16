use pg_capture::config::{Config, KafkaConfig, PostgresConfig, ReplicationConfig, SslMode};
use pg_capture::replicator::Replicator;
use rdkafka::config::ClientConfig;
use rdkafka::consumer::{Consumer, StreamConsumer};
use rdkafka::Message;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use tokio::time::timeout;
use tokio_postgres::{Client, NoTls};
use tracing::info;

#[tokio::test]
#[ignore] // Run with: cargo test --ignored exactly_once_test::test_no_duplicate_messages
async fn test_no_duplicate_messages() {
    tracing_subscriber::fmt()
        .with_env_filter("pg_capture=debug")
        .try_init()
        .ok();

    let (client, mut test_config) = setup_exactly_once_test().await;

    // Configure for exactly-once semantics
    test_config.kafka.acks = "all".to_string();
    test_config.replication.checkpoint_interval_secs = 1;
    let checkpoint_file = format!("/tmp/pg_capture_eo_test_{}.checkpoint", std::process::id());
    test_config.replication.checkpoint_file = Some(PathBuf::from(checkpoint_file.clone()));

    // Start replicator
    let mut replicator = Replicator::new(test_config.clone());
    let mut handle = tokio::spawn(async move { replicator.run().await });

    // Wait for startup
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Insert test data with unique IDs
    let num_messages = 100;
    for i in 0..num_messages {
        client.execute(
            &format!("INSERT INTO exactly_once_test (unique_id, data) VALUES ('msg_{}', 'test_data_{}')", i, i),
            &[]
        ).await.unwrap();

        // Simulate replicator crash and restart at various points
        if i == 25 || i == 50 || i == 75 {
            info!("Simulating crash at message {}", i);
            handle.abort();
            tokio::time::sleep(Duration::from_millis(500)).await;

            // Restart replicator
            let mut replicator = Replicator::new(test_config.clone());
            let new_handle = tokio::spawn(async move { replicator.run().await });

            // Replace handle for next iteration
            let _ = std::mem::replace(&mut handle, new_handle);
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    // Wait for all messages to be processed
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Consume all messages and check for duplicates
    let consumer = create_exactly_once_consumer(&test_config.kafka).await;
    let mut message_ids = HashMap::new();
    let mut duplicate_count = 0;

    let timeout_duration = Duration::from_secs(10);
    let start = tokio::time::Instant::now();

    while start.elapsed() < timeout_duration {
        if let Ok(Ok(message)) = timeout(Duration::from_millis(500), consumer.recv()).await {
            if let Some(payload) = message.payload() {
                let json: Value = serde_json::from_slice(payload).unwrap();
                if json["op"] == "INSERT" && json["table"] == "exactly_once_test" {
                    if let Some(unique_id) = json["after"]["unique_id"].as_str() {
                        let count = message_ids.entry(unique_id.to_string()).or_insert(0);
                        *count += 1;
                        if *count > 1 {
                            duplicate_count += 1;
                            info!("Duplicate detected: {} (count: {})", unique_id, count);
                        }
                    }
                }
            }
        }
    }

    // Verify results
    assert_eq!(
        message_ids.len(),
        num_messages,
        "Expected {} unique messages, got {}",
        num_messages,
        message_ids.len()
    );
    assert_eq!(
        duplicate_count, 0,
        "Found {} duplicate messages",
        duplicate_count
    );

    // Verify no messages were lost
    for i in 0..num_messages {
        let msg_id = format!("msg_{}", i);
        assert!(
            message_ids.contains_key(&msg_id),
            "Missing message: {}",
            msg_id
        );
        assert_eq!(
            *message_ids.get(&msg_id).unwrap(),
            1,
            "Message {} was delivered {} times",
            msg_id,
            message_ids.get(&msg_id).unwrap()
        );
    }

    // Cleanup
    handle.abort();
    cleanup_exactly_once_test(&client).await;
    tokio::fs::remove_file(&checkpoint_file).await.ok();
}

#[tokio::test]
#[ignore] // Run with: cargo test --ignored exactly_once_test::test_checkpoint_consistency
async fn test_checkpoint_consistency() {
    tracing_subscriber::fmt()
        .with_env_filter("pg_capture=debug")
        .try_init()
        .ok();

    let (client, mut test_config) = setup_exactly_once_test().await;

    // Configure with checkpoint
    let checkpoint_file = format!(
        "/tmp/pg_capture_consistency_test_{}.checkpoint",
        std::process::id()
    );
    test_config.replication.checkpoint_file = Some(PathBuf::from(checkpoint_file.clone()));
    test_config.replication.checkpoint_interval_secs = 1;

    // Track LSNs for verification
    let mut _lsn_before_crash = None;

    // First run: process some messages and crash
    {
        let mut replicator = Replicator::new(test_config.clone());
        let handle = tokio::spawn(async move { replicator.run().await });

        tokio::time::sleep(Duration::from_secs(2)).await;

        // Insert batch 1
        for i in 0..50 {
            client.execute(
                &format!("INSERT INTO exactly_once_test (unique_id, data) VALUES ('batch1_{}', 'data_{}')", i, i),
                &[]
            ).await.unwrap();
        }

        // Wait for checkpoint
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Get current LSN before crash
        let row = client
            .query_one("SELECT pg_current_wal_lsn()", &[])
            .await
            .unwrap();
        let lsn: String = row.get(0);
        _lsn_before_crash = Some(lsn.clone());
        info!("LSN before crash: {}", lsn);

        // Crash
        handle.abort();
    }

    // Insert more data while replicator is down
    for i in 0..50 {
        client.execute(
            &format!("INSERT INTO exactly_once_test (unique_id, data) VALUES ('batch2_{}', 'data_{}')", i, i),
            &[]
        ).await.unwrap();
    }

    // Second run: should resume from checkpoint
    {
        let mut replicator = Replicator::new(test_config.clone());
        let _handle = tokio::spawn(async move { replicator.run().await });

        tokio::time::sleep(Duration::from_secs(5)).await;
    }

    // Verify all messages were delivered exactly once
    let consumer = create_exactly_once_consumer(&test_config.kafka).await;
    let mut batch1_count = 0;
    let mut batch2_count = 0;
    let mut message_map = HashMap::new();

    let timeout_duration = Duration::from_secs(10);
    let start = tokio::time::Instant::now();

    while start.elapsed() < timeout_duration {
        if let Ok(Ok(message)) = timeout(Duration::from_millis(500), consumer.recv()).await {
            if let Some(payload) = message.payload() {
                let json: Value = serde_json::from_slice(payload).unwrap();
                if json["op"] == "INSERT" && json["table"] == "exactly_once_test" {
                    if let Some(unique_id) = json["after"]["unique_id"].as_str() {
                        *message_map.entry(unique_id.to_string()).or_insert(0) += 1;

                        if unique_id.starts_with("batch1_") {
                            batch1_count += 1;
                        } else if unique_id.starts_with("batch2_") {
                            batch2_count += 1;
                        }
                    }
                }
            }
        }
    }

    // Verify counts
    assert_eq!(
        batch1_count, 50,
        "Expected 50 batch1 messages, got {}",
        batch1_count
    );
    assert_eq!(
        batch2_count, 50,
        "Expected 50 batch2 messages, got {}",
        batch2_count
    );

    // Verify no duplicates
    for (id, count) in &message_map {
        assert_eq!(*count, 1, "Message {} was delivered {} times", id, count);
    }

    // Cleanup
    cleanup_exactly_once_test(&client).await;
    tokio::fs::remove_file(&checkpoint_file).await.ok();
}

#[tokio::test]
#[ignore] // Run with: cargo test --ignored exactly_once_test::test_transaction_boundaries
async fn test_transaction_boundaries() {
    tracing_subscriber::fmt()
        .with_env_filter("pg_capture=debug")
        .try_init()
        .ok();

    let (client, test_config) = setup_exactly_once_test().await;

    // Start replicator
    let mut replicator = Replicator::new(test_config.clone());
    let _handle = tokio::spawn(async move { replicator.run().await });

    tokio::time::sleep(Duration::from_secs(2)).await;

    // Test 1: Multi-statement transaction
    client.execute("BEGIN", &[]).await.unwrap();
    client
        .execute(
            "INSERT INTO exactly_once_test (unique_id, data) VALUES ('tx1_1', 'first')",
            &[],
        )
        .await
        .unwrap();
    client
        .execute(
            "INSERT INTO exactly_once_test (unique_id, data) VALUES ('tx1_2', 'second')",
            &[],
        )
        .await
        .unwrap();
    client
        .execute(
            "INSERT INTO exactly_once_test (unique_id, data) VALUES ('tx1_3', 'third')",
            &[],
        )
        .await
        .unwrap();
    client.execute("COMMIT", &[]).await.unwrap();

    // Test 2: Rolled back transaction (should not appear in Kafka)
    client.execute("BEGIN", &[]).await.unwrap();
    client
        .execute(
            "INSERT INTO exactly_once_test (unique_id, data) VALUES ('tx2_1', 'rolled_back')",
            &[],
        )
        .await
        .unwrap();
    client.execute("ROLLBACK", &[]).await.unwrap();

    // Test 3: Another committed transaction
    client.execute("BEGIN", &[]).await.unwrap();
    client
        .execute(
            "INSERT INTO exactly_once_test (unique_id, data) VALUES ('tx3_1', 'committed')",
            &[],
        )
        .await
        .unwrap();
    client.execute("COMMIT", &[]).await.unwrap();

    // Wait for replication
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Verify transaction atomicity
    let consumer = create_exactly_once_consumer(&test_config.kafka).await;
    let mut received_ids = Vec::new();

    let timeout_duration = Duration::from_secs(5);
    let start = tokio::time::Instant::now();

    while start.elapsed() < timeout_duration {
        if let Ok(Ok(message)) = timeout(Duration::from_millis(500), consumer.recv()).await {
            if let Some(payload) = message.payload() {
                let json: Value = serde_json::from_slice(payload).unwrap();
                if json["op"] == "INSERT" && json["table"] == "exactly_once_test" {
                    if let Some(unique_id) = json["after"]["unique_id"].as_str() {
                        received_ids.push(unique_id.to_string());
                    }
                }
            }
        }
    }

    // Verify correct messages were received
    assert!(received_ids.contains(&"tx1_1".to_string()));
    assert!(received_ids.contains(&"tx1_2".to_string()));
    assert!(received_ids.contains(&"tx1_3".to_string()));
    assert!(received_ids.contains(&"tx3_1".to_string()));

    // Verify rolled back transaction was not received
    assert!(
        !received_ids.contains(&"tx2_1".to_string()),
        "Rolled back transaction should not be replicated"
    );

    cleanup_exactly_once_test(&client).await;
}

// Helper functions

async fn setup_exactly_once_test() -> (Client, Config) {
    let db_config = PostgresConfig {
        host: "localhost".to_string(),
        port: 5432,
        database: "postgres".to_string(),
        username: "postgres".to_string(),
        password: "postgres".to_string(),
        publication: "exactly_once_publication".to_string(),
        slot_name: format!("exactly_once_slot_{}", std::process::id()),
        connect_timeout_secs: 30,
        ssl_mode: SslMode::Disable,
    };

    let (client, connection) = tokio_postgres::connect(
        &format!(
            "host={} port={} dbname={} user={} password={}",
            db_config.host,
            db_config.port,
            db_config.database,
            db_config.username,
            db_config.password
        ),
        NoTls,
    )
    .await
    .unwrap();

    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("Connection error: {}", e);
        }
    });

    // Clean up any existing test objects
    client
        .execute(
            &format!("DROP PUBLICATION IF EXISTS {}", db_config.publication),
            &[],
        )
        .await
        .ok();
    client.execute(&format!("SELECT pg_drop_replication_slot('{}') WHERE EXISTS (SELECT 1 FROM pg_replication_slots WHERE slot_name = '{}')", 
                           db_config.slot_name, db_config.slot_name), &[])
        .await.ok();
    client
        .execute("DROP TABLE IF EXISTS exactly_once_test", &[])
        .await
        .ok();

    // Create test table
    client
        .execute(
            "CREATE TABLE exactly_once_test (
            id SERIAL PRIMARY KEY,
            unique_id TEXT UNIQUE NOT NULL,
            data TEXT,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        )",
            &[],
        )
        .await
        .unwrap();

    // Create publication
    client
        .execute(
            &format!(
                "CREATE PUBLICATION {} FOR TABLE exactly_once_test",
                db_config.publication
            ),
            &[],
        )
        .await
        .unwrap();

    let kafka_config = KafkaConfig {
        brokers: vec!["localhost:9092".to_string()],
        topic_prefix: format!("exactly_once_{}", std::process::id()),
        compression: "none".to_string(),
        acks: "all".to_string(), // Required for exactly-once
        linger_ms: 0,
        batch_size: 16384,
        buffer_memory: 33554432,
    };

    let replication_config = ReplicationConfig {
        poll_interval_ms: 100,
        keepalive_interval_secs: 10,
        checkpoint_interval_secs: 2,
        checkpoint_file: None,
        max_buffer_size: 1000,
    };

    let config = Config {
        postgres: db_config,
        kafka: kafka_config,
        replication: replication_config,
    };

    (client, config)
}

async fn cleanup_exactly_once_test(client: &Client) {
    let slot_name = format!("exactly_once_slot_{}", std::process::id());

    client
        .execute("DROP TABLE IF EXISTS exactly_once_test CASCADE", &[])
        .await
        .ok();
    client
        .execute("DROP PUBLICATION IF EXISTS exactly_once_publication", &[])
        .await
        .ok();
    client.execute(&format!("SELECT pg_drop_replication_slot('{}') WHERE EXISTS (SELECT 1 FROM pg_replication_slots WHERE slot_name = '{}')", 
                           slot_name, slot_name), &[])
        .await.ok();
}

async fn create_exactly_once_consumer(kafka_config: &KafkaConfig) -> StreamConsumer {
    let consumer: StreamConsumer = ClientConfig::new()
        .set("bootstrap.servers", kafka_config.brokers.join(","))
        .set(
            "group.id",
            format!("exactly_once_consumer_{}", std::process::id()),
        )
        .set("auto.offset.reset", "earliest")
        .set("enable.auto.commit", "false")
        .set("isolation.level", "read_committed") // Only read committed messages
        .create()
        .expect("Failed to create consumer");

    let topic = format!("{}.public.exactly_once_test", kafka_config.topic_prefix);
    consumer
        .subscribe(&[&topic])
        .expect("Failed to subscribe to topic");

    consumer
}
