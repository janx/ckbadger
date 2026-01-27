-- ClickHouse Statistics and Materialized Views Schema
-- Phase 2: Statistics tables and materialized views for network metrics
--
-- Design Philosophy:
-- 1. Use materialized views ONLY when query cost > storage cost
-- 2. Prefer real-time aggregation for fast queries (< 50ms)
-- 3. Pre-aggregate expensive historical queries (> 100ms)
-- 4. Avoid over-engineering - start simple, add MVs only when needed
--
-- Materialized View Strategy:
-- - Daily statistics: Pre-aggregate (historical data, queried frequently)
-- - Script usage: Pre-aggregate (expensive full table scan)
-- - Address balance: Real-time query (8.26ms P95, fast enough)
-- - Network metrics: Real-time query (simple queries on blocks table)
--
-- Performance Expectations (from Phase 0 benchmarks):
-- - Address balance query: 8.26ms (P95) - NO materialized view needed
-- - Single OutPoint query: 7.97ms (P95) - NO materialized view needed
-- - JOIN query: 60.92ms (P95) - Acceptable for real-time

USE ckbadger;

-- ============================================================================
-- DAILY STATISTICS MATERIALIZED VIEW
-- ============================================================================
-- Pre-aggregates daily blockchain metrics for historical charts and dashboards
-- Engine: SummingMergeTree - automatically sums numeric columns during merges
-- Partition: By month (toYYYYMM) for efficient time-range queries
-- Update: Automatically on INSERT to blocks/transactions/cells tables
--
-- Query pattern: SELECT * FROM daily_statistics WHERE date >= '2024-01-01'
-- Performance: < 10ms for 1 year of data (365 rows)
--
-- Why materialized view?
-- - Historical data queried frequently (dashboard, charts)
-- - Expensive to compute on-demand (full table scan of blocks/transactions)
-- - Data changes infrequently (only new blocks added)
-- - Storage cost acceptable (~365 rows/year)

CREATE TABLE IF NOT EXISTS daily_statistics (
    -- Date dimension
    date Date,                          -- Day (YYYY-MM-DD)
    
    -- Block metrics
    blocks_count UInt32,                -- Number of blocks mined this day
    avg_block_time_ms UInt32,           -- Average block time in milliseconds
    min_block_time_ms UInt32,           -- Minimum block time
    max_block_time_ms UInt32,           -- Maximum block time
    
    -- Transaction metrics
    transactions_count UInt32,          -- Number of transactions
    avg_tx_per_block Float32,           -- Average transactions per block
    
    -- Cell metrics
    cells_created UInt32,               -- Number of cells created (outputs)
    cells_consumed UInt32,              -- Number of cells consumed (inputs)
    
    -- Capacity metrics (in shannon)
    total_capacity UInt64,              -- Total capacity in all transactions
    avg_capacity_per_tx UInt64,         -- Average capacity per transaction
    
    -- Network metrics
    avg_difficulty Float64,             -- Average difficulty
    total_uncles UInt32                 -- Total uncle blocks
) ENGINE = SummingMergeTree()
PARTITION BY toYYYYMM(date)
ORDER BY (date)
PRIMARY KEY (date)
COMMENT 'Daily blockchain statistics - pre-aggregated for historical queries';

-- Materialized view to populate daily_statistics from blocks table
-- Triggers on INSERT to blocks table
-- Aggregates by day using toDate(timestamp)
--
-- SummingMergeTree automatically sums numeric columns during merges
-- No aggregate functions in SELECT - just column references
-- The engine handles aggregation automatically
CREATE MATERIALIZED VIEW IF NOT EXISTS daily_statistics_mv TO daily_statistics AS
SELECT
    toDate(timestamp) as date,
    1 as blocks_count,
    0 as avg_block_time_ms,
    0 as min_block_time_ms,
    0 as max_block_time_ms,
    transactions_count,
    toFloat32(transactions_count) as avg_tx_per_block,
    0 as cells_created,
    0 as cells_consumed,
    0 as total_capacity,
    0 as avg_capacity_per_tx,
    compact_target as avg_difficulty,
    uncles_count as total_uncles
FROM blocks;

-- Materialized view to update cells_created from cells table
CREATE MATERIALIZED VIEW IF NOT EXISTS daily_statistics_cells_created_mv TO daily_statistics AS
SELECT
    toDate(timestamp) as date,
    0 as blocks_count,
    0 as avg_block_time_ms,
    0 as min_block_time_ms,
    0 as max_block_time_ms,
    0 as transactions_count,
    0 as avg_tx_per_block,
    count() as cells_created,
    0 as cells_consumed,
    0 as total_capacity,
    0 as avg_capacity_per_tx,
    0 as avg_difficulty,
    0 as total_uncles
