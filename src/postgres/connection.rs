use bytes::{Buf, BufMut, Bytes, BytesMut};
use fallible_iterator::FallibleIterator;
use postgres_protocol::message::backend::Message;
use postgres_protocol::message::{backend, frontend};
use std::time::{Duration, SystemTime};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::net::TcpStream;
use tokio::time::interval;
use tokio_postgres::{Config, NoTls, SimpleQueryMessage};
use tracing::{debug, error, info, warn};

use crate::{Error, Result};

// CopyBoth Protocol Implementation
// ================================
// This implementation uses a hybrid approach:
// 1. tokio-postgres for connection setup and replication commands
// 2. postgres-protocol crate for low-level CopyBoth message handling
// 3. Direct socket access for receiving replication messages
//
// The CopyBoth protocol is used for PostgreSQL logical replication:
// - Server sends CopyBothResponse after START_REPLICATION
// - Client and server can exchange CopyData messages
// - Replication messages are wrapped in CopyData messages
// - Client must send periodic standby status updates
pub struct ReplicationConnection {
    client: tokio_postgres::Client,
    connection_task: tokio::task::JoinHandle<()>,
    slot_name: String,
    publication_name: String,
    replication_started: bool,
    keepalive_task: Option<tokio::task::JoinHandle<()>>,
    // For CopyBoth protocol handling
    replication_stream: Option<ReplicationStream>,
    connection_string: String,
}

// Direct socket connection for replication
struct ReplicationStream {
    reader: BufReader<tokio::io::ReadHalf<TcpStream>>,
    writer: BufWriter<tokio::io::WriteHalf<TcpStream>>,
}

impl ReplicationStream {
    async fn new(config: &Config) -> Result<Self> {
        let host = &config.get_hosts()[0];
        let port = config.get_ports()[0];

        let host_str = match host {
            tokio_postgres::config::Host::Tcp(hostname) => hostname.clone(),
            tokio_postgres::config::Host::Unix(_) => {
                return Err(Error::Connection(
                    "Unix sockets not supported for replication".to_string(),
                ));
            }
        };

        let socket = TcpStream::connect((host_str, port)).await?;

        // Split socket into read and write halves
        let (read_half, write_half) = tokio::io::split(socket);

        Ok(Self {
            reader: BufReader::new(read_half),
            writer: BufWriter::new(write_half),
        })
    }

    async fn authenticate(&mut self, config: &Config) -> Result<()> {
        // Send startup message
        let user = config.get_user().unwrap_or("postgres");
        let dbname = config.get_dbname().unwrap_or("postgres");

        let mut startup_params = Vec::new();
        startup_params.push(("user", user));
        startup_params.push(("database", dbname));
        startup_params.push(("replication", "database"));

        let mut buf = BytesMut::new();
        frontend::startup_message(startup_params, &mut buf)?;
        self.write_message(&buf.freeze()).await?;

        // Handle authentication flow
        loop {
            let message = self.read_message().await?;
            match message {
                Message::AuthenticationOk => {
                    info!("Authentication successful");
                    break;
                }
                Message::AuthenticationCleartextPassword => {
                    if let Some(password) = config.get_password() {
                        let mut buf = BytesMut::new();
                        frontend::password_message(password, &mut buf)?;
                        self.write_message(&buf.freeze()).await?;
                    } else {
                        return Err(Error::Authentication("Password required".to_string()));
                    }
                }
                Message::ErrorResponse(err) => {
                    let mut fields = Vec::new();
                    let mut field_iter = err.fields();
                    while let Ok(Some(field)) = field_iter.next() {
                        fields.push((
                            field.type_().to_string(),
                            String::from_utf8_lossy(field.value_bytes()).to_string(),
                        ));
                    }
                    return Err(Error::Authentication(format!("Auth error: {:?}", fields)));
                }
                _ => {
                    debug!("Ignoring auth message");
                }
            }
        }

        // Wait for ReadyForQuery
        loop {
            let message = self.read_message().await?;
            match message {
                Message::ReadyForQuery(_) => {
                    info!("Connection ready for queries");
                    break;
                }
                Message::ParameterStatus(_) => {
                    debug!("Parameter status received");
                }
                Message::BackendKeyData(_) => {
                    debug!("Backend key data received");
                }
                _ => {
                    debug!("Ignoring startup message");
                }
            }
        }

        Ok(())
    }

    async fn write_message(&mut self, message: &Bytes) -> Result<()> {
        self.writer.write_all(message).await?;
        self.writer.flush().await?;
        Ok(())
    }

