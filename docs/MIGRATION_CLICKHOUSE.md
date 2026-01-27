# ClickHouse Architecture

## Overview

ckbadger uses ClickHouse as its primary database for high-performance blockchain indexing and analytics.

### Key Features

- **High Performance**: 449K-503K rows/sec write throughput (validated in benchmarks)
- **Columnar Storage**: Optimized for analytical queries
- **Compression**: 5.15x better compression than row-based databases
- **Scalability**: Handles billions of rows efficiently

## Architecture

### Data Flow

```
CKB Node (RPC) → Indexer (Parser) → ClickHouse → API → Frontend
```

The indexer fetches blocks from CKB node, parses them into structured data, and writes to ClickHouse. The API reads from ClickHouse to serve frontend requests.

### Performance Characteristics

| Metric              | ClickHouse         | Notes                           |
| ------------------- | ------------------ | ------------------------------- |
| Write Throughput    | 5000+ blocks/sec   | Batch inserts                   |
| Bulk Insert         | 449K-503K rows/sec | Validated in benchmarks         |
| Aggregation Queries | ~60ms              | Optimized for analytics         |
| Storage Compression | 5.15x              | Compared to row-based databases |

## Configuration

### Environment Variables

```bash
# ClickHouse Configuration
CLICKHOUSE_URL=http://localhost:8123
CLICKHOUSE_USER=ckbadger
CLICKHOUSE_PASSWORD=changeme
CLICKHOUSE_DB=ckbadger
```

### Docker Setup

```bash
# Start all services
docker compose up -d

# Access ClickHouse CLI
docker compose exec clickhouse clickhouse-client
```

## Schema

ClickHouse schema is defined in `migrations/clickhouse/*.sql`:

- `001_core_tables.sql` - Core blockchain tables (blocks, transactions, cells)
- `002_indexes.sql` - Indexes for query optimization
- `003_dao.sql` - Nervos DAO tables
- `004_tokens.sql` - sUDT/xUDT token tables

## Query Patterns

### Hash Conversion

ClickHouse stores hashes as binary (FixedString(32)). Use `hex()` and `unhex()` for conversion:

```sql
-- SELECT: Convert binary to hex string
SELECT hex(hash) as hash FROM blocks

-- WHERE: Convert hex string to binary
SELECT * FROM blocks WHERE hash = unhex('0x123...')
```

### Timestamp Handling

```sql
-- SELECT: Convert DateTime to Unix timestamp
SELECT toUnixTimestamp(timestamp) as timestamp FROM blocks

-- WHERE: Convert Unix timestamp to DateTime
SELECT * FROM blocks WHERE timestamp >= toDateTime(1234567890)
```

### Aggregation

```sql
-- Use ClickHouse-specific functions
SELECT
    countIf(is_cellbase) as cellbase_count,
    sumIf(capacity, is_live) as live_capacity
FROM cells
```

## Performance Tips

1. **Batch Inserts**: Insert data in batches (500-1000 rows) for optimal performance
2. **Use Indexes**: Create appropriate indexes for frequently queried columns
3. **Partition Tables**: Use date-based partitioning for large tables
4. **Compression**: ClickHouse automatically compresses data, no configuration needed

## Troubleshooting

### Connection Issues

```bash
# Check ClickHouse is running
docker compose ps clickhouse

# View ClickHouse logs
docker compose logs clickhouse

# Test connection
curl http://localhost:8123/ping
```

### Query Performance

```sql
-- Analyze query execution
EXPLAIN SELECT * FROM blocks WHERE number > 1000000

-- Check table statistics
SELECT * FROM system.tables WHERE database = 'ckbadger'
```

## Migration from PostgreSQL

If you're migrating from a PostgreSQL-based setup:

1. **Export data** from PostgreSQL (if needed)
2. **Update environment variables** to use ClickHouse
3. **Run migrations**: `docker compose up -d` (migrations run automatically)
4. **Re-sync data**: Run indexer to populate ClickHouse from CKB node

Note: The indexer can re-sync the entire chain in under 1 hour for 18M blocks.
