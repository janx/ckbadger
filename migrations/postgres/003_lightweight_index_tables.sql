-- ============================================
-- Lightweight index tables for Zero-RPC architecture.
-- These tables store only the columns needed for list queries,
-- sorting, and filtering. Full block/transaction data is read
-- directly from CKB's RocksDB on demand.
-- ============================================

-- ---- blocks_index ----
-- Replaces the full `blocks` table for list/filter queries.
-- ~2.2GB vs ~15GB for the full blocks table.
CREATE TABLE IF NOT EXISTS blocks_index (
    number          BIGINT PRIMARY KEY,
    hash            BYTEA NOT NULL,
    timestamp       TIMESTAMPTZ NOT NULL,
    tx_count        INTEGER NOT NULL DEFAULT 0,
    proposals_count INTEGER NOT NULL DEFAULT 0,
    uncles_count    INTEGER NOT NULL DEFAULT 0,
    epoch_number    BIGINT NOT NULL,
    epoch_index     INTEGER NOT NULL,
    epoch_length    INTEGER NOT NULL,
    compact_target  BIGINT NOT NULL,
    miner_lock_hash BYTEA,
    dao             BYTEA NOT NULL   -- 32 bytes, needed for DAO stats
);

-- Hash lookup for block detail pages
CREATE INDEX IF NOT EXISTS idx_blocks_index_hash ON blocks_index(hash);

-- Timestamp range queries for statistics
CREATE INDEX IF NOT EXISTS idx_blocks_index_timestamp ON blocks_index(timestamp);

-- Epoch queries
CREATE INDEX IF NOT EXISTS idx_blocks_index_epoch ON blocks_index(epoch_number);

-- ---- transactions_index ----
-- Replaces the full `transactions` table for list/filter queries.
-- ~15GB vs ~40GB for the full transactions table. Keeps partitioning.
CREATE TABLE IF NOT EXISTS transactions_index (
    hash          BYTEA NOT NULL,
    block_number  BIGINT NOT NULL,
    tx_index      INTEGER NOT NULL,
    is_cellbase   BOOLEAN NOT NULL DEFAULT FALSE,
    timestamp     TIMESTAMPTZ NOT NULL,
    inputs_count  SMALLINT NOT NULL DEFAULT 0,
    outputs_count SMALLINT NOT NULL DEFAULT 0,
    fee           BIGINT NOT NULL DEFAULT 0,
    cycles        BIGINT,
    PRIMARY KEY (block_number, hash)
) PARTITION BY RANGE (block_number);

-- 10 partitions matching the existing scheme
CREATE TABLE IF NOT EXISTS transactions_index_p00 PARTITION OF transactions_index FOR VALUES FROM (0) TO (5000000);
CREATE TABLE IF NOT EXISTS transactions_index_p01 PARTITION OF transactions_index FOR VALUES FROM (5000000) TO (10000000);
CREATE TABLE IF NOT EXISTS transactions_index_p02 PARTITION OF transactions_index FOR VALUES FROM (10000000) TO (15000000);
CREATE TABLE IF NOT EXISTS transactions_index_p03 PARTITION OF transactions_index FOR VALUES FROM (15000000) TO (20000000);
CREATE TABLE IF NOT EXISTS transactions_index_p04 PARTITION OF transactions_index FOR VALUES FROM (20000000) TO (25000000);
CREATE TABLE IF NOT EXISTS transactions_index_p05 PARTITION OF transactions_index FOR VALUES FROM (25000000) TO (30000000);
CREATE TABLE IF NOT EXISTS transactions_index_p06 PARTITION OF transactions_index FOR VALUES FROM (30000000) TO (35000000);
CREATE TABLE IF NOT EXISTS transactions_index_p07 PARTITION OF transactions_index FOR VALUES FROM (35000000) TO (40000000);
CREATE TABLE IF NOT EXISTS transactions_index_p08 PARTITION OF transactions_index FOR VALUES FROM (40000000) TO (45000000);
CREATE TABLE IF NOT EXISTS transactions_index_p09 PARTITION OF transactions_index FOR VALUES FROM (45000000) TO (50000000);

-- Short hash prefix for search
CREATE INDEX IF NOT EXISTS idx_transactions_index_short_hash
    ON transactions_index (substring(hash, 1, 10));

-- Timestamp range queries
CREATE INDEX IF NOT EXISTS idx_transactions_index_timestamp
    ON transactions_index (timestamp);

-- Cycles backfill: find transactions without cycles
CREATE INDEX IF NOT EXISTS idx_transactions_index_cycles_null
    ON transactions_index (block_number)
    WHERE NOT is_cellbase AND (cycles IS NULL OR cycles = 0);
