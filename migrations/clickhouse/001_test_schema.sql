-- ClickHouse Test Schema for CKB Cell Benchmark
-- Purpose: Validate ClickHouse performance for 1M+ cell writes and queries
-- Based on: migrations/postgres/001_init.sql cells table (lines 224-267)

CREATE DATABASE IF NOT EXISTS ckbadger_test;

USE ckbadger_test;

-- Simplified cells table for benchmark testing
-- Uses MergeTree engine optimized for high-throughput writes
CREATE TABLE IF NOT EXISTS cells (
    -- Identity
    id UInt64,
    tx_hash FixedString(32),
    output_index UInt16,
    capacity UInt64,

    -- Lock Script (required)
    lock_code_hash FixedString(32),
    lock_hash_type UInt8,
    lock_args String,
    lock_script_hash FixedString(32),

    -- Type Script (optional, nullable)
    type_code_hash Nullable(FixedString(32)),
    type_hash_type Nullable(UInt8),
    type_args Nullable(String),
    type_script_hash Nullable(FixedString(32)),

    -- Data
    data_hash FixedString(32),
    data_size UInt32,
    data Nullable(String),

    -- Lifecycle
    status UInt8,
    created_at_block UInt64,
    consumed_at_block Nullable(UInt64),
    consumed_by_tx Nullable(FixedString(32)),
    consumed_at_index Nullable(UInt16),

    -- Metadata
    created_at DateTime DEFAULT now()
)
ENGINE = MergeTree()
PARTITION BY intDiv(created_at_block, 1000000)
ORDER BY (created_at_block, tx_hash, output_index)
PRIMARY KEY (created_at_block, tx_hash, output_index)
SETTINGS index_granularity = 8192;

-- Live cells materialized view for O(1) lookup
-- Simulates the live_cells table in Postgres
CREATE TABLE IF NOT EXISTS live_cells (
    tx_hash FixedString(32),
    output_index UInt16,
    capacity UInt64,
    lock_script_hash FixedString(32),
    type_script_hash Nullable(FixedString(32)),
    created_at_block UInt64,
    cell_id UInt64
)
ENGINE = MergeTree()
ORDER BY (tx_hash, output_index)
PRIMARY KEY (tx_hash, output_index)
SETTINGS index_granularity = 8192;

-- Index for lock_script_hash queries (address balance lookups)
CREATE TABLE IF NOT EXISTS cells_by_lock (
    lock_script_hash FixedString(32),
    tx_hash FixedString(32),
    output_index UInt16,
    capacity UInt64,
    status UInt8,
    created_at_block UInt64
)
ENGINE = MergeTree()
ORDER BY (lock_script_hash, created_at_block, tx_hash)
PRIMARY KEY (lock_script_hash, created_at_block)
SETTINGS index_granularity = 8192;

-- Statistics table for benchmark results
CREATE TABLE IF NOT EXISTS benchmark_stats (
    test_name String,
    operation String,
    rows_affected UInt64,
    duration_ms UInt64,
    rows_per_second Float64,
    timestamp DateTime DEFAULT now()
)
ENGINE = MergeTree()
ORDER BY (test_name, timestamp)
SETTINGS index_granularity = 8192;
