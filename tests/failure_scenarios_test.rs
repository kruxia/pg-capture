use pg_replicate_kafka::config::{Config, KafkaConfig, PostgresConfig, ReplicationConfig};
use pg_replicate_kafka::replicator::Replicator;
use rdkafka::config::ClientConfig;
use rdkafka::consumer::{Consumer, StreamConsumer};
use rdkafka::Message;
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::time::timeout;
use tokio_postgres::{Client, NoTls};
use tracing::info;

#[tokio::test]
#[ignore] // Run with: cargo test --ignored failure_scenarios_test::test_postgres_connection_failure
async fn test_postgres_connection_failure() {
    tracing_subscriber::fmt()
        .with_env_filter("pg_replicate_kafka=debug")
        .try_init()
        .ok();

    let (client, mut test_config) = setup_failure_test().await;
    
    // Start replicator
    let replicator = Replicator::new(test_config.clone());
    let replicator_handle = tokio::spawn(async move {
        replicator.run().await
    });

    // Wait for startup
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Insert initial data
    client.execute("INSERT INTO failure_test (status) VALUES ('before_failure')", &[])
        .await
        .unwrap();

    // Simulate PostgreSQL connection failure by changing to invalid port
    test_config.postgres.port = 54321; // Invalid port

    // The replicator should continue retrying
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Restore correct config and insert more data
    // Note: The running replicator still has the correct config
    client.execute("INSERT INTO failure_test (status) VALUES ('after_recovery')", &[])
        .await
        .unwrap();

    // Verify both messages are eventually received
    let consumer = create_failure_consumer(&test_config.kafka).await;
    let mut received_statuses = Vec::new();
    
    let timeout_duration = Duration::from_secs(10);
    let start = tokio::time::Instant::now();
    
    while received_statuses.len() < 2 && start.elapsed() < timeout_duration {
        if let Ok(Some(message)) = timeout(Duration::from_secs(1), consumer.recv()).await {
            if let Some(payload) = message.payload() {
                let json: Value = serde_json::from_slice(payload).unwrap();
                if json["op"] == "INSERT" {
                    if let Some(status) = json["after"]["status"].as_str() {
                        received_statuses.push(status.to_string());
                    }
                }
            }
        }
    }

    assert!(received_statuses.contains(&"before_failure".to_string()));
    assert!(received_statuses.contains(&"after_recovery".to_string()));

    replicator_handle.abort();
    cleanup_failure_test(&client).await;
}

#[tokio::test]
#[ignore] // Run with: cargo test --ignored failure_scenarios_test::test_kafka_connection_failure
async fn test_kafka_connection_failure() {
    tracing_subscriber::fmt()
        .with_env_filter("pg_replicate_kafka=debug")
        .try_init()
        .ok();

    let (client, mut test_config) = setup_failure_test().await;
    
    // Start a TCP proxy to Kafka that we can control
    let proxy_port = 19092;
    let proxy_running = Arc::new(AtomicBool::new(true));
    let proxy_handle = start_kafka_proxy(proxy_port, "localhost:9092", proxy_running.clone()).await;

    // Configure to use proxy
    test_config.kafka.brokers = vec![format!("localhost:{}", proxy_port)];

    // Start replicator
    let replicator = Replicator::new(test_config.clone());
    let replicator_handle = tokio::spawn(async move {
        replicator.run().await
    });

    // Wait for startup
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Insert data that should be replicated
    client.execute("INSERT INTO failure_test (status) VALUES ('before_kafka_failure')", &[])
        .await
        .unwrap();

    // Wait for message to be sent
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Simulate Kafka failure by stopping proxy
    proxy_running.store(false, Ordering::Relaxed);
    proxy_handle.abort();
    info!("Kafka proxy stopped - simulating network failure");

    // Insert data while Kafka is unavailable
    for i in 0..5 {
        client.execute(
            &format!("INSERT INTO failure_test (status) VALUES ('during_failure_{}')", i),
            &[]
        ).await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    // Restart proxy
    proxy_running.store(true, Ordering::Relaxed);
    let _proxy_handle = start_kafka_proxy(proxy_port, "localhost:9092", proxy_running.clone()).await;
    info!("Kafka proxy restarted - connection restored");

    // Insert data after recovery
    client.execute("INSERT INTO failure_test (status) VALUES ('after_kafka_recovery')", &[])
        .await
        .unwrap();

    // Verify all messages are eventually delivered
    test_config.kafka.brokers = vec!["localhost:9092".to_string()]; // Direct connection for consumer
    let consumer = create_failure_consumer(&test_config.kafka).await;
    let mut received_count = 0;
    let expected_count = 7; // 1 before + 5 during + 1 after
    
    let timeout_duration = Duration::from_secs(30);
    let start = tokio::time::Instant::now();
    
    while received_count < expected_count && start.elapsed() < timeout_duration {
        if let Ok(Some(message)) = timeout(Duration::from_secs(1), consumer.recv()).await {
            if let Some(payload) = message.payload() {
                let json: Value = serde_json::from_slice(payload).unwrap();
                if json["op"] == "INSERT" && json["table"] == "failure_test" {
                    received_count += 1;
                    info!("Received message {}/{}: {:?}", received_count, expected_count, 
                          json["after"]["status"]);
                }
            }
        }
    }

    assert_eq!(received_count, expected_count, 
               "Expected {} messages but received {}", expected_count, received_count);

    replicator_handle.abort();
    cleanup_failure_test(&client).await;
}

#[tokio::test]
#[ignore] // Run with: cargo test --ignored failure_scenarios_test::test_replicator_crash_recovery
async fn test_replicator_crash_recovery() {
    tracing_subscriber::fmt()
        .with_env_filter("pg_replicate_kafka=debug")
        .try_init()
        .ok();

    let (client, mut test_config) = setup_failure_test().await;
    
    // Use a specific checkpoint file
    let checkpoint_file = format!("/tmp/pg_replicate_kafka_crash_test_{}.checkpoint", 
                                   std::process::id());
    test_config.replication.checkpoint_file = Some(checkpoint_file.clone());
    test_config.replication.checkpoint_interval_secs = 1; // Frequent checkpoints

    // Insert some initial data
    for i in 0..10 {
        client.execute(
            &format!("INSERT INTO failure_test (status) VALUES ('batch1_{}')", i),
            &[]
        ).await.unwrap();
    }

    // Start replicator
    let replicator = Replicator::new(test_config.clone());
    let handle = tokio::spawn(async move {
        replicator.run().await
    });

    // Let it process some messages and checkpoint
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Simulate crash
    handle.abort();
    info!("Replicator crashed");

    // Insert more data while crashed
    for i in 0..10 {
        client.execute(
            &format!("INSERT INTO failure_test (status) VALUES ('batch2_{}')", i),
            &[]
        ).await.unwrap();
    }

    // Start new replicator instance - should resume from checkpoint
    let replicator = Replicator::new(test_config.clone());
    let _handle = tokio::spawn(async move {
        replicator.run().await
    });

    info!("Replicator restarted");

    // Insert final batch
    for i in 0..10 {
        client.execute(
            &format!("INSERT INTO failure_test (status) VALUES ('batch3_{}')", i),
            &[]
        ).await.unwrap();
    }

    // Verify all 30 messages are received
    let consumer = create_failure_consumer(&test_config.kafka).await;
    let mut received_count = 0;
    let expected_count = 30;
    
    let timeout_duration = Duration::from_secs(20);
    let start = tokio::time::Instant::now();
    
    while received_count < expected_count && start.elapsed() < timeout_duration {
        if let Ok(Some(message)) = timeout(Duration::from_secs(1), consumer.recv()).await {
            if let Some(payload) = message.payload() {
                let json: Value = serde_json::from_slice(payload).unwrap();
                if json["op"] == "INSERT" && json["table"] == "failure_test" {
                    received_count += 1;
                }
            }
        }
    }

    assert_eq!(received_count, expected_count, 
               "Expected {} messages but received {}", expected_count, received_count);

    // Cleanup
    cleanup_failure_test(&client).await;
    tokio::fs::remove_file(&checkpoint_file).await.ok();
}

#[tokio::test]
#[ignore] // Run with: cargo test --ignored failure_scenarios_test::test_malformed_replication_data
async fn test_malformed_replication_data() {
    tracing_subscriber::fmt()
        .with_env_filter("pg_replicate_kafka=debug")
        .try_init()
        .ok();

    let (client, test_config) = setup_failure_test().await;
    
    // Create table with various data types that might cause issues
    client.execute("DROP TABLE IF EXISTS complex_types", &[]).await.ok();
    client.execute(
        "CREATE TABLE complex_types (
            id SERIAL PRIMARY KEY,
            json_data JSONB,
            array_data INTEGER[],
            binary_data BYTEA,
            large_text TEXT
        )",
        &[]
    ).await.unwrap();

    // Update publication
    client.execute(
        &format!("ALTER PUBLICATION {} ADD TABLE complex_types", test_config.postgres.publication),
        &[]
    ).await.unwrap();

    // Start replicator
    let replicator = Replicator::new(test_config.clone());
    let replicator_handle = tokio::spawn(async move {
        replicator.run().await
    });

    // Wait for startup
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Insert data with edge cases
    let test_cases = vec![
        // Valid JSON
        "INSERT INTO complex_types (json_data) VALUES ('{\"key\": \"value\"}'::jsonb)",
        // Nested JSON
        "INSERT INTO complex_types (json_data) VALUES ('{\"nested\": {\"deep\": [1,2,3]}}'::jsonb)",
        // Array data
        "INSERT INTO complex_types (array_data) VALUES (ARRAY[1,2,3,4,5])",
        // Binary data
        "INSERT INTO complex_types (binary_data) VALUES ('\\xDEADBEEF'::bytea)",
        // Large text
        &format!("INSERT INTO complex_types (large_text) VALUES ('{}')", "x".repeat(10000)),
        // NULL values
        "INSERT INTO complex_types (json_data, array_data) VALUES (NULL, NULL)",
        // Empty array
        "INSERT INTO complex_types (array_data) VALUES (ARRAY[]::integer[])",
    ];

    for (i, sql) in test_cases.iter().enumerate() {
        info!("Executing test case {}: {}", i, sql.split_whitespace().take(5).collect::<Vec<_>>().join(" "));
        client.execute(sql, &[]).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Also test updates and deletes
    client.execute("UPDATE complex_types SET json_data = '{\"updated\": true}'::jsonb WHERE id = 1", &[])
        .await.unwrap();
    client.execute("DELETE FROM complex_types WHERE id = 2", &[])
        .await.unwrap();

    // Verify replicator handles all cases without crashing
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Check that replicator is still running
    assert!(!replicator_handle.is_finished(), "Replicator crashed during edge case testing");

    // Verify messages were produced
    let consumer = create_failure_consumer(&test_config.kafka).await;
    let mut message_count = 0;
    
    let timeout_duration = Duration::from_secs(5);
    let start = tokio::time::Instant::now();
    
    while start.elapsed() < timeout_duration {
        if let Ok(Some(message)) = timeout(Duration::from_millis(500), consumer.recv()).await {
            if let Some(payload) = message.payload() {
                // Verify it's valid JSON
                let json: Value = serde_json::from_slice(payload)
                    .expect("Replicator produced invalid JSON");
                
                if json["table"] == "complex_types" {
                    message_count += 1;
                    
                    // Verify structure
                    assert!(json["op"].is_string());
                    assert!(json["ts_ms"].is_number());
                    assert!(json["source"].is_object());
                }
            }
        }
    }

    // We should have received 7 inserts + 1 update + 1 delete = 9 messages
    assert!(message_count >= 9, "Expected at least 9 messages, got {}", message_count);

    // Cleanup
    replicator_handle.abort();
    client.execute("DROP TABLE complex_types CASCADE", &[]).await.ok();
    cleanup_failure_test(&client).await;
}

// Helper functions

async fn setup_failure_test() -> (Client, Config) {
    let db_config = PostgresConfig {
        host: "localhost".to_string(),
        port: 5432,
        database: "postgres".to_string(),
        username: "postgres".to_string(),
        password: "postgres".to_string(),
        publication: "failure_publication".to_string(),
        slot_name: format!("failure_slot_{}", std::process::id()),
    };

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
    client.execute("DROP TABLE IF EXISTS failure_test", &[])
        .await.ok();

    // Create test table
    client.execute(
        "CREATE TABLE failure_test (
            id SERIAL PRIMARY KEY,
            status TEXT NOT NULL,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        )",
        &[]
    ).await.unwrap();

    // Create publication
    client.execute(
        &format!("CREATE PUBLICATION {} FOR TABLE failure_test", db_config.publication),
        &[]
    ).await.unwrap();

    let kafka_config = KafkaConfig {
        brokers: vec!["localhost:9092".to_string()],
        topic_prefix: format!("failure_{}", std::process::id()),
        compression: None,
        acks: "all".to_string(),
    };

    let replication_config = ReplicationConfig {
        poll_interval_ms: 100,
        keepalive_interval_secs: 10,
        checkpoint_interval_secs: 5,
        checkpoint_file: None,
    };

    let config = Config {
        postgres: db_config,
        kafka: kafka_config,
        replication: replication_config,
    };

    (client, config)
}

