-- ClickHouse Core Tables Schema
-- Phase 1: Core blockchain data (blocks, transactions, cells, cell_consumptions)
--
-- Design Principles:
-- 1. Immutable insert-only model (no UPDATE semantics)
-- 2. FixedString(32) for all hash fields (binary serialization)
-- 3. Partitioning by 5M blocks (~18 partitions for full mainnet)
-- 4. Sort keys optimized for common query patterns
-- 5. Separate cells and cell_consumptions tables (no status column)
--
-- Partitioning Strategy:
-- - CKB mainnet: ~18M blocks currently, growing ~1M blocks/year
-- - Partition size: 5M blocks = ~18 partitions for full chain
-- - Benefits: Faster queries on recent data, easier data management
-- - Partition key: intDiv(block_number, 5000000)
--
-- Performance Expectations (from Phase 0 benchmarks):
-- - Write throughput: 450K+ rows/s sustained (100K batch size)
-- - Single OutPoint query: < 10ms (P95)
-- - Batch OutPoint query (50 cells): < 500ms (P95)
-- - Compression ratio: 5-10x (LZ4 default)

CREATE DATABASE IF NOT EXISTS ckbadger;

USE ckbadger;

-- ============================================================================
-- BLOCKS TABLE
-- ============================================================================
-- Stores blockchain block headers and metadata
-- Primary access pattern: Query by block number or hash
-- Partition: 5M blocks per partition (~18 partitions for full chain)
-- Sort key: (number) - optimized for sequential block queries

CREATE TABLE IF NOT EXISTS blocks (
    -- Block identification
    number UInt64,                      -- Block height (primary key)
    hash FixedString(32),               -- Block hash (32 bytes binary)
    parent_hash FixedString(32),        -- Parent block hash
    timestamp DateTime,                 -- Block timestamp (Unix epoch)
    
    -- Block header fields
    version UInt32,                     -- Block version
    compact_target UInt64,              -- Difficulty target
    nonce FixedString(32),              -- Proof-of-work nonce (32 bytes)
    
    -- Merkle roots
    transactions_root FixedString(32),  -- Transactions merkle root
    proposals_hash FixedString(32),     -- Proposals hash
    extra_hash FixedString(32),         -- Extra data hash
    uncles_hash FixedString(32),        -- Uncles hash
    
    -- Epoch information
    epoch_number UInt64,                -- Epoch number
    epoch_index UInt32,                 -- Block index within epoch
    epoch_length UInt32,                -- Total blocks in epoch
    
    -- DAO field (32 bytes containing multiple values)
    dao FixedString(32),                -- DAO data (C, AR, S, U encoded)
    
    -- Block statistics
    transactions_count UInt32,          -- Number of transactions
    proposals_count UInt32,             -- Number of proposals
    uncles_count UInt32,                -- Number of uncles
    
    -- Optional fields
    extension Nullable(String),         -- Block extension data (hex)
    miner_lock_hash Nullable(FixedString(32)),  -- Miner lock script hash
    miner_message Nullable(String),     -- Miner message (hex)
    
    -- Difficulty tracking
    total_difficulty String             -- Cumulative difficulty (large number as string)
) ENGINE = MergeTree()
PARTITION BY intDiv(number, 5000000)
ORDER BY (number)
PRIMARY KEY (number)
COMMENT 'Blockchain block headers and metadata';

-- ============================================================================
-- TRANSACTIONS TABLE
-- ============================================================================
-- Stores transaction metadata (inputs/outputs stored separately in cells)
-- Primary access pattern: Query by tx hash or block number
-- Partition: 5M blocks per partition (aligned with blocks)
-- Sort key: (block_number, hash) - optimized for block queries and tx lookup

CREATE TABLE IF NOT EXISTS transactions (
    -- Transaction identification
    hash FixedString(32),               -- Transaction hash (32 bytes binary)
    block_number UInt64,                -- Block height (for partitioning)
    tx_index UInt32,                    -- Transaction index within block
    timestamp DateTime,                 -- Block timestamp (denormalized)
    
    -- Transaction structure
    version UInt32,                     -- Transaction version
    inputs_count UInt16,                -- Number of inputs
    outputs_count UInt16,               -- Number of outputs
    witnesses_count UInt16,             -- Number of witnesses
    cell_deps_count UInt16,             -- Number of cell dependencies
    header_deps_count UInt16,           -- Number of header dependencies
    
    -- Capacity tracking
    total_input_capacity UInt64,        -- Sum of input capacities (shannon)
    total_output_capacity UInt64,       -- Sum of output capacities (shannon)
    fee UInt64,                         -- Transaction fee (shannon)
    
    -- Transaction metadata
    is_cellbase UInt8,                  -- 1 if cellbase transaction, 0 otherwise
    tx_size Nullable(UInt32),           -- Transaction size in bytes
    cycles Nullable(UInt64)             -- Execution cycles consumed
) ENGINE = MergeTree()
PARTITION BY intDiv(block_number, 5000000)
ORDER BY (block_number, hash)
PRIMARY KEY (block_number, hash)
COMMENT 'Transaction metadata and statistics';

