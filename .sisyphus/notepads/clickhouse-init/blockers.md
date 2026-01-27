# ClickHouse Indexer Initialization - Blockers

## BLOCKER: Indexer::new() Signature Mismatch

**Status**: BLOCKING - Prevents compilation

**Location**: `crates/indexer/src/sync/indexer.rs:269`

**Current Signature**:

```rust
pub async fn new(
    config: Config,
    pool: PgPool,
    integrity_handle: Option<IntegrityServiceHandle>,
) -> Result<Self>
```

**Problem**:

- The signature expects `PgPool` (PostgreSQL connection pool)
- We're trying to pass `ClickHouseWriter` instead
- The Indexer struct still uses PostgreSQL-based `Repository` internally

**What Needs to Change**:

1. Update `Indexer::new()` to accept `ClickHouseWriter` instead of `PgPool`
2. Update `Repository` to work with ClickHouse instead of PostgreSQL
3. Adapt all database operations in `Indexer` to use ClickHouse queries

**Current Code in main.rs** (lines 101):

```rust
let indexer = ckbadger_indexer::sync::Indexer::new(config.clone(), writer, None).await?;
```

**Error Message**:

```
error[E0308]: mismatched types
   --> crates/indexer/src/main.rs:101:72
    |
101 |     let indexer = ckbadger_indexer::sync::Indexer::new(config.clone(), writer, None).await?;
    |                   ------------------------------------                 ^^^^^^ expected `Pool<Postgres>`, found `ClickHouseWriter`
```

## Implementation Status

✅ **Completed**:

- ClickHouseClient initialization
- ClickHouseWriter creation
- Main function structure with proper error handling

❌ **Blocked**:

- Indexer initialization (requires signature update)
- Sync pipeline startup (depends on Indexer initialization)

## Next Steps

To unblock this:

1. Modify `Indexer::new()` signature to accept `ClickHouseWriter`
2. Update `Repository` struct to use ClickHouse instead of PostgreSQL
3. Adapt all SQL queries in Repository to ClickHouse syntax
4. Update Indexer's internal database operations