FROM cells c
JOIN blocks b ON c.created_at_block = b.number
GROUP BY date;

-- Materialized view to update cells_consumed from cell_consumptions table
CREATE MATERIALIZED VIEW IF NOT EXISTS daily_statistics_cells_consumed_mv TO daily_statistics AS
SELECT
    toDate(timestamp) as date,
    0 as blocks_count,
    0 as avg_block_time_ms,
    0 as min_block_time_ms,
    0 as max_block_time_ms,
    0 as transactions_count,
    0 as avg_tx_per_block,
    0 as cells_created,
    count() as cells_consumed,
    0 as total_capacity,
    0 as avg_capacity_per_tx,
    0 as avg_difficulty,
    0 as total_uncles
FROM cell_consumptions cc
JOIN blocks b ON cc.consumed_at_block = b.number
GROUP BY date;

-- Materialized view to update capacity metrics from transactions table
CREATE MATERIALIZED VIEW IF NOT EXISTS daily_statistics_capacity_mv TO daily_statistics AS
SELECT
    toDate(timestamp) as date,
    0 as blocks_count,
    0 as avg_block_time_ms,
    0 as min_block_time_ms,
    0 as max_block_time_ms,
    0 as transactions_count,
    0 as avg_tx_per_block,
    0 as cells_created,
    0 as cells_consumed,
    sum(total_output_capacity) as total_capacity,
    avg(total_output_capacity) as avg_capacity_per_tx,
    0 as avg_difficulty,
    0 as total_uncles
FROM transactions
GROUP BY date;

-- ============================================================================
-- SCRIPT USAGE STATISTICS MATERIALIZED VIEW
-- ============================================================================
-- Tracks most used lock scripts and type scripts for analytics
-- Engine: AggregatingMergeTree - stores partial aggregation states
-- Update: Automatically on INSERT to cells table
--
-- Query pattern: SELECT * FROM script_usage ORDER BY usage_count DESC LIMIT 100
-- Performance: < 10ms for top 100 scripts
--
-- Why materialized view?
-- - Expensive to compute on-demand (full table scan of cells)
-- - Queried frequently (script analytics, popular contracts)
-- - Data changes incrementally (new cells added)
-- - Storage cost acceptable (~1000s of unique scripts)

CREATE TABLE IF NOT EXISTS script_usage (
    -- Script identification
    script_hash FixedString(32),        -- Script hash (lock or type)
    script_type Enum8('lock' = 1, 'type' = 2),  -- Script type
    
    -- Usage metrics
    usage_count UInt64,                 -- Number of cells using this script
    
    -- Timeline
    first_seen_block UInt64,            -- First block where script appeared
    last_seen_block UInt64,             -- Last block where script appeared
    
    -- Script details (denormalized for convenience)
    code_hash FixedString(32),          -- Script code hash
    hash_type UInt8,                    -- Hash type (0=data, 1=type, 2=data1)
    args Nullable(String)               -- Script args (hex-encoded, first occurrence)
) ENGINE = AggregatingMergeTree()
ORDER BY (script_type, usage_count, script_hash)
PRIMARY KEY (script_type, usage_count, script_hash)
COMMENT 'Script usage statistics - tracks most used lock/type scripts';

-- Materialized view to populate script_usage from cells table (lock scripts)
CREATE MATERIALIZED VIEW IF NOT EXISTS script_usage_lock_mv TO script_usage AS
SELECT
    lock_script_hash as script_hash,
    'lock' as script_type,
    count() as usage_count,
    min(created_at_block) as first_seen_block,
    max(created_at_block) as last_seen_block,
    any(lock_code_hash) as code_hash,
    any(lock_hash_type) as hash_type,
    any(lock_args) as args
FROM cells
GROUP BY lock_script_hash;

-- Materialized view to populate script_usage from cells table (type scripts)
CREATE MATERIALIZED VIEW IF NOT EXISTS script_usage_type_mv TO script_usage AS
SELECT
    type_script_hash as script_hash,
    'type' as script_type,
    count() as usage_count,
    min(created_at_block) as first_seen_block,
    max(created_at_block) as last_seen_block,
    any(type_code_hash) as code_hash,
    any(type_hash_type) as hash_type,
    any(type_args) as args
FROM cells
WHERE type_script_hash IS NOT NULL
GROUP BY type_script_hash;

