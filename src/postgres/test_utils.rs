use bytes::{Bytes, BytesMut, BufMut};
use std::collections::HashMap;
// use serde_json::{json, Value}; // Unused for now

/// Mock message builder for testing pgoutput decoder
pub struct MockMessageBuilder {
    lsn: u64,
    timestamp: i64,
    relations: HashMap<u32, MockRelation>,
}

#[derive(Debug, Clone)]
pub struct MockRelation {
    pub id: u32,
    pub schema: String,
    pub table: String,
    pub columns: Vec<MockColumn>,
}

#[derive(Debug, Clone)]
pub struct MockColumn {
    pub name: String,
    pub type_id: u32,
    pub is_key: bool,
}

impl MockMessageBuilder {
    pub fn new() -> Self {
        Self {
            lsn: 1000,
            timestamp: 1697369400000000, // 2023-10-15 10:30:00 UTC in microseconds
            relations: HashMap::new(),
        }
    }
    
    pub fn with_lsn(mut self, lsn: u64) -> Self {
        self.lsn = lsn;
        self
    }
    
    pub fn with_timestamp(mut self, timestamp: i64) -> Self {
        self.timestamp = timestamp;
        self
    }
    
    pub fn add_relation(mut self, id: u32, schema: &str, table: &str, columns: Vec<(&str, u32, bool)>) -> Self {
        let mock_columns = columns.into_iter()
            .map(|(name, type_id, is_key)| MockColumn {
                name: name.to_string(),
                type_id,
                is_key,
            })
            .collect();
        
        self.relations.insert(id, MockRelation {
            id,
            schema: schema.to_string(),
            table: table.to_string(),
            columns: mock_columns,
        });
        self
    }
    
    /// Create XLogData header for messages
    fn create_xlogdata_header(&self) -> BytesMut {
        let mut buf = BytesMut::new();
        buf.put_u8(b'w'); // XLogData tag
        buf.put_u64(self.lsn); // Start LSN
        buf.put_u64(self.lsn + 100); // End LSN
        buf.put_i64(self.timestamp); // Timestamp
        buf
    }
    
    /// Build a BEGIN message
    pub fn begin_message(&self, xid: u32) -> Bytes {
        let mut buf = self.create_xlogdata_header();
        buf.put_u8(b'B'); // BEGIN
        buf.put_u64(self.lsn); // Final LSN
        buf.put_i64(self.timestamp); // Timestamp
        buf.put_u32(xid); // Transaction ID
        buf.freeze()
    }
    
    /// Build a COMMIT message
    pub fn commit_message(&self) -> Bytes {
        let mut buf = self.create_xlogdata_header();
        buf.put_u8(b'C'); // COMMIT
        buf.put_u8(0); // Flags
        buf.put_u64(self.lsn); // Commit LSN
        buf.put_u64(self.lsn + 100); // End LSN
        buf.put_i64(self.timestamp); // Timestamp
        buf.freeze()
    }
    
    /// Build a RELATION message
    pub fn relation_message(&self, rel_id: u32) -> Bytes {
        let relation = self.relations.get(&rel_id)
            .expect("Relation not found. Use add_relation() first.");
        
        let mut buf = self.create_xlogdata_header();
        buf.put_u8(b'R'); // RELATION
        buf.put_u32(rel_id);
        
        // Schema name with length prefix
        buf.put_u8(relation.schema.len() as u8);
        buf.put(relation.schema.as_bytes());
        
        // Table name with length prefix  
        buf.put_u8(relation.table.len() as u8);
        buf.put(relation.table.as_bytes());
        
        buf.put_u8(b'n'); // replica identity (nothing)
        buf.put_u16(relation.columns.len() as u16);
        
        for column in &relation.columns {
            buf.put_u8(if column.is_key { 1 } else { 0 });
            
            // Column name with length prefix
            buf.put_u8(column.name.len() as u8);
            buf.put(column.name.as_bytes());
            
            buf.put_u32(column.type_id);
            buf.put_i32(-1); // type modifier
        }
        
        buf.freeze()
    }
    
    /// Build an INSERT message
    pub fn insert_message(&self, rel_id: u32, values: Vec<(&str, Option<&str>)>) -> Bytes {
        let mut buf = self.create_xlogdata_header();
        buf.put_u8(b'I'); // INSERT
        buf.put_u32(rel_id);
        buf.put_u8(b'N'); // new tuple
        buf.put_u16(values.len() as u16);
        
        for (_, value) in values {
            match value {
                Some(v) => {
                    buf.put_u8(b't'); // text value
                    buf.put_i32(v.len() as i32);
                    buf.put(v.as_bytes());
                }
                None => {
                    buf.put_u8(b'n'); // null value
                }
            }
        }
        
        buf.freeze()
    }
    
