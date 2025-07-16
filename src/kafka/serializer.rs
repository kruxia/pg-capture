use crate::{postgres::ChangeEvent, Result};
use serde::{Deserialize, Serialize};
use serde_json::{self, Value};
use tracing::{debug, instrument};

#[derive(Debug, Clone, Default)]
pub enum SerializationFormat {
    #[default]
    Json,
    JsonCompact,
    JsonDebezium,
}

pub trait EventSerializer {
    fn serialize(&self, event: &ChangeEvent) -> Result<String>;
}

pub struct JsonSerializer {
    format: SerializationFormat,
}

impl JsonSerializer {
    pub fn new(format: SerializationFormat) -> Self {
        Self { format }
    }

    pub fn serialize(event: &ChangeEvent) -> Result<String> {
        serde_json::to_string(event).map_err(Into::into)
    }
}

impl EventSerializer for JsonSerializer {
    #[instrument(skip(self, event), fields(op = ?event.op, table = %event.table))]
    fn serialize(&self, event: &ChangeEvent) -> Result<String> {
        debug!("Serializing event with format: {:?}", self.format);

        let result = match &self.format {
            SerializationFormat::Json => serde_json::to_string_pretty(event),
            SerializationFormat::JsonCompact => serde_json::to_string(event),
            SerializationFormat::JsonDebezium => serialize_debezium_format(event),
        };

        result.map_err(|e| e.into())
    }
}

#[derive(Serialize, Deserialize)]
struct DebeziumEnvelope {
    schema: Option<Value>,
    payload: DebeziumPayload,
}

#[derive(Serialize, Deserialize)]
struct DebeziumPayload {
    before: Option<Value>,
    after: Option<Value>,
    source: DebeziumSource,
    op: String,
    ts_ms: i64,
    transaction: Option<DebeziumTransaction>,
}

#[derive(Serialize, Deserialize)]
struct DebeziumSource {
    version: String,
    connector: String,
    name: String,
    ts_ms: i64,
    snapshot: String,
    db: String,
    schema: String,
    table: String,
    lsn: Option<i64>,
    xmin: Option<i64>,
}

#[derive(Serialize, Deserialize)]
struct DebeziumTransaction {
    id: String,
    total_order: i64,
    data_collection_order: i64,
}

fn serialize_debezium_format(event: &ChangeEvent) -> serde_json::Result<String> {
    let op_str = match &event.op {
        crate::postgres::ChangeOperation::Insert => "c",
        crate::postgres::ChangeOperation::Update => "u",
        crate::postgres::ChangeOperation::Delete => "d",
    };

    let lsn_numeric = event
        .source
        .lsn
        .strip_prefix("0/")
        .and_then(|s| i64::from_str_radix(s, 16).ok());

    let source = DebeziumSource {
        version: event.source.version.clone(),
        connector: "postgresql".to_string(),
        name: format!("{}.{}", event.source.db, event.source.schema),
        ts_ms: event.source.ts_ms,
        snapshot: "false".to_string(),
        db: event.source.db.clone(),
        schema: event.source.schema.clone(),
        table: event.source.table.clone(),
        lsn: lsn_numeric,
        xmin: None,
    };

    let transaction = event.source.xid.map(|xid| DebeziumTransaction {
        id: xid.to_string(),
        total_order: 1,
        data_collection_order: 1,
    });

    let payload = DebeziumPayload {
        before: event.before.clone(),
        after: event.after.clone(),
        source,
        op: op_str.to_string(),
        ts_ms: event.ts_ms,
        transaction,
    };

    let envelope = DebeziumEnvelope {
        schema: None,
        payload,
    };

    serde_json::to_string(&envelope)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::postgres::{ChangeOperation, SourceMetadata};
    use serde_json::json;

    fn create_test_event() -> ChangeEvent {
        ChangeEvent {
            schema: "public".to_string(),
            table: "users".to_string(),
            op: ChangeOperation::Insert,
            ts_ms: 1234567890,
            before: None,
            after: Some(json!({
                "id": 1,
                "name": "Test User"
            })),
            source: SourceMetadata::new(
                "testdb".to_string(),
                "public".to_string(),
                "users".to_string(),
                "0/1234ABC".to_string(),
            )
            .with_xid(12345),
        }
    }

    #[test]
    fn test_json_serialization() {
        let event = create_test_event();
        let serializer = JsonSerializer::new(SerializationFormat::JsonCompact);

        let result = serializer.serialize(&event);
        assert!(result.is_ok());

        let json: serde_json::Value = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(json["op"], "INSERT");
        assert_eq!(json["table"], "users");
    }

    #[test]
    fn test_debezium_format() {
        let event = create_test_event();
        let serializer = JsonSerializer::new(SerializationFormat::JsonDebezium);

        let result = serializer.serialize(&event);
        assert!(result.is_ok());

        let json: serde_json::Value = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(json["payload"]["op"], "c");
        assert_eq!(json["payload"]["source"]["connector"], "postgresql");
    }
}
