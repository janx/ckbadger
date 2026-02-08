# Performance Tuning Guide

This guide covers performance optimization strategies for the ckbadger indexer at scale (10M+ blocks).

## Quick Wins

### 1. PostgreSQL Configuration

The `docker/postgres/postgresql.conf` is pre-optimized for write-heavy blockchain indexing (93GB RAM, NVMe SSD, 24-core CPU):

**Memory Settings:**

| Parameter              | Value | Purpose                                     |
| ---------------------- | ----- | ------------------------------------------- |
| `shared_buffers`       | 24GB  | 25% of RAM for buffer cache                 |
| `work_mem`             | 512MB | Per-operation memory for complex queries    |
| `maintenance_work_mem` | 4GB   | Memory for VACUUM, CREATE INDEX             |
| `effective_cache_size` | 70GB  | Query planner's estimate of available cache |

**WAL & Checkpoint Settings (Critical for Bulk Sync):**

| Parameter                      | Value | Purpose                                               |
| ------------------------------ | ----- | ----------------------------------------------------- |
| `synchronous_commit`           | off   | 2-3x write speed (safe for re-syncable data)          |
| `commit_delay`                 | 10000 | Batch commits in 10ms window                          |
| `wal_compression`              | lz4   | Reduce WAL volume by 30-50% for COPY workloads        |
| `full_page_writes`             | off   | Eliminate 8KB page images (data is re-syncable)       |
| `max_wal_size`                 | 8GB   | Trigger checkpoint every ~8GB of WAL                  |
| `min_wal_size`                 | 4GB   | Keep WAL segments cached                              |
| `checkpoint_timeout`           | 15min | Frequent small checkpoints prevent dirty page buildup |
| `checkpoint_completion_target` | 0.9   | Spread checkpoint I/O over time                       |

> **Note:** With `wal_compression=lz4` + `full_page_writes=off`, WAL volume is ~3-4x smaller than default. This allows frequent small checkpoints (15min/8GB) without causing stalls. The key tradeoff: too-infrequent checkpoints cause dirty page accumulation in shared_buffers, forcing backends to evict dirty pages themselves (backend writes in `pg_stat_bgwriter`), which causes periodic 2x write time spikes.

### 2. Indexer Parameters

Optimized defaults for high throughput:

```bash
cargo run -p ckbadger-indexer -- \
  --batch-size 1000 \
  --parallel-fetch-size 32 \
  --pipeline-buffer 6
```

| Parameter             | Default | Tuning Range | Notes                                                                          |
| --------------------- | ------- | ------------ | ------------------------------------------------------------------------------ |
| `batch_size`          | 1000    | 500-2000     | Higher = more work per DB round-trip                                           |
| `parallel_fetch_size` | 32      | 16-64        | RPC is fast, prefetch more                                                     |
| `pipeline_buffer`     | 6       | 4-8          | DB is bottleneck, reduce memory                                                |
| `bulk_sync_threshold` | 72      | 50-10000     | Blocks behind chain tip to auto-enable bulk sync (default: 2x DEEP_FORK_DEPTH) |
| `use_copy_bulk_sync`  | true    | true/false   | Use PostgreSQL COPY during bulk sync (5-10x faster)                            |
| `copy_pool_size`      | 8       | 4-16         | Number of COPY connection pool connections                                     |

### 3. Bulk Sync Mode (Auto-Enabled)

Bulk sync is **automatically enabled** when more than `bulk_sync_threshold` blocks behind the chain tip (default: 72, which is 2x DEEP_FORK_DEPTH). No manual configuration needed.

**When active (blocks_remaining > threshold):**

- Uses PostgreSQL Binary COPY for 5-10x faster writes
- Skips non-critical chart statistics

**Skipped writes (chart data only):**

- `hourly_statistics` - Hourly aggregates for charts
- `miner_statistics` - Miner ranking data
- `block_time_distribution` - Block time histogram
- `epoch_time_distribution` - Epoch time histogram

**Always written:**

- Blocks, transactions, cells, inputs (core data)
- Address balances, address_transactions (address pages)
- Daily statistics, epoch statistics (important aggregates)
- DAO deposits/withdrawals (financial data)
- Sync status (crash recovery)

**Impact during bulk sync:**

- Address pages work normally (balance, transaction history)
- Some chart pages show incomplete data until sync catches up

**Expected speedup:** 5-10x faster initial sync with COPY

```bash
# Default: auto-enables bulk sync + COPY when far from tip
cargo run -p ckbadger-indexer

# Custom threshold (enter normal mode when within 5000 blocks)
cargo run -p ckbadger-indexer -- --bulk-sync-threshold 5000

# Disable COPY (use UNNEST only)
cargo run -p ckbadger-indexer -- --use-copy-bulk-sync false
```

## Architecture Optimizations

### Parallel Statistics Writes

Statistics updates (`hourly_statistics`, `daily_statistics`, etc.) are now written in parallel using `tokio::try_join!`, reducing the critical path by 30-50%.

### Concurrent Cell Consumption

Cell consumption (`UPDATE cells SET status = 1`) uses partition-aware batch updates for efficient cross-partition operations.

### LRU Cache

A 200,000-entry LRU cache stores recently created cells, eliminating DB lookups for cells consumed within the same sync window.