-- ============================================================================
-- CELLS TABLE (Creation Events Only)
-- ============================================================================
-- Stores cell creation events (outputs)
-- Primary access pattern: Query by OutPoint (tx_hash, output_index) or block range
-- Partition: 5M blocks per partition (aligned with creation block)
-- Sort key: (created_at_block, tx_hash, output_index) - optimized for OutPoint lookup
--
-- IMPORTANT: This table only records cell CREATION events.
-- Cell consumption is tracked separately in cell_consumptions table.
-- To query live cells: LEFT ANTI JOIN with cell_consumptions.

CREATE TABLE IF NOT EXISTS cells (
    -- Cell identification (OutPoint)
    tx_hash FixedString(32),            -- Transaction hash (32 bytes binary)
    output_index UInt16,                -- Output index within transaction
    created_at_block UInt64,            -- Block height when cell was created
    
    -- Cell capacity
    capacity UInt64,                    -- Cell capacity in shannon (1 CKB = 10^8 shannon)
    
    -- Lock script (required)
    lock_code_hash FixedString(32),     -- Lock script code hash
    lock_hash_type UInt8,               -- Lock script hash type (0=data, 1=type, 2=data1)
    lock_args String,                   -- Lock script args (hex-encoded, variable length)
    lock_script_hash FixedString(32),   -- Lock script hash (computed)
    
    -- Type script (optional)
    type_code_hash Nullable(FixedString(32)),  -- Type script code hash
    type_hash_type Nullable(UInt8),     -- Type script hash type
    type_args Nullable(String),         -- Type script args (hex-encoded)
    type_script_hash Nullable(FixedString(32)),  -- Type script hash (computed)
    
    -- Cell data
    data_hash FixedString(32),          -- Data hash (blake2b)
    data_size UInt32,                   -- Data size in bytes
    data Nullable(String)               -- Cell data (hex-encoded, up to 512 bytes for preview)
) ENGINE = MergeTree()
PARTITION BY intDiv(created_at_block, 5000000)
ORDER BY (created_at_block, tx_hash, output_index)
PRIMARY KEY (created_at_block, tx_hash, output_index)
COMMENT 'Cell creation events (outputs) - immutable insert-only';

-- ============================================================================
-- CELL_CONSUMPTIONS TABLE (Consumption Events)
-- ============================================================================
-- Stores cell consumption events (inputs)
-- Primary access pattern: Query by OutPoint to check if cell is consumed
-- Partition: 5M blocks per partition (aligned with consumption block)
-- Sort key: (consumed_at_block, tx_hash, output_index) - optimized for OutPoint lookup
--
-- IMPORTANT: This table only records cell CONSUMPTION events.
-- To query live cells: SELECT from cells WHERE (tx_hash, output_index) NOT IN cell_consumptions.

CREATE TABLE IF NOT EXISTS cell_consumptions (
    -- Cell identification (OutPoint being consumed)
    tx_hash FixedString(32),            -- Original transaction hash (32 bytes binary)
    output_index UInt16,                -- Original output index
    
    -- Consumption metadata
    consumed_at_block UInt64,           -- Block height when cell was consumed
    consumed_by_tx FixedString(32),     -- Transaction hash that consumed this cell
    consumed_at_index UInt16            -- Input index within consuming transaction
) ENGINE = MergeTree()
PARTITION BY intDiv(consumed_at_block, 5000000)
ORDER BY (consumed_at_block, tx_hash, output_index)
PRIMARY KEY (consumed_at_block, tx_hash, output_index)
COMMENT 'Cell consumption events (inputs) - immutable insert-only';

