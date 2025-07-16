use crate::postgres::ChangeEvent;
use serde_json::Value;
use tracing::{debug, warn};

#[derive(Debug, Clone)]
pub enum KeyStrategy {
    TableName,
    PrimaryKey(Vec<String>),
    FieldPath(String),
    Composite(Vec<String>),
    None,
}

impl KeyStrategy {
    pub fn extract_key(&self, event: &ChangeEvent) -> Option<String> {
        match self {
            KeyStrategy::TableName => {
                Some(format!("{}.{}", event.schema, event.table))
            }
            
            KeyStrategy::PrimaryKey(columns) => {
                let record = match event.op {
                    crate::postgres::ChangeOperation::Delete => &event.before,
                    _ => &event.after,
                };
                
                if let Some(record) = record {
                    extract_composite_key(record, columns)
                } else {
                    warn!("No record data available for key extraction");
                    None
                }
            }
            
            KeyStrategy::FieldPath(path) => {
                let record = match event.op {
                    crate::postgres::ChangeOperation::Delete => &event.before,
                    _ => &event.after,
                };
                
                if let Some(record) = record {
                    extract_field_value(record, path)
                } else {
                    warn!("No record data available for key extraction");
                    None
                }
            }
            
            KeyStrategy::Composite(fields) => {
                let record = match event.op {
                    crate::postgres::ChangeOperation::Delete => &event.before,
                    _ => &event.after,
                };
                
                if let Some(record) = record {
                    extract_composite_key(record, fields)
                } else {
                    warn!("No record data available for key extraction");
                    None
                }
            }
            
            KeyStrategy::None => None,
        }
    }
}

fn extract_field_value(record: &Value, field_path: &str) -> Option<String> {
    let parts: Vec<&str> = field_path.split('.').collect();
    let mut current = record;
    
    for part in parts {
        match current.get(part) {
            Some(value) => current = value,
            None => {
                debug!("Field '{}' not found in record", part);
                return None;
            }
        }
    }
    
    match current {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Null => None,
        _ => Some(current.to_string()),
    }
}

fn extract_composite_key(record: &Value, fields: &[String]) -> Option<String> {
    let mut key_parts = Vec::new();
    
    for field in fields {
        if let Some(value) = extract_field_value(record, field) {
            key_parts.push(value);
        } else {
            debug!("Missing field '{}' for composite key", field);
            return None;
        }
    }
    
    if key_parts.is_empty() {
        None
    } else {
        Some(key_parts.join(":"))
    }
}

impl Default for KeyStrategy {
    fn default() -> Self {
        KeyStrategy::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use crate::postgres::{ChangeOperation, SourceMetadata};
    
    fn create_test_event(op: ChangeOperation, before: Option<Value>, after: Option<Value>) -> ChangeEvent {
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
                "0/123456".to_string(),
            ),
        }
    }
    
    #[test]
    fn test_table_name_strategy() {
        let event = create_test_event(ChangeOperation::Insert, None, Some(json!({})));
        let strategy = KeyStrategy::TableName;
        
        assert_eq!(strategy.extract_key(&event), Some("public.users".to_string()));
    }
    
    #[test]
    fn test_primary_key_strategy() {
        let after = json!({
            "id": 123,
            "name": "John Doe"
        });
        
        let event = create_test_event(ChangeOperation::Insert, None, Some(after));
        let strategy = KeyStrategy::PrimaryKey(vec!["id".to_string()]);
        
        assert_eq!(strategy.extract_key(&event), Some("123".to_string()));
    }
    
    #[test]
    fn test_composite_key_strategy() {
        let after = json!({
            "org_id": 456,
            "user_id": 789,
            "name": "John Doe"
        });
        
        let event = create_test_event(ChangeOperation::Insert, None, Some(after));
        let strategy = KeyStrategy::Composite(vec!["org_id".to_string(), "user_id".to_string()]);
        
        assert_eq!(strategy.extract_key(&event), Some("456:789".to_string()));
    }
    
    #[test]
    fn test_field_path_strategy() {
        let after = json!({
            "user": {
                "profile": {
                    "email": "john@example.com"
                }
            }
        });
        
        let event = create_test_event(ChangeOperation::Insert, None, Some(after));
        let strategy = KeyStrategy::FieldPath("user.profile.email".to_string());
        
        assert_eq!(strategy.extract_key(&event), Some("john@example.com".to_string()));
    }
    
    #[test]
    fn test_delete_operation_uses_before() {
        let before = json!({
            "id": 999,
            "name": "Deleted User"
        });
        
        let event = create_test_event(ChangeOperation::Delete, Some(before), None);
        let strategy = KeyStrategy::PrimaryKey(vec!["id".to_string()]);
        
        assert_eq!(strategy.extract_key(&event), Some("999".to_string()));
    }
}