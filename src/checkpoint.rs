//! Checkpoint management for exactly-once delivery semantics.
//!
//! This module provides checkpoint persistence to ensure that replication
//! can resume from the last processed position after a restart or failure.
//!
//! # Example
//!
//! ```rust,no_run
//! use pg_capture::checkpoint::{Checkpoint, CheckpointManager};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let manager = CheckpointManager::new("checkpoint.json");
//!     
//!     // Load existing checkpoint
//!     if let Some(checkpoint) = manager.load().await? {
//!         println!("Resuming from LSN: {}", checkpoint.lsn);
//!     }
//!     
//!     // Save new checkpoint
//!     let checkpoint = Checkpoint::new("1234/5678".to_string(), 100);
//!     manager.save(&checkpoint).await?;
//!     
//!     Ok(())
//! }
//! ```

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tracing::{debug, error, info};

/// Represents a checkpoint in the replication stream.
///
/// A checkpoint contains the last successfully processed Log Sequence Number (LSN)
/// and metadata about the replication progress. This allows replication to resume
/// from the exact position after a restart.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    /// The last successfully processed LSN
    pub lsn: String,
    /// The timestamp when this checkpoint was created
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Number of messages processed since startup
    pub message_count: u64,
}

/// Manages checkpoint persistence to disk.
///
/// The `CheckpointManager` handles atomic writes to ensure that checkpoints
/// are never corrupted, even if the process crashes during a write operation.
///
/// # Example
///
/// ```rust,no_run
/// use pg_capture::checkpoint::CheckpointManager;
/// use std::path::PathBuf;
///
/// let manager = CheckpointManager::new(PathBuf::from("/var/lib/pg-capture/checkpoint.json"));
/// ```
pub struct CheckpointManager {
    file_path: PathBuf,
}

impl CheckpointManager {
    /// Creates a new checkpoint manager with the specified file path.
    ///
    /// # Arguments
    ///
    /// * `checkpoint_path` - Path where checkpoint file will be stored
    ///
    /// # Example
    ///
    /// ```rust
    /// use pg_capture::checkpoint::CheckpointManager;
    ///
    /// let manager = CheckpointManager::new("checkpoint.json");
    /// ```
    pub fn new(checkpoint_path: impl AsRef<Path>) -> Self {
        Self {
            file_path: checkpoint_path.as_ref().to_path_buf(),
        }
    }

    /// Loads checkpoint from disk if it exists.
    ///
    /// Returns `None` if the checkpoint file doesn't exist, which typically
    /// means this is the first run or the checkpoint was deleted.
    ///
    /// # Errors
    ///
    /// Returns `Err` if:
    /// - The file exists but cannot be read
    /// - The file contains invalid JSON
    /// - The JSON doesn't match the `Checkpoint` structure
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use pg_capture::checkpoint::CheckpointManager;
    /// # async fn example() -> pg_capture::Result<()> {
    /// let manager = CheckpointManager::new("checkpoint.json");
    /// 
    /// match manager.load().await? {
    ///     Some(checkpoint) => {
    ///         println!("Found checkpoint at LSN: {}", checkpoint.lsn);
    ///         println!("Messages processed: {}", checkpoint.message_count);
    ///     }
    ///     None => {
    ///         println!("No checkpoint found, starting from beginning");
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn load(&self) -> Result<Option<Checkpoint>> {
        if !self.file_path.exists() {
            debug!("No checkpoint file found at {:?}", self.file_path);
            return Ok(None);
        }

        match fs::read_to_string(&self.file_path).await {
            Ok(content) => match serde_json::from_str::<Checkpoint>(&content) {
                Ok(checkpoint) => {
                    info!(
                        "Loaded checkpoint: LSN={}, timestamp={}",
                        checkpoint.lsn, checkpoint.timestamp
                    );
                    Ok(Some(checkpoint))
                }
                Err(e) => {
                    error!("Failed to parse checkpoint file: {}", e);
                    Err(Error::Connection(format!("Invalid checkpoint file: {}", e)))
                }
            },
            Err(e) => {
                error!("Failed to read checkpoint file: {}", e);
                Err(Error::Io(e))
            }
        }
    }

    /// Saves checkpoint to disk atomically.
    ///
    /// This method ensures that the checkpoint is written atomically by:
    /// 1. Writing to a temporary file
    /// 2. Syncing the file to ensure data is on disk
    /// 3. Atomically renaming the temp file to the final location
    ///
    /// This guarantees that the checkpoint file is never partially written.
    ///
    /// # Errors
    ///
    /// Returns `Err` if:
    /// - The checkpoint cannot be serialized to JSON
    /// - The file cannot be written
    /// - The filesystem operations fail
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use pg_capture::checkpoint::{Checkpoint, CheckpointManager};
    /// # async fn example() -> pg_capture::Result<()> {
    /// let manager = CheckpointManager::new("checkpoint.json");
    /// let checkpoint = Checkpoint::new("1234/5678".to_string(), 100);
    /// 
    /// manager.save(&checkpoint).await?;
    /// println!("Checkpoint saved at LSN: {}", checkpoint.lsn);
    /// # Ok(())
    /// # }
    /// ```
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

    /// Deletes the checkpoint file if it exists.
    ///
    /// This is useful for resetting replication to start from the beginning.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the file exists but cannot be deleted.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use pg_capture::checkpoint::CheckpointManager;
    /// # async fn example() -> pg_capture::Result<()> {
    /// let manager = CheckpointManager::new("checkpoint.json");
    /// manager.delete().await?;
    /// println!("Checkpoint deleted, will start from beginning on next run");
    /// # Ok(())
    /// # }
    /// ```
    pub async fn delete(&self) -> Result<()> {
        if self.file_path.exists() {
            fs::remove_file(&self.file_path).await?;
            info!("Deleted checkpoint file");
        }
        Ok(())
    }
}

impl Checkpoint {
    /// Creates a new checkpoint with the current timestamp.
    ///
    /// # Arguments
    ///
    /// * `lsn` - The Log Sequence Number in PostgreSQL format (e.g., "1234/5678")
    /// * `message_count` - Total number of messages processed since startup
    ///
    /// # Example
    ///
    /// ```rust
    /// use pg_capture::checkpoint::Checkpoint;
    ///
    /// let checkpoint = Checkpoint::new("1234/5678".to_string(), 100);
    /// assert_eq!(checkpoint.lsn, "1234/5678");
    /// assert_eq!(checkpoint.message_count, 100);
    /// ```
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
