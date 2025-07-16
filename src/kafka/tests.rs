#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::config::KafkaConfig;
    use crate::postgres::{ChangeEvent, ChangeOperation, SourceMetadata};
    use serde_json::json;

    fn create_test_kafka_config() -> KafkaConfig {
        KafkaConfig {
            brokers: vec!["localhost:9092".to_string()],
            topic_prefix: "test".to_string(),
            compression: "none".to_string(),
            acks: "1".to_string(),
            linger_ms: 0,
            batch_size: 1,
            buffer_memory: 1024,
        }
    }

    fn create_test_event(op: ChangeOperation) -> ChangeEvent {
        let before = match &op {
            ChangeOperation::Delete => Some(json!({"id": 1, "name": "Old User"})),
            ChangeOperation::Update => Some(json!({"id": 1, "name": "Old User"})),
            _ => None,
        };
        let after = match &op {
            ChangeOperation::Delete => None,
            _ => Some(json!({"id": 1, "name": "New User", "email": "user@example.com"})),
        };

        ChangeEvent {
            schema: "public".to_string(),
            table: "users".to_string(),
            op,
            ts_ms: 1234567890,
            before,
            after,
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
    fn test_key_strategy_extraction() {
        let event = create_test_event(ChangeOperation::Insert);

        // Test table name strategy
        let strategy = KeyStrategy::TableName;
        assert_eq!(
            strategy.extract_key(&event),
            Some("public.users".to_string())
        );

        // Test primary key strategy
        let strategy = KeyStrategy::PrimaryKey(vec!["id".to_string()]);
        assert_eq!(strategy.extract_key(&event), Some("1".to_string()));

        // Test field path strategy
        let strategy = KeyStrategy::FieldPath("email".to_string());
        assert_eq!(
            strategy.extract_key(&event),
            Some("user@example.com".to_string())
        );

        // Test composite key strategy
        let strategy = KeyStrategy::Composite(vec!["id".to_string(), "name".to_string()]);
        assert_eq!(strategy.extract_key(&event), Some("1:New User".to_string()));

        // Test none strategy
        let strategy = KeyStrategy::None;
        assert_eq!(strategy.extract_key(&event), None);
    }

    #[test]
    fn test_key_extraction_for_delete() {
        let event = create_test_event(ChangeOperation::Delete);

        // Should use 'before' field for deletes
        let strategy = KeyStrategy::PrimaryKey(vec!["id".to_string()]);
        assert_eq!(strategy.extract_key(&event), Some("1".to_string()));
    }

    #[test]
    fn test_json_serialization_formats() {
        let event = create_test_event(ChangeOperation::Insert);

        // Test compact JSON
        let serializer = JsonSerializer::new(SerializationFormat::JsonCompact);
        let result = serializer.serialize(&event).unwrap();
        assert!(result.contains("\"op\":\"INSERT\""));
        assert!(!result.contains("\n")); // Compact format should not have newlines

        // Test pretty JSON
        let serializer = JsonSerializer::new(SerializationFormat::Json);
        let result = serializer.serialize(&event).unwrap();
        assert!(result.contains("\"op\": \"INSERT\""));
        assert!(result.contains("\n")); // Pretty format should have newlines

        // Test Debezium format
        let serializer = JsonSerializer::new(SerializationFormat::JsonDebezium);
        let result = serializer.serialize(&event).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["payload"]["op"], "c"); // Insert is 'c' in Debezium
        assert_eq!(parsed["payload"]["source"]["connector"], "postgresql");
    }

    #[test]
    fn test_topic_name_generation() {
        let config = create_test_kafka_config();
        let topic = config.topic_name("public", "users");
        assert_eq!(topic, "test.public.users");
    }

    #[tokio::test]
    #[ignore] // May fail if system has specific network configurations
    async fn test_producer_creation() {
        let config = create_test_kafka_config();
        let result = KafkaProducer::new(&config.brokers, &config);

        // Should succeed even if Kafka is not running (just creates the producer)
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[ignore] // Requires running Kafka
    async fn test_send_change_event() {
        let config = create_test_kafka_config();
        let producer = KafkaProducer::new(&config.brokers, &config).unwrap();

        let event = create_test_event(ChangeOperation::Insert);
        let key_strategy = KeyStrategy::PrimaryKey(vec!["id".to_string()]);
        let serializer = JsonSerializer::new(SerializationFormat::JsonCompact);

        let result = producer
            .send_change_event(&event, &key_strategy, &serializer)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[ignore] // Requires running Kafka
    async fn test_batch_sending() {
        let config = create_test_kafka_config();
        let producer = KafkaProducer::new(&config.brokers, &config).unwrap();

        let events = vec![
            create_test_event(ChangeOperation::Insert),
            create_test_event(ChangeOperation::Update),
            create_test_event(ChangeOperation::Delete),
        ];

        let key_strategy = KeyStrategy::PrimaryKey(vec!["id".to_string()]);
        let serializer = JsonSerializer::new(SerializationFormat::JsonCompact);

        let results = producer
            .send_change_events_batch(&events, &key_strategy, &serializer)
            .await
            .unwrap();
        assert_eq!(results.len(), 3);

        for result in results {
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_missing_key_field() {
        let mut event = create_test_event(ChangeOperation::Insert);
        event.after = Some(json!({"name": "No ID User"})); // Missing 'id' field

        let strategy = KeyStrategy::PrimaryKey(vec!["id".to_string()]);
        assert_eq!(strategy.extract_key(&event), None);
    }

    #[test]
    fn test_nested_field_extraction() {
        let mut event = create_test_event(ChangeOperation::Insert);
        event.after = Some(json!({
            "id": 1,
            "profile": {
                "personal": {
                    "email": "nested@example.com"
                }
            }
        }));

        let strategy = KeyStrategy::FieldPath("profile.personal.email".to_string());
        assert_eq!(
            strategy.extract_key(&event),
            Some("nested@example.com".to_string())
        );
    }

    #[test]
    fn test_various_value_types() {
        let mut event = create_test_event(ChangeOperation::Insert);
        event.after = Some(json!({
            "int_val": 42,
            "bool_val": true,
            "null_val": null,
            "float_val": 3.14,
            "string_val": "test"
        }));

        // Test integer extraction
        let strategy = KeyStrategy::FieldPath("int_val".to_string());
        assert_eq!(strategy.extract_key(&event), Some("42".to_string()));

        // Test boolean extraction
        let strategy = KeyStrategy::FieldPath("bool_val".to_string());
        assert_eq!(strategy.extract_key(&event), Some("true".to_string()));

        // Test null extraction
        let strategy = KeyStrategy::FieldPath("null_val".to_string());
        assert_eq!(strategy.extract_key(&event), None);

        // Test float extraction
        let strategy = KeyStrategy::FieldPath("float_val".to_string());
        assert_eq!(strategy.extract_key(&event), Some("3.14".to_string()));
    }
}
