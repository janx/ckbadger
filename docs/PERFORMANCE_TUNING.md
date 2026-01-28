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

| Parameter                      | Value | Purpose                                      |
| ------------------------------ | ----- | -------------------------------------------- |
| `synchronous_commit`           | off   | 2-3x write speed (safe for re-syncable data) |
| `commit_delay`                 | 10000 | Batch commits in 10ms window                 |
| `max_wal_size`                 | 64GB  | Reduce WAL-triggered checkpoint frequency    |
| `min_wal_size`                 | 8GB   | Keep more WAL segments cached                |
| `checkpoint_timeout`           | 60min | Reduce time-triggered checkpoint frequency   |
| `checkpoint_completion_target` | 0.9   | Spread checkpoint I/O over time              |

> **Note:** Checkpoint optimization is critical for sustained write performance. During bulk sync, PostgreSQL can write 17+ GB of WAL between checkpoints. If checkpoints occur too frequently, they cause 15-20 second write stalls.

### 2. Indexer Parameters

Optimized defaults for high throughput:

```bash
cargo run -p ckbadger-indexer -- \
  --batch-size 1000 \
  --parallel-fetch-size 32 \
  --pipeline-buffer 6
```

| Parameter             | Default | Tuning Range | Notes                                               |
| --------------------- | ------- | ------------ | --------------------------------------------------- |
| `batch_size`          | 1000    | 500-2000     | Higher = more work per DB round-trip                |
| `parallel_fetch_size` | 32      | 16-64        | RPC is fast, prefetch more                          |
| `pipeline_buffer`     | 6       | 4-8          | DB is bottleneck, reduce memory                     |
| `bulk_sync_threshold` | 1000    | 500-10000    | Blocks behind chain tip to auto-enable bulk sync    |
| `use_copy_bulk_sync`  | true    | true/false   | Use PostgreSQL COPY during bulk sync (5-10x faster) |
| `copy_pool_size`      | 8       | 4-16         | Number of COPY connection pool connections          |

### 3. Bulk Sync Mode (Auto-Enabled)

Bulk sync is **automatically enabled** when more than `bulk_sync_threshold` blocks behind the chain tip. No manual configuration needed.

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
- Address balances, live cells, address_transactions (address pages)
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

Cell consumption (`UPDATE cells` + `DELETE live_cells`) now runs concurrently rather than sequentially.

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

### Periodic DB Write Spikes (15-25 seconds)

**Symptom:** `PERF` logs show DB time jumping from ~3s to 15-25s periodically.

**Cause:** PostgreSQL checkpoint in progress.

**Diagnosis:**

```bash
# Check if checkpoint correlates with slow PERF
docker logs ckbadger-indexer 2>&1 | grep -E "PERF|slow statement" | tail -20
docker exec ckbadger-postgres grep "checkpoint" \
  /var/lib/postgresql/data/log/postgresql-$(date +%Y-%m-%d).log | tail -5
```

**Solution:** Increase checkpoint interval:

```sql
-- Apply without restart
ALTER SYSTEM SET checkpoint_timeout = '60min';
ALTER SYSTEM SET max_wal_size = '64GB';
SELECT pg_reload_conf();
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
VACUUM ANALYZE live_cells;
```

Or enable aggressive autovacuum (already configured in `postgresql.conf`).
