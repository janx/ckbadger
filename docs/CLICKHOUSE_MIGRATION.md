# ClickHouse Migration Guide

This document describes the migration from PostgreSQL + RocksDB to ClickHouse as the sole data store for CKBadger.

## Overview

**Migration Status**: Core implementation complete, integration testing pending.

### What Changed

| Component        | Before                             | After                                |
| ---------------- | ---------------------------------- | ------------------------------------ |
| Primary Database | PostgreSQL 16                      | ClickHouse 24.1                      |
| Cell Cache       | RocksDB                            | In-memory LRU (1M entries)           |
| Schema Location  | `migrations/postgres/001_init.sql` | `migrations/clickhouse/001_init.sql` |
| Connection Pool  | sqlx::PgPool                       | clickhouse::Client                   |

### Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    New ClickHouse Architecture                   │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│   CKB Node (RPC)                                                 │
│        │                                                         │
│        ▼                                                         │
│   ┌─────────────────────────────────────────────────────────┐   │
│   │                    Indexer (Rust)                        │   │
│   │                                                          │   │
│   │  ┌──────────────────┐  ┌──────────────────┐             │   │
│   │  │ LRU Cell Cache   │  │ Canon Version    │             │   │
│   │  │ (~1M entries)    │  │ Manager          │             │   │
│   │  │ ~200MB RAM       │  │                  │             │   │
│   │  └──────────────────┘  └──────────────────┘             │   │
│   │                                                          │   │
│   └──────────────────────────────┬──────────────────────────┘   │
│                                  │                               │
│                                  ▼                               │
│   ┌─────────────────────────────────────────────────────────┐   │
│   │                      ClickHouse                          │   │
│   │                                                          │   │
│   │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐      │   │
│   │  │ Immutable   │  │ Canonical   │  │ Cell State  │      │   │
│   │  │ Fact Tables │  │ Mapping     │  │ (Versioned) │      │   │
│   │  │             │  │ (Versioned) │  │             │      │   │
│   │  │ blocks_all  │  │             │  │ Live cells  │      │   │
│   │  │ txs_all     │  │ height →    │  │ per outpoint│      │   │
│   │  │ outputs_all │  │ block_hash  │  │             │      │   │
│   │  │ inputs_all  │  │             │  │             │      │   │
│   │  │ activities  │  │             │  │             │      │   │
│   │  └─────────────┘  └─────────────┘  └─────────────┘      │   │
│   │                                                          │   │
│   └─────────────────────────────────────────────────────────┘   │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

## Key Design Decisions

### 1. Canonical State is NOT a Row Property

Unlike PostgreSQL where we might use an `is_canonical` column, ClickHouse uses a separate `canonical_blocks` table:

```sql
-- Query canonical blocks
SELECT b.* FROM blocks_all b
INNER JOIN canonical_blocks c ON b.number = c.number AND b.hash = c.block_hash
ORDER BY b.number DESC
```

This approach:

- Keeps fact tables immutable (append-only)
- Avoids expensive ClickHouse mutations
- Preserves orphaned blocks for future P2P analysis

### 2. Table Engine Strategy

| Engine               | Use Case               | Tables                                                                          |
| -------------------- | ---------------------- | ------------------------------------------------------------------------------- |
| MergeTree            | Immutable facts        | blocks_all, transactions_all, cell_outputs_all, cell_inputs_all, activities_all |
| ReplacingMergeTree   | State with versioning  | canonical_blocks, cell_state, dao_deposits, sync_status, tokens                 |
| SummingMergeTree     | Incremental aggregates | address_balances                                                                |
| AggregatingMergeTree | Pre-computed stats     | daily_stats, hourly_stats, miner_statistics                                     |

### 3. Cell State Versioning

Cell state uses insert-only versioning with `canon_version`:

```sql
-- Get latest state for each cell
SELECT * FROM cell_state
ORDER BY canon_version DESC
LIMIT 1 BY (tx_hash, output_index)
HAVING is_live = 1 AND is_present = 1
```

On reorg:

- Consumed cells: Insert new row with `is_live = 1` (restored)
- Disconnected outputs: Insert new row with `is_present = 0` (invalidated)
- Never DELETE or UPDATE existing rows

### 4. FixedString for Hashes

```sql
hash FixedString(32)           -- 32-byte block/tx hashes
nonce FixedString(16)          -- 128-bit nonce
proposal_id FixedString(10)    -- 10-byte proposal short ID
```

Empty string represents NULL (ClickHouse primitives don't have NULL by default).

## Environment Variables

```bash
# ClickHouse connection
CLICKHOUSE_URL=http://localhost:8123
CLICKHOUSE_DATABASE=ckbadger
CLICKHOUSE_USER=default
CLICKHOUSE_PASSWORD=

# CKB Node
CKB_RPC_URL=http://localhost:8114

# Redis (optional, for caching)
REDIS_URL=redis://localhost:6379
```

## Manual Integration Testing

### Prerequisites

1. Docker and Docker Compose installed
2. CKB node running (mainnet or testnet)
3. At least 8GB RAM available

### Step 1: Start ClickHouse

```bash
# Start ClickHouse and Redis
docker compose up -d clickhouse redis

# Wait for ClickHouse to be ready
docker compose exec clickhouse clickhouse-client --query "SELECT 1"

# Verify schema was initialized
docker compose exec clickhouse clickhouse-client --query "SHOW TABLES"
```

Expected output should include:

- blocks_all
- transactions_all
- cell_outputs_all
- cell_inputs_all
- activities_all
- canonical_blocks
- cell_state
- address_balances
- sync_status

### Step 2: Run Indexer

```bash
# Set environment variables
export CLICKHOUSE_URL=http://localhost:8123
export CLICKHOUSE_DATABASE=ckbadger
export CKB_RPC_URL=http://localhost:8114

# Run indexer (sync first 10,000 blocks)
cargo run -p ckbadger-indexer --release -- --target-block 10000
```

Expected output:

```
[INFO] Starting indexer...
[INFO] Connecting to ClickHouse at http://localhost:8123
[INFO] Syncing blocks 0 to 10000...
[INFO] Batch 1: blocks 0-999 (1000 blocks in 2.5s)
...
[INFO] Sync complete: 10000 blocks
```

### Step 3: Verify Data

```bash
# Check block count
docker compose exec clickhouse clickhouse-client --query \
  "SELECT count() FROM blocks_all"

# Check canonical blocks
docker compose exec clickhouse clickhouse-client --query \
  "SELECT count() FROM canonical_blocks FINAL"

# Check transactions
docker compose exec clickhouse clickhouse-client --query \
  "SELECT count() FROM transactions_all t
   INNER JOIN canonical_blocks c ON t.block_number = c.number AND t.block_hash = c.block_hash"

# Check live cells
docker compose exec clickhouse clickhouse-client --query \
  "SELECT count() FROM cell_state
   ORDER BY canon_version DESC
   LIMIT 1 BY (tx_hash, output_index)
   HAVING is_live = 1 AND is_present = 1"
```

### Step 4: Test API

```bash
# Start API server
export CLICKHOUSE_URL=http://localhost:8123
export CLICKHOUSE_DATABASE=ckbadger
cargo run -p ckbadger-api --release &

# Wait for server to start
sleep 5

# Test endpoints
curl http://localhost:3001/api/v1/blocks | jq '.data | length'
curl http://localhost:3001/api/v1/blocks/0 | jq '.data.number'
curl http://localhost:3001/api/v1/transactions | jq '.data | length'
curl http://localhost:3001/api/v1/statistics/network | jq '.'
```

### Step 5: Performance Benchmark

```bash
# Reset database
docker compose exec clickhouse clickhouse-client --query "TRUNCATE TABLE blocks_all"
docker compose exec clickhouse clickhouse-client --query "TRUNCATE TABLE transactions_all"
docker compose exec clickhouse clickhouse-client --query "TRUNCATE TABLE canonical_blocks"
# ... truncate other tables

# Run timed sync
time cargo run -p ckbadger-indexer --release -- --target-block 100000

# Calculate blocks/sec
# Expected: >= 10,000 blocks/sec
```

## Troubleshooting

### Connection Refused

```
Error: Connection refused (os error 111)
```

**Solution**: Ensure ClickHouse is running:

```bash
docker compose up -d clickhouse
docker compose logs clickhouse
```

### Table Not Found

```
Error: Table 'ckbadger.blocks_all' doesn't exist
```

**Solution**: Initialize schema:

```bash
docker compose exec clickhouse clickhouse-client < migrations/clickhouse/001_init.sql
```

### Out of Memory

```
Error: Memory limit exceeded
```

**Solution**: Increase ClickHouse memory limits in docker-compose.yml:

```yaml
clickhouse:
  environment:
    CLICKHOUSE_MAX_MEMORY_USAGE: 8000000000 # 8GB
```

### Slow Queries

For queries on large tables, ensure you're using the canonical JOIN pattern:

```sql
-- Good: Uses canonical_blocks JOIN
SELECT b.* FROM blocks_all b
INNER JOIN canonical_blocks c ON b.number = c.number AND b.hash = c.block_hash

-- Bad: Scans entire table
SELECT * FROM blocks_all WHERE is_canonical = 1  -- Don't do this!
```

## Migration Commits

| Commit  | Description                                          |
| ------- | ---------------------------------------------------- |
| 6ca3c40 | Wave 1: ClickHouse schema, remove PostgreSQL/RocksDB |
| 8a92cb8 | Wave 2: ClickHouse client and canon version manager  |
| 1d370fd | Wave 2: Batch writer and row types                   |
| b469169 | Wave 3: Block connect/disconnect and activities      |
| ae5b869 | Wave 4: API routes for ClickHouse                    |
| fad45da | Wave 5: Docker configuration                         |

## Files Changed

### New Files

- `migrations/clickhouse/001_init.sql` - ClickHouse schema (41 tables)
- `crates/common/src/clickhouse.rs` - Shared ClickHouse client
- `crates/indexer/src/cache/cell_cache.rs` - LRU cell cache
- `crates/indexer/src/state/canon_version.rs` - Canon version manager
- `crates/indexer/src/db/writer/rows.rs` - ClickHouse row types

### Modified Files

- `docker-compose.yml` - PostgreSQL → ClickHouse
- `crates/api/src/routes/*.rs` - ClickHouse queries
- `crates/indexer/src/sync/indexer.rs` - Block connect/disconnect

### Deleted Files

- `crates/indexer/src/db/rocksdb_live_cell_store.rs`
- `crates/indexer/src/db/parallel_copy.rs`
- `crates/indexer/src/db/copy_*.rs` (PostgreSQL COPY writers)
- `crates/api/tests/api_integration.rs` (PostgreSQL tests)

## Performance Expectations

| Metric        | PostgreSQL      | ClickHouse (Expected)      |
| ------------- | --------------- | -------------------------- |
| Sync Speed    | 3-5K blocks/sec | 15-40K blocks/sec          |
| Storage       | ~650GB          | ~50GB (10-15x compression) |
| Query Latency | 10-50ms         | 5-20ms                     |
