# Checkpoint Design

## Overview

The checkpoint system in pg-capture provides persistent state management to ensure reliable replication and enable resumability after failures. This document describes the design and implementation of the checkpoint mechanism.

## Goals

1. **Durability**: Persist the last successfully processed LSN to disk
2. **Resumability**: Allow replication to resume from the last checkpoint after restart
3. **Atomicity**: Ensure checkpoint updates are atomic to prevent corruption
4. **Performance**: Minimize impact on replication throughput

## Architecture

### Components

1. **Checkpoint Structure**
   - `lsn`: String representation of PostgreSQL LSN (e.g., "1234/5678ABCD")
   - `timestamp`: UTC timestamp when checkpoint was created
   - `message_count`: Total number of messages processed since startup

2. **CheckpointManager**
   - Handles reading and writing checkpoint files
   - Provides atomic file operations using temp file + rename pattern
   - JSON format for human readability and debugging

3. **Integration with Replicator**
   - Tracks current LSN from XLogData messages
   - Periodic checkpoint saves based on configuration
   - Final checkpoint on graceful shutdown

### File Format

```json
{
  "lsn": "1234/5678ABCD",
  "timestamp": "2023-10-20T15:30:45.123456789Z",
  "message_count": 150000
}
```

### LSN Tracking

LSN (Log Sequence Number) tracking follows this flow:

1. **Message Reception**: Each XLogData message contains `wal_start` and `wal_end`
2. **LSN Update**: Replicator updates `current_lsn` with `wal_end` from each message
3. **Standby Updates**: Send standby status updates to PostgreSQL with current LSN
4. **Checkpoint Save**: Periodically save current LSN to checkpoint file

### Checkpoint Strategy

Checkpoints are saved:
- Every `checkpoint_interval_secs` (configurable, default 10 seconds)
- On graceful shutdown
- After processing significant batches (future enhancement)

## Implementation Details

### Atomic Writes

To ensure checkpoint integrity:
1. Write to temporary file (`.tmp` extension)
2. Sync file to disk (`sync_all()`)
3. Atomic rename to final location
4. This prevents partial writes from corrupting checkpoint

### Resume Logic

On startup:
1. Load checkpoint if exists
2. Extract LSN and message count
3. Pass LSN to `START_REPLICATION` command
4. PostgreSQL resumes from specified LSN
5. Continue tracking from loaded message count

### Error Handling

- **Missing Checkpoint**: Start from beginning (LSN "0/0")
- **Corrupted Checkpoint**: Log error and start fresh
- **Write Failures**: Log error but continue replication
- **Recovery**: Exponential backoff with retry on connection failures

## Configuration

No additional configuration required. Checkpoint behavior is controlled by:
- `checkpoint_interval_secs`: How often to save checkpoints (default: 10)
- Checkpoint file location: Currently hardcoded to `checkpoint.json` in working directory

## Future Enhancements

1. **Configurable Path**: Allow checkpoint file path configuration
2. **Multiple Checkpoints**: Keep history of checkpoints for debugging
3. **Checkpoint Metrics**: Track checkpoint lag and save frequency
4. **Cloud Storage**: Support S3/GCS for checkpoint storage
5. **Compaction**: Combine checkpoint with Kafka offset tracking

## Testing

The checkpoint system includes comprehensive tests:
- Basic save/load functionality
- Atomic write verification
- Recovery simulation
- Concurrent write handling

## Performance Considerations

1. **Write Frequency**: Balance between durability and I/O overhead
2. **File Size**: JSON format keeps files small (<1KB)
3. **Sync Operations**: Use `sync_all()` for durability at cost of latency
4. **Background Saves**: Future optimization to save checkpoints asynchronously

## Operational Notes

1. **Monitoring**: Check checkpoint age to detect stalled replication
2. **Backup**: Include checkpoint file in backup procedures
3. **Migration**: Checkpoint format may change between versions
4. **Debugging**: Checkpoint file is human-readable JSON for troubleshooting