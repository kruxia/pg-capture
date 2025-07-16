use clap::Parser;
use pg_replicate_kafka::{Config, Error, Result};
use std::path::PathBuf;
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

#[derive(Parser, Debug)]
#[command(name = "pg-replicate-kafka")]
#[command(about = "PostgreSQL to Kafka CDC replicator", long_about = None)]
struct Args {
    #[arg(short, long, value_name = "FILE", default_value = "config.toml")]
    config: PathBuf,
    
    #[arg(short, long, help = "Enable JSON output for logs")]
    json_logs: bool,
    
    #[arg(short, long, help = "Verbose logging")]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    
    init_logging(args.json_logs, args.verbose);
    
    info!("Starting pg-replicate-kafka");
    info!("Loading configuration from {:?}", args.config);
    
    let config = match Config::from_file(&args.config) {
        Ok(cfg) => {
            info!("Configuration loaded successfully");
            cfg
        }
        Err(e) => {
            error!("Failed to load configuration: {}", e);
            return Err(Error::Config(e));
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
    
    // TODO: Implement replicator
    error!("Replicator not yet implemented");
    
    Ok(())
}

fn init_logging(json: bool, verbose: bool) {
    let env_filter = if verbose {
        EnvFilter::new("pg_replicate_kafka=debug,info")
    } else {
        EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("pg_replicate_kafka=info,warn"))
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