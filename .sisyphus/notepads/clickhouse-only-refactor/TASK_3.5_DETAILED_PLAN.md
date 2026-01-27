# Task 3.5: Update sync module for ClickHouse-only - Detailed Implementation Plan

## Current Blocker

File: `crates/indexer/src/sync/indexer.rs`
Issue: 7 usages of `self.repo` (Repository struct) that need ClickHouse equivalents

## Step-by-Step Implementation Guide

### Step 1: Identify All Repository Usages

Run: `grep -n "self\.repo" crates/indexer/src/sync/indexer.rs`

Expected usages (based on typical indexer patterns):

1. `self.repo.get_sync_tip()` - Get current sync status
2. Deep fork detection queries
3. Sync status updates
4. Block validation queries
5. Cell lookup queries
6. Transaction verification
7. Reorg handling

### Step 2: Remove Repository Field from Indexer Struct

**Location**: Around line 260 in `sync/indexer.rs`

**Current**:

```rust
pub struct Indexer {
    config: Config,
    rpc: CkbRpcClient,
    repo: Repository,  // ← REMOVE THIS
    writer: BatchWriter,
    // ... other fields
}
```

**Change to**:

```rust
pub struct Indexer {
    config: Config,
    rpc: CkbRpcClient,
    // repo: Repository,  // ← REMOVED
    writer: BatchWriter,
    // ... other fields
}
```

### Step 3: Update Indexer::new() Method

**Already done** in previous session:

- Signature changed to accept `writer: BatchWriter`
- Repository initialization removed
- Sync tip query stubbed with TODO

**Remaining**: Implement proper sync tip query

**Change**:

```rust
// Current (line ~278):
let tip_number = 0i64;  // TODO: Get from ClickHouse sync_status

// Change to:
let tip_number = writer.get_sync_tip().await?.unwrap_or(0);
```

**Required**: Implement `get_sync_tip()` in ClickHouseWriter

### Step 4: Implement Missing ClickHouseWriter Methods

Add to `crates/indexer/src/db/clickhouse_writer.rs`:

```rust
/// Get current sync tip from sync_status table
pub async fn get_sync_tip(&self) -> Result<Option<i64>> {
    #[derive(Row, Deserialize)]
    struct SyncTipRow {
        tip_block_number: i64,
    }

    let row = self.client
        .client()
        .query("SELECT tip_block_number FROM sync_status WHERE id = 1")
        .fetch_optional::<SyncTipRow>()
        .await?;

    Ok(row.map(|r| r.tip_block_number))
}

/// Update sync status
pub async fn update_sync_status(&self, tip_number: i64, tip_hash: &[u8]) -> Result<()> {
    let query = format!(
        "ALTER TABLE sync_status UPDATE tip_block_number = {}, tip_block_hash = unhex('{}'), updated_at = now() WHERE id = 1",
        tip_number,
        hex::encode(tip_hash)
    );
    self.client.client().query(&query).execute().await?;
    Ok(())
}
```

### Step 5: Replace Each self.repo Usage

For each usage found in Step 1, replace with ClickHouse equivalent:

**Pattern 1: Sync Tip Query**

```rust
// Old:
let (tip_number, tip_hash) = self.repo.get_sync_tip().await?;

// New:
let tip_number = self.writer.get_sync_tip().await?.unwrap_or(0);
let tip_hash = vec![]; // Or query from blocks table if needed
```

**Pattern 2: Deep Fork Detection**

```rust
// Old:
let is_deep_fork = self.repo.check_deep_fork(block_number).await?;

// New:
// Implement check_deep_fork() in ClickHouseWriter or inline the query
let is_deep_fork = false; // Stub for now, implement based on logic
```

**Pattern 3: Block Validation**

```rust
// Old:
let exists = self.repo.block_exists(hash).await?;

// New:
let exists = self.writer.block_exists(hash).await?;
// Implement block_exists() in ClickHouseWriter
```

### Step 6: Implement Additional ClickHouseWriter Methods as Needed

Based on actual usages found, implement:

- `block_exists(hash: &[u8]) -> Result<bool>`
- `check_deep_fork(block_number: i64) -> Result<bool>`
- Any other methods that Repository provided

### Step 7: Remove Repository Import and Struct

**File**: `crates/indexer/src/sync/indexer.rs`

Remove:

```rust
use crate::db::Repository;
```

**File**: `crates/indexer/src/db/mod.rs`

Remove:

```rust
pub mod repository;
pub use repository::Repository;
```

**File**: `crates/indexer/src/db/repository.rs`

Delete this file entirely (after all usages removed)

### Step 8: Verify Compilation

```bash
cargo check -p ckbadger-indexer
```

Should compile with 0 errors.

### Step 9: Test Sync Operation

```bash
# Start ClickHouse
docker compose up -d clickhouse

# Run indexer
cargo run -p ckbadger-indexer
```

Verify:

- Indexer starts without errors
- Syncs blocks from ClickHouse
- Updates sync_status table
- No PostgreSQL connections attempted

## Estimated Time

- Step 1-2: 15 minutes (identify and remove field)
- Step 3-4: 30 minutes (implement get_sync_tip)
- Step 5: 2-3 hours (replace all usages)
- Step 6: 1-2 hours (implement missing methods)
- Step 7: 15 minutes (cleanup)
- Step 8-9: 30 minutes (verify and test)

**Total**: 4-6 hours

## Common Pitfalls

1. **Forgetting to implement a method** - Compile errors will catch this
2. **Incorrect ClickHouse query syntax** - Test queries in clickhouse-client first
3. **Type mismatches** - Use Row structs with proper types (u64 not i64)
4. **Missing hex conversions** - Remember hex() for SELECT, unhex() for WHERE

## Success Criteria

- [ ] `cargo check -p ckbadger-indexer` passes
- [ ] No `self.repo` references remain
- [ ] No Repository imports
- [ ] Indexer starts and syncs blocks
- [ ] sync_status table updates correctly

## Next Steps After This Task

Once Task 3.5 is complete:

- Task 3.3: Delete writer.rs, rename clickhouse_writer.rs
- Task 3.4: Remove sqlx from Cargo.toml
- Phase 3 complete!
