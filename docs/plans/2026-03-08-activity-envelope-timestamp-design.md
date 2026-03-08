# Design: Remove timestamp from ActivityTxEnvelope

## Problem

A 1-block reorg at block 18790897 creates an infinite sync loop:

1. Block 18790897 (old fork) is indexed → activity envelope written to append-only store
2. Reorg detected → rollback cleans domain store, skips append-only (by design)
3. Re-indexing: cellbase tx_hash is identical across forks (same miner, same epoch) → same key
4. But `ActivityTxEnvelope.timestamp` comes from the block header (different per fork) → different value
5. Append-only overwrite protection rejects the write → rollback → retry → infinite loop

Root cause: `ActivityTxEnvelope.timestamp` is block-derived (not tx-derived), making the envelope non-deterministic for identical transactions across reorg forks.

## Design

Remove `timestamp` from `ActivityTxEnvelope`. Derive it from block headers at read time.

### Schema Change

```rust
// ActivityTxEnvelope — remove timestamp field
pub struct ActivityTxEnvelope {
    pub tx_hash: Vec<u8>,
    pub block_number: i64,
    pub tx_index: i32,
    // timestamp removed — derive from block header at read time
    pub is_cellbase: bool,
    pub participants: Vec<Vec<u8>>,
    pub owner_views: Vec<OwnerActivityViewStored>,
}
```

`ActivityEntry` keeps its `timestamp` field (used by API responses).

### Write Path

`normalize_activities_for_storage()` stops writing timestamp into the envelope. The envelope becomes purely tx-content-derived → deterministic → idempotent across reorg replays.

### Read Path

- `activity_ops.rs`: `load_activity_entry_from_owner_ref()` sets `timestamp: 0` (placeholder)
- `routes/activities.rs`: After loading activity page, batch-lookup `get_block_header(block_num)` from domain store for all unique block numbers, fill in real timestamps before building response

### Files Changed

| File                                         | Change                                                                   |
| -------------------------------------------- | ------------------------------------------------------------------------ |
| `crates/ckbadger-store/src/types.rs`         | Remove `timestamp` from `ActivityTxEnvelope`                             |
| `crates/ckbadger-store/src/activity_ops.rs`  | Set `timestamp: 0` placeholder in entry reconstruction                   |
| `crates/indexer/src/db/writer/activities.rs` | Stop writing timestamp to envelope in `normalize_activities_for_storage` |
| `crates/api/src/routes/activities.rs`        | Batch-fill timestamps from block headers after loading activities        |
| Tests in above files                         | Update to match new schema                                               |

### No Changes Required

- Frontend types and components (timestamp still in API response)
- `ActivityEntry` struct (keeps timestamp field)
- Activity cursor format
- Activity filtering logic

### Rebuild Required

Bincode serialization format changes → delete RocksDB and re-sync from genesis.

## Principle Alignment

- **CKB Native**: Block timestamp belongs to block header, not activity envelope
- **Local First**: No external dependencies; block headers always available in domain store
- **Single Calculation Path**: Timestamp has one source (block header), not duplicated in envelope
