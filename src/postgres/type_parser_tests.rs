#[cfg(test)]
mod tests {
    use crate::postgres::decoder::{parse_postgres_value, parse_postgres_binary_value};
    use serde_json::Value;
    
    #[test]
    fn test_bool_parsing() {
        assert_eq!(parse_postgres_value("t", 16), Value::Bool(true));
        assert_eq!(parse_postgres_value("f", 16), Value::Bool(false));
        assert_eq!(parse_postgres_value("invalid", 16), Value::String("invalid".to_string()));
    }
    
    #[test]
    fn test_integer_parsing() {
        // int2
        assert_eq!(parse_postgres_value("123", 21), Value::Number(123.into()));
        assert_eq!(parse_postgres_value("invalid", 21), Value::String("invalid".to_string()));
        
        // int4
        assert_eq!(parse_postgres_value("456789", 23), Value::Number(456789.into()));
        
        // int8
        assert_eq!(parse_postgres_value("9876543210", 20), Value::Number(9876543210i64.into()));
    }
    
    #[test]
    fn test_float_parsing() {
        // float4 - check approximate equality due to precision differences
        let result = parse_postgres_value("3.14", 700);
        assert!((result.as_f64().unwrap() - 3.14).abs() < 0.001);
        
        // float8
        let result = parse_postgres_value("2.718281828", 701);
        assert!((result.as_f64().unwrap() - 2.718281828).abs() < 0.000001);
    }
    
    #[test]
    fn test_string_types() {
        // text
        assert_eq!(parse_postgres_value("Hello, World!", 25), Value::String("Hello, World!".to_string()));
        
        // varchar
        assert_eq!(parse_postgres_value("Variable length", 1043), Value::String("Variable length".to_string()));
        
        // char
        assert_eq!(parse_postgres_value("A", 18), Value::String("A".to_string()));
    }
    
    #[test]
    fn test_datetime_types() {
        // timestamp
        assert_eq!(parse_postgres_value("2023-10-15 10:30:00", 1114), Value::String("2023-10-15 10:30:00".to_string()));
        
        // timestamptz
        assert_eq!(parse_postgres_value("2023-10-15 10:30:00+00", 1184), Value::String("2023-10-15 10:30:00+00".to_string()));
        
        // date
        assert_eq!(parse_postgres_value("2023-10-15", 1082), Value::String("2023-10-15".to_string()));
    }
    
    #[test]
    fn test_json_types() {
        let json_text = r#"{"key": "value", "number": 42}"#;
        let parsed = parse_postgres_value(json_text, 114); // json
        
        assert!(parsed.is_object());
        assert_eq!(parsed["key"], "value");
        assert_eq!(parsed["number"], 42);
        
        // Invalid JSON should return as string
        let invalid_json = r#"{"invalid": json"#;
        assert_eq!(parse_postgres_value(invalid_json, 114), Value::String(invalid_json.to_string()));
    }
    
    #[test]
    fn test_uuid_parsing() {
        let uuid = "550e8400-e29b-41d4-a716-446655440000";
        assert_eq!(parse_postgres_value(uuid, 2950), Value::String(uuid.to_string()));
    }
    
    #[test]
    fn test_numeric_parsing() {
        // numeric/decimal - should remain as string to preserve precision
        assert_eq!(parse_postgres_value("123.456789", 1700), Value::String("123.456789".to_string()));
    }
    
    #[test]
    fn test_array_parsing() {
        // Simple integer array
        let result = parse_postgres_value("{1,2,3,4,5}", 1007); // _int4
        assert!(result.is_array());
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 5);
        assert_eq!(arr[0], 1);
        assert_eq!(arr[4], 5);
        
        // Text array
        let result = parse_postgres_value("{hello,world}", 1009); // _text
        assert!(result.is_array());
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0], "hello");
        assert_eq!(arr[1], "world");
        
        // Empty array
        let result = parse_postgres_value("{}", 1007);
        assert!(result.is_array());
        assert_eq!(result.as_array().unwrap().len(), 0);
    }
    
    #[test]
    fn test_unknown_type() {
        // Unknown type should return as string
        assert_eq!(parse_postgres_value("some value", 9999), Value::String("some value".to_string()));
    }
    
    #[test]
    fn test_binary_bool_parsing() {
        assert_eq!(parse_postgres_binary_value(&[1], 16), Value::Bool(true));
        assert_eq!(parse_postgres_binary_value(&[0], 16), Value::Bool(false));
        assert_eq!(parse_postgres_binary_value(&[], 16), Value::Null);
    }
    
    #[test]
    fn test_binary_int_parsing() {
        // int2
        let bytes = 123i16.to_be_bytes();
        assert_eq!(parse_postgres_binary_value(&bytes, 21), Value::Number(123.into()));
        
        // int4
        let bytes = 456789i32.to_be_bytes();
        assert_eq!(parse_postgres_binary_value(&bytes, 23), Value::Number(456789.into()));
        
        // int8
        let bytes = 9876543210i64.to_be_bytes();
        assert_eq!(parse_postgres_binary_value(&bytes, 20), Value::Number(9876543210i64.into()));
    }
    
    #[test]
    fn test_binary_float_parsing() {
        // float4
        let bytes = 3.14f32.to_be_bytes();
        let result = parse_postgres_binary_value(&bytes, 700);
        assert!((result.as_f64().unwrap() - 3.14).abs() < 0.001);
        
        // float8
        let bytes = 2.718281828f64.to_be_bytes();
        let result = parse_postgres_binary_value(&bytes, 701);
        assert!((result.as_f64().unwrap() - 2.718281828).abs() < 0.000001);
    }
    
    #[test]
    fn test_binary_uuid_parsing() {
        // UUID bytes for 550e8400-e29b-41d4-a716-446655440000
        let uuid_bytes = vec![
            0x55, 0x0e, 0x84, 0x00, 0xe2, 0x9b, 0x41, 0xd4,
            0xa7, 0x16, 0x44, 0x66, 0x55, 0x44, 0x00, 0x00
        ];
        
        let result = parse_postgres_binary_value(&uuid_bytes, 2950);
        assert_eq!(result, Value::String("550e8400-e29b-41d4-a716-446655440000".to_string()));
    }
    
    #[test]
    fn test_binary_unknown_type() {
        // Unknown type should be base64 encoded
        let data = vec![0x01, 0x02, 0x03, 0x04];
        let result = parse_postgres_binary_value(&data, 9999);
        assert!(result.as_str().unwrap().starts_with("base64:"));
    }
}

// Helper function to test the parsing functions
// This makes the internal functions accessible for testing
