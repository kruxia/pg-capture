use crate::{postgres::ChangeEvent, Result};
use serde_json;

pub struct JsonSerializer;

impl JsonSerializer {
    pub fn serialize(event: &ChangeEvent) -> Result<String> {
        serde_json::to_string(event).map_err(Into::into)
    }
}