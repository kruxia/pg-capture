use bytes::Buf;
use std::collections::HashMap;
use tracing::{debug, trace};

use super::types::{ChangeEvent, ChangeOperation, SourceMetadata};
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct RelationInfo {
    pub id: u32,
    pub schema: String,
    pub table: String,
    pub columns: Vec<ColumnInfo>,
}

#[derive(Debug, Clone)]
pub struct ColumnInfo {
    pub name: String,
    pub type_id: u32,
    pub is_key: bool,
}

pub struct PgOutputDecoder {
    relations: HashMap<u32, RelationInfo>,
    current_lsn: Option<String>,
    current_xid: Option<u32>,
    database_name: String,
}

impl PgOutputDecoder {
    pub fn new(database_name: String) -> Self {
        Self {
            relations: HashMap::new(),
            current_lsn: None,
            current_xid: None,
            database_name,
        }
    }

    pub fn decode(&mut self, data: &[u8]) -> Result<Option<DecodedMessage>> {
        if data.is_empty() {
            return Ok(None);
        }

        // Skip the 'w' tag that marks XLogData
        if data[0] != b'w' {
            return Ok(None);
        }

        let mut cursor = &data[1..];

        // Read XLogData header
        if cursor.remaining() < 24 {
            return Err(Error::InvalidMessage {
                message: "Invalid XLogData header size".to_string(),
            });
        }

        let _start_lsn = cursor.get_u64();
        let end_lsn = cursor.get_u64();
        let _timestamp = cursor.get_i64();

        self.current_lsn = Some(format_lsn(end_lsn));

        // Now decode the actual pgoutput message
        if cursor.is_empty() {
            return Ok(None);
        }

        let msg_type = cursor.get_u8();

        match msg_type {
            b'B' => self.decode_begin(cursor),
            b'C' => self.decode_commit(cursor),
            b'R' => self.decode_relation(cursor),
            b'I' => self.decode_insert(cursor),
            b'U' => self.decode_update(cursor),
            b'D' => self.decode_delete(cursor),
            b'T' => self.decode_truncate(cursor),
            _ => {
                debug!("Unknown pgoutput message type: {}", msg_type as char);
                Ok(None)
            }
        }
    }

    fn decode_begin(&mut self, mut cursor: &[u8]) -> Result<Option<DecodedMessage>> {
        if cursor.remaining() < 20 {
            return Err(Error::InvalidMessage {
                message: "Invalid BEGIN message size".to_string(),
            });
        }

        let final_lsn = cursor.get_u64();
        let _timestamp = cursor.get_i64();
        let xid = cursor.get_u32();

        self.current_xid = Some(xid);

        trace!("BEGIN: lsn={}, xid={}", format_lsn(final_lsn), xid);
        Ok(Some(DecodedMessage::Begin { xid }))
    }

    fn decode_commit(&mut self, mut cursor: &[u8]) -> Result<Option<DecodedMessage>> {
        if cursor.remaining() < 25 {
            return Err(Error::InvalidMessage {
                message: "Invalid COMMIT message size".to_string(),
            });
        }

        let _flags = cursor.get_u8();
        let _commit_lsn = cursor.get_u64();
        let end_lsn = cursor.get_u64();
        let _timestamp = cursor.get_i64();

        trace!("COMMIT: lsn={}", format_lsn(end_lsn));
        Ok(Some(DecodedMessage::Commit))
    }

