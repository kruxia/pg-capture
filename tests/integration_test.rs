use pg_replicate_kafka::config::{Config, KafkaConfig, PostgresConfig, ReplicationConfig};
use pg_replicate_kafka::replicator::Replicator;
use rdkafka::config::ClientConfig;
use rdkafka::consumer::{Consumer, StreamConsumer};
use rdkafka::Message;
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::timeout;
use tokio_postgres::{Client, NoTls};
use tracing::info;

#[tokio::test]
#[ignore] // Run with: cargo test --ignored integration_test::test_end_to_end_replication
async fn test_end_to_end_replication() {
    tracing_subscriber::fmt()
        .with_env_filter("pg_replicate_kafka=debug,rdkafka=info")
        .try_init()
        .ok();

    // Setup test database and table
    let (client, test_config) = setup_test_database().await;
    
    // Start replicator in background
    let replicator = Replicator::new(test_config.clone());
    let replicator_handle = tokio::spawn(async move {
        replicator.run().await
    });

    // Give replicator time to start
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Perform database operations
    let operations = vec![
        ("INSERT", "INSERT INTO test_table (name, age) VALUES ('Alice', 30)"),
        ("UPDATE", "UPDATE test_table SET age = 31 WHERE name = 'Alice'"),
        ("DELETE", "DELETE FROM test_table WHERE name = 'Alice'"),
    ];

    let mut expected_messages = HashMap::new();
    
    for (op_type, sql) in operations {
        info!("Executing SQL: {}", sql);
        client.execute(sql, &[]).await.unwrap();
        expected_messages.insert(op_type.to_string(), true);
        
        // Small delay to ensure message ordering
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Consume messages from Kafka
    let consumer = create_test_consumer(&test_config.kafka).await;
    let mut received_ops = HashMap::new();
    
    let timeout_duration = Duration::from_secs(10);
    let start = tokio::time::Instant::now();
    
    while received_ops.len() < expected_messages.len() && start.elapsed() < timeout_duration {
        if let Ok(Some(message)) = timeout(Duration::from_secs(1), consumer.recv()).await {
            if let Some(payload) = message.payload() {
                let json: Value = serde_json::from_slice(payload).unwrap();
                let op = json["op"].as_str().unwrap();
                info!("Received message with operation: {}", op);
                received_ops.insert(op.to_string(), true);
                
                // Validate message structure
                assert!(json["schema"].is_string());
                assert!(json["table"].is_string());
                assert!(json["ts_ms"].is_number());
                assert!(json["source"].is_object());
                
                match op {
                    "INSERT" => {
                        assert!(json["before"].is_null());
                        assert!(json["after"].is_object());
                        assert_eq!(json["after"]["name"], "Alice");
                        assert_eq!(json["after"]["age"], 30);
                    }
                    "UPDATE" => {
                        assert!(json["before"].is_object());
                        assert!(json["after"].is_object());
                        assert_eq!(json["before"]["age"], 30);
                        assert_eq!(json["after"]["age"], 31);
                    }
                    "DELETE" => {
                        assert!(json["before"].is_object());
                        assert!(json["after"].is_null());
                        assert_eq!(json["before"]["name"], "Alice");
                    }
                    _ => panic!("Unexpected operation: {}", op),
                }
            }
        }
    }

    // Verify all expected messages were received
    for op in expected_messages.keys() {
        assert!(received_ops.contains_key(op), "Missing operation: {}", op);
    }

    // Cleanup
    replicator_handle.abort();
    cleanup_test_database(&client).await;
}

#[tokio::test]
#[ignore] // Run with: cargo test --ignored integration_test::test_replicator_recovery
async fn test_replicator_recovery() {
    tracing_subscriber::fmt()
        .with_env_filter("pg_replicate_kafka=debug")
        .try_init()
        .ok();

    let (client, test_config) = setup_test_database().await;
    
    // Start replicator
    let replicator = Replicator::new(test_config.clone());
    let handle = tokio::spawn(async move {
        replicator.run().await
    });

    // Wait for startup
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Insert initial data
    client.execute("INSERT INTO test_table (name, age) VALUES ('Bob', 25)", &[])
        .await
        .unwrap();

    // Wait for replication
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Simulate replicator crash
    handle.abort();
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Insert more data while replicator is down
    client.execute("INSERT INTO test_table (name, age) VALUES ('Charlie', 35)", &[])
        .await
        .unwrap();

    // Restart replicator
    let replicator = Replicator::new(test_config.clone());
    let _handle = tokio::spawn(async move {
        replicator.run().await
    });

    // Verify both messages are eventually received
    let consumer = create_test_consumer(&test_config.kafka).await;
    let mut received_names = Vec::new();
    
    let timeout_duration = Duration::from_secs(10);
    let start = tokio::time::Instant::now();
    
    while received_names.len() < 2 && start.elapsed() < timeout_duration {
        if let Ok(Some(message)) = timeout(Duration::from_secs(1), consumer.recv()).await {
            if let Some(payload) = message.payload() {
                let json: Value = serde_json::from_slice(payload).unwrap();
                if json["op"] == "INSERT" {
                    if let Some(name) = json["after"]["name"].as_str() {
                        received_names.push(name.to_string());
                    }
                }
            }
        }
    }

    assert!(received_names.contains(&"Bob".to_string()));
    assert!(received_names.contains(&"Charlie".to_string()));

    cleanup_test_database(&client).await;
}

#[tokio::test]
#[ignore] // Run with: cargo test --ignored integration_test::test_checkpoint_persistence
async fn test_checkpoint_persistence() {
    tracing_subscriber::fmt()
        .with_env_filter("pg_replicate_kafka=debug")
        .try_init()
        .ok();

    let (client, mut test_config) = setup_test_database().await;
    
    // Use a specific checkpoint file for this test
    let checkpoint_file = format!("/tmp/pg_replicate_kafka_test_{}.checkpoint", 
                                   std::process::id());
    test_config.replication.checkpoint_file = Some(checkpoint_file.clone());

    // Start replicator
    let replicator = Replicator::new(test_config.clone());
    let handle = tokio::spawn(async move {
        replicator.run().await
    });

    // Wait for startup
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Insert data
    for i in 0..5 {
        client.execute(
            &format!("INSERT INTO test_table (name, age) VALUES ('User{}', {})", i, i * 10),
            &[]
        ).await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    // Wait for checkpoint to be written
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Stop replicator
    handle.abort();

    // Verify checkpoint file exists
    assert!(tokio::fs::metadata(&checkpoint_file).await.is_ok());

    // Insert more data while replicator is down
    for i in 5..10 {
        client.execute(
            &format!("INSERT INTO test_table (name, age) VALUES ('User{}', {})", i, i * 10),
            &[]
        ).await.unwrap();
    }

    // Restart replicator - it should resume from checkpoint
    let replicator = Replicator::new(test_config.clone());
    let _handle = tokio::spawn(async move {
        replicator.run().await
    });

    // Consume all messages
    let consumer = create_test_consumer(&test_config.kafka).await;
    let mut user_numbers = Vec::new();
    
    let timeout_duration = Duration::from_secs(15);
    let start = tokio::time::Instant::now();
    
    while user_numbers.len() < 10 && start.elapsed() < timeout_duration {
        if let Ok(Some(message)) = timeout(Duration::from_secs(1), consumer.recv()).await {
            if let Some(payload) = message.payload() {
                let json: Value = serde_json::from_slice(payload).unwrap();
                if json["op"] == "INSERT" {
                    if let Some(name) = json["after"]["name"].as_str() {
                        if let Some(num) = name.strip_prefix("User") {
                            if let Ok(n) = num.parse::<i32>() {
                                user_numbers.push(n);
                            }
                        }
                    }
                }
            }
        }
    }

    // Sort and verify we got all users 0-9
    user_numbers.sort();
    user_numbers.dedup();
    assert_eq!(user_numbers, vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);

    // Cleanup
    cleanup_test_database(&client).await;
    tokio::fs::remove_file(&checkpoint_file).await.ok();
}

// Helper functions

async fn setup_test_database() -> (Client, Config) {
    let db_config = PostgresConfig {
        host: "localhost".to_string(),
        port: 5432,
        database: "postgres".to_string(),
        username: "postgres".to_string(),
        password: "postgres".to_string(),
        publication: "test_publication".to_string(),
        slot_name: format!("test_slot_{}", std::process::id()),
    };

    // Connect to create test setup
    let (client, connection) = tokio_postgres::connect(
        &format!(
            "host={} port={} dbname={} user={} password={}",
            db_config.host, db_config.port, db_config.database, 
            db_config.username, db_config.password
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
    client.execute(&format!("DROP PUBLICATION IF EXISTS {}", db_config.publication), &[])
        .await.ok();
    client.execute(&format!("SELECT pg_drop_replication_slot('{}') WHERE EXISTS (SELECT 1 FROM pg_replication_slots WHERE slot_name = '{}')", 
                           db_config.slot_name, db_config.slot_name), &[])
        .await.ok();
    client.execute("DROP TABLE IF EXISTS test_table", &[])
        .await.ok();

    // Create test table
    client.execute(
        "CREATE TABLE test_table (
            id SERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            age INTEGER NOT NULL,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        )",
        &[]
    ).await.unwrap();

    // Create publication
    client.execute(
        &format!("CREATE PUBLICATION {} FOR TABLE test_table", db_config.publication),
        &[]
    ).await.unwrap();

    let kafka_config = KafkaConfig {
        brokers: vec!["localhost:9092".to_string()],
        topic_prefix: format!("test_{}", std::process::id()),
        compression: None,
        acks: "all".to_string(),
    };

    let replication_config = ReplicationConfig {
        poll_interval_ms: 100,
        keepalive_interval_secs: 10,
        checkpoint_interval_secs: 2,
        checkpoint_file: None,
    };

    let config = Config {
        postgres: db_config,
        kafka: kafka_config,
        replication: replication_config,
    };

    (client, config)
}

async fn cleanup_test_database(client: &Client) {
    let slot_name = format!("test_slot_{}", std::process::id());
    
    client.execute("DROP TABLE IF EXISTS test_table CASCADE", &[]).await.ok();
    client.execute("DROP PUBLICATION IF EXISTS test_publication", &[]).await.ok();
    client.execute(&format!("SELECT pg_drop_replication_slot('{}') WHERE EXISTS (SELECT 1 FROM pg_replication_slots WHERE slot_name = '{}')", 
                           slot_name, slot_name), &[])
        .await.ok();
}

async fn create_test_consumer(kafka_config: &KafkaConfig) -> StreamConsumer {
    let consumer: StreamConsumer = ClientConfig::new()
        .set("bootstrap.servers", kafka_config.brokers.join(","))
        .set("group.id", format!("test_consumer_{}", std::process::id()))
        .set("auto.offset.reset", "earliest")
        .set("enable.auto.commit", "false")
        .create()
        .expect("Failed to create consumer");

    let topic = format!("{}.public.test_table", kafka_config.topic_prefix);
    consumer
        .subscribe(&[&topic])
        .expect("Failed to subscribe to topic");

    consumer
}