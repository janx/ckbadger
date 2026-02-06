-- ============================================
-- ckbadger ClickHouse Schema
-- Optimized for CKB blockchain indexer analytics
-- 
-- Table Engine Strategy:
--   MergeTree: Immutable fact tables (blocks, transactions, cells, activities)
--   ReplacingMergeTree: Canonical/state tables with versioning (canonical_blocks, cell_state, dao_deposits)
--   SummingMergeTree: Incremental aggregates (address_balances)
--   AggregatingMergeTree: Pre-computed statistics (daily_stats)
--
-- Partitioning: intDiv(block_number, 1000000) for 1M block partitions
-- ============================================

-- ===========================================
-- 1. Canonical Chain Tracking
-- Small table tracking which blocks are currently canonical.
-- On reorg: insert new row with higher canon_version for affected block numbers.
-- Query with FINAL or use subquery for max version.
-- ===========================================

CREATE TABLE IF NOT EXISTS canonical_blocks
(
    -- Block identity
    number UInt64,
    block_hash FixedString(32),
    
    -- Version for ReplacingMergeTree (higher = newer)
    canon_version UInt64,
    
    -- Metadata
    updated_at DateTime64(3, 'UTC') DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(canon_version)
ORDER BY (number)
COMMENT 'Tracks current canonical chain. Use FINAL or max(canon_version) per number to get current state.';


-- ===========================================
-- 2. Immutable Fact Tables (Append-Only)
-- These tables store all blockchain data, including orphaned blocks.
-- Canonical state is determined by JOINing with canonical_blocks.
-- ===========================================

-- ---- blocks_all ----
-- Stores ALL blocks including orphaned ones.
-- No is_canonical column - determine canonicity via JOIN with canonical_blocks.
CREATE TABLE IF NOT EXISTS blocks_all
(
    -- Identity
    number UInt64,
    hash FixedString(32) CODEC(ZSTD(1)),
    parent_hash FixedString(32) CODEC(ZSTD(1)),
    
    -- Timestamp
    timestamp DateTime64(3, 'UTC'),
    
    -- Block metadata
    version UInt32,
    compact_target UInt64,
    transactions_count UInt32 DEFAULT 0,
    proposals_count UInt32 DEFAULT 0,
    uncles_count UInt8 DEFAULT 0,
    
    -- Epoch info
    epoch_number UInt64,
    epoch_index UInt32,
    epoch_length UInt32,
    
    -- DAO field (32 bytes): C|AR|S|U (each 8 bytes LE u64)
    dao FixedString(32) CODEC(ZSTD(1)),
    
    -- Consensus fields
    nonce FixedString(16) CODEC(ZSTD(1)),  -- 16 bytes (u128)
    extra_hash FixedString(32) CODEC(ZSTD(1)),
    extension String CODEC(ZSTD(3)),  -- Variable length, Nullable represented as empty
    proposals_hash FixedString(32) CODEC(ZSTD(1)),
    transactions_root FixedString(32) CODEC(ZSTD(1)),
    uncles_hash FixedString(32) CODEC(ZSTD(1)),
    
    -- Miner info (extracted from cellbase)
    miner_lock_hash FixedString(32) CODEC(ZSTD(1)),  -- Empty string if unknown
    miner_message String CODEC(ZSTD(3)),
    
    -- Aggregates
    total_difficulty UInt256 DEFAULT 0,
    reward UInt64 DEFAULT 0,  -- In shannons
    
    -- Bloom filter indexes for hash lookups
    INDEX idx_hash hash TYPE bloom_filter(0.01) GRANULARITY 1,
    INDEX idx_parent_hash parent_hash TYPE bloom_filter(0.01) GRANULARITY 1
)
ENGINE = MergeTree
PARTITION BY intDiv(number, 1000000)
ORDER BY (number)
COMMENT 'All blocks including orphaned. JOIN with canonical_blocks to filter canonical chain.';


-- ---- transactions_all ----
-- Stores ALL transactions from all blocks (including orphaned).
CREATE TABLE IF NOT EXISTS transactions_all
(
    -- Identity
    hash FixedString(32) CODEC(ZSTD(1)),
    block_number UInt64,
    block_hash FixedString(32) CODEC(ZSTD(1)),
    tx_index UInt32,
    
    -- Transaction metadata
    version UInt32,
    inputs_count UInt16 DEFAULT 0,
    outputs_count UInt16 DEFAULT 0,
    witnesses_count UInt16 DEFAULT 0,
    cell_deps_count UInt16 DEFAULT 0,
    header_deps_count UInt16 DEFAULT 0,
    
    -- Capacity tracking
    total_input_capacity UInt64 DEFAULT 0,
    total_output_capacity UInt64 DEFAULT 0,
    fee UInt64 DEFAULT 0,
    
    -- Size and cost
    tx_size UInt32 DEFAULT 0,
    cycles UInt64 DEFAULT 0,
    
    -- Flags
    is_cellbase UInt8 DEFAULT 0,
    
    -- Denormalized timestamp for efficient queries
    timestamp DateTime64(3, 'UTC'),
    
    -- Bloom filter indexes
    INDEX idx_hash hash TYPE bloom_filter(0.01) GRANULARITY 1,
    INDEX idx_block_hash block_hash TYPE bloom_filter(0.01) GRANULARITY 1
)
ENGINE = MergeTree
PARTITION BY intDiv(block_number, 1000000)
ORDER BY (block_number, tx_index)
COMMENT 'All transactions. JOIN with canonical_blocks via block_number to filter canonical.';

-- Projection for tx hash lookup (common query pattern)
ALTER TABLE transactions_all ADD PROJECTION prj_by_hash
(
    SELECT *
    ORDER BY hash
);


-- ---- cell_outputs_all ----
-- Immutable record of all cell outputs ever created.
-- Cell state (live/consumed) is tracked separately in cell_state table.
CREATE TABLE IF NOT EXISTS cell_outputs_all
(
    -- Cell identity (OutPoint)
    tx_hash FixedString(32) CODEC(ZSTD(1)),
    output_index UInt16,
    
    -- Block context
    block_number UInt64,
    block_hash FixedString(32) CODEC(ZSTD(1)),
    
    -- Capacity
    capacity UInt64,  -- In shannons
    
    -- Lock Script (required)
    lock_code_hash FixedString(32) CODEC(ZSTD(1)),
    lock_hash_type UInt8,  -- 0=data, 1=type, 2=data1, 4=data2
    lock_args String CODEC(ZSTD(3)),
    lock_script_hash FixedString(32) CODEC(ZSTD(1)),
    
    -- Type Script (optional, empty string if none)
    type_code_hash FixedString(32) CODEC(ZSTD(1)),  -- Empty if no type script
    type_hash_type UInt8 DEFAULT 0,
    type_args String CODEC(ZSTD(3)),
    type_script_hash FixedString(32) CODEC(ZSTD(1)),  -- Empty if no type script
    
    -- Cell data
    data_hash FixedString(32) CODEC(ZSTD(1)),
    data_size UInt32 DEFAULT 0,
    data String CODEC(ZSTD(3)),  -- Up to 512 bytes for preview
    
    -- Bloom filter indexes
    INDEX idx_tx_hash tx_hash TYPE bloom_filter(0.01) GRANULARITY 1,
    INDEX idx_lock_script_hash lock_script_hash TYPE bloom_filter(0.01) GRANULARITY 1,
    INDEX idx_type_script_hash type_script_hash TYPE bloom_filter(0.01) GRANULARITY 1,
    INDEX idx_lock_code_hash lock_code_hash TYPE bloom_filter(0.01) GRANULARITY 1,
    INDEX idx_type_code_hash type_code_hash TYPE bloom_filter(0.01) GRANULARITY 1
)
ENGINE = MergeTree
PARTITION BY intDiv(block_number, 1000000)
ORDER BY (block_number, tx_hash, output_index)
COMMENT 'Immutable cell output records. Cell lifecycle tracked in cell_state.';

-- Projection for cell lookup by OutPoint
ALTER TABLE cell_outputs_all ADD PROJECTION prj_by_outpoint
(
    SELECT *
    ORDER BY (tx_hash, output_index)
);

-- Projection for address (lock_script_hash) lookup
ALTER TABLE cell_outputs_all ADD PROJECTION prj_by_lock
(
    SELECT *
    ORDER BY (lock_script_hash, block_number)
);


-- ---- cell_inputs_all ----
-- Records all cell consumptions (inputs to transactions).
CREATE TABLE IF NOT EXISTS cell_inputs_all
(
    -- Transaction context
    tx_hash FixedString(32) CODEC(ZSTD(1)),
    tx_block_number UInt64,
    input_index UInt16,
    
    -- Referenced cell (OutPoint being consumed)
    previous_tx_hash FixedString(32) CODEC(ZSTD(1)),
    previous_output_index UInt16,
    
    -- Since value for time-lock verification
    since UInt64 DEFAULT 0,
    
    -- Bloom filter indexes
    INDEX idx_tx_hash tx_hash TYPE bloom_filter(0.01) GRANULARITY 1,
    INDEX idx_prev_tx_hash previous_tx_hash TYPE bloom_filter(0.01) GRANULARITY 1
)
ENGINE = MergeTree
PARTITION BY intDiv(tx_block_number, 1000000)
ORDER BY (tx_block_number, tx_hash, input_index)
COMMENT 'All cell inputs (consumptions). JOIN with canonical_blocks to filter.';


-- ---- activities_all ----
-- Unified activity feed: semantic interpretation of blockchain events.
CREATE TABLE IF NOT EXISTS activities_all
(
    -- Deterministic unique key: blake2b(tx_hash || activity_type || activity_index)
    activity_id FixedString(32) CODEC(ZSTD(1)),
    
    -- Classification
    activity_type LowCardinality(String),  -- CKB_TRANSFER, TOKEN_MINT, DAO_DEPOSIT, etc.
    activity_category LowCardinality(String),  -- ckb, cellbase, token, dob, nft, dao, script, rgbpp
    
    -- Transaction context
    block_number UInt64,
    tx_hash FixedString(32) CODEC(ZSTD(1)),
    tx_index UInt32,
    activity_index UInt16 DEFAULT 0,  -- Order within tx
    
    -- Participants (lock_script_hash)
    from_lock_hash FixedString(32) CODEC(ZSTD(1)),  -- Empty for mint/cellbase
    to_lock_hash FixedString(32) CODEC(ZSTD(1)),    -- Empty for burn
    
    -- Value (semantic depends on activity_type)
    amount UInt256 DEFAULT 0,  -- Large enough for any token amount
    
    -- Asset reference (type_script_hash for tokens, spore_id for DOB, etc.)
    asset_id FixedString(32) CODEC(ZSTD(1)),  -- Empty if N/A
    
    -- Type-specific metadata
    metadata String CODEC(ZSTD(3)),  -- JSON string
    
    -- Timestamp
    timestamp DateTime64(3, 'UTC'),
    
    -- Indexes
    INDEX idx_activity_id activity_id TYPE bloom_filter(0.01) GRANULARITY 1,
    INDEX idx_tx_hash tx_hash TYPE bloom_filter(0.01) GRANULARITY 1,
    INDEX idx_from_lock from_lock_hash TYPE bloom_filter(0.01) GRANULARITY 1,
    INDEX idx_to_lock to_lock_hash TYPE bloom_filter(0.01) GRANULARITY 1,
    INDEX idx_asset_id asset_id TYPE bloom_filter(0.01) GRANULARITY 1
)
ENGINE = MergeTree
PARTITION BY intDiv(block_number, 1000000)
ORDER BY (block_number, tx_hash, activity_index)
COMMENT 'Unified activity feed for all blockchain events.';

-- Projection for from_lock_hash queries (sender activity)
ALTER TABLE activities_all ADD PROJECTION prj_by_from_lock
(
    SELECT *
    ORDER BY (from_lock_hash, block_number DESC, activity_index DESC)
);

-- Projection for to_lock_hash queries (receiver activity)
ALTER TABLE activities_all ADD PROJECTION prj_by_to_lock
(
    SELECT *
    ORDER BY (to_lock_hash, block_number DESC, activity_index DESC)
);


-- ===========================================
-- 3. State Snapshot Tables (ReplacingMergeTree)
-- Track mutable state with version-based deduplication.
-- ===========================================

-- ---- cell_state ----
-- Tracks cell lifecycle: live, consumed, or removed by reorg.
-- On consumption: insert new row with is_live=0 and consumption info.
-- On reorg: insert new row with is_present=0 to mark as invalid.
CREATE TABLE IF NOT EXISTS cell_state
(
    -- Cell identity (OutPoint)
    tx_hash FixedString(32) CODEC(ZSTD(1)),
    output_index UInt16,
    
    -- Version for ReplacingMergeTree
    canon_version UInt64,
    
    -- State flags
    is_present UInt8 DEFAULT 1,  -- 1 = valid cell, 0 = removed by reorg
    is_live UInt8 DEFAULT 1,     -- 1 = unspent, 0 = consumed
    
    -- Consumption info (populated when is_live=0)
    consumed_by_tx FixedString(32) CODEC(ZSTD(1)),  -- Empty if live
    consumed_at_block UInt64 DEFAULT 0,
    consumed_at_index UInt16 DEFAULT 0,
    
    -- Denormalized cell info for efficient queries (avoid JOIN to cell_outputs_all)
    capacity UInt64,
    lock_script_hash FixedString(32) CODEC(ZSTD(1)),
    type_script_hash FixedString(32) CODEC(ZSTD(1)),  -- Empty if no type script
    lock_code_hash FixedString(32) CODEC(ZSTD(1)),
    type_code_hash FixedString(32) CODEC(ZSTD(1)),  -- Empty if no type script
    data_size UInt32 DEFAULT 0,
    created_at_block UInt64,
    
    -- Metadata
    updated_at DateTime64(3, 'UTC') DEFAULT now64(3),
    
    -- Indexes
    INDEX idx_lock_script_hash lock_script_hash TYPE bloom_filter(0.01) GRANULARITY 1,
    INDEX idx_type_script_hash type_script_hash TYPE bloom_filter(0.01) GRANULARITY 1
)
ENGINE = ReplacingMergeTree(canon_version)
ORDER BY (tx_hash, output_index)
COMMENT 'Cell lifecycle state. Use FINAL to get current state.';

-- Projection for live cells by lock_script_hash (address)
ALTER TABLE cell_state ADD PROJECTION prj_live_by_lock
(
    SELECT *
    ORDER BY (lock_script_hash, created_at_block DESC)
    WHERE is_present = 1 AND is_live = 1
);


-- ---- dao_deposits ----
-- DAO deposit lifecycle tracking.
CREATE TABLE IF NOT EXISTS dao_deposits
(
    -- Deposit cell identity
    tx_hash FixedString(32) CODEC(ZSTD(1)),
    output_index UInt16,
    
    -- Version for ReplacingMergeTree
    canon_version UInt64,
    
    -- Depositor
    lock_script_hash FixedString(32) CODEC(ZSTD(1)),
    
    -- Deposit info
    capacity UInt64,  -- In shannons
    deposit_block_number UInt64,
    deposit_tx_hash FixedString(32) CODEC(ZSTD(1)),
    deposit_timestamp DateTime64(3, 'UTC'),
    deposit_ar UInt64,  -- AR at deposit time (from DAO field)
    
    -- Status: 0=active, 1=requesting withdrawal, 2=withdrawn
    status UInt8 DEFAULT 0,
    
    -- Withdraw request info (populated when status >= 1)
    withdraw_request_block UInt64 DEFAULT 0,
    withdraw_request_tx FixedString(32) CODEC(ZSTD(1)),  -- Empty if not requested
    withdraw_request_timestamp DateTime64(3, 'UTC') DEFAULT toDateTime64(0, 3, 'UTC'),
    withdraw_request_ar UInt64 DEFAULT 0,
    
    -- Withdraw completion info (populated when status = 2)
    withdraw_block UInt64 DEFAULT 0,
    withdraw_tx FixedString(32) CODEC(ZSTD(1)),  -- Empty if not withdrawn
    withdraw_timestamp DateTime64(3, 'UTC') DEFAULT toDateTime64(0, 3, 'UTC'),
    
    -- Computed compensation (in shannons)
    compensation UInt64 DEFAULT 0,
    
    -- Metadata
    updated_at DateTime64(3, 'UTC') DEFAULT now64(3),
    
    -- Indexes
    INDEX idx_lock_script_hash lock_script_hash TYPE bloom_filter(0.01) GRANULARITY 1,
    INDEX idx_withdraw_request_tx withdraw_request_tx TYPE bloom_filter(0.01) GRANULARITY 1
)
ENGINE = ReplacingMergeTree(canon_version)
ORDER BY (tx_hash, output_index)
COMMENT 'DAO deposit lifecycle tracking. Use FINAL to get current state.';


-- ===========================================
-- 4. Aggregate Tables
-- ===========================================

-- ---- address_balances ----
-- Incremental balance tracking per address.
-- On new cell: INSERT row with balance delta.
-- On cell consumption: INSERT row with negative balance delta.
-- SummingMergeTree automatically sums balance column on merge.
CREATE TABLE IF NOT EXISTS address_balances
(
    -- Address identity
    lock_script_hash FixedString(32),
    
    -- Balance delta (positive for created cells, negative for consumed)
    balance Int128,  -- Signed for negative deltas
    
    -- Cell count deltas
    live_cells_delta Int64,
    total_cells_delta Int64,
    
    -- Transaction count delta
    transactions_delta Int64,
    
    -- Timeline tracking (use max aggregation)
    first_seen_block UInt64,
    last_activity_block UInt64
)
ENGINE = SummingMergeTree((balance, live_cells_delta, total_cells_delta, transactions_delta))
ORDER BY (lock_script_hash)
COMMENT 'Incremental address balance tracking. Query with GROUP BY and SUM for totals.';


-- ---- daily_stats ----
-- Pre-aggregated daily statistics using AggregatingMergeTree.
CREATE TABLE IF NOT EXISTS daily_stats
(
    -- Time bucket
    date Date,
    
    -- Aggregate states for incremental updates
    blocks_count SimpleAggregateFunction(sum, UInt64),
    transactions_count SimpleAggregateFunction(sum, UInt64),
    cells_created SimpleAggregateFunction(sum, UInt64),
    cells_consumed SimpleAggregateFunction(sum, UInt64),
    capacity_transferred SimpleAggregateFunction(sum, UInt256),
    
    -- Cumulative totals (use max to get latest)
    total_blocks SimpleAggregateFunction(max, UInt64),
    total_transactions SimpleAggregateFunction(max, UInt64),
    total_live_cells SimpleAggregateFunction(max, UInt64),
    
    -- Block time stats
    avg_block_time_ms SimpleAggregateFunction(avg, UInt32),
    
    -- Address activity
    new_addresses SimpleAggregateFunction(sum, UInt64),
    active_addresses SimpleAggregateFunction(max, UInt64),
    
    -- Common Knowledge Size (DAO U field)
    knowledge_size SimpleAggregateFunction(max, UInt256)
)
ENGINE = AggregatingMergeTree
ORDER BY (date)
COMMENT 'Pre-aggregated daily statistics. Use -Merge combinators for final values.';


-- ---- hourly_stats ----
-- Hourly statistics for recent data visualization.
CREATE TABLE IF NOT EXISTS hourly_stats
(
    hour DateTime,
    
    blocks_count SimpleAggregateFunction(sum, UInt64),
    transactions_count SimpleAggregateFunction(sum, UInt64),
    cells_created SimpleAggregateFunction(sum, UInt64),
    cells_consumed SimpleAggregateFunction(sum, UInt64),
    capacity_transferred SimpleAggregateFunction(sum, UInt256)
)
ENGINE = AggregatingMergeTree
ORDER BY (hour)
TTL hour + INTERVAL 30 DAY  -- Keep only 30 days of hourly data
COMMENT 'Hourly statistics with 30-day retention.';


-- ===========================================
-- 5. DAO Statistics
-- ===========================================

-- ---- dao_statistics ----
-- Global DAO statistics (single row, versioned).
CREATE TABLE IF NOT EXISTS dao_statistics
(
    -- Singleton key (always 1)
    id UInt8 DEFAULT 1,
    
    -- Version for updates
    canon_version UInt64,
    
    -- Deposit stats
    total_deposited UInt64 DEFAULT 0,
    total_depositors UInt32 DEFAULT 0,
    active_deposits UInt32 DEFAULT 0,
    
    -- Compensation tracking
    total_compensation_paid UInt64 DEFAULT 0,
    unclaimed_compensation UInt64 DEFAULT 0,
    
    -- Averages
    average_deposit_epochs UInt32 DEFAULT 0,
    estimated_apc String DEFAULT '',  -- Percentage string
    
    -- Issuance breakdown
    mining_reward String DEFAULT '0',
    deposit_compensation String DEFAULT '0',
    burnt String DEFAULT '0',
    cumulative_burnt String DEFAULT '',
    cumulative_secondary_issuance String DEFAULT '',
    cumulative_miner_secondary String DEFAULT '',
    cumulative_dao_compensation String DEFAULT '',
    secondary_issuance UInt64 DEFAULT 0,
    
    -- Tracking
    last_processed_block UInt64 DEFAULT 0,
    updated_at DateTime64(3, 'UTC') DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(canon_version)
ORDER BY (id)
COMMENT 'Global DAO statistics singleton.';


-- ---- block_secondary_issuance ----
-- Per-block secondary issuance breakdown (exact, not sampled).
CREATE TABLE IF NOT EXISTS block_secondary_issuance
(
    block_number UInt64,
    block_timestamp DateTime64(3, 'UTC'),
    
    -- RFC-0015 issuance breakdown (all in shannons)
    secondary_issuance UInt64 DEFAULT 0,
    miner_secondary UInt64 DEFAULT 0,
    dao_compensation UInt64 DEFAULT 0,
    burnt UInt64 DEFAULT 0
)
ENGINE = MergeTree
PARTITION BY intDiv(block_number, 1000000)
ORDER BY (block_number)
COMMENT 'Per-block secondary issuance breakdown.';


-- ---- dao_daily_snapshots ----
-- Daily DAO snapshots for charts.
CREATE TABLE IF NOT EXISTS dao_daily_snapshots
(
    date Date,
    canon_version UInt64,
    
    total_deposit UInt64 DEFAULT 0,
    depositors_count UInt32 DEFAULT 0,
    daily_deposit UInt64 DEFAULT 0,
    daily_deposit_count UInt32 DEFAULT 0,
    
    -- Total issuance from DAO field C
    total_issuance UInt64 DEFAULT 0,
    dao_data FixedString(32) CODEC(ZSTD(1)),
    
    secondary_issuance UInt64 DEFAULT 0,
    burnt UInt64 DEFAULT 0,
    cumulative_burnt String DEFAULT '',
    cumulative_mining_reward String DEFAULT '',
    cumulative_deposit_compensation String DEFAULT ''
)
ENGINE = ReplacingMergeTree(canon_version)
ORDER BY (date)
COMMENT 'Daily DAO metrics snapshots.';


-- ===========================================
-- 6. Token Tables
-- ===========================================

-- ---- tokens ----
-- UDT token metadata (sUDT, xUDT).
CREATE TABLE IF NOT EXISTS tokens
(
    -- Token identity
    type_script_hash FixedString(32),
    
    -- Version for updates
    canon_version UInt64,
    
    -- Type script components
    type_code_hash FixedString(32) CODEC(ZSTD(1)),
    type_hash_type UInt8,
    type_args String CODEC(ZSTD(3)),
    
    -- Token standard
    standard LowCardinality(String),  -- 'sudt' or 'xudt'
    
    -- Metadata
    name String DEFAULT '',
    symbol String DEFAULT '',
    decimals UInt8 DEFAULT 8,
    description String DEFAULT '' CODEC(ZSTD(3)),
    icon_url String DEFAULT '' CODEC(ZSTD(3)),
    
    -- Label info
    published UInt8 DEFAULT 0,
    famous UInt8 DEFAULT 0,
    tags Array(String) DEFAULT [],
    udt_type String DEFAULT '',
    manager String DEFAULT '',
    email String DEFAULT '',
    operator_website String DEFAULT '',
    label_updated_at DateTime64(3, 'UTC') DEFAULT toDateTime64(0, 3, 'UTC'),
    
    -- Statistics
    total_supply UInt256 DEFAULT 0,
    holders_count UInt32 DEFAULT 0,
    transfers_count UInt64 DEFAULT 0,
    transfers_24h UInt64 DEFAULT 0,
    
    -- Origin
    first_seen_block UInt64,
    first_seen_tx FixedString(32) CODEC(ZSTD(1)),
    
    -- Timestamps
    created_at DateTime64(3, 'UTC') DEFAULT now64(3),
    updated_at DateTime64(3, 'UTC') DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(canon_version)
ORDER BY (type_script_hash)
COMMENT 'UDT token metadata and statistics.';


-- ---- token_balances ----
-- Token balances per holder.
CREATE TABLE IF NOT EXISTS token_balances
(
    -- Composite key
    type_script_hash FixedString(32),
    lock_script_hash FixedString(32),
    
    -- Version for updates
    canon_version UInt64,
    
    -- Balance (can be large for high-supply tokens)
    balance UInt256 DEFAULT 0,
    
    -- Transaction tracking
    first_tx FixedString(32) CODEC(ZSTD(1)),
    last_tx FixedString(32) CODEC(ZSTD(1)),
    
    -- Timestamps
    created_at DateTime64(3, 'UTC') DEFAULT now64(3),
    updated_at DateTime64(3, 'UTC') DEFAULT now64(3),
    
    -- Indexes
    INDEX idx_lock lock_script_hash TYPE bloom_filter(0.01) GRANULARITY 1
)
ENGINE = ReplacingMergeTree(canon_version)
ORDER BY (type_script_hash, lock_script_hash)
COMMENT 'Token balances per holder address.';


-- ---- udt_cells ----
-- Live UDT cells for balance verification.
CREATE TABLE IF NOT EXISTS udt_cells
(
    -- Cell identity
    tx_hash FixedString(32) CODEC(ZSTD(1)),
    output_index UInt16,
    
    -- Version for state updates
    canon_version UInt64,
    
    -- Token identity
    type_script_hash FixedString(32) CODEC(ZSTD(1)),
    type_code_hash FixedString(32) CODEC(ZSTD(1)),
    type_hash_type UInt8,
    type_args String CODEC(ZSTD(3)),
    
    -- Owner
    lock_script_hash FixedString(32) CODEC(ZSTD(1)),
    
    -- Amount (parsed from cell data)
    amount UInt256,
    standard LowCardinality(String),
    
    -- Lifecycle
    is_live UInt8 DEFAULT 1,
    created_at_block UInt64,
    consumed_at_block UInt64 DEFAULT 0,
    consumed_by_tx FixedString(32) CODEC(ZSTD(1)),
    
    -- Indexes
    INDEX idx_type type_script_hash TYPE bloom_filter(0.01) GRANULARITY 1,
    INDEX idx_lock lock_script_hash TYPE bloom_filter(0.01) GRANULARITY 1
)
ENGINE = ReplacingMergeTree(canon_version)
ORDER BY (tx_hash, output_index)
COMMENT 'UDT cell tracking for balance verification.';


-- ===========================================
-- 7. Spore/NFT Tables
-- ===========================================

-- ---- spore_clusters ----
-- Spore NFT collections.
CREATE TABLE IF NOT EXISTS spore_clusters
(
    -- Cluster identity
    cluster_id FixedString(32),
    
    -- Version for updates
    canon_version UInt64,
    
    type_script_hash FixedString(32) CODEC(ZSTD(1)),
    
    -- Metadata
    name String DEFAULT '' CODEC(ZSTD(3)),
    description String DEFAULT '' CODEC(ZSTD(3)),
    
    -- Owner
    owner_lock_hash FixedString(32) CODEC(ZSTD(1)),
    
    -- Statistics
    spores_count UInt32 DEFAULT 0,
    
    -- Origin
    created_at_block UInt64,
    created_at_tx FixedString(32) CODEC(ZSTD(1)),
    
    -- Timestamps
    created_at DateTime64(3, 'UTC') DEFAULT now64(3),
    updated_at DateTime64(3, 'UTC') DEFAULT now64(3),
    
    -- Indexes
    INDEX idx_owner owner_lock_hash TYPE bloom_filter(0.01) GRANULARITY 1
)
ENGINE = ReplacingMergeTree(canon_version)
ORDER BY (cluster_id)
COMMENT 'Spore NFT collections.';


-- ---- spore_cells ----
-- Individual Spore NFTs.
CREATE TABLE IF NOT EXISTS spore_cells
(
    -- Spore identity
    spore_id FixedString(32),
    
    -- Version for updates
    canon_version UInt64,
    
    type_script_hash FixedString(32) CODEC(ZSTD(1)),
    tx_hash FixedString(32) CODEC(ZSTD(1)),
    output_index UInt16,
    
    -- Cluster reference (empty if standalone)
    cluster_id FixedString(32) CODEC(ZSTD(1)),
    
    -- Content info
    content_type String DEFAULT '' CODEC(ZSTD(3)),
    content_size UInt32 DEFAULT 0,
    
    -- Owner
    owner_lock_hash FixedString(32) CODEC(ZSTD(1)),
    
    -- Lifecycle
    is_live UInt8 DEFAULT 1,
    
    -- Origin
    created_at_block UInt64,
    created_at_tx FixedString(32) CODEC(ZSTD(1)),
    
    -- Consumption
    consumed_at_block UInt64 DEFAULT 0,
    consumed_by_tx FixedString(32) CODEC(ZSTD(1)),
    
    -- Timestamps
    created_at DateTime64(3, 'UTC') DEFAULT now64(3),
    updated_at DateTime64(3, 'UTC') DEFAULT now64(3),
    
    -- Indexes
    INDEX idx_cluster cluster_id TYPE bloom_filter(0.01) GRANULARITY 1,
    INDEX idx_owner owner_lock_hash TYPE bloom_filter(0.01) GRANULARITY 1
)
ENGINE = ReplacingMergeTree(canon_version)
ORDER BY (spore_id)
COMMENT 'Individual Spore NFTs.';


-- ---- spore_content ----
-- Spore NFT content (stored separately due to size).
CREATE TABLE IF NOT EXISTS spore_content
(
    spore_id FixedString(32),
    canon_version UInt64,
    content String CODEC(ZSTD(3)),
    created_at DateTime64(3, 'UTC') DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(canon_version)
ORDER BY (spore_id)
COMMENT 'Spore NFT content blobs.';


-- ===========================================
-- 7b. M-NFT Tables (Legacy)
-- ===========================================

-- ---- mnft_issuers ----
CREATE TABLE IF NOT EXISTS mnft_issuers
(
    issuer_id FixedString(20),  -- 20 bytes
    canon_version UInt64,
    
    type_script_hash FixedString(32) CODEC(ZSTD(1)),
    name String DEFAULT '' CODEC(ZSTD(3)),
    info String DEFAULT '' CODEC(ZSTD(3)),
    owner_lock_hash FixedString(32) CODEC(ZSTD(1)),
    classes_count UInt32 DEFAULT 0,
    is_live UInt8 DEFAULT 1,
    
    created_at_block UInt64,
    created_at_tx FixedString(32) CODEC(ZSTD(1)),
    consumed_at_block UInt64 DEFAULT 0,
    consumed_by_tx FixedString(32) CODEC(ZSTD(1)),
    
    created_at DateTime64(3, 'UTC') DEFAULT now64(3),
    updated_at DateTime64(3, 'UTC') DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(canon_version)
ORDER BY (issuer_id)
COMMENT 'M-NFT issuers (legacy).';


-- ---- mnft_classes ----
CREATE TABLE IF NOT EXISTS mnft_classes
(
    class_id String,  -- Variable length: issuer_id + class_index
    canon_version UInt64,
    
    type_script_hash FixedString(32) CODEC(ZSTD(1)),
    issuer_id FixedString(20),
    
    name String DEFAULT '' CODEC(ZSTD(3)),
    description String DEFAULT '' CODEC(ZSTD(3)),
    renderer String DEFAULT '' CODEC(ZSTD(3)),
    
    total UInt32 DEFAULT 0,
    issued UInt32 DEFAULT 0,
    holders_count UInt32 DEFAULT 0,
    transfers_count UInt64 DEFAULT 0,
    transfers_24h UInt32 DEFAULT 0,
    
    owner_lock_hash FixedString(32) CODEC(ZSTD(1)),
    is_live UInt8 DEFAULT 1,
    
    created_at_block UInt64,
    created_at_tx FixedString(32) CODEC(ZSTD(1)),
    consumed_at_block UInt64 DEFAULT 0,
    consumed_by_tx FixedString(32) CODEC(ZSTD(1)),
    
    created_at DateTime64(3, 'UTC') DEFAULT now64(3),
    updated_at DateTime64(3, 'UTC') DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(canon_version)
ORDER BY (class_id)
COMMENT 'M-NFT classes (legacy).';


-- ---- mnft_tokens ----
CREATE TABLE IF NOT EXISTS mnft_tokens
(
    token_id String,  -- class_id + token_index
    canon_version UInt64,
    
    type_script_hash FixedString(32) CODEC(ZSTD(1)),
    tx_hash FixedString(32) CODEC(ZSTD(1)),
    output_index UInt16,
    
    class_id String CODEC(ZSTD(3)),
    token_index UInt32,
    
    characteristic String DEFAULT '' CODEC(ZSTD(3)),
    configure UInt8 DEFAULT 0,
    state UInt8 DEFAULT 0,
    
    owner_lock_hash FixedString(32) CODEC(ZSTD(1)),
    is_live UInt8 DEFAULT 1,
    
    created_at_block UInt64,
    created_at_tx FixedString(32) CODEC(ZSTD(1)),
    consumed_at_block UInt64 DEFAULT 0,
    consumed_by_tx FixedString(32) CODEC(ZSTD(1)),
    
    created_at DateTime64(3, 'UTC') DEFAULT now64(3),
    updated_at DateTime64(3, 'UTC') DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(canon_version)
ORDER BY (token_id)
COMMENT 'M-NFT tokens (legacy).';


-- ===========================================
-- 7c. DotBit Tables
-- ===========================================

CREATE TABLE IF NOT EXISTS dotbit_accounts
(
    account_id FixedString(32),
    canon_version UInt64,
    
    type_script_hash FixedString(32) CODEC(ZSTD(1)),
    tx_hash FixedString(32) CODEC(ZSTD(1)),
    output_index UInt16,
    
    account_name String CODEC(ZSTD(3)),
    
    owner_lock_hash FixedString(32) CODEC(ZSTD(1)),
    manager_lock_hash FixedString(32) CODEC(ZSTD(1)),
    
    registered_at UInt64 DEFAULT 0,
    expired_at UInt64 DEFAULT 0,
    status UInt8 DEFAULT 0,
    is_live UInt8 DEFAULT 1,
    
    created_at_block UInt64,
    created_at_tx FixedString(32) CODEC(ZSTD(1)),
    consumed_at_block UInt64 DEFAULT 0,
    consumed_by_tx FixedString(32) CODEC(ZSTD(1)),
    
    created_at DateTime64(3, 'UTC') DEFAULT now64(3),
    updated_at DateTime64(3, 'UTC') DEFAULT now64(3),
    
    INDEX idx_name account_name TYPE bloom_filter(0.01) GRANULARITY 1,
    INDEX idx_owner owner_lock_hash TYPE bloom_filter(0.01) GRANULARITY 1
)
ENGINE = ReplacingMergeTree(canon_version)
ORDER BY (account_id)
COMMENT 'DotBit (.bit) accounts.';


-- ===========================================
-- 8. Script Tables
-- ===========================================

-- ---- scripts ----
-- Script metadata and usage statistics.
CREATE TABLE IF NOT EXISTS scripts
(
    -- Script identity
    hash FixedString(32),
    canon_version UInt64,
    
    -- Script components
    code_hash FixedString(32) CODEC(ZSTD(1)),
    hash_type UInt8,
    args String CODEC(ZSTD(3)),
    
    -- Origin
    first_seen_block UInt64,
    first_seen_tx FixedString(32) CODEC(ZSTD(1)),
    
    -- Usage stats
    cells_count UInt64 DEFAULT 0,
    live_cells_count UInt64 DEFAULT 0,
    capacity_sum UInt256 DEFAULT 0,
    live_capacity_sum UInt256 DEFAULT 0,
    
    -- Timestamps
    created_at DateTime64(3, 'UTC') DEFAULT now64(3),
    updated_at DateTime64(3, 'UTC') DEFAULT now64(3),
    
    INDEX idx_code_hash code_hash TYPE bloom_filter(0.01) GRANULARITY 1
)
ENGINE = ReplacingMergeTree(canon_version)
ORDER BY (hash)
COMMENT 'Script metadata and usage statistics.';


-- ---- known_scripts ----
-- Known CKB script metadata from token-labels repository.
CREATE TABLE IF NOT EXISTS known_scripts
(
    -- Composite key
    code_hash FixedString(32),
    network LowCardinality(String) DEFAULT 'mainnet',  -- mainnet or testnet
    tag String DEFAULT '',  -- Version tag
    
    -- Version for updates
    canon_version UInt64,
    
    -- Metadata
    name String CODEC(ZSTD(3)),
    description String DEFAULT '' CODEC(ZSTD(3)),
    
    -- Extended metadata
    rfc String DEFAULT '' CODEC(ZSTD(3)),
    website String DEFAULT '' CODEC(ZSTD(3)),
    source_url String DEFAULT '' CODEC(ZSTD(3)),
    decoder_type LowCardinality(String) DEFAULT '',  -- udt, spore, dao, ckbfs
    
    -- Deployment info
    hash_type LowCardinality(String) DEFAULT '',  -- type, data, data1, data2
    data_hash FixedString(32) CODEC(ZSTD(1)),
    type_hash FixedString(32) CODEC(ZSTD(1)),
    deprecated UInt8 DEFAULT 0,
    
    -- Flags
    is_system UInt8 DEFAULT 0,
    label_source String DEFAULT 'token-labels',
    label_updated_at DateTime64(3, 'UTC') DEFAULT toDateTime64(0, 3, 'UTC'),
    
    created_at DateTime64(3, 'UTC') DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(canon_version)
ORDER BY (code_hash, network, tag)
COMMENT 'Known CKB script metadata.';


-- ---- script_usage_stats ----
-- Precomputed script usage statistics.
CREATE TABLE IF NOT EXISTS script_usage_stats
(
    code_hash FixedString(32),
    script_kind LowCardinality(String),  -- 'lock' or 'type'
    canon_version UInt64,
    
    cells_count UInt64 DEFAULT 0,
    live_cells_count UInt64 DEFAULT 0,
    capacity_sum UInt256 DEFAULT 0,
    live_capacity_sum UInt256 DEFAULT 0,
    
    updated_at DateTime64(3, 'UTC') DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(canon_version)
ORDER BY (code_hash, script_kind)
COMMENT 'Precomputed script usage statistics.';


-- ===========================================
-- 9. Auxiliary Tables
-- ===========================================

-- ---- uncle_blocks ----
CREATE TABLE IF NOT EXISTS uncle_blocks
(
    block_number UInt64,  -- Block that includes this uncle
    uncle_index UInt32,
    
    hash FixedString(32) CODEC(ZSTD(1)),
    proposals_hash FixedString(32) CODEC(ZSTD(1)),
    timestamp DateTime64(3, 'UTC'),
    compact_target UInt64,
    epoch_number UInt64,
    epoch_index UInt32,
    epoch_length UInt32,
    parent_hash FixedString(32) CODEC(ZSTD(1)),
    transactions_root FixedString(32) CODEC(ZSTD(1)),
    extra_hash FixedString(32) CODEC(ZSTD(1)),
    dao FixedString(32) CODEC(ZSTD(1)),
    nonce FixedString(16) CODEC(ZSTD(1)),
    
    INDEX idx_hash hash TYPE bloom_filter(0.01) GRANULARITY 1
)
ENGINE = MergeTree
PARTITION BY intDiv(block_number, 1000000)
ORDER BY (block_number, uncle_index)
COMMENT 'Uncle blocks.';


-- ---- block_proposals ----
CREATE TABLE IF NOT EXISTS block_proposals
(
    block_number UInt64,
    proposal_index UInt16,
    proposal_id FixedString(10) CODEC(ZSTD(1))  -- 10-byte short tx ID
)
ENGINE = MergeTree
PARTITION BY intDiv(block_number, 1000000)
ORDER BY (block_number, proposal_index)
COMMENT 'Block proposal short IDs.';


-- ---- transaction_cell_deps ----
CREATE TABLE IF NOT EXISTS transaction_cell_deps
(
    tx_hash FixedString(32) CODEC(ZSTD(1)),
    tx_block_number UInt64,
    dep_index UInt16,
    
    out_point_tx_hash FixedString(32) CODEC(ZSTD(1)),
    out_point_index UInt16,
    dep_type UInt8,  -- 0=code, 1=dep_group
    
    INDEX idx_tx_hash tx_hash TYPE bloom_filter(0.01) GRANULARITY 1
)
ENGINE = MergeTree
PARTITION BY intDiv(tx_block_number, 1000000)
ORDER BY (tx_block_number, tx_hash, dep_index)
COMMENT 'Transaction cell dependencies.';


-- ---- cell_data ----
-- Large cell data stored separately.
CREATE TABLE IF NOT EXISTS cell_data
(
    tx_hash FixedString(32),
    output_index UInt16,
    data String CODEC(ZSTD(3))
)
ENGINE = MergeTree
ORDER BY (tx_hash, output_index)
COMMENT 'Large cell data stored separately.';


-- ---- tx_block_map ----
-- Maps tx_hash to block_number for partition pruning.
CREATE TABLE IF NOT EXISTS tx_block_map
(
    tx_hash FixedString(32),
    block_number UInt64
)
ENGINE = MergeTree
ORDER BY (tx_hash)
COMMENT 'Transaction to block number mapping for partition pruning.';


-- ===========================================
-- 10. Reorg Tracking
-- ===========================================

-- ---- reorg_events ----
CREATE TABLE IF NOT EXISTS reorg_events
(
    id UInt64,  -- Use explicit ID instead of auto-increment
    detected_at DateTime64(3, 'UTC') DEFAULT now64(3),
    
    fork_point_number UInt64,
    fork_point_hash FixedString(32) CODEC(ZSTD(1)),
    
    old_tip_number UInt64,
    old_tip_hash FixedString(32) CODEC(ZSTD(1)),
    
    new_tip_number UInt64,
    new_tip_hash FixedString(32) CODEC(ZSTD(1)),
    
    depth UInt32,
    orphaned_blocks_count UInt32 DEFAULT 0,
    orphaned_txs_count UInt32 DEFAULT 0,
    
    event_type LowCardinality(String) DEFAULT 'auto',  -- auto, deep, resolved
    
    resolved_at DateTime64(3, 'UTC') DEFAULT toDateTime64(0, 3, 'UTC'),
    resolved_by String DEFAULT '',
    resolution_action LowCardinality(String) DEFAULT '',
    resolution_notes String DEFAULT '' CODEC(ZSTD(3))
)
ENGINE = MergeTree
ORDER BY (detected_at DESC, id)
COMMENT 'Chain reorganization events.';


-- ---- orphaned_blocks ----
CREATE TABLE IF NOT EXISTS orphaned_blocks
(
    reorg_event_id UInt64,
    
    number UInt64,
    hash FixedString(32) CODEC(ZSTD(1)),
    parent_hash FixedString(32) CODEC(ZSTD(1)),
    timestamp DateTime64(3, 'UTC'),
    transactions_count UInt32,
    miner_lock_hash FixedString(32) CODEC(ZSTD(1)),
    
    orphaned_at DateTime64(3, 'UTC') DEFAULT now64(3),
    
    INDEX idx_hash hash TYPE bloom_filter(0.01) GRANULARITY 1
)
ENGINE = MergeTree
ORDER BY (reorg_event_id, number)
COMMENT 'Orphaned blocks from reorgs.';


-- ---- orphaned_transactions ----
CREATE TABLE IF NOT EXISTS orphaned_transactions
(
    reorg_event_id UInt64,
    
    hash FixedString(32) CODEC(ZSTD(1)),
    block_number UInt64,
    block_hash FixedString(32) CODEC(ZSTD(1)),
    tx_index UInt32,
    
    inputs_count UInt16 DEFAULT 0,
    outputs_count UInt16 DEFAULT 0,
    total_capacity UInt64 DEFAULT 0,
    
    orphaned_at DateTime64(3, 'UTC') DEFAULT now64(3),
    
    INDEX idx_hash hash TYPE bloom_filter(0.01) GRANULARITY 1
)
ENGINE = MergeTree
ORDER BY (reorg_event_id, block_number, tx_index)
COMMENT 'Orphaned transactions from reorgs.';


-- ===========================================
-- 11. Sync/Task Management
-- ===========================================

-- ---- sync_status ----
-- Singleton table for sync state tracking.
CREATE TABLE IF NOT EXISTS sync_status
(
    id UInt8 DEFAULT 1,
    canon_version UInt64,
    
    -- Deep fork detection
    deep_fork_detected UInt8 DEFAULT 0,
    deep_fork_at DateTime64(3, 'UTC') DEFAULT toDateTime64(0, 3, 'UTC'),
    deep_fork_db_tip UInt64 DEFAULT 0,
    deep_fork_db_tip_hash FixedString(32) CODEC(ZSTD(1)),
    deep_fork_chain_tip UInt64 DEFAULT 0,
    deep_fork_chain_tip_hash FixedString(32) CODEC(ZSTD(1)),
    deep_fork_depth UInt32 DEFAULT 0,
    deep_fork_fork_point UInt64 DEFAULT 0,
    
    -- Last reorg
    last_reorg_at DateTime64(3, 'UTC') DEFAULT toDateTime64(0, 3, 'UTC'),
    last_reorg_depth UInt32 DEFAULT 0,
    
    -- Deferred flags
    indexes_deferred UInt8 DEFAULT 0,
    indexes_dropped_at DateTime64(3, 'UTC') DEFAULT toDateTime64(0, 3, 'UTC'),
    activities_deferred UInt8 DEFAULT 0,
    activities_deferred_at DateTime64(3, 'UTC') DEFAULT toDateTime64(0, 3, 'UTC'),
    activities_rebuild_completed_at DateTime64(3, 'UTC') DEFAULT toDateTime64(0, 3, 'UTC'),
    address_balances_deferred UInt8 DEFAULT 0,
    address_balances_deferred_at DateTime64(3, 'UTC') DEFAULT toDateTime64(0, 3, 'UTC'),
    address_balances_rebuild_completed_at DateTime64(3, 'UTC') DEFAULT toDateTime64(0, 3, 'UTC'),
    token_deferred UInt8 DEFAULT 0,
    token_deferred_at DateTime64(3, 'UTC') DEFAULT toDateTime64(0, 3, 'UTC'),
    token_rebuild_completed_at DateTime64(3, 'UTC') DEFAULT toDateTime64(0, 3, 'UTC'),
    spore_deferred UInt8 DEFAULT 0,
    spore_deferred_at DateTime64(3, 'UTC') DEFAULT toDateTime64(0, 3, 'UTC'),
    spore_rebuild_completed_at DateTime64(3, 'UTC') DEFAULT toDateTime64(0, 3, 'UTC'),
    dao_deferred UInt8 DEFAULT 0,
    dao_deferred_at DateTime64(3, 'UTC') DEFAULT toDateTime64(0, 3, 'UTC'),
    dao_rebuild_completed_at DateTime64(3, 'UTC') DEFAULT toDateTime64(0, 3, 'UTC'),
    tx_block_map_deferred UInt8 DEFAULT 0,
    tx_block_map_deferred_at DateTime64(3, 'UTC') DEFAULT toDateTime64(0, 3, 'UTC'),
    tx_block_map_rebuild_completed_at DateTime64(3, 'UTC') DEFAULT toDateTime64(0, 3, 'UTC'),
    stats_rebuild_in_progress UInt8 DEFAULT 0
)
ENGINE = ReplacingMergeTree(canon_version)
ORDER BY (id)
COMMENT 'Sync status singleton. Use id=1.';


-- ---- tasks ----
-- Background task management.
CREATE TABLE IF NOT EXISTS tasks
(
    id UUID,
    
    task_type LowCardinality(String),  -- cycles_backfill, index_rebuild, etc.
    status LowCardinality(String) DEFAULT 'pending',  -- pending, running, completed, failed, cancelled, paused
    priority Int32 DEFAULT 0,
    
    -- Configuration
    config String DEFAULT '{}' CODEC(ZSTD(3)),  -- JSON
    
    -- Progress
    progress_total UInt64 DEFAULT 0,
    progress_current UInt64 DEFAULT 0,
    progress_message String DEFAULT '',
    
    -- Result/error
    result String DEFAULT '' CODEC(ZSTD(3)),  -- JSON
    error_message String DEFAULT '' CODEC(ZSTD(3)),
    
    -- Timestamps
    created_at DateTime64(3, 'UTC') DEFAULT now64(3),
    started_at DateTime64(3, 'UTC') DEFAULT toDateTime64(0, 3, 'UTC'),
    completed_at DateTime64(3, 'UTC') DEFAULT toDateTime64(0, 3, 'UTC'),
    heartbeat_at DateTime64(3, 'UTC') DEFAULT toDateTime64(0, 3, 'UTC'),
    
    -- Runtime
    runner_id String DEFAULT '',
    retry_count UInt32 DEFAULT 0,
    max_retries UInt32 DEFAULT 3,
    
    -- Rate tracking
    rate_samples String DEFAULT '[]' CODEC(ZSTD(3)),  -- JSON array
    rate_ema Float64 DEFAULT 0,
    
    -- Log tail
    log_tail String DEFAULT '' CODEC(ZSTD(3))
)
ENGINE = MergeTree
ORDER BY (status, created_at DESC, id)
COMMENT 'Background task management.';


-- ===========================================
-- 12. Epoch Statistics
-- ===========================================

CREATE TABLE IF NOT EXISTS epoch_statistics
(
    epoch_number UInt64,
    canon_version UInt64,
    
    start_block UInt64,
    end_block UInt64 DEFAULT 0,
    blocks_count UInt32 DEFAULT 0,
    length UInt32,
    
    start_timestamp DateTime64(3, 'UTC'),
    end_timestamp DateTime64(3, 'UTC') DEFAULT toDateTime64(0, 3, 'UTC'),
    
    difficulty UInt256 DEFAULT 0,
    hash_rate UInt256 DEFAULT 0,
    transactions_count UInt32 DEFAULT 0,
    
    created_at DateTime64(3, 'UTC') DEFAULT now64(3),
    updated_at DateTime64(3, 'UTC') DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(canon_version)
ORDER BY (epoch_number)
COMMENT 'Per-epoch statistics.';


-- ===========================================
-- 13. Distribution Tables
-- ===========================================

-- ---- block_time_distribution ----
-- Block time histogram (100ms buckets).
CREATE TABLE IF NOT EXISTS block_time_distribution
(
    bucket_ms UInt32,  -- 100, 200, ..., 50000
    block_count SimpleAggregateFunction(sum, UInt64)
)
ENGINE = AggregatingMergeTree
ORDER BY (bucket_ms)
COMMENT 'Block time distribution histogram.';


-- ---- epoch_time_distribution ----
CREATE TABLE IF NOT EXISTS epoch_time_distribution
(
    bucket_minutes UInt32,
    epoch_count SimpleAggregateFunction(sum, UInt64)
)
ENGINE = AggregatingMergeTree
ORDER BY (bucket_minutes)
COMMENT 'Epoch time distribution histogram.';


-- ===========================================
-- 14. Miner Statistics
-- ===========================================

CREATE TABLE IF NOT EXISTS miner_statistics
(
    date Date,
    miner_lock_hash FixedString(32),
    
    blocks_count SimpleAggregateFunction(sum, UInt64),
    total_reward SimpleAggregateFunction(sum, UInt64),
    last_block_number SimpleAggregateFunction(max, UInt64)
)
ENGINE = AggregatingMergeTree
ORDER BY (date, miner_lock_hash)
COMMENT 'Daily miner statistics.';


-- ===========================================
-- Done. Schema ready for CKB blockchain indexer.
-- 
-- Query Patterns:
-- 1. Canonical chain: JOIN with canonical_blocks or use MAX(canon_version)
-- 2. Live cells: cell_state FINAL WHERE is_present=1 AND is_live=1
-- 3. Address balance: SELECT SUM(balance) FROM address_balances WHERE lock_script_hash=X
-- 4. Daily stats: SELECT date, sumMerge(blocks_count) FROM daily_stats GROUP BY date
-- ===========================================
