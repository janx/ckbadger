# Learnings - ClickHouse Migration

## Conventions & Patterns

(This file will accumulate conventions, patterns, and learnings discovered during implementation)

---

## Task 0.1: ClickHouse Benchmark Environment Setup (Completed)

### Infrastructure Configuration

**Docker Compose Service:**

- Image: `clickhouse/clickhouse-server:latest`
- Ports: 8123 (HTTP), 9000 (Native protocol)
- Profile: `benchmark` (isolated from production services)
- Volume: `clickhouse-data` for persistence
- Healthcheck: `clickhouse-client --query 'SELECT 1'`
- ulimits: nofile 262144 (required for high-throughput writes)

**Key Configuration Decisions:**

1. Used `profiles: [benchmark]` to prevent accidental startup with production services
2. Mounted `migrations/clickhouse/` to `/docker-entrypoint-initdb.d` for auto-initialization
3. Set `CLICKHOUSE_DEFAULT_ACCESS_MANAGEMENT: 1` for user management

### Schema Design

**MergeTree Engine Configuration:**

- `PARTITION BY intDiv(created_at_block, 1000000)` - 1M block partitions (18 partitions for full chain)
- `ORDER BY (created_at_block, tx_hash, output_index)` - Optimized for block range queries
- `PRIMARY KEY (created_at_block, tx_hash, output_index)` - Enables OutPoint lookup
- `index_granularity = 8192` - Default, good balance for our use case

**Table Structure:**

1. `cells` - Main table (simplified from Postgres schema)
2. `live_cells` - Separate table for O(1) OutPoint lookup (70% of cells)
3. `cells_by_lock` - Index table for address balance queries
4. `benchmark_stats` - Performance metrics storage

**Data Type Choices:**

- `FixedString(32)` for hashes (more efficient than String for fixed-length data)
- `UInt64` for capacity/block numbers (matches CKB domain)
- `Nullable()` for optional fields (type*script, consumed_at*\*)
- `DateTime` for timestamps (automatic now() default)

### Sample Data Generation

**Statistics (1M rows):**

- Total Cells: 1,000,000
- Live Cells: 700,000 (70%)
- Dead Cells: 300,000 (30%)
- Cells with Type Script: ~300,000 (30%)

**Compression Results:**

- Uncompressed: 300.03 MiB
- Compressed: 58.29 MiB
- Disk Size: 58.31 MiB
- **Compression Ratio: 5.15x** (validates 5-10x expectation)

**Data Distribution:**

- Capacity: 70% small (61-200 CKB), 25% medium (200-1000 CKB), 5% large (1000-10000 CKB)
- Output Index: 0-3 (realistic transaction output distribution)
- Block Range: 0-18M (full mainnet range)
- Lock Script: Secp256k1 (most common)
- Type Script: 30% have type script (sUDT/DAO/etc)

### Gotchas Encountered

1. **File Execution Order:** docker-entrypoint-initdb.d runs files alphabetically
   - Solution: Renamed to `001_test_schema.sql` and `002_generate_sample_data.sql`

2. **UNION ALL Type Mismatch:** ClickHouse strict type checking in UNION
   - Error: "No supertype for Float64, UInt64"
   - Solution: Cast count() to Float64 explicitly: `toFloat64(count())`

3. **MD5 Hash Length:** MD5 produces 16-byte hash, need 32 bytes for CKB hashes
   - Solution: Concatenate two MD5 hashes: `concat(substring(MD5(...), 1, 32), substring(MD5(...), 1, 32))`

4. **Lock Args Length:** Need 20-byte address hash (40 hex chars)
   - Solution: Concatenate two 20-byte MD5 substrings

### Verification Commands

```bash
# Start ClickHouse
docker compose --profile benchmark up -d clickhouse

# Test connectivity
docker compose exec clickhouse clickhouse-client --query "SELECT 1"

# Show tables
docker compose exec clickhouse clickhouse-client --query "SHOW TABLES FROM ckbadger_test"

# Check row counts
docker compose exec clickhouse clickhouse-client --query "SELECT count() FROM ckbadger_test.cells"
docker compose exec clickhouse clickhouse-client --query "SELECT count() FROM ckbadger_test.live_cells"

# Check compression
docker compose exec clickhouse clickhouse-client --query "
  SELECT
    formatReadableSize(sum(bytes_on_disk)) AS disk_size,
    formatReadableSize(sum(data_compressed_bytes)) AS compressed,
    formatReadableSize(sum(data_uncompressed_bytes)) AS uncompressed
  FROM system.parts
  WHERE database = 'ckbadger_test' AND table = 'cells' AND active
"
```

### Next Steps (Task 0.2)

Ready for write performance benchmark:

- Test bulk insert of 1M rows in batches
- Measure rows/second throughput
- Compare with Postgres baseline
- Target: 100K+ rows/second sustained
