use pg_replicate_kafka::checkpoint::{Checkpoint, CheckpointManager};
use tempfile::TempDir;
use std::time::Duration;

#[tokio::test]
async fn test_checkpoint_persistence() {
    let temp_dir = TempDir::new().unwrap();
    let checkpoint_path = temp_dir.path().join("test_checkpoint.json");
    
    let manager = CheckpointManager::new(&checkpoint_path);
    
    // Create and save a checkpoint
    let checkpoint = Checkpoint::new("1234/5678ABCD".to_string(), 1000);
    manager.save(&checkpoint).await.unwrap();
    
    // Load the checkpoint
    let loaded = manager.load().await.unwrap().expect("Checkpoint should exist");
    
    assert_eq!(loaded.lsn, "1234/5678ABCD");
    assert_eq!(loaded.message_count, 1000);
    
    // Update checkpoint
    let checkpoint2 = Checkpoint::new("2345/6789BCDE".to_string(), 2000);
    manager.save(&checkpoint2).await.unwrap();
    
    // Load updated checkpoint
    let loaded2 = manager.load().await.unwrap().expect("Checkpoint should exist");
    
    assert_eq!(loaded2.lsn, "2345/6789BCDE");
    assert_eq!(loaded2.message_count, 2000);
}

#[tokio::test]
async fn test_checkpoint_recovery_simulation() {
    let temp_dir = TempDir::new().unwrap();
    let checkpoint_path = temp_dir.path().join("recovery_checkpoint.json");
    
    // Simulate first run
    {
        let manager = CheckpointManager::new(&checkpoint_path);
        
        // No checkpoint initially
        assert!(manager.load().await.unwrap().is_none());
        
        // Process some messages and save checkpoint
        let checkpoint = Checkpoint::new("AAAA/BBBBCCCC".to_string(), 500);
        manager.save(&checkpoint).await.unwrap();
    }
    
    // Simulate restart/recovery
    {
        let manager = CheckpointManager::new(&checkpoint_path);
        
        // Should load previous checkpoint
        let loaded = manager.load().await.unwrap().expect("Should recover checkpoint");
        assert_eq!(loaded.lsn, "AAAA/BBBBCCCC");
        assert_eq!(loaded.message_count, 500);
        
        // Continue processing from checkpoint
        let new_checkpoint = Checkpoint::new("CCCC/DDDDEEEE".to_string(), loaded.message_count + 300);
        manager.save(&new_checkpoint).await.unwrap();
    }
    
    // Verify final state
    {
        let manager = CheckpointManager::new(&checkpoint_path);
        let final_checkpoint = manager.load().await.unwrap().expect("Should have final checkpoint");
        assert_eq!(final_checkpoint.lsn, "CCCC/DDDDEEEE");
        assert_eq!(final_checkpoint.message_count, 800);
    }
}

#[tokio::test] 
async fn test_concurrent_checkpoint_writes() {
    let temp_dir = TempDir::new().unwrap();
    let checkpoint_path = temp_dir.path().join("concurrent_checkpoint.json");
    
    let manager = CheckpointManager::new(&checkpoint_path);
    
    // Simulate rapid checkpoint updates (like during high throughput)
    for i in 0..10 {
        let lsn = format!("{:04X}/{:08X}", i, i * 1000);
        let checkpoint = Checkpoint::new(lsn, i * 100);
        manager.save(&checkpoint).await.unwrap();
        
        // Small delay to simulate processing
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    
    // Final checkpoint should be the last one
    let final_checkpoint = manager.load().await.unwrap().expect("Should have checkpoint");
    assert_eq!(final_checkpoint.lsn, "0009/00002328");
    assert_eq!(final_checkpoint.message_count, 900);
}