#[cfg(test)]
mod tests {
    use super::super::decoder::*;
    use super::super::types::ChangeOperation;
    use bytes::{Bytes, BytesMut, BufMut};
    
    fn create_decoder() -> PgOutputDecoder {
        PgOutputDecoder::new("testdb".to_string())
    }
    
    fn create_xlogdata_header(lsn: u64, timestamp: i64) -> BytesMut {
        let mut buf = BytesMut::new();
        buf.put_u8(b'w'); // XLogData tag
        buf.put_u64(lsn); // LSN
        buf.put_u64(lsn); // LSN end
        buf.put_i64(timestamp); // Timestamp (microseconds since 2000-01-01)
        buf
    }
    
    fn create_begin_message(xid: u32, lsn: u64) -> Bytes {
        let mut buf = create_xlogdata_header(lsn, 0);
        buf.put_u8(b'B'); // BEGIN
        buf.put_u64(lsn); // Final LSN
        buf.put_i64(0); // Timestamp
        buf.put_u32(xid); // XID
        buf.freeze()
    }
    
    fn create_commit_message(lsn: u64) -> Bytes {
        let mut buf = create_xlogdata_header(lsn, 0);
        buf.put_u8(b'C'); // COMMIT
        buf.put_u8(0); // Flags
        buf.put_u64(lsn); // Commit LSN
        buf.put_u64(lsn); // End LSN
        buf.put_i64(0); // Timestamp
        buf.freeze()
    }
    
    fn create_relation_message(rel_id: u32, schema: &str, table: &str, columns: Vec<(&str, u32, bool)>) -> Bytes {
        let mut buf = create_xlogdata_header(0, 0);
        buf.put_u8(b'R'); // RELATION
        buf.put_u32(rel_id);
        buf.put_u8(schema.len() as u8);
        buf.put(schema.as_bytes());
        buf.put_u8(table.len() as u8);
        buf.put(table.as_bytes());
        buf.put_u8(b'n'); // replica identity
        buf.put_u16(columns.len() as u16);
        
        for (name, type_id, is_key) in columns {
            buf.put_u8(if is_key { 1 } else { 0 });
            buf.put_u8(name.len() as u8);
            buf.put(name.as_bytes());
            buf.put_u32(type_id);
            buf.put_i32(-1); // type modifier
        }
        
        buf.freeze()
    }
    