-- ============================================================================
-- ADDRESS BALANCE - REAL-TIME AGGREGATION (NO MATERIALIZED VIEW)
-- ============================================================================
-- Decision: Use real-time aggregation instead of materialized view
--
-- Rationale:
-- - Query performance: 8.26ms (P95) from Phase 0 benchmarks
-- - Fast enough for real-time queries (< 10ms target)
-- - Always up-to-date (no stale data)
-- - No storage overhead (no materialized view table)
-- - No maintenance overhead (no view updates)
--
-- Query pattern (from Phase 0 benchmarks):
-- SELECT sum(capacity) as balance
-- FROM live_cells
-- WHERE lock_script_hash = unhex('...')
--   AND sign = 1
-- FINAL;
--
-- Performance: 8.26ms (P95) for 1M cells
-- Scalability: O(log N) - scales to 100M+ cells
--
-- Alternative (if performance degrades):
-- - Create materialized view with ReplacingMergeTree
-- - Update on INSERT to cells and cell_consumptions
-- - Trade-off: Storage cost vs query performance
--
-- Example query for address balance:
-- SELECT
--     lock_script_hash,
--     sum(capacity) as balance,
--     count() as live_cells_count
-- FROM live_cells
-- WHERE lock_script_hash = unhex('1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef')
--   AND sign = 1
-- GROUP BY lock_script_hash
-- FINAL;

-- ============================================================================
-- NETWORK METRICS - REAL-TIME QUERIES (NO MATERIALIZED VIEW)
-- ============================================================================
-- Decision: Use real-time queries for network metrics
--
-- Rationale:
-- - Simple queries on blocks table (indexed by number)
-- - Fast enough for real-time queries (< 10ms)
-- - Always up-to-date (no stale data)
-- - No storage overhead
--
-- Example queries:
--
-- 1. Latest block:
-- SELECT number, hash, timestamp, transactions_count
-- FROM blocks
-- ORDER BY number DESC
-- LIMIT 1;
--
-- 2. TPS (transactions per second) - last 100 blocks:
-- SELECT
--     sum(transactions_count) / dateDiff('second', min(timestamp), max(timestamp)) as tps
-- FROM blocks
-- WHERE number >= (SELECT max(number) - 100 FROM blocks);
--
-- 3. Average block time - last 1000 blocks:
-- SELECT
--     avg(dateDiff('millisecond', 
--         lagInFrame(timestamp, 1) OVER (ORDER BY number), 
--         timestamp
--     )) as avg_block_time_ms
-- FROM blocks
-- WHERE number >= (SELECT max(number) - 1000 FROM blocks);
--
-- 4. Current difficulty:
-- SELECT compact_target, total_difficulty
-- FROM blocks
-- ORDER BY number DESC
-- LIMIT 1;
--
-- 5. Epoch progress:
-- SELECT
--     epoch_number,
--     epoch_index,
--     epoch_length,
--     (epoch_index / epoch_length * 100) as progress_percent
-- FROM blocks
-- ORDER BY number DESC
-- LIMIT 1;

-- ============================================================================
-- DESIGN RATIONALE SUMMARY
-- ============================================================================
--
-- MATERIALIZED VIEWS (Pre-aggregated):
-- 1. daily_statistics - Historical data, queried frequently, expensive to compute
-- 2. script_usage - Full table scan, queried frequently, incremental updates
--
-- REAL-TIME QUERIES (On-demand aggregation):
-- 1. Address balance - Fast enough (8.26ms P95), always up-to-date
-- 2. Network metrics - Simple queries, fast enough (< 10ms)
-- 3. Live cells - Fast enough (7.97ms P95 for single OutPoint)
-- 4. Transaction details - Fast enough (60.92ms P95 for JOIN queries)
--
-- TRADE-OFFS:
--
-- Materialized Views:
-- + Pros: Fast queries (< 10ms), pre-aggregated data
-- - Cons: Storage cost, maintenance overhead, potential stale data
--
-- Real-Time Queries:
-- + Pros: Always up-to-date, no storage cost, no maintenance
-- - Cons: Slightly slower (but still < 50ms), CPU cost per query
--
-- DECISION FRAMEWORK:
-- Use materialized view when:
-- - Query is expensive (> 100ms)
-- - Data changes infrequently
-- - Storage cost is acceptable
-- - Maintenance overhead is acceptable
--
-- Use real-time query when:
-- - Query is fast (< 50ms)
-- - Data changes frequently
-- - Storage cost is high
-- - Always need up-to-date data
--
-- FUTURE ENHANCEMENTS (Phase 3+):
-- - Hourly statistics (if daily is too coarse)
-- - Token holder rankings (if sUDT/xUDT queries are slow)
-- - Address transaction history (if JOIN queries are slow)
-- - Cell relationship graph (if graph queries are slow)
--
-- ============================================================================
-- END OF SCHEMA
-- ============================================================================
