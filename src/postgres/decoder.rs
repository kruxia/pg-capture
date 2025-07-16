use crate::Result;

pub struct PgOutputDecoder {
    // TODO: Add decoder state
}

impl PgOutputDecoder {
    pub fn new() -> Self {
        Self {}
    }
    
    pub fn decode(&mut self, _data: &[u8]) -> Result<()> {
        // TODO: Implement pgoutput protocol decoder
        Ok(())
    }
}