-- ============================================================================
-- SCHEMA DESIGN NOTES
-- ============================================================================
--
-- 1. IMMUTABLE INSERT-ONLY MODEL
--    - No UPDATE or DELETE operations (ClickHouse optimized for append-only)
--    - Cell lifecycle: INSERT into cells → INSERT into cell_consumptions
--    - Live cells query: LEFT ANTI JOIN or NOT IN subquery
--
-- 2. FIXEDSTRING(32) FOR HASHES
--    - All hash fields use FixedString(32) for binary storage
--    - Rust code must serialize as Vec<u8> (32 bytes), not hex strings
--    - 50% storage savings vs hex-encoded strings (64 chars)
--    - 10x performance improvement (from Phase 0 benchmarks)
--
-- 3. PARTITIONING STRATEGY
--    - Partition by intDiv(block_number, 5000000) = 5M blocks per partition
--    - Current mainnet: ~18M blocks = 4 partitions (0-5M, 5M-10M, 10M-15M, 15M-20M)
--    - Future growth: ~1M blocks/year = new partition every 5 years
--    - Benefits: Faster queries on recent data, easier partition management
--
-- 4. SORT KEYS (ORDER BY)
--    - blocks: (number) - sequential block queries
--    - transactions: (block_number, hash) - block queries + tx hash lookup
--    - cells: (created_at_block, tx_hash, output_index) - OutPoint lookup
--    - cell_consumptions: (consumed_at_block, tx_hash, output_index) - OutPoint lookup
--
-- 5. DATA TYPES
--    - UInt64: block numbers, capacity, timestamps (shannon precision)
--    - UInt32: counts, sizes, indexes (sufficient range)
--    - UInt16: small indexes (output_index, input_index)
--    - UInt8: flags, enums (hash_type, is_cellbase)
--    - String: variable-length hex data (lock_args, type_args, data)
--    - FixedString(32): fixed-length binary hashes
--    - DateTime: timestamps (automatic conversion from Unix epoch)
--    - Nullable(): optional fields (type_script, data, miner_*)
--
-- 6. LIVE CELLS QUERY PATTERN
--    -- Option 1: LEFT ANTI JOIN (recommended for large result sets)
--    SELECT c.*
--    FROM cells c
--    LEFT ANTI JOIN cell_consumptions cc
--      ON c.tx_hash = cc.tx_hash AND c.output_index = cc.output_index
--    WHERE c.created_at_block >= 0;
--
--    -- Option 2: NOT IN subquery (recommended for small result sets)
--    SELECT *
--    FROM cells
--    WHERE (tx_hash, output_index) NOT IN (
--      SELECT tx_hash, output_index FROM cell_consumptions
--    );
--
--    -- Option 3: NOT EXISTS (recommended for single OutPoint lookup)
--    SELECT *
--    FROM cells c
--    WHERE c.tx_hash = unhex('...')
--      AND c.output_index = 0
--      AND NOT EXISTS (
--        SELECT 1 FROM cell_consumptions cc
--        WHERE cc.tx_hash = c.tx_hash AND cc.output_index = c.output_index
--      );
--
-- 7. COMPRESSION
--    - Default LZ4 compression: 5-10x ratio (from Phase 0 benchmarks)
--    - No explicit compression settings needed
--    - ClickHouse automatically compresses columnar data
--
-- 8. PERFORMANCE EXPECTATIONS (from Phase 0 benchmarks)
--    - Write throughput: 450K+ rows/s sustained (100K batch size)
--    - Single OutPoint query: < 10ms (P95)
--    - Batch OutPoint query (50 cells): < 500ms (P95)
--    - Address balance query: < 10ms (P95)
--    - JOIN query (tx inputs → cells): < 200ms (P95)
--
-- 9. MIGRATION FROM POSTGRESQL
--    - PostgreSQL cells.status column → ClickHouse cell_consumptions table
--    - PostgreSQL UPDATE cells SET status=1 → ClickHouse INSERT INTO cell_consumptions
--    - PostgreSQL WHERE status=0 → ClickHouse LEFT ANTI JOIN cell_consumptions
--
-- 10. FUTURE ENHANCEMENTS (Phase 2+)
--     - Secondary indexes for lock_script_hash, type_script_hash
--     - Materialized views for live_cells, address_balances
--     - Aggregating tables for statistics (daily_stats, token_holders)
--     - ReplacingMergeTree for deduplication (if needed)

-- ============================================================================
-- SYNC STATUS TABLE
-- ============================================================================
-- Tracks indexer synchronization progress
-- Single row table (id = 1) updated by indexer

CREATE TABLE IF NOT EXISTS sync_status (
    id UInt8,                           -- Always 1 (single row)
    tip_block_number UInt64,            -- Latest synced block number
    tip_block_hash FixedString(32),     -- Latest synced block hash (binary)
    updated_at DateTime DEFAULT now()  -- Last update timestamp
) ENGINE = ReplacingMergeTree(updated_at)
ORDER BY id
PRIMARY KEY (id)
COMMENT 'Indexer synchronization status';

-- ============================================================================
-- BLOCK PROPOSALS TABLE
-- ============================================================================
-- Stores transaction proposals in blocks
-- Proposals are transactions suggested for inclusion in future blocks

CREATE TABLE IF NOT EXISTS block_proposals (
    block_number UInt64,                -- Block height
    block_hash FixedString(32),         -- Block hash
    proposal_hash FixedString(32),      -- Proposed transaction hash
    proposal_index UInt16               -- Index within proposals array
) ENGINE = MergeTree()
PARTITION BY intDiv(block_number, 5000000)
ORDER BY (block_number, proposal_index)
COMMENT 'Transaction proposals in blocks';

-- ============================================================================
-- END OF SCHEMA
-- ============================================================================