    fn decode_relation(&mut self, mut cursor: &[u8]) -> Result<Option<DecodedMessage>> {
        if cursor.remaining() < 7 {
            return Err(Error::InvalidMessage {
                message: "Invalid RELATION message size".to_string(),
            });
        }

        let rel_id = cursor.get_u32();
        let namespace_len = cursor.get_u8() as usize;

        if cursor.remaining() < namespace_len {
            return Err(Error::InvalidMessage {
                message: "Invalid namespace length".to_string(),
            });
        }

        let schema = String::from_utf8_lossy(&cursor[..namespace_len]).to_string();
        cursor.advance(namespace_len);

        let table_len = cursor.get_u8() as usize;
        if cursor.remaining() < table_len {
            return Err(Error::InvalidMessage {
                message: "Invalid table name length".to_string(),
            });
        }

        let table = String::from_utf8_lossy(&cursor[..table_len]).to_string();
        cursor.advance(table_len);

        let _replica_identity = cursor.get_u8();
        let num_columns = cursor.get_u16();

        let mut columns = Vec::with_capacity(num_columns as usize);

        for _ in 0..num_columns {
            let flags = cursor.get_u8();
            let is_key = (flags & 1) != 0;

            let col_name_len = cursor.get_u8() as usize;
            if cursor.remaining() < col_name_len {
                return Err(Error::InvalidMessage {
                    message: format!(
                        "Invalid column name length: {} (remaining: {})",
                        col_name_len,
                        cursor.remaining()
                    ),
                });
            }
            let col_name = String::from_utf8_lossy(&cursor[..col_name_len]).to_string();
            cursor.advance(col_name_len);

            let type_id = cursor.get_u32();
            let _type_modifier = cursor.get_i32();

            columns.push(ColumnInfo {
                name: col_name,
                type_id,
                is_key,
            });
        }

        let relation = RelationInfo {
            id: rel_id,
            schema,
            table,
            columns,
        };

        debug!(
            "RELATION: {}={}.{}",
            rel_id, relation.schema, relation.table
        );
        self.relations.insert(rel_id, relation);

        Ok(None)
    }

    fn decode_insert(&mut self, mut cursor: &[u8]) -> Result<Option<DecodedMessage>> {
        if cursor.remaining() < 6 {
            return Err(Error::InvalidMessage {
                message: "Invalid INSERT message size".to_string(),
            });
        }

        let rel_id = cursor.get_u32();
        let tuple_type = cursor.get_u8();

        if tuple_type != b'N' {
            return Err(Error::InvalidMessage {
                message: format!("Unexpected tuple type in INSERT: {}", tuple_type),
            });
        }

        let relation = self
            .relations
            .get(&rel_id)
            .ok_or_else(|| Error::InvalidMessage {
                message: format!("Unknown relation ID: {}", rel_id),
            })?;

        let tuple_data = self.decode_tuple_data(&mut cursor, &relation.columns)?;

        let event = ChangeEvent {
            schema: relation.schema.clone(),
            table: relation.table.clone(),
            op: ChangeOperation::Insert,
            ts_ms: chrono::Utc::now().timestamp_millis(),
            before: None,
            after: Some(tuple_data),
            source: SourceMetadata::new(
                self.database_name.clone(),
                relation.schema.clone(),
                relation.table.clone(),
                self.current_lsn.clone().unwrap_or_default(),
            )
            .with_xid(self.current_xid.unwrap_or(0)),
        };

        Ok(Some(DecodedMessage::Change(event)))
    }

    fn decode_update(&mut self, mut cursor: &[u8]) -> Result<Option<DecodedMessage>> {
        if cursor.remaining() < 5 {
            return Err(Error::InvalidMessage {
                message: "Invalid UPDATE message size".to_string(),
            });
        }

        let rel_id = cursor.get_u32();
        let relation = self
            .relations
            .get(&rel_id)
            .ok_or_else(|| Error::InvalidMessage {
                message: format!("Unknown relation ID: {}", rel_id),
            })?;

        let mut old_tuple = None;
        let mut new_tuple = None;

        // Check for old tuple
        if cursor.remaining() > 0 {
            let tuple_type = cursor.get_u8();
            match tuple_type {
                b'O' | b'K' => {
                    old_tuple = Some(self.decode_tuple_data(&mut cursor, &relation.columns)?);
                    // Check for new tuple
                    if cursor.remaining() > 0 {
                        let next_type = cursor.get_u8();
                        if next_type == b'N' {
                            new_tuple =
                                Some(self.decode_tuple_data(&mut cursor, &relation.columns)?);
                        }
                    }
                }
                b'N' => {
                    new_tuple = Some(self.decode_tuple_data(&mut cursor, &relation.columns)?);
                }
                _ => {
                    return Err(Error::InvalidMessage {
                        message: format!("Unexpected tuple type in UPDATE: {}", tuple_type),
                    });
                }
            }
        }

        let event = ChangeEvent {
            schema: relation.schema.clone(),
            table: relation.table.clone(),
            op: ChangeOperation::Update,
            ts_ms: chrono::Utc::now().timestamp_millis(),
            before: old_tuple,
            after: new_tuple,
            source: SourceMetadata::new(
                self.database_name.clone(),
                relation.schema.clone(),
                relation.table.clone(),
                self.current_lsn.clone().unwrap_or_default(),
            )
            .with_xid(self.current_xid.unwrap_or(0)),
        };

        Ok(Some(DecodedMessage::Change(event)))
    }

    fn decode_delete(&mut self, mut cursor: &[u8]) -> Result<Option<DecodedMessage>> {
        if cursor.remaining() < 6 {
            return Err(Error::InvalidMessage {
                message: "Invalid DELETE message size".to_string(),
            });
        }

        let rel_id = cursor.get_u32();
        let tuple_type = cursor.get_u8();

        if tuple_type != b'O' && tuple_type != b'K' {
            return Err(Error::InvalidMessage {
                message: format!("Unexpected tuple type in DELETE: {}", tuple_type),
            });
        }

        let relation = self
            .relations
            .get(&rel_id)
            .ok_or_else(|| Error::InvalidMessage {
                message: format!("Unknown relation ID: {}", rel_id),
            })?;

        let old_tuple = self.decode_tuple_data(&mut cursor, &relation.columns)?;

        let event = ChangeEvent {
            schema: relation.schema.clone(),
            table: relation.table.clone(),
            op: ChangeOperation::Delete,
            ts_ms: chrono::Utc::now().timestamp_millis(),
            before: Some(old_tuple),
            after: None,
            source: SourceMetadata::new(
                self.database_name.clone(),
                relation.schema.clone(),
                relation.table.clone(),
                self.current_lsn.clone().unwrap_or_default(),
            )
            .with_xid(self.current_xid.unwrap_or(0)),
        };

        Ok(Some(DecodedMessage::Change(event)))
    }

    fn decode_truncate(&mut self, _cursor: &[u8]) -> Result<Option<DecodedMessage>> {
        // For MVP, we'll just log truncate operations
        debug!("TRUNCATE operation received (not implemented in MVP)");
        Ok(None)
    }

    fn decode_tuple_data(
        &self,
        cursor: &mut &[u8],
        columns: &[ColumnInfo],
    ) -> Result<serde_json::Value> {
        let num_columns = cursor.get_u16();

        if num_columns as usize != columns.len() {
            return Err(Error::InvalidMessage {
                message: format!(
                    "Column count mismatch: {} vs {}",
                    num_columns,
                    columns.len()
                ),
            });
        }

        let mut tuple = serde_json::Map::new();

        for column in columns.iter() {
            let col_type = cursor.get_u8();

            match col_type {
                b'n' => {
                    // NULL value
                    tuple.insert(column.name.clone(), serde_json::Value::Null);
                }
                b't' => {
                    // Text value
                    let len = cursor.get_u32() as usize;
                    if cursor.remaining() < len {
                        return Err(Error::InvalidMessage {
                            message: "Invalid text value length".to_string(),
                        });
                    }
                    let value = String::from_utf8_lossy(&cursor[..len]).to_string();
                    cursor.advance(len);

                    // Try to parse as appropriate type based on PostgreSQL type OID
                    let parsed_value = parse_postgres_value(&value, column.type_id);
                    tuple.insert(column.name.clone(), parsed_value);
                }
                b'b' => {
                    // Binary value
                    let value_len = cursor.get_i32();
                    if value_len > 0 {
                        let value_bytes = cursor.copy_to_bytes(value_len as usize);
                        let parsed_value =
                            parse_postgres_binary_value(&value_bytes, column.type_id);
                        tuple.insert(column.name.clone(), parsed_value);
                    } else {
                        // NULL value (indicated by -1 length)
                        tuple.insert(column.name.clone(), serde_json::Value::Null);
                    }
                }
                _ => {
                    return Err(Error::InvalidMessage {
                        message: format!("Unknown column type: {}", col_type),
                    });
                }
            }
        }

        Ok(serde_json::Value::Object(tuple))
    }
}

fn format_lsn(lsn: u64) -> String {
    format!("{:X}/{:X}", lsn >> 32, lsn & 0xFFFFFFFF)
}

pub fn parse_postgres_value(text: &str, type_id: u32) -> serde_json::Value {
    trace!(
        "Parsing PostgreSQL value with type OID {}: {}",
        type_id,
        text
    );

    // PostgreSQL type OIDs reference:
    // https://www.postgresql.org/docs/current/datatype-oid.html
    match type_id {
        // Boolean types
        16 => {
            // bool
            match text {
                "t" => serde_json::Value::Bool(true),
                "f" => serde_json::Value::Bool(false),
                _ => serde_json::Value::String(text.to_string()),
            }
        }

        // Numeric types
        20 => {
            // int8 (bigint)
            text.parse::<i64>()
                .map(serde_json::Value::from)
                .unwrap_or_else(|_| serde_json::Value::String(text.to_string()))
        }
        21 => {
            // int2 (smallint)
            text.parse::<i16>()
                .map(|v| serde_json::Value::from(v as i64))
                .unwrap_or_else(|_| serde_json::Value::String(text.to_string()))
        }
        23 => {
            // int4 (integer)
            text.parse::<i32>()
                .map(|v| serde_json::Value::from(v as i64))
                .unwrap_or_else(|_| serde_json::Value::String(text.to_string()))
        }
        26 => {
            // oid
            text.parse::<u32>()
                .map(|v| serde_json::Value::from(v as i64))
                .unwrap_or_else(|_| serde_json::Value::String(text.to_string()))
        }
        700 => {
            // float4 (real)
            text.parse::<f32>()
                .map(|v| serde_json::Value::from(v as f64))
                .unwrap_or_else(|_| serde_json::Value::String(text.to_string()))
        }
        701 => {
            // float8 (double precision)
            text.parse::<f64>()
                .map(serde_json::Value::from)
                .unwrap_or_else(|_| serde_json::Value::String(text.to_string()))
        }
        1700 => {
            // numeric/decimal
            // Parse as string to preserve precision
            // Consumers can parse with appropriate decimal library
            serde_json::Value::String(text.to_string())
        }

        // String types
        18 | 19 | 25 | 1042 | 1043 => {
            // char, name, text, bpchar (char(n)), varchar
            serde_json::Value::String(text.to_string())
        }

        // Date/Time types
        1082 => {
            // date
            serde_json::Value::String(text.to_string())
        }
        1083 => {
            // time
            serde_json::Value::String(text.to_string())
        }
        1114 => {
            // timestamp
            serde_json::Value::String(text.to_string())
        }
        1184 => {
            // timestamptz
            serde_json::Value::String(text.to_string())
        }
        1186 => {
            // interval
            serde_json::Value::String(text.to_string())
        }
        1266 => {
            // timetz
            serde_json::Value::String(text.to_string())
        }

        // UUID type
        2950 => {
            // uuid
            serde_json::Value::String(text.to_string())
        }

        // JSON types
        114 | 3802 => {
            // json, jsonb
            // Try to parse as JSON, fall back to string if invalid
            serde_json::from_str(text)
                .unwrap_or_else(|_| serde_json::Value::String(text.to_string()))
        }

        // Network types
        650 | 869 => {
            // cidr, inet
            serde_json::Value::String(text.to_string())
        }
        774 => {
            // macaddr
            serde_json::Value::String(text.to_string())
        }
        829 => {
            // macaddr8
            serde_json::Value::String(text.to_string())
        }

        // Array types (prefix with underscore)
        1000 => {
            // _bool
            parse_array_value(text, 16)
        }
        1005 => {
            // _int2
            parse_array_value(text, 21)
        }
        1007 => {
            // _int4
            parse_array_value(text, 23)
        }
        1016 => {
            // _int8
            parse_array_value(text, 20)
        }
        1009 => {
            // _text
            parse_array_value(text, 25)
        }
        1015 => {
            // _varchar
            parse_array_value(text, 1043)
        }
        1021 => {
            // _float4
            parse_array_value(text, 700)
        }
        1022 => {
            // _float8
            parse_array_value(text, 701)
        }

        // Other common types
        2278 => {
            // void
            serde_json::Value::Null
        }
        2249 => {
            // record
            // Keep as string for complex types
            serde_json::Value::String(text.to_string())
        }

        _ => {
            // Default: keep as string
            debug!("Unknown PostgreSQL type OID {}, keeping as string", type_id);
            serde_json::Value::String(text.to_string())
        }
    }
}

