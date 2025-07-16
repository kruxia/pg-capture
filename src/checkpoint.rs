use crate::{Error, Result};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use serde::{Deserialize, Serialize};
use tracing::{info, error, debug};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    /// The last successfully processed LSN
    pub lsn: String,
    /// The timestamp when this checkpoint was created
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Number of messages processed since startup
    pub message_count: u64,
}

pub struct CheckpointManager {
    file_path: PathBuf,
}

impl CheckpointManager {
    pub fn new(checkpoint_path: impl AsRef<Path>) -> Self {
        Self {
            file_path: checkpoint_path.as_ref().to_path_buf(),
        }
    }
    
    /// Load checkpoint from disk if it exists
    pub async fn load(&self) -> Result<Option<Checkpoint>> {
        if !self.file_path.exists() {
            debug!("No checkpoint file found at {:?}", self.file_path);
            return Ok(None);
        }
        
        match fs::read_to_string(&self.file_path).await {
            Ok(content) => {
                match serde_json::from_str::<Checkpoint>(&content) {
                    Ok(checkpoint) => {
                        info!("Loaded checkpoint: LSN={}, timestamp={}", 
                              checkpoint.lsn, checkpoint.timestamp);
                        Ok(Some(checkpoint))
                    }
                    Err(e) => {
                        error!("Failed to parse checkpoint file: {}", e);
                        Err(Error::Connection(format!("Invalid checkpoint file: {}", e)))
                    }
                }
            }
            Err(e) => {
                error!("Failed to read checkpoint file: {}", e);
                Err(Error::Io(e))
            }
        }
    }
    
    /// Save checkpoint to disk atomically
    pub async fn save(&self, checkpoint: &Checkpoint) -> Result<()> {
        debug!("Saving checkpoint: LSN={}", checkpoint.lsn);
        
        // Create temporary file
        let temp_path = self.file_path.with_extension("tmp");
        
        // Write to temporary file
        let json = serde_json::to_string_pretty(checkpoint)?;
        let mut file = fs::File::create(&temp_path).await?;
        file.write_all(json.as_bytes()).await?;
        file.sync_all().await?;
        
        // Atomically rename temp file to final location
        fs::rename(&temp_path, &self.file_path).await?;
        
        debug!("Checkpoint saved successfully");
        Ok(())
    }
    
    /// Delete checkpoint file
    pub async fn delete(&self) -> Result<()> {
        if self.file_path.exists() {
            fs::remove_file(&self.file_path).await?;
            info!("Deleted checkpoint file");
        }
        Ok(())
    }
}

impl Checkpoint {
    pub fn new(lsn: String, message_count: u64) -> Self {
        Self {
            lsn,
            timestamp: chrono::Utc::now(),
            message_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[tokio::test]
    async fn test_checkpoint_save_load() {
        let temp_dir = TempDir::new().unwrap();
        let checkpoint_path = temp_dir.path().join("checkpoint.json");
        
        let manager = CheckpointManager::new(&checkpoint_path);
        
        // Initially no checkpoint
        assert!(manager.load().await.unwrap().is_none());
        
        // Save checkpoint
        let checkpoint = Checkpoint::new("1234/5678".to_string(), 100);
        manager.save(&checkpoint).await.unwrap();
        
        // Load checkpoint
        let loaded = manager.load().await.unwrap().unwrap();
        assert_eq!(loaded.lsn, "1234/5678");
        assert_eq!(loaded.message_count, 100);
    }
    
    #[tokio::test]
    async fn test_checkpoint_atomic_write() {
        let temp_dir = TempDir::new().unwrap();
        let checkpoint_path = temp_dir.path().join("checkpoint.json");
        
        let manager = CheckpointManager::new(&checkpoint_path);
        
        // Save first checkpoint
        let checkpoint1 = Checkpoint::new("1111/2222".to_string(), 50);
        manager.save(&checkpoint1).await.unwrap();
        
        // Save second checkpoint (should overwrite atomically)
        let checkpoint2 = Checkpoint::new("3333/4444".to_string(), 150);
        manager.save(&checkpoint2).await.unwrap();
        
        // Load should get the second checkpoint
        let loaded = manager.load().await.unwrap().unwrap();
        assert_eq!(loaded.lsn, "3333/4444");
        assert_eq!(loaded.message_count, 150);
    }
}