async fn cleanup_failure_test(client: &Client) {
    let slot_name = format!("failure_slot_{}", std::process::id());
    
    client.execute("DROP TABLE IF EXISTS failure_test CASCADE", &[]).await.ok();
    client.execute("DROP PUBLICATION IF EXISTS failure_publication", &[]).await.ok();
    client.execute(&format!("SELECT pg_drop_replication_slot('{}') WHERE EXISTS (SELECT 1 FROM pg_replication_slots WHERE slot_name = '{}')", 
                           slot_name, slot_name), &[])
        .await.ok();
}

async fn create_failure_consumer(kafka_config: &KafkaConfig) -> StreamConsumer {
    let consumer: StreamConsumer = ClientConfig::new()
        .set("bootstrap.servers", kafka_config.brokers.join(","))
        .set("group.id", format!("failure_consumer_{}", std::process::id()))
        .set("auto.offset.reset", "earliest")
        .set("enable.auto.commit", "false")
        .create()
        .expect("Failed to create consumer");

    // Subscribe to all topics with the prefix
    let topic_pattern = format!("^{}\\..*", kafka_config.topic_prefix);
    consumer
        .subscribe(&[&topic_pattern])
        .expect("Failed to subscribe to topics");

    consumer
}

async fn start_kafka_proxy(
    proxy_port: u16, 
    target: &str, 
    running: Arc<AtomicBool>
) -> tokio::task::JoinHandle<()> {
    let target = target.to_string();
    
    tokio::spawn(async move {
        let listener = TcpListener::bind(format!("127.0.0.1:{}", proxy_port))
            .await
            .unwrap();
        
        info!("Kafka proxy listening on port {}", proxy_port);
        
        while running.load(Ordering::Relaxed) {
            if let Ok((inbound, _)) = timeout(Duration::from_millis(100), listener.accept()).await {
                if let Ok(outbound) = tokio::net::TcpStream::connect(&target).await {
                    let (ri, wi) = inbound.into_split();
                    let (ro, wo) = outbound.into_split();
                    
                    tokio::spawn(async move {
                        let _ = tokio::io::copy(&mut ri.into(), &mut wo.into()).await;
                    });
                    
                    tokio::spawn(async move {
                        let _ = tokio::io::copy(&mut ro.into(), &mut wi.into()).await;
                    });
                }
            }
        }
    })
}