// Helper function to parse PostgreSQL array values
fn parse_array_value(text: &str, element_type_id: u32) -> serde_json::Value {
    // PostgreSQL array format: {elem1,elem2,elem3} or {"elem 1","elem 2"}
    if !text.starts_with('{') || !text.ends_with('}') {
        return serde_json::Value::String(text.to_string());
    }

    let inner = &text[1..text.len() - 1];
    if inner.is_empty() {
        return serde_json::Value::Array(vec![]);
    }

    // Simple parsing for common cases (doesn't handle all edge cases)
    let elements: Vec<serde_json::Value> = if inner.contains('"') {
        // Quoted elements - more complex parsing needed
        // For now, return as string
        return serde_json::Value::String(text.to_string());
    } else {
        // Simple comma-separated elements
        inner
            .split(',')
            .map(|elem| {
                if elem == "NULL" {
                    serde_json::Value::Null
                } else {
                    parse_postgres_value(elem, element_type_id)
                }
            })
            .collect()
    };

    serde_json::Value::Array(elements)
}

// Parse PostgreSQL binary format values
pub fn parse_postgres_binary_value(data: &[u8], type_id: u32) -> serde_json::Value {
    trace!("Parsing PostgreSQL binary value with type OID {}", type_id);

    // For MVP, we'll handle common binary types
    // Full binary parsing requires postgres-protocol crate
    match type_id {
        16 => {
            // bool
            if data.len() >= 1 {
                serde_json::Value::Bool(data[0] != 0)
            } else {
                serde_json::Value::Null
            }
        }
        20 => {
            // int8
            if data.len() >= 8 {
                let value = i64::from_be_bytes([
                    data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
                ]);
                serde_json::Value::from(value)
            } else {
                serde_json::Value::Null
            }
        }
        21 => {
            // int2
            if data.len() >= 2 {
                let value = i16::from_be_bytes([data[0], data[1]]);
                serde_json::Value::from(value as i64)
            } else {
                serde_json::Value::Null
            }
        }
        23 => {
            // int4
            if data.len() >= 4 {
                let value = i32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                serde_json::Value::from(value as i64)
            } else {
                serde_json::Value::Null
            }
        }
        700 => {
            // float4
            if data.len() >= 4 {
                let bits = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                let value = f32::from_bits(bits) as f64;
                serde_json::Value::from(value)
            } else {
                serde_json::Value::Null
            }
        }
        701 => {
            // float8
            if data.len() >= 8 {
                let bits = u64::from_be_bytes([
                    data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
                ]);
                let value = f64::from_bits(bits);
                serde_json::Value::from(value)
            } else {
                serde_json::Value::Null
            }
        }
        2950 => {
            // uuid
            if data.len() >= 16 {
                // Format as standard UUID string
                let uuid = format!(
                    "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
                    data[0], data[1], data[2], data[3],
                    data[4], data[5],
                    data[6], data[7],
                    data[8], data[9],
                    data[10], data[11], data[12], data[13], data[14], data[15]
                );
                serde_json::Value::String(uuid)
            } else {
                serde_json::Value::Null
            }
        }
        _ => {
            // For other types, encode as base64
            debug!(
                "Binary format for type OID {} not implemented, encoding as base64",
                type_id
            );
            use base64::{engine::general_purpose, Engine as _};
            let encoded = general_purpose::STANDARD.encode(data);
            serde_json::Value::String(format!("base64:{}", encoded))
        }
    }
}

#[derive(Debug)]
pub enum DecodedMessage {
    Begin { xid: u32 },
    Commit,
    Change(ChangeEvent),
}
