use bytes::Buf;
use std::collections::HashMap;
use tracing::{debug, trace};

use crate::{Error, Result};
use super::types::{ChangeEvent, ChangeOperation, SourceMetadata};

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

        debug!("RELATION: {}={}.{}", rel_id, relation.schema, relation.table);
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

        let relation = self.relations.get(&rel_id).ok_or_else(|| Error::InvalidMessage {
            message: format!("Unknown relation ID: {}", rel_id),
        })?;

        let tuple_data = self.decode_tuple_data(cursor, &relation.columns)?;

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
            ).with_xid(self.current_xid.unwrap_or(0)),
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
        let relation = self.relations.get(&rel_id).ok_or_else(|| Error::InvalidMessage {
            message: format!("Unknown relation ID: {}", rel_id),
        })?;

        let mut old_tuple = None;
        let mut new_tuple = None;

        // Check for old tuple
        if cursor.remaining() > 0 {
            let tuple_type = cursor.get_u8();
            match tuple_type {
                b'O' | b'K' => {
                    old_tuple = Some(self.decode_tuple_data(cursor, &relation.columns)?);
                    // Check for new tuple
                    if cursor.remaining() > 0 {
                        let next_type = cursor.get_u8();
                        if next_type == b'N' {
                            new_tuple = Some(self.decode_tuple_data(cursor, &relation.columns)?);
                        }
                    }
                }
                b'N' => {
                    new_tuple = Some(self.decode_tuple_data(cursor, &relation.columns)?);
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
            ).with_xid(self.current_xid.unwrap_or(0)),
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

        let relation = self.relations.get(&rel_id).ok_or_else(|| Error::InvalidMessage {
            message: format!("Unknown relation ID: {}", rel_id),
        })?;

        let old_tuple = self.decode_tuple_data(cursor, &relation.columns)?;

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
            ).with_xid(self.current_xid.unwrap_or(0)),
        };

        Ok(Some(DecodedMessage::Change(event)))
    }

    fn decode_truncate(&mut self, _cursor: &[u8]) -> Result<Option<DecodedMessage>> {
        // For MVP, we'll just log truncate operations
        debug!("TRUNCATE operation received (not implemented in MVP)");
        Ok(None)
    }

    fn decode_tuple_data(&self, mut cursor: &[u8], columns: &[ColumnInfo]) -> Result<serde_json::Value> {
        let num_columns = cursor.get_u16();
        
        if num_columns as usize != columns.len() {
            return Err(Error::InvalidMessage {
                message: format!("Column count mismatch: {} vs {}", num_columns, columns.len()),
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
                    // Binary value (not implemented in MVP)
                    return Err(Error::InvalidMessage {
                        message: "Binary values not supported in MVP".to_string(),
                    });
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

fn parse_postgres_value(text: &str, type_id: u32) -> serde_json::Value {
    // Common PostgreSQL type OIDs
    match type_id {
        16 => {  // bool
            match text {
                "t" => serde_json::Value::Bool(true),
                "f" => serde_json::Value::Bool(false),
                _ => serde_json::Value::String(text.to_string()),
            }
        }
        20 | 21 | 23 => {  // int8, int2, int4
            text.parse::<i64>()
                .map(serde_json::Value::from)
                .unwrap_or_else(|_| serde_json::Value::String(text.to_string()))
        }
        700 | 701 => {  // float4, float8
            text.parse::<f64>()
                .map(serde_json::Value::from)
                .unwrap_or_else(|_| serde_json::Value::String(text.to_string()))
        }
        1184 => {  // timestamptz
            // Keep as string for now, let consumers parse
            serde_json::Value::String(text.to_string())
        }
        _ => {
            // Default: keep as string
            serde_json::Value::String(text.to_string())
        }
    }
}

#[derive(Debug)]
pub enum DecodedMessage {
    Begin { xid: u32 },
    Commit,
    Change(ChangeEvent),
}