    async fn read_message(&mut self) -> Result<Message> {
        // Read message length
        let mut length_buf = [0u8; 5];
        self.reader.read_exact(&mut length_buf).await?;

        let tag = length_buf[0];
        let length =
            u32::from_be_bytes([length_buf[1], length_buf[2], length_buf[3], length_buf[4]]);

        // Read message body
        let mut body = vec![0u8; (length - 4) as usize];
        self.reader.read_exact(&mut body).await?;

        // Parse message
        let mut full_message = Vec::with_capacity(5 + body.len());
        full_message.push(tag);
        full_message.extend_from_slice(&(length.to_be_bytes()));
        full_message.extend_from_slice(&body);

        let mut buf = BytesMut::from(&full_message[..]);
        backend::Message::parse(&mut buf)
            .map_err(|e| Error::Protocol(format!("Failed to parse message: {}", e)))?
            .ok_or_else(|| Error::Protocol("Incomplete message".to_string()))
    }
}

// CopyBoth protocol message types
#[derive(Debug)]
pub enum CopyBothMessage {
    XLogData {
        wal_start: u64,
        wal_end: u64,
        timestamp: i64,
        data: Bytes,
    },
    PrimaryKeepalive {
        wal_end: u64,
        timestamp: i64,
        reply_requested: bool,
    },
}

impl ReplicationConnection {
    pub async fn new(
        connection_string: &str,
        slot_name: String,
        publication_name: String,
    ) -> Result<Self> {
        info!("Creating replication connection to PostgreSQL");

        // Parse connection string and add replication parameter
        // For replication mode, we need to add replication=database to the options
        let replication_string = if connection_string.contains("replication=") {
            connection_string.to_string()
        } else if connection_string.contains("?") {
            format!("{}&replication=database", connection_string)
        } else {
            format!("{}?replication=database", connection_string)
        };
        let config = replication_string.parse::<Config>()?;

        let (client, connection) = config.connect(NoTls).await?;

        let connection_task = tokio::spawn(async move {
            if let Err(e) = connection.await {
                error!("Connection error: {}", e);
            }
        });

        info!("Successfully connected to PostgreSQL in replication mode");

        Ok(Self {
            client,
            connection_task,
            slot_name,
            publication_name,
            replication_started: false,
            keepalive_task: None,
            replication_stream: None,
            connection_string: connection_string.to_string(),
        })
    }

    pub async fn create_replication_slot(&mut self) -> Result<()> {
        info!("Creating replication slot: {}", self.slot_name);

        let query = format!(
            "CREATE_REPLICATION_SLOT {} LOGICAL pgoutput NOEXPORT_SNAPSHOT",
            self.slot_name
        );

        match self.client.simple_query(&query).await {
            Ok(messages) => {
                for message in messages {
                    if let SimpleQueryMessage::Row(row) = message {
                        let slot_name = row.get("slot_name").unwrap_or("unknown");
                        let lsn = row.get("consistent_point").unwrap_or("unknown");
                        info!("Created replication slot '{}' at LSN {}", slot_name, lsn);
                    }
                }
                Ok(())
            }
            Err(e) => {
                if e.to_string().contains("already exists") {
                    info!("Replication slot '{}' already exists", self.slot_name);
                    Ok(())
                } else {
                    Err(Error::Postgres(e))
                }
            }
        }
    }

    pub async fn drop_replication_slot(&mut self) -> Result<()> {
        info!("Dropping replication slot: {}", self.slot_name);

        let query = format!("DROP_REPLICATION_SLOT {}", self.slot_name);

        match self.client.simple_query(&query).await {
            Ok(_) => {
                info!("Dropped replication slot '{}'", self.slot_name);
                Ok(())
            }
            Err(e) => {
                if e.to_string().contains("does not exist") {
                    warn!("Replication slot '{}' does not exist", self.slot_name);
                    Ok(())
                } else {
                    Err(Error::Postgres(e))
                }
            }
        }
    }

    pub async fn identify_system(&mut self) -> Result<SystemInfo> {
        debug!("Sending IDENTIFY_SYSTEM command");

        let rows = self.client.simple_query("IDENTIFY_SYSTEM").await?;

        for message in rows {
            if let SimpleQueryMessage::Row(row) = message {
                let system_id = row.get("systemid").unwrap_or("unknown").to_string();
                let timeline = row
                    .get("timeline")
                    .unwrap_or("1")
                    .parse::<i32>()
                    .unwrap_or(1);
                let xlogpos = row.get("xlogpos").unwrap_or("0/0").to_string();
                let dbname = row.get("dbname").map(|s| s.to_string());

                let info = SystemInfo {
                    system_id,
                    timeline,
                    xlogpos,
                    dbname,
                };

                debug!("System info: {:?}", info);
                return Ok(info);
            }
        }

        Err(Error::Replication {
            message: "Failed to get system info".to_string(),
        })
    }