    fn create_insert_message(rel_id: u32, values: Vec<(&str, Option<&str>)>) -> Bytes {
        let mut buf = create_xlogdata_header(0, 0);
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
    
    fn create_update_message(rel_id: u32, old_values: Option<Vec<(&str, Option<&str>)>>, new_values: Vec<(&str, Option<&str>)>) -> Bytes {
        let mut buf = create_xlogdata_header(0, 0);
        buf.put_u8(b'U'); // UPDATE
        buf.put_u32(rel_id);
        
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
    
    fn create_delete_message(rel_id: u32, key_values: Vec<(&str, Option<&str>)>) -> Bytes {
        let mut buf = create_xlogdata_header(0, 0);
        buf.put_u8(b'D'); // DELETE
        buf.put_u32(rel_id);
        buf.put_u8(b'K'); // key tuple
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
    
    #[test]
    fn test_decode_begin_message() {
        let mut decoder = create_decoder();
        let msg = create_begin_message(12345, 1000);
        
        let result = decoder.decode(&msg).unwrap();
        match result {
            Some(DecodedMessage::Begin { xid }) => {
                assert_eq!(xid, 12345);
            }
            _ => panic!("Expected Begin message"),
        }
    }
    
    #[test]
    fn test_decode_commit_message() {
        let mut decoder = create_decoder();
        let msg = create_commit_message(2000);
        
        let result = decoder.decode(&msg).unwrap();
        match result {
            Some(DecodedMessage::Commit) => {
                // Success
            }
            _ => panic!("Expected Commit message"),
        }
    }
    
    #[test]
    fn test_decode_relation_message() {
        let mut decoder = create_decoder();
        let columns = vec![
            ("id", 23, true),      // int4, primary key
            ("name", 25, false),   // text
            ("active", 16, false), // bool
        ];
        let msg = create_relation_message(100, "public", "users", columns);
        
        let result = decoder.decode(&msg);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none()); // RELATION messages don't produce output
        
        // Verify relation was stored (we can't directly check, but subsequent messages will use it)
    }
    
    #[test]
    fn test_decode_insert_message() {
        let mut decoder = create_decoder();
        
        // First register the relation
        let columns = vec![
            ("id", 23, true),
            ("name", 25, false),
            ("active", 16, false),
        ];
        let rel_msg = create_relation_message(100, "public", "users", columns);
        decoder.decode(&rel_msg).unwrap();
        
        // Now decode INSERT
        let values = vec![
            ("id", Some("42")),
            ("name", Some("John Doe")),
            ("active", Some("t")),
        ];
        let insert_msg = create_insert_message(100, values);
        
        let result = decoder.decode(&insert_msg).unwrap();
        match result {
            Some(DecodedMessage::Change(event)) => {
                assert_eq!(event.schema, "public");
                assert_eq!(event.table, "users");
                assert!(matches!(event.op, ChangeOperation::Insert));
                assert!(event.before.is_none());
                
                let after = event.after.unwrap();
                assert_eq!(after["id"], 42);
                assert_eq!(after["name"], "John Doe");
                assert_eq!(after["active"], true);
            }
            _ => panic!("Expected Change message"),
        }
    }
    
    #[test]
    fn test_decode_update_message() {
        let mut decoder = create_decoder();
        
        // First register the relation
        let columns = vec![
            ("id", 23, true),
            ("name", 25, false),
            ("email", 25, false),
        ];
        let rel_msg = create_relation_message(200, "public", "contacts", columns);
        decoder.decode(&rel_msg).unwrap();
        
        // Decode UPDATE with old and new values
        let old_values = vec![
            ("id", Some("10")),
            ("name", Some("Old Name")),
            ("email", Some("old@example.com")),
        ];
        let new_values = vec![
            ("id", Some("10")),
            ("name", Some("New Name")),
            ("email", Some("new@example.com")),
        ];
        let update_msg = create_update_message(200, Some(old_values), new_values);
        
        let result = decoder.decode(&update_msg).unwrap();
        match result {
            Some(DecodedMessage::Change(event)) => {
                assert!(matches!(event.op, ChangeOperation::Update));
                
                let before = event.before.unwrap();
                assert_eq!(before["name"], "Old Name");
                assert_eq!(before["email"], "old@example.com");
                
                let after = event.after.unwrap();
                assert_eq!(after["name"], "New Name");
                assert_eq!(after["email"], "new@example.com");
            }
            _ => panic!("Expected Change message"),
        }
    }
    
    #[test]
    fn test_decode_delete_message() {
        let mut decoder = create_decoder();
        
        // First register the relation
        let columns = vec![
            ("id", 23, true),
            ("name", 25, false),
        ];
        let rel_msg = create_relation_message(300, "public", "items", columns);
        decoder.decode(&rel_msg).unwrap();
        
        // Decode DELETE
        let key_values = vec![
            ("id", Some("99")),
            ("name", Some("Deleted Item")),
        ];
        let delete_msg = create_delete_message(300, key_values);
        
        let result = decoder.decode(&delete_msg).unwrap();
        match result {
            Some(DecodedMessage::Change(event)) => {
                assert!(matches!(event.op, ChangeOperation::Delete));
                assert!(event.after.is_none());
                
                let before = event.before.unwrap();
                assert_eq!(before["id"], 99);
                assert_eq!(before["name"], "Deleted Item");
            }
            _ => panic!("Expected Change message"),
        }
    }
    
    #[test]
    fn test_type_conversions() {
        let mut decoder = create_decoder();
        
        // Register relation with various types
        let columns = vec![
            ("bool_col", 16, false),      // bool
            ("int2_col", 21, false),      // smallint
            ("int4_col", 23, false),      // integer
            ("int8_col", 20, false),      // bigint
            ("float4_col", 700, false),   // real
            ("float8_col", 701, false),   // double
            ("text_col", 25, false),      // text
            ("json_col", 114, false),     // json
            ("uuid_col", 2950, false),    // uuid
            ("timestamp_col", 1114, false), // timestamp
        ];
        let rel_msg = create_relation_message(400, "public", "types_test", columns);
        decoder.decode(&rel_msg).unwrap();
        
        // Insert with various values
        let values = vec![
            ("bool_col", Some("t")),
            ("int2_col", Some("123")),
            ("int4_col", Some("45678")),
            ("int8_col", Some("9876543210")),
            ("float4_col", Some("3.14")),
            ("float8_col", Some("2.718281828")),
            ("text_col", Some("Hello, World!")),
            ("json_col", Some(r#"{"key": "value"}"#)),
            ("uuid_col", Some("550e8400-e29b-41d4-a716-446655440000")),
            ("timestamp_col", Some("2023-10-15 10:30:00")),
        ];
        let insert_msg = create_insert_message(400, values);
        
        let result = decoder.decode(&insert_msg).unwrap();
        match result {
            Some(DecodedMessage::Change(event)) => {
                let after = event.after.unwrap();
                assert_eq!(after["bool_col"], true);
                assert_eq!(after["int2_col"], 123);
                assert_eq!(after["int4_col"], 45678);
                assert_eq!(after["int8_col"], 9876543210i64);
                // Float4 has limited precision, so we need to compare with tolerance
                let float4_val = after["float4_col"].as_f64().unwrap();
                assert!((float4_val - 3.14).abs() < 0.001);
                assert_eq!(after["float8_col"], 2.718281828);
                assert_eq!(after["text_col"], "Hello, World!");
                assert_eq!(after["json_col"]["key"], "value");
                assert_eq!(after["uuid_col"], "550e8400-e29b-41d4-a716-446655440000");
                assert_eq!(after["timestamp_col"], "2023-10-15 10:30:00");
            }
            _ => panic!("Expected Change message"),
        }
    }
    
    #[test]
    fn test_null_values() {
        let mut decoder = create_decoder();
        
        // Register relation
        let columns = vec![
            ("id", 23, true),
            ("optional_field", 25, false),
        ];
        let rel_msg = create_relation_message(500, "public", "nullable_test", columns);
        decoder.decode(&rel_msg).unwrap();
        
        // Insert with NULL value
        let values = vec![
            ("id", Some("1")),
            ("optional_field", None),
        ];
        let insert_msg = create_insert_message(500, values);
        
        let result = decoder.decode(&insert_msg).unwrap();
        match result {
            Some(DecodedMessage::Change(event)) => {
                let after = event.after.unwrap();
                assert_eq!(after["id"], 1);
                assert!(after["optional_field"].is_null());
            }
            _ => panic!("Expected Change message"),
        }
    }
    
    #[test]
    fn test_array_parsing() {
        let mut decoder = create_decoder();
        
        // Register relation with array types
        let columns = vec![
            ("int_array", 1007, false),  // _int4
            ("text_array", 1009, false), // _text
        ];
        let rel_msg = create_relation_message(600, "public", "array_test", columns);
        decoder.decode(&rel_msg).unwrap();
        
        // Insert with array values
        let values = vec![
            ("int_array", Some("{1,2,3,4,5}")),
            ("text_array", Some("{hello,world}")),
        ];
        let insert_msg = create_insert_message(600, values);
        
        let result = decoder.decode(&insert_msg).unwrap();
        match result {
            Some(DecodedMessage::Change(event)) => {
                let after = event.after.unwrap();
                let int_array = &after["int_array"];
                assert!(int_array.is_array());
                assert_eq!(int_array[0], 1);
                assert_eq!(int_array[4], 5);
                
                let text_array = &after["text_array"];
                assert!(text_array.is_array());
                assert_eq!(text_array[0], "hello");
                assert_eq!(text_array[1], "world");
            }
            _ => panic!("Expected Change message"),
        }
    }
    
    #[test]
    fn test_binary_value_decoding() {
        let mut decoder = create_decoder();
        
        // Register relation
        let columns = vec![
            ("id", 23, true),       // int4
            ("data", 16, false),    // bool (binary)
        ];
        let rel_msg = create_relation_message(700, "public", "binary_test", columns);
        decoder.decode(&rel_msg).unwrap();
        
        // Create INSERT with binary value
        let mut buf = create_xlogdata_header(0, 0);
        buf.put_u8(b'I'); // INSERT
        buf.put_u32(700); // rel_id
        buf.put_u8(b'N'); // new tuple
        buf.put_u16(2);   // num columns
        
        // id column (text)
        buf.put_u8(b't');
        buf.put_i32(1);
        buf.put_u8(b'1');
        
        // data column (binary bool)
        buf.put_u8(b'b');
        buf.put_i32(1); // 1 byte
        buf.put_u8(1);  // true
        
        let insert_msg = buf.freeze();
        
        let result = decoder.decode(&insert_msg).unwrap();
        match result {
            Some(DecodedMessage::Change(event)) => {
                let after = event.after.unwrap();
                assert_eq!(after["id"], 1);
                assert_eq!(after["data"], true);
            }
            _ => panic!("Expected Change message"),
        }
    }
    
    #[test]
    fn test_error_handling() {
        let mut decoder = create_decoder();
        
        // Test empty data
        assert!(decoder.decode(&[]).unwrap().is_none());
        
        // Test invalid tag
        let invalid = vec![b'x', 1, 2, 3];
        assert!(decoder.decode(&invalid).unwrap().is_none());
        
        // Test truncated message
        let truncated = create_begin_message(123, 456);
        let truncated = &truncated[..10]; // Cut off message
        assert!(decoder.decode(truncated).is_err());
        
        // Test unknown relation ID
        let insert_msg = create_insert_message(999, vec![("id", Some("1"))]);
        assert!(decoder.decode(&insert_msg).is_err());
    }
}