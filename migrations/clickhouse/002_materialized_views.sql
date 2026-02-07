-- ============================================
-- Materialized Views for Statistics Optimization
-- These views pre-aggregate data for chart queries
-- ============================================

USE ckbadger;

-- ---- mv_daily_tx_count ----
-- Materialized view for daily transaction counts
CREATE MATERIALIZED VIEW IF NOT EXISTS mv_daily_tx_count
ENGINE = SummingMergeTree()
ORDER BY (date)
POPULATE
AS SELECT
    toDate(fromUnixTimestamp64Milli(b.timestamp)) as date,
    count() as tx_count
FROM transactions_all t
INNER JOIN canonical_blocks c ON t.block_number = c.number AND t.block_hash = c.block_hash
INNER JOIN blocks_all b ON c.number = b.number AND c.block_hash = b.hash
GROUP BY date;


-- ---- mv_daily_cell_count ----
-- Materialized view for daily cell creation counts
CREATE MATERIALIZED VIEW IF NOT EXISTS mv_daily_cell_count
ENGINE = SummingMergeTree()
ORDER BY (date)
POPULATE
AS SELECT
    toDate(fromUnixTimestamp64Milli(b.timestamp)) as date,
    count() as cell_count
FROM cell_outputs_all co
INNER JOIN canonical_blocks c ON co.block_number = c.number AND co.block_hash = c.block_hash
INNER JOIN blocks_all b ON c.number = b.number AND c.block_hash = b.hash
GROUP BY date;


-- ---- mv_daily_block_stats ----
-- Materialized view for daily block statistics (hash rate, difficulty)
CREATE MATERIALIZED VIEW IF NOT EXISTS mv_daily_block_stats
ENGINE = AggregatingMergeTree()
ORDER BY (date)
POPULATE
AS SELECT
    toDate(fromUnixTimestamp64Milli(b.timestamp)) as date,
    avgState(b.difficulty) as avg_difficulty,
    countState() as block_count,
    minState(b.timestamp) as min_timestamp,
    maxState(b.timestamp) as max_timestamp
FROM blocks_all b
INNER JOIN canonical_blocks c ON b.number = c.number AND b.hash = c.block_hash
GROUP BY date;


-- ---- mv_hourly_tx_count ----
-- Materialized view for hourly transaction counts (for TX Stats)
CREATE MATERIALIZED VIEW IF NOT EXISTS mv_hourly_tx_count
ENGINE = SummingMergeTree()
ORDER BY (hour_bucket)
POPULATE
AS SELECT
    toStartOfHour(fromUnixTimestamp64Milli(b.timestamp)) as hour_bucket,
    count() as tx_count
FROM transactions_all t
INNER JOIN canonical_blocks c ON t.block_number = c.number AND t.block_hash = c.block_hash
INNER JOIN blocks_all b ON c.number = b.number AND c.block_hash = b.hash
GROUP BY hour_bucket;


-- ---- mv_five_min_tx_count ----
-- Materialized view for 5-minute transaction counts (for TX Stats hourly chart)
CREATE MATERIALIZED VIEW IF NOT EXISTS mv_five_min_tx_count
ENGINE = SummingMergeTree()
ORDER BY (five_min_bucket)
POPULATE
AS SELECT
    toStartOfFiveMinutes(fromUnixTimestamp64Milli(b.timestamp)) as five_min_bucket,
    count() as tx_count
FROM transactions_all t
INNER JOIN canonical_blocks c ON t.block_number = c.number AND t.block_hash = c.block_hash
INNER JOIN blocks_all b ON c.number = b.number AND c.block_hash = b.hash
GROUP BY five_min_bucket;