    /// Build an UPDATE message
    pub fn update_message(&self, rel_id: u32, old_values: Option<Vec<(&str, Option<&str>)>>, new_values: Vec<(&str, Option<&str>)>) -> Bytes {
        let mut buf = self.create_xlogdata_header();
        buf.put_u8(b'U'); // UPDATE
        buf.put_u32(rel_id);
        
        // Add old tuple if provided
        if let Some(old) = old_values {
            buf.put_u8(b'O'); // old tuple
            buf.put_u16(old.len() as u16);
            
            for (_, value) in old {
                match value {
                    Some(v) => {
                        buf.put_u8(b't'); // text value
                        buf.put_i32(v.len() as i32);
                        buf.put(v.as_bytes());
                    }
                    None => {
                        buf.put_u8(b'n'); // null value
                    }
                }
            }
        }
        
        // Add new tuple
        buf.put_u8(b'N'); // new tuple
        buf.put_u16(new_values.len() as u16);
        
        for (_, value) in new_values {
            match value {
                Some(v) => {
                    buf.put_u8(b't'); // text value
                    buf.put_i32(v.len() as i32);
                    buf.put(v.as_bytes());
                }
                None => {
                    buf.put_u8(b'n'); // null value
                }
            }
        }
        
        buf.freeze()
    }
    
    /// Build a DELETE message
    pub fn delete_message(&self, rel_id: u32, key_values: Vec<(&str, Option<&str>)>) -> Bytes {
        let mut buf = self.create_xlogdata_header();
        buf.put_u8(b'D'); // DELETE
        buf.put_u32(rel_id);
        buf.put_u8(b'O'); // old tuple (key)
        buf.put_u16(key_values.len() as u16);
        
        for (_, value) in key_values {
            match value {
                Some(v) => {
                    buf.put_u8(b't'); // text value
                    buf.put_i32(v.len() as i32);
                    buf.put(v.as_bytes());
                }
                None => {
                    buf.put_u8(b'n'); // null value
                }
            }
        }
        
        buf.freeze()
    }
    
    /// Build a binary INSERT message for testing binary format
    pub fn binary_insert_message(&self, rel_id: u32, values: Vec<(&str, BinaryValue)>) -> Bytes {
        let mut buf = self.create_xlogdata_header();
        buf.put_u8(b'I'); // INSERT
        buf.put_u32(rel_id);
        buf.put_u8(b'N'); // new tuple
        buf.put_u16(values.len() as u16);
        
        for (_, value) in values {
            match value {
                BinaryValue::Null => {
                    buf.put_u8(b'n'); // null value
                }
                BinaryValue::Text(text) => {
                    buf.put_u8(b't'); // text value
                    buf.put_i32(text.len() as i32);
                    buf.put(text.as_bytes());
                }
                BinaryValue::Binary(data) => {
                    buf.put_u8(b'b'); // binary value
                    buf.put_i32(data.len() as i32);
                    buf.put(data.as_slice());
                }
            }
        }
        
        buf.freeze()
    }
    
    /// Build a TRUNCATE message
    pub fn truncate_message(&self, rel_ids: Vec<u32>) -> Bytes {
        let mut buf = self.create_xlogdata_header();
        buf.put_u8(b'T'); // TRUNCATE
        buf.put_u32(rel_ids.len() as u32);
        buf.put_u8(0); // options
        
        for rel_id in rel_ids {
            buf.put_u32(rel_id);
        }
        
        buf.freeze()
    }
    
    /// Create a complete transaction: BEGIN -> RELATION -> INSERT -> COMMIT
    pub fn simple_transaction(&self, xid: u32, _table_name: &str, values: Vec<(&str, Option<&str>)>) -> Vec<Bytes> {
        vec![
            self.begin_message(xid),
            self.relation_message(1),
            self.insert_message(1, values),
            self.commit_message(),
        ]
    }
}

/// Enum for binary value types in mock messages
#[derive(Debug, Clone)]
pub enum BinaryValue {
    Null,
    Text(String),
    Binary(Vec<u8>),
}

/// Common PostgreSQL type OIDs for testing
pub mod type_oids {
    pub const BOOL: u32 = 16;
    pub const INT2: u32 = 21;
    pub const INT4: u32 = 23;
    pub const INT8: u32 = 20;
    pub const FLOAT4: u32 = 700;
    pub const FLOAT8: u32 = 701;
    pub const TEXT: u32 = 25;
    pub const VARCHAR: u32 = 1043;
    pub const TIMESTAMP: u32 = 1114;
    pub const TIMESTAMPTZ: u32 = 1184;
    pub const UUID: u32 = 2950;
    pub const JSON: u32 = 114;
    pub const JSONB: u32 = 3802;
    pub const NUMERIC: u32 = 1700;
    
