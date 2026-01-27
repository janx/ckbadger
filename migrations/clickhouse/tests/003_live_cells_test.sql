-- Live Cells Query Performance Test Schema
-- Purpose: Test ClickHouse live cell query performance using ReplacingMergeTree
-- Based on: migrations/postgres/001_init.sql live_cells table (lines 269-291)

USE ckbadger_test;

-- Option A: ReplacingMergeTree with sign column
-- This approach uses ClickHouse's ReplacingMergeTree to handle cell lifecycle
-- sign = 1: cell created (live)
-- sign = -1: cell consumed (dead)
-- FINAL keyword collapses rows to show only latest state

CREATE TABLE IF NOT EXISTS live_cells_rmt (
    -- OutPoint (primary key)
    tx_hash FixedString(32),
    output_index UInt16,
    
    -- Cell data
    capacity UInt64,
    lock_script_hash FixedString(32),
    lock_code_hash FixedString(32),
    lock_args String,
    type_script_hash Nullable(FixedString(32)),
    type_code_hash Nullable(FixedString(32)),
    data_size UInt32,
    created_at_block UInt64,
    
    -- Lifecycle tracking
    sign Int8,  -- 1 = created, -1 = consumed
    version UInt64  -- For ReplacingMergeTree deduplication
)
ENGINE = ReplacingMergeTree(version)
ORDER BY (tx_hash, output_index)
PRIMARY KEY (tx_hash, output_index)
SETTINGS index_granularity = 8192;

-- Secondary index for lock_script_hash queries (address balance)
CREATE TABLE IF NOT EXISTS live_cells_by_lock (
    lock_script_hash FixedString(32),
    tx_hash FixedString(32),
    output_index UInt16,
    capacity UInt64,
    type_script_hash Nullable(FixedString(32)),
    created_at_block UInt64,
    sign Int8,
    version UInt64
)
ENGINE = ReplacingMergeTree(version)
ORDER BY (lock_script_hash, created_at_block, tx_hash, output_index)
PRIMARY KEY (lock_script_hash)
SETTINGS index_granularity = 8192;

-- Secondary index for type_script_hash queries (token holders)
CREATE TABLE IF NOT EXISTS live_cells_by_type (
    type_script_hash FixedString(32),
    tx_hash FixedString(32),
    output_index UInt16,
    capacity UInt64,
    lock_script_hash FixedString(32),
    created_at_block UInt64,
    sign Int8,
    version UInt64
)
ENGINE = ReplacingMergeTree(version)
ORDER BY (type_script_hash, created_at_block, tx_hash, output_index)
PRIMARY KEY (type_script_hash)
SETTINGS index_granularity = 8192;

-- Transaction inputs table (for JOIN testing)
-- Simulates: SELECT cells.* FROM transaction_inputs JOIN cells ON ...
CREATE TABLE IF NOT EXISTS transaction_inputs (
    tx_hash FixedString(32),
    input_index UInt16,
    previous_tx_hash FixedString(32),
    previous_output_index UInt16,
    block_number UInt64
)
ENGINE = MergeTree()
ORDER BY (tx_hash, input_index)
PRIMARY KEY (tx_hash)
SETTINGS index_granularity = 8192;

-- Query benchmark results table
CREATE TABLE IF NOT EXISTS query_benchmark_results (
    test_name String,
    query_type String,
    query_description String,
    rows_returned UInt64,
    duration_ms Float64,
    used_final Boolean,
    timestamp DateTime DEFAULT now()
)
ENGINE = MergeTree()
ORDER BY (test_name, query_type, timestamp)
SETTINGS index_granularity = 8192;
