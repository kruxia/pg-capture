use chrono::Utc;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ChangeOperation {
    Insert,
    Update,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeEvent {
    pub schema: String,
    pub table: String,
    pub op: ChangeOperation,
    pub ts_ms: i64,
    pub before: Option<serde_json::Value>,
    pub after: Option<serde_json::Value>,
    pub source: SourceMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceMetadata {
    pub version: String,
    pub connector: String,
    pub ts_ms: i64,
    pub db: String,
    pub schema: String,
    pub table: String,
    pub lsn: String,
    pub xid: Option<u32>,
}

impl SourceMetadata {
    pub fn new(db: String, schema: String, table: String, lsn: String) -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            connector: "pg-replicate-kafka".to_string(),
            ts_ms: Utc::now().timestamp_millis(),
            db,
            schema,
            table,
            lsn,
            xid: None,
        }
    }
}