    // Array types
    pub const BOOL_ARRAY: u32 = 1000;
    pub const INT2_ARRAY: u32 = 1005;
    pub const INT4_ARRAY: u32 = 1007;
    pub const INT8_ARRAY: u32 = 1016;
    pub const TEXT_ARRAY: u32 = 1009;
    pub const VARCHAR_ARRAY: u32 = 1015;
    pub const FLOAT8_ARRAY: u32 = 1022;
}

/// Helper functions for creating binary values
impl BinaryValue {
    pub fn bool(value: bool) -> Self {
        BinaryValue::Binary(vec![if value { 1 } else { 0 }])
    }
    
    pub fn int2(value: i16) -> Self {
        BinaryValue::Binary(value.to_be_bytes().to_vec())
    }
    
    pub fn int4(value: i32) -> Self {
        BinaryValue::Binary(value.to_be_bytes().to_vec())
    }
    
    pub fn int8(value: i64) -> Self {
        BinaryValue::Binary(value.to_be_bytes().to_vec())
    }
    
    pub fn float4(value: f32) -> Self {
        BinaryValue::Binary(value.to_be_bytes().to_vec())
    }
    
    pub fn float8(value: f64) -> Self {
        BinaryValue::Binary(value.to_be_bytes().to_vec())
    }
    
    pub fn uuid(value: &str) -> Self {
        // Simple UUID parsing for testing
        let hex_string = value.replace("-", "");
        let bytes = (0..32)
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex_string[i..i+2], 16).unwrap_or(0))
            .collect();
        BinaryValue::Binary(bytes)
    }
    
    pub fn text(value: &str) -> Self {
        BinaryValue::Text(value.to_string())
    }
}

/// Predefined test scenarios
pub mod scenarios {
    use super::*;
    
    /// Create a typical user table transaction
    pub fn user_table_transaction() -> (MockMessageBuilder, Vec<Bytes>) {
        let builder = MockMessageBuilder::new()
            .add_relation(1, "public", "users", vec![
                ("id", type_oids::INT4, true),
                ("name", type_oids::TEXT, false),
                ("email", type_oids::TEXT, false),
                ("active", type_oids::BOOL, false),
                ("created_at", type_oids::TIMESTAMPTZ, false),
            ]);
        
        let messages = vec![
            builder.begin_message(12345),
            builder.relation_message(1),
            builder.insert_message(1, vec![
                ("id", Some("1")),
                ("name", Some("John Doe")),
                ("email", Some("john@example.com")),
                ("active", Some("t")),
                ("created_at", Some("2023-10-15 10:30:00+00")),
            ]),
            builder.update_message(1, 
                Some(vec![
                    ("id", Some("1")),
                    ("name", Some("John Doe")),
                    ("email", Some("john@example.com")),
                    ("active", Some("t")),
                    ("created_at", Some("2023-10-15 10:30:00+00")),
                ]),
                vec![
                    ("id", Some("1")),
                    ("name", Some("John Doe")),
                    ("email", Some("john.doe@example.com")),
                    ("active", Some("t")),
                    ("created_at", Some("2023-10-15 10:30:00+00")),
                ]
            ),
            builder.delete_message(1, vec![
                ("id", Some("1")),
                ("name", Some("John Doe")),
                ("email", Some("john.doe@example.com")),
                ("active", Some("t")),
                ("created_at", Some("2023-10-15 10:30:00+00")),
            ]),
            builder.commit_message(),
        ];
        
        (builder, messages)
    }
    
    /// Create a complex types transaction
    pub fn complex_types_transaction() -> (MockMessageBuilder, Vec<Bytes>) {
        let builder = MockMessageBuilder::new()
            .add_relation(2, "public", "complex_table", vec![
                ("id", type_oids::UUID, true),
                ("data", type_oids::JSONB, false),
                ("tags", type_oids::TEXT_ARRAY, false),
                ("price", type_oids::NUMERIC, false),
                ("coordinates", type_oids::FLOAT8_ARRAY, false),
            ]);
        
        let messages = vec![
            builder.begin_message(67890),
            builder.relation_message(2),
            builder.insert_message(2, vec![
                ("id", Some("550e8400-e29b-41d4-a716-446655440000")),
                ("data", Some(r#"{"type": "product", "category": "electronics"}"#)),
                ("tags", Some("{electronics,gadget,mobile}")),
                ("price", Some("299.99")),
                ("coordinates", Some("{-73.935242,40.730610}")),
            ]),
            builder.commit_message(),
        ];
        
        (builder, messages)
    }
}