    pub async fn start_replication(&mut self, start_lsn: Option<String>) -> Result<()> {
        let lsn = start_lsn.unwrap_or_else(|| "0/0".to_string());

        info!("Starting replication from LSN: {}", lsn);

        // Create a separate replication stream for CopyBoth protocol
        let replication_string = if self.connection_string.contains("replication=") {
            self.connection_string.clone()
        } else if self.connection_string.contains("?") {
            format!("{}&replication=database", self.connection_string)
        } else {
            format!("{}?replication=database", self.connection_string)
        };
        let config = replication_string.parse::<tokio_postgres::Config>()?;

        let mut stream = ReplicationStream::new(&config).await?;
        stream.authenticate(&config).await?;

        let options = format!(
            "proto_version '1', publication_names '{}'",
            self.publication_name
        );

        let query = format!(
            "START_REPLICATION SLOT {} LOGICAL {} ({})",
            self.slot_name, lsn, options
        );

        // Send START_REPLICATION command through the replication stream
        let mut buf = BytesMut::new();
        frontend::query(&query, &mut buf)?;
        stream.write_message(&buf.freeze()).await?;

        // Wait for CopyBothResponse
        let response = stream.read_message().await?;
        match response {
            Message::CopyOutResponse(_) => {
                info!("Received CopyOutResponse - replication stream active");
                self.replication_started = true;
                self.replication_stream = Some(stream);
                Ok(())
            }
            Message::ErrorResponse(err) => {
                error!("Error starting replication");
                Err(Error::Replication {
                    message: {
                        let mut fields = Vec::new();
                        let mut field_iter = err.fields();
                        while let Ok(Some(field)) = field_iter.next() {
                            fields.push((
                                field.type_().to_string(),
                                String::from_utf8_lossy(field.value_bytes()).to_string(),
                            ));
                        }
                        format!("Failed to start replication: {:?}", fields)
                    },
                })
            }
            _ => {
                error!("Unexpected response to START_REPLICATION");
                Err(Error::Replication {
                    message: "Unexpected response to START_REPLICATION".to_string(),
                })
            }
        }
    }

    pub async fn recv_replication_message(&mut self) -> Result<Option<ReplicationMessage>> {
        if !self.replication_started {
            return Err(Error::Replication {
                message: "Replication not started".to_string(),
            });
        }

        if self.replication_stream.is_none() {
            return Err(Error::Replication {
                message: "No replication stream available".to_string(),
            });
        }

        self.recv_copyboth_message().await
    }

    async fn recv_copyboth_message(&mut self) -> Result<Option<ReplicationMessage>> {
        debug!("Waiting for CopyBoth message from PostgreSQL");

        loop {
            let message = self
                .replication_stream
                .as_mut()
                .unwrap()
                .read_message()
                .await?;

            match message {
                Message::CopyData(data) => {
                    let data_bytes = data.data();
                    debug!("Received CopyData message with {} bytes", data_bytes.len());

                    // Parse the CopyBoth message inside CopyData
                    if let Some(copyboth_msg) = self.parse_copyboth_message(data_bytes)? {
                        match copyboth_msg {
                            CopyBothMessage::XLogData {
                                wal_start,
                                wal_end,
                                timestamp,
                                data,
                            } => {
                                debug!("Received XLogData: start={}, end={}, timestamp={}, data_len={}", 
                                       wal_start, wal_end, timestamp, data.len());

                                return Ok(Some(ReplicationMessage {
                                    data,
                                    timestamp: SystemTime::now(), // Convert from PostgreSQL timestamp if needed
                                    wal_start,
                                    wal_end,
                                }));
                            }
                            CopyBothMessage::PrimaryKeepalive {
                                wal_end,
                                timestamp,
                                reply_requested,
                            } => {
                                debug!("Received primary keepalive: wal_end={}, timestamp={}, reply_requested={}", 
                                       wal_end, timestamp, reply_requested);

                                if reply_requested {
                                    self.send_standby_status_update(wal_end).await?;
                                }

                                // Continue reading for next message
                                continue;
                            }
                        }
                    }
                }
                Message::ErrorResponse(err) => {
                    error!("Error in replication stream");
                    return Err(Error::Replication {
                        message: {
                            let mut fields = Vec::new();
                            let mut field_iter = err.fields();
                            while let Ok(Some(field)) = field_iter.next() {
                                fields.push((
                                    field.type_().to_string(),
                                    String::from_utf8_lossy(field.value_bytes()).to_string(),
                                ));
                            }
                            format!("Replication error: {:?}", fields)
                        },
                    });
                }
                _ => {
                    debug!("Ignoring non-CopyData message");
                    continue;
                }
            }
        }
    }

