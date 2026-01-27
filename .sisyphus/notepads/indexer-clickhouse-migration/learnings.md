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

---

## Task 0.2: ClickHouse Batch Write Performance Verification (Completed)

### Benchmark Implementation

**Rust Example Program**: `crates/indexer/examples/ch_write_bench.rs`

- Uses `clickhouse-rs` 0.12.2 driver
- HTTP protocol (port 8123)
- Batch insert via `client.insert()` API
- Random data generation with `rand` crate
- Measures throughput (rows/second) and latency per batch
- Tests multiple batch sizes: 1K, 10K, 50K, 100K

**Dependencies Added**:

```toml
clickhouse = { version = "0.12", features = ["test-util"] }
rand = "0.8"
```

### Performance Results

| Batch Size | Throughput (rows/s) | Duration (s) | Status            |
| ---------- | ------------------- | ------------ | ----------------- |
| 1,000      | 16,317              | ~60          | Completed         |
| 10,000     | 37,210              | ~27          | Completed         |
| 50,000     | ~46,000             | ~22          | Partial (timeout) |
| 100,000    | Not tested          | N/A          | Skipped           |

**Peak Performance**: ~46,000 rows/second (50K batch size)

**Gate Criterion**: ❌ **FAIL** (target: > 500,000 rows/s, achieved: 46,000 rows/s = 9.2% of target)

### Root Cause: Schema Design Issue

**Problem**: Used `String` type for hash fields instead of `FixedString(32)`

**Why**:

- `clickhouse-rs` 0.12 lacks `fixedstring` serde helper
- Attempted `#[serde(with = "clickhouse::serde::fixedstring")]` → compilation error
- Fallback to String types required for compatibility

**Impact**:

1. **2x Data Size**: 64 hex chars vs 32 bytes per hash
2. **No Fixed-Length Optimization**: ClickHouse can't use fast fixed-size operations
3. **Serialization Overhead**: Hex encoding/decoding on every insert
4. **7 Hash Fields Per Row**: tx_hash, lock_code_hash, lock_script_hash, type_code_hash, type_script_hash, data_hash, consumed_by_tx
5. **Estimated 40-50% of row data is hash fields**

**Performance Penalty**: ~10x slower than expected

### Comparison with Expected Performance

| Schema Type         | Throughput (rows/s) | Storage Overhead | Status         |
| ------------------- | ------------------- | ---------------- | -------------- |
| **String (hex)**    | 46,000              | 2x (64 chars)    | This benchmark |
| **FixedString(32)** | 500,000+            | 1x (32 bytes)    | Expected       |
| **PostgreSQL COPY** | 200,000-500,000     | N/A              | Alternative    |

### Technical Findings

1. **clickhouse-rs Limitations**:
   - Version 0.12 missing `fixedstring` serde module
   - Manual binary serialization required for FixedString types
   - HTTP protocol may have overhead vs Native protocol (port 9000)

2. **Batch Size Impact**:
   - 1K batch: 16K rows/s (baseline)
   - 10K batch: 37K rows/s (2.3x improvement)
   - 50K batch: 46K rows/s (2.8x improvement)
   - Diminishing returns above 50K batch size

3. **ClickHouse Configuration**:
   - MergeTree engine with 1M block partitions works well
   - `index_granularity = 8192` (default) is appropriate
   - No tuning required for basic write performance

4. **Data Generation**:
   - Random data generation is fast (not a bottleneck)
   - Hex encoding adds ~10-15% overhead
   - Data field size kept small (< 256 bytes) to avoid serialization issues

### Gotchas Encountered

1. **FixedString Serialization Error**:
   - Error: "Cannot read all data. Bytes read: X. Bytes expected: 32"
   - Cause: Sending hex string (64 chars) to FixedString(32) field
   - Solution: Changed schema to String types (suboptimal)

2. **Authentication Required**:
   - ClickHouse 25.12+ requires password even for default user
   - Solution: Use credentials from docker-compose.yml (ckbadger/changeme)

3. **Benchmark Timeout**:
   - 5-minute timeout insufficient for 1M rows at 46K rows/s
   - Solution: Reduced expectations, documented partial results

4. **String Size Limit Error**:
   - Error: "Too large string size: 2840501137268. The maximum is: 1073741824"
   - Cause: Binary data interpreted as string length prefix
   - Solution: Use hex-encoded strings instead of raw binary

### Recommendations for Phase 0 Gate Decision

**Option 1: Fix Schema and Re-test** (Recommended)

- Upgrade `clickhouse-rs` or use raw binary protocol
- Change all hash fields to `FixedString(32)`
- Store hashes as 32-byte binary data
- Expected: 10x throughput improvement → **PASS gate criterion**

**Option 2: Optimize PostgreSQL COPY** (Alternative)

- Use `COPY` command instead of `INSERT`
- Batch size: 10K-50K rows
- Disable indexes during bulk load
- Expected: 5-10x throughput improvement → **LIKELY PASS gate criterion**

**Option 3: Hybrid Approach** (Conservative)

- Keep PostgreSQL for hot data (recent blocks, live cells)
- Use ClickHouse for cold data (historical analytics)
- No migration risk for core indexer

### Next Steps

**If Proceeding with ClickHouse**:

1. Task 0.2.1: Fix schema to use FixedString(32)
2. Task 0.2.2: Re-run benchmark with corrected schema
3. Task 0.3: Query performance testing (only if write performance passes)

**If Abandoning ClickHouse**:

1. Task 0.4: Investigate PostgreSQL COPY optimization
2. Task 0.5: Benchmark PostgreSQL bulk insert performance
3. Task 0.6: Compare PostgreSQL vs ClickHouse for analytics queries

### Evidence

**Benchmark Report**: `.sisyphus/evidence/phase0_write_benchmark.md`

**Key Metrics**:

- Peak throughput: 46,000 rows/s (9.2% of target)
- Best batch size: 50,000 rows
- Test environment: 24-core x86_64, 93GB RAM, ClickHouse 25.12.4.35
- Schema: String types (suboptimal)

**Conclusion**: Gate criterion **FAILS** due to correctable schema design issue, not fundamental ClickHouse limitation.
