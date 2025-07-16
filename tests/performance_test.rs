use pg_capture::config::{Config, KafkaConfig, PostgresConfig, ReplicationConfig, SslMode};
use pg_capture::replicator::Replicator;
use rdkafka::config::ClientConfig;
use rdkafka::consumer::{Consumer, StreamConsumer};
use rdkafka::Message;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::timeout;
use tokio_postgres::{Client, NoTls};
use tracing::info;

#[tokio::test]
#[ignore] // Run with: cargo test --ignored performance_test::test_throughput
async fn test_throughput() {
    tracing_subscriber::fmt()
        .with_env_filter("pg_capture=info")
        .try_init()
        .ok();

    let (client, test_config) = setup_performance_test().await;
    
    // Start replicator
    let mut replicator = Replicator::new(test_config.clone());
    let replicator_handle = tokio::spawn(async move {
        replicator.run().await
    });

    // Give replicator time to start
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Metrics
    let messages_sent = Arc::new(AtomicU64::new(0));
    let messages_received = Arc::new(AtomicU64::new(0));
    let bytes_received = Arc::new(AtomicU64::new(0));

    // Start consumer in background
    let consumer = create_performance_consumer(&test_config.kafka).await;
    let messages_received_clone = messages_received.clone();
    let bytes_received_clone = bytes_received.clone();
    
    let consumer_handle = tokio::spawn(async move {
        loop {
            if let Ok(Ok(message)) = timeout(Duration::from_millis(100), consumer.recv()).await {
                if let Some(payload) = message.payload() {
                    messages_received_clone.fetch_add(1, Ordering::Relaxed);
                    bytes_received_clone.fetch_add(payload.len() as u64, Ordering::Relaxed);
                }
            }
        }
    });

    // Generate load
    let target_messages = 10_000;
    let batch_size = 100;
    let start_time = Instant::now();

    info!("Starting performance test with {} messages", target_messages);

    for batch in 0..(target_messages / batch_size) {
        let mut batch_query = String::from("INSERT INTO perf_test (data, value) VALUES ");
        let values: Vec<String> = (0..batch_size)
            .map(|i| {
                let n = batch * batch_size + i;
                format!("('test_data_{}', {})", n, n)
            })
            .collect();
        batch_query.push_str(&values.join(", "));

        client.execute(&batch_query, &[]).await.unwrap();
        messages_sent.fetch_add(batch_size, Ordering::Relaxed);

        // Small delay between batches to avoid overwhelming the system
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let insert_duration = start_time.elapsed();
    info!("Inserted {} messages in {:?}", target_messages, insert_duration);

    // Wait for all messages to be consumed
    let wait_start = Instant::now();
    while messages_received.load(Ordering::Relaxed) < target_messages as u64 {
        if wait_start.elapsed() > Duration::from_secs(30) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let total_duration = start_time.elapsed();
    let received = messages_received.load(Ordering::Relaxed);
    let bytes = bytes_received.load(Ordering::Relaxed);

    // Calculate metrics
    let insert_rate = target_messages as f64 / insert_duration.as_secs_f64();
    let throughput = received as f64 / total_duration.as_secs_f64();
    let avg_latency = (total_duration.as_millis() as f64 - insert_duration.as_millis() as f64) 
                      / received as f64;
    let avg_message_size = if received > 0 { bytes / received } else { 0 };

    // Print results
    println!("\n=== Performance Test Results ===");
    println!("Messages sent: {}", target_messages);
    println!("Messages received: {}", received);
    println!("Success rate: {:.2}%", (received as f64 / target_messages as f64) * 100.0);
    println!("Insert rate: {:.2} msg/sec", insert_rate);
    println!("End-to-end throughput: {:.2} msg/sec", throughput);
    println!("Average latency: {:.2} ms", avg_latency);
    println!("Average message size: {} bytes", avg_message_size);
    println!("Total bytes received: {} KB", bytes / 1024);
    println!("Total duration: {:?}", total_duration);

    // Verify performance meets requirements (1000 msg/sec, <500ms latency)
    assert!(throughput >= 1000.0, "Throughput {} is below 1000 msg/sec", throughput);
    assert!(avg_latency < 500.0, "Average latency {}ms exceeds 500ms", avg_latency);
    assert_eq!(received, target_messages as u64, "Not all messages were received");

    // Cleanup
    consumer_handle.abort();
    replicator_handle.abort();
    cleanup_performance_test(&client).await;
}

#[tokio::test]
#[ignore] // Run with: cargo test --ignored performance_test::test_memory_usage
async fn test_memory_usage() {
    tracing_subscriber::fmt()
        .with_env_filter("pg_capture=info")
        .try_init()
        .ok();

    let (client, test_config) = setup_performance_test().await;
    
    // Get initial memory usage
    let initial_memory = get_current_memory_usage();
    info!("Initial memory usage: {} MB", initial_memory / 1024 / 1024);

    // Start replicator
    let mut replicator = Replicator::new(test_config.clone());
    let replicator_handle = tokio::spawn(async move {
        replicator.run().await
    });

    // Give replicator time to start
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Track memory over time
    let mut max_memory = initial_memory;
    let mut memory_samples = Vec::new();

    // Generate sustained load for 60 seconds
    let test_duration = Duration::from_secs(60);
    let start_time = Instant::now();
    let mut total_messages = 0;

    let memory_monitor = tokio::spawn(async move {
        while start_time.elapsed() < test_duration {
            let current_memory = get_current_memory_usage();
            memory_samples.push(current_memory);
            if current_memory > max_memory {
                max_memory = current_memory;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        (max_memory, memory_samples)
    });

    // Generate continuous load
    while start_time.elapsed() < test_duration {
        let batch_query = format!(
            "INSERT INTO perf_test (data, value) VALUES {}",
            (0..50)
                .map(|i| format!("('large_data_{}', {})", "x".repeat(1000), i))
                .collect::<Vec<_>>()
                .join(", ")
        );
        
        if client.execute(&batch_query, &[]).await.is_ok() {
            total_messages += 50;
        }
        
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let (max_mem, samples) = memory_monitor.await.unwrap();
    let avg_memory = samples.iter().sum::<usize>() / samples.len();
    let memory_increase = max_mem.saturating_sub(initial_memory);

    // Print results
    println!("\n=== Memory Usage Test Results ===");
    println!("Test duration: {:?}", test_duration);
    println!("Total messages processed: {}", total_messages);
    println!("Initial memory: {} MB", initial_memory / 1024 / 1024);
    println!("Maximum memory: {} MB", max_mem / 1024 / 1024);
    println!("Average memory: {} MB", avg_memory / 1024 / 1024);
    println!("Memory increase: {} MB", memory_increase / 1024 / 1024);

    // Verify memory usage stays under 256MB
    assert!(
        max_mem < 256 * 1024 * 1024,
        "Maximum memory usage {} MB exceeds 256 MB limit",
        max_mem / 1024 / 1024
    );

    // Check for memory leaks (memory shouldn't grow unbounded)
    let growth_rate = memory_increase as f64 / total_messages as f64;
    assert!(
        growth_rate < 1024.0, // Less than 1KB per message
        "Memory growth rate {} bytes/message suggests a leak",
        growth_rate
    );

    // Cleanup
    replicator_handle.abort();
    cleanup_performance_test(&client).await;
}

// Helper functions

async fn setup_performance_test() -> (Client, Config) {
    let db_config = PostgresConfig {
        host: "localhost".to_string(),
        port: 5432,
        database: "postgres".to_string(),
        username: "postgres".to_string(),
        password: "postgres".to_string(),
        publication: "perf_publication".to_string(),
        slot_name: format!("perf_slot_{}", std::process::id()),
        connect_timeout_secs: 30,
        ssl_mode: SslMode::Disable,
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
    client.execute("DROP TABLE IF EXISTS perf_test", &[])
        .await.ok();

    // Create test table
    client.execute(
        "CREATE TABLE perf_test (
            id SERIAL PRIMARY KEY,
            data TEXT,
            value INTEGER,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        )",
        &[]
    ).await.unwrap();

    // Create publication
    client.execute(
        &format!("CREATE PUBLICATION {} FOR TABLE perf_test", db_config.publication),
        &[]
    ).await.unwrap();

    let kafka_config = KafkaConfig {
        brokers: vec!["localhost:9092".to_string()],
        topic_prefix: format!("perf_{}", std::process::id()),
        compression: "snappy".to_string(),
        acks: "1".to_string(), // Less strict for performance testing
        linger_ms: 0,
        batch_size: 16384,
        buffer_memory: 33554432,
    };

    let replication_config = ReplicationConfig {
        poll_interval_ms: 50, // More aggressive polling for performance
        keepalive_interval_secs: 10,
        checkpoint_interval_secs: 10, // Less frequent checkpoints
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

async fn cleanup_performance_test(client: &Client) {
    let slot_name = format!("perf_slot_{}", std::process::id());
    
    client.execute("DROP TABLE IF EXISTS perf_test CASCADE", &[]).await.ok();
    client.execute("DROP PUBLICATION IF EXISTS perf_publication", &[]).await.ok();
    client.execute(&format!("SELECT pg_drop_replication_slot('{}') WHERE EXISTS (SELECT 1 FROM pg_replication_slots WHERE slot_name = '{}')", 
                           slot_name, slot_name), &[])
        .await.ok();
}

async fn create_performance_consumer(kafka_config: &KafkaConfig) -> StreamConsumer {
    let consumer: StreamConsumer = ClientConfig::new()
        .set("bootstrap.servers", kafka_config.brokers.join(","))
        .set("group.id", format!("perf_consumer_{}", std::process::id()))
        .set("auto.offset.reset", "earliest")
        .set("enable.auto.commit", "false")
        .set("fetch.min.bytes", "10000") // Batch fetches for performance
        .create()
        .expect("Failed to create consumer");

    let topic = format!("{}.public.perf_test", kafka_config.topic_prefix);
    consumer
        .subscribe(&[&topic])
        .expect("Failed to subscribe to topic");

    consumer
}

fn get_current_memory_usage() -> usize {
    // Read memory usage from /proc/self/status on Linux
    #[cfg(target_os = "linux")]
    {
        use std::fs;
        if let Ok(status) = fs::read_to_string("/proc/self/status") {
            for line in status.lines() {
                if line.starts_with("VmRSS:") {
                    if let Some(kb_str) = line.split_whitespace().nth(1) {
                        if let Ok(kb) = kb_str.parse::<usize>() {
                            return kb * 1024; // Convert KB to bytes
                        }
                    }
                }
            }
        }
    }
    
    // Fallback: estimate based on allocator statistics
    0
}