use pg_capture::postgres::connection::ReplicationConnection;
use tracing::info;
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter("pg_capture=debug,info")
        .init();

    // Configuration - these would come from environment variables in production
    let host = std::env::var("PG_HOST").unwrap_or_else(|_| "localhost".to_string());
    let port = std::env::var("PG_PORT").unwrap_or_else(|_| "5432".to_string());
    let database = std::env::var("PG_DATABASE").unwrap_or_else(|_| "postgres".to_string());
    let username = std::env::var("PG_USERNAME").unwrap_or_else(|_| "postgres".to_string());
    let password = std::env::var("PG_PASSWORD").unwrap_or_else(|_| "postgres".to_string());

    let connection_string = format!(
        "postgres://{}:{}@{}:{}/{}?replication=database",
        username, password, host, port, database
    );

    let slot_name = std::env::var("PG_SLOT_NAME").unwrap_or_else(|_| "test_slot".to_string());
    let publication_name =
        std::env::var("PG_PUBLICATION").unwrap_or_else(|_| "test_pub".to_string());

    info!("Testing PostgreSQL Copy Both protocol implementation");
    info!("Connection string: {}", connection_string);
    info!("Slot name: {}", slot_name);
    info!("Publication name: {}", publication_name);

    // Create replication connection
    let mut conn =
        ReplicationConnection::new(&connection_string, slot_name, publication_name).await?;

    // Identify system
    let system_info = conn.identify_system().await?;
    info!("System info: {:?}", system_info);

    // Create replication slot
    conn.create_replication_slot().await?;

    // Start replication
    conn.start_replication(None).await?;
    info!("Replication started successfully!");

    // Receive a few replication messages
    info!("Waiting for replication messages...");
    let mut message_count = 0;

    while message_count < 5 {
        match tokio::time::timeout(
            std::time::Duration::from_secs(10),
            conn.recv_replication_message(),
        )
        .await
        {
            Ok(Ok(Some(msg))) => {
                info!("Received replication message with {} bytes", msg.data.len());
                message_count += 1;
            }
            Ok(Ok(None)) => {
                info!("No message received");
            }
            Ok(Err(e)) => {
                eprintln!("Error receiving message: {}", e);
                break;
            }
            Err(_) => {
                info!("Timeout waiting for message");
                break;
            }
        }
    }

    // Clean up
    info!("Dropping replication slot...");
    conn.drop_replication_slot().await?;

    info!("Test completed successfully!");
    Ok(())
}