    #[allow(dead_code)]
    fn parse_copyboth_message(&self, data: &[u8]) -> Result<Option<CopyBothMessage>> {
        if data.is_empty() {
            return Ok(None);
        }

        let message_type = data[0];
        let mut cursor = &data[1..];

        match message_type {
            b'w' => {
                // XLogData message
                if cursor.len() < 24 {
                    return Err(Error::InvalidMessage {
                        message: "XLogData message too short".to_string(),
                    });
                }

                let wal_start = cursor.get_u64();
                let wal_end = cursor.get_u64();
                let timestamp = cursor.get_i64();
                let data = Bytes::copy_from_slice(cursor);

                Ok(Some(CopyBothMessage::XLogData {
                    wal_start,
                    wal_end,
                    timestamp,
                    data,
                }))
            }
            b'k' => {
                // Primary keepalive message
                if cursor.len() < 17 {
                    return Err(Error::InvalidMessage {
                        message: "Primary keepalive message too short".to_string(),
                    });
                }

                let wal_end = cursor.get_u64();
                let timestamp = cursor.get_i64();
                let reply_requested = cursor.get_u8() != 0;

                Ok(Some(CopyBothMessage::PrimaryKeepalive {
                    wal_end,
                    timestamp,
                    reply_requested,
                }))
            }
            _ => {
                debug!("Unknown CopyBoth message type: {}", message_type);
                Ok(None)
            }
        }
    }

    pub async fn send_standby_status_update(&mut self, lsn: u64) -> Result<()> {
        if !self.replication_started {
            return Err(Error::Replication {
                message: "No active replication stream".to_string(),
            });
        }

        // Create standby status update message first
        let message_data = self.create_standby_status_update(lsn);

        // Wrap in CopyData message
        let mut buf = BytesMut::new();
        buf.put_u8(b'd'); // CopyData message
        buf.put_i32((message_data.len() + 4) as i32); // message length
        buf.extend_from_slice(&message_data);

        debug!(
            "Sending standby status update for LSN {} ({} bytes)",
            lsn,
            message_data.len()
        );

        // Now get the stream and send the message
        let stream = self
            .replication_stream
            .as_mut()
            .ok_or_else(|| Error::Replication {
                message: "No replication stream available".to_string(),
            })?;

        stream.write_message(&buf.freeze()).await?;

        Ok(())
    }

    fn create_standby_status_update(&self, lsn: u64) -> Vec<u8> {
        let mut buf = Vec::new();

        // Standby status update message format:
        // 1 byte: message type ('r' for standby status update)
        // 8 bytes: WAL position of last received data
        // 8 bytes: WAL position of last flushed data
        // 8 bytes: WAL position of last applied data
        // 8 bytes: timestamp
        // 1 byte: reply requested flag

        buf.push(b'r'); // Message type
        buf.extend_from_slice(&lsn.to_be_bytes()); // Received LSN
        buf.extend_from_slice(&lsn.to_be_bytes()); // Flushed LSN
        buf.extend_from_slice(&lsn.to_be_bytes()); // Applied LSN

        // Current timestamp (PostgreSQL timestamp format)
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_micros() as i64;
        buf.extend_from_slice(&now.to_be_bytes());

        buf.push(0); // Reply not requested

        buf
    }

    pub async fn send_keepalive(&mut self) -> Result<()> {
        if !self.replication_started {
            return Err(Error::Replication {
                message: "No active replication stream".to_string(),
            });
        }

        // Send a standby status update as keepalive
        // In a real implementation, we would track the last received LSN
        let last_lsn = 0; // Would be tracked from received messages
        self.send_standby_status_update(last_lsn).await
    }

    pub fn start_keepalive_sender(&mut self, interval_duration: Duration) -> Result<()> {
        if self.keepalive_task.is_some() {
            return Ok(());
        }

        info!(
            "Starting keepalive sender with interval: {:?}",
            interval_duration
        );

        let keepalive_task = tokio::spawn(async move {
            let mut ticker = interval(interval_duration);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                ticker.tick().await;
                debug!("Keepalive interval tick - sender will be implemented with shared state");
            }
        });

        self.keepalive_task = Some(keepalive_task);
        Ok(())
    }

    pub async fn close(mut self) -> Result<()> {
        info!("Closing replication connection");

        if let Some(task) = self.keepalive_task.take() {
            task.abort();
        }

        // Note: In a real implementation with CopyBoth, we would close the stream here

        self.connection_task.abort();
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct SystemInfo {
    pub system_id: String,
    pub timeline: i32,
    pub xlogpos: String,
    pub dbname: Option<String>,
}

#[derive(Debug)]
pub struct ReplicationMessage {
    pub data: Bytes,
    pub timestamp: SystemTime,
    pub wal_start: u64,
    pub wal_end: u64,
}

impl ReplicationMessage {
    pub fn format_lsn(lsn: u64) -> String {
        format!("{:X}/{:X}", lsn >> 32, lsn & 0xFFFFFFFF)
    }

    pub fn wal_start_str(&self) -> String {
        Self::format_lsn(self.wal_start)
    }

    pub fn wal_end_str(&self) -> String {
        Self::format_lsn(self.wal_end)
    }
}