## Expected Performance

| Configuration        | Throughput          | Notes                     |
| -------------------- | ------------------- | ------------------------- |
| Default (untuned)    | ~150-200 blocks/sec | Conservative settings     |
| Optimized PostgreSQL | ~250-350 blocks/sec | Config changes only       |
| Full optimization    | ~400-500 blocks/sec | All optimizations applied |

## Monitoring

### Key Metrics

Watch the `PERF` log lines:

```
PERF[1000blks] RPC=125.3ms DB=1450.2ms
```

- **RPC time** < 500ms: Good (fetcher isn't bottleneck)
- **DB time** > 1000ms: Database is bottleneck (expected)

### PostgreSQL Monitoring

```sql
-- Check for slow queries
SELECT * FROM pg_stat_statements
ORDER BY total_exec_time DESC
LIMIT 10;

-- Check table sizes
SELECT schemaname, relname, pg_size_pretty(pg_total_relation_size(relid))
FROM pg_stat_user_tables
ORDER BY pg_total_relation_size(relid) DESC;

-- Check index usage
SELECT indexrelname, idx_scan, idx_tup_read, idx_tup_fetch
FROM pg_stat_user_indexes
ORDER BY idx_scan DESC;
```

### Checkpoint Monitoring

Checkpoints are the #1 cause of periodic DB write stalls during bulk sync.

```bash
# Check current checkpoint settings
docker exec ckbadger-postgres psql -U ckbadger -d ckbadger -c \
  "SHOW checkpoint_timeout; SHOW max_wal_size;"

# Check checkpoint statistics
docker exec ckbadger-postgres psql -U ckbadger -d ckbadger -c \
  "SELECT checkpoints_timed, checkpoints_req,
          checkpoint_write_time/1000 as write_sec,
          checkpoint_sync_time/1000 as sync_sec,
          buffers_checkpoint
   FROM pg_stat_bgwriter;"

# View recent checkpoint events (with timing)
docker exec ckbadger-postgres grep -i "checkpoint" \
  /var/lib/postgresql/data/log/postgresql-$(date +%Y-%m-%d).log | tail -10
```

**Healthy checkpoint indicators:**

- `checkpoints_req` (WAL-triggered) should be low relative to `checkpoints_timed`
- Checkpoint write time should be < 5 minutes
- No correlation between checkpoint times and PERF DB spikes

## Scaling Beyond Single Node

For extreme scale (100M+ blocks), consider:

1. **Citus** for distributed PostgreSQL
2. **TimescaleDB** for automatic time-series partitioning
3. **Read replicas** for API queries
4. **Redis caching** for hot data (already supported via `--redis-url`)

## Troubleshooting

### Sync Stuck / Slow

1. Check `PERF` logs for bottleneck (RPC vs DB)
2. Verify PostgreSQL config is loaded: `SHOW synchronous_commit;`
3. Check for lock contention: `SELECT * FROM pg_stat_activity WHERE wait_event IS NOT NULL;`
4. Increase `batch_size` if DB time is low

### Periodic DB Write Spikes (2x normal)

**Symptom:** `PERF` logs show DB time jumping from ~3s to 7-9s every 2-3 batches.

**Cause 1: Backend dirty page eviction.** When `checkpoint_timeout` or `max_wal_size` is too large, dirty pages accumulate in shared_buffers. Backends are forced to evict dirty pages to disk before reading new ones.

**Diagnosis:**

```bash
# Check backend writes vs checkpoint writes
docker exec ckbadger-postgres psql -U ckbadger -d ckbadger -c \
  "SELECT pg_size_pretty(buffers_checkpoint * 8192::bigint) as ckpt_written,
          pg_size_pretty(buffers_clean * 8192::bigint) as bgwriter_written,
          pg_size_pretty(buffers_backend * 8192::bigint) as backend_written,
          checkpoints_timed, checkpoints_req
   FROM pg_stat_bgwriter;"
```

If `backend_written` is large (GBs), checkpoints are too infrequent.

**Solution:** Reduce checkpoint interval (with `wal_compression=lz4` + `full_page_writes=off`, small checkpoints are fast):

```sql
-- Apply without restart
ALTER SYSTEM SET checkpoint_timeout = '15min';
ALTER SYSTEM SET max_wal_size = '8GB';
SELECT pg_reload_conf();
```

**Cause 2: DAO statistics recalculation.** `recalculate_dao_extended_statistics()` runs every 1,000 blocks and scans all active deposits. During bulk sync this is skipped automatically, but if the `is_bulk_sync_active()` guard is missing, it causes connection pool contention.

**Diagnosis:**

```bash
# Look for slow DAO queries and pool acquire warnings
docker logs ckbadger-indexer 2>&1 | grep -E "slow statement.*dao|pool::acquire" | tail -10
```

Or update `docker/postgres/postgresql.conf` for persistence.

### High Memory Usage

1. Reduce `pipeline_buffer` (e.g., 4)
2. Reduce `batch_size` (e.g., 500)
3. Check for memory leaks with `cargo run --release`

### Database Bloat

Run periodic maintenance:

```sql
VACUUM ANALYZE cells;
VACUUM ANALYZE transactions;
```

Or enable aggressive autovacuum (already configured in `postgresql.conf`).
