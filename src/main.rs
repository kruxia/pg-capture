use clap::Parser;
use pg_capture::{Config, Result, Replicator};
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

#[derive(Parser, Debug)]
#[command(name = "pg-capture")]
#[command(about = "PostgreSQL to Kafka CDC replicator", long_about = None)]
#[command(version)]
struct Args {
    #[arg(short, long, help = "Enable JSON output for logs")]
    json_logs: bool,
    
    #[arg(short, long, help = "Verbose logging")]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    
    init_logging(args.json_logs, args.verbose);
    
    info!("Starting pg-capture v{}", env!("CARGO_PKG_VERSION"));
    info!("Loading configuration from environment variables");
    
    let config = match Config::from_env() {
        Ok(cfg) => {
            info!("Configuration loaded successfully");
            cfg
        }
        Err(e) => {
            error!("Failed to load configuration: {}", e);
            eprintln!("\nRequired environment variables:");
            eprintln!("  PG_DATABASE      - PostgreSQL database name");
            eprintln!("  PG_USERNAME      - PostgreSQL username");
            eprintln!("  PG_PASSWORD      - PostgreSQL password");
            eprintln!("  KAFKA_BROKERS    - Comma-separated list of Kafka brokers");
            eprintln!("\nSee .envrc.example for all available options");
            std::process::exit(1);
        }
    };
    
    info!(
        postgres_host = %config.postgres.host,
        postgres_port = %config.postgres.port,
        postgres_database = %config.postgres.database,
        postgres_publication = %config.postgres.publication,
        kafka_brokers = ?config.kafka.brokers,
        kafka_topic_prefix = %config.kafka.topic_prefix,
        "Configuration summary"
    );
    
    info!("Starting replication from PostgreSQL to Kafka");
    
    // Create and run the replicator
    let mut replicator = Replicator::new(config);
    
    match replicator.run().await {
        Ok(()) => {
            info!("Replication completed successfully");
            Ok(())
        }
        Err(e) => {
            error!("Replication failed: {}", e);
            Err(e)
        }
    }
}

fn init_logging(json: bool, verbose: bool) {
    let env_filter = if verbose {
        EnvFilter::new("pg_capture=debug,info")
    } else {
        EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("pg_capture=info,warn"))
    };
    
    let fmt_layer = if json {
        tracing_subscriber::fmt::layer()
            .json()
            .flatten_event(true)
            .with_current_span(false)
            .with_span_list(false)
            .boxed()
    } else {
        tracing_subscriber::fmt::layer()
            .with_target(false)
            .with_thread_ids(false)
            .with_thread_names(false)
            .boxed()
    };
    
    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .init();
}