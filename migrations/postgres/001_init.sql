-- ============================================
-- ckbadger Database Schema
-- Optimized for 2-5x CKB mainnet data volume
-- Partition size: 5M blocks (~1.5 years)
-- ============================================

-- ===========================================
-- 1. Sync Status (Persistent State Only)
-- High-frequency sync data is stored in Redis (sync:status key)
-- This table stores only data that must survive restarts
-- ===========================================

CREATE TABLE sync_status (
    id INTEGER PRIMARY KEY DEFAULT 1,
    
    -- Deep fork detection (reorg depth > REORG_LIMIT)
    deep_fork_detected BOOLEAN NOT NULL DEFAULT FALSE,
    deep_fork_at TIMESTAMPTZ,
    deep_fork_db_tip BIGINT,
    deep_fork_db_tip_hash BYTEA,
    deep_fork_chain_tip BIGINT,
    deep_fork_chain_tip_hash BYTEA,
    deep_fork_depth INT,
    deep_fork_fork_point BIGINT,
    
    -- Last reorg tracking
    last_reorg_at TIMESTAMPTZ,
    last_reorg_depth INT,
    
    -- Deferred index optimization (tracks actual DB state)
    indexes_deferred BOOLEAN NOT NULL DEFAULT FALSE,
    indexes_dropped_at TIMESTAMPTZ,

    CONSTRAINT single_row CHECK (id = 1)
);

INSERT INTO sync_status (id) VALUES (1);

-- ===========================================
-- 1b. Chain Reorganization Tracking
-- ===========================================

-- Track reorg events
CREATE TABLE reorg_events (
    id SERIAL PRIMARY KEY,
    detected_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Fork point (common ancestor)
    fork_point_number BIGINT NOT NULL,
    fork_point_hash BYTEA NOT NULL,
    
    -- Old chain tip (replaced)
    old_tip_number BIGINT NOT NULL,
    old_tip_hash BYTEA NOT NULL,
    
    -- New chain tip (after reorg)
    new_tip_number BIGINT NOT NULL,
    new_tip_hash BYTEA NOT NULL,
    
    -- Statistics
    depth INT NOT NULL,  -- old_tip - fork_point
    orphaned_blocks_count INT NOT NULL DEFAULT 0,
    orphaned_txs_count INT NOT NULL DEFAULT 0,
    
    -- Type: 'auto' = automatic reorg, 'deep' = deep fork pending, 'resolved' = manually resolved
    event_type VARCHAR(20) NOT NULL DEFAULT 'auto',
    
    -- Manual resolution tracking
    resolved_at TIMESTAMPTZ,
    resolved_by VARCHAR(100),
    resolution_action VARCHAR(50),  -- 'rollback', 'reset', 'dismissed'
    resolution_notes TEXT
);

CREATE INDEX idx_reorg_events_detected_at ON reorg_events(detected_at DESC);
CREATE INDEX idx_reorg_events_type ON reorg_events(event_type) WHERE event_type = 'deep';

-- Orphaned blocks (blocks that were on the old fork)
CREATE TABLE orphaned_blocks (
    id SERIAL PRIMARY KEY,
    reorg_event_id INT NOT NULL REFERENCES reorg_events(id) ON DELETE CASCADE,
    
    -- Original block info
    number BIGINT NOT NULL,
    hash BYTEA NOT NULL,
    parent_hash BYTEA NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL,
    transactions_count INT NOT NULL,
    miner_lock_hash BYTEA,
    
    orphaned_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_orphaned_blocks_reorg ON orphaned_blocks(reorg_event_id);
CREATE INDEX idx_orphaned_blocks_hash ON orphaned_blocks(hash);
CREATE INDEX idx_orphaned_blocks_number ON orphaned_blocks(number);

-- Orphaned transactions
CREATE TABLE orphaned_transactions (
    id SERIAL PRIMARY KEY,
    reorg_event_id INT NOT NULL REFERENCES reorg_events(id) ON DELETE CASCADE,
    
    hash BYTEA NOT NULL,
    block_number BIGINT NOT NULL,
    block_hash BYTEA NOT NULL,
    tx_index INT NOT NULL,
    
    -- Key transaction data for display
    inputs_count SMALLINT,
    outputs_count SMALLINT,
    total_capacity NUMERIC(20,0),
    
    orphaned_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_orphaned_txs_reorg ON orphaned_transactions(reorg_event_id);
CREATE INDEX idx_orphaned_txs_hash ON orphaned_transactions(hash);

CREATE TABLE integrity_recent_fixes (
    id SERIAL PRIMARY KEY,
    tx_hash BYTEA NOT NULL,
    cycles BIGINT NOT NULL,
    fixed_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_integrity_recent_fixes_fixed_at ON integrity_recent_fixes (fixed_at DESC);

-- ===========================================
-- 2. Core Tables (Partitioned by 5M blocks)
-- ===========================================

-- ---- blocks ----
CREATE TABLE blocks (
    number BIGINT NOT NULL,
    hash BYTEA NOT NULL,
    parent_hash BYTEA NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL,
    version INTEGER NOT NULL,
    compact_target BIGINT NOT NULL,
    transactions_count INTEGER NOT NULL DEFAULT 0,
    proposals_count INTEGER NOT NULL DEFAULT 0,
    uncles_count INTEGER NOT NULL DEFAULT 0,
    epoch_number BIGINT NOT NULL,
    epoch_index INTEGER NOT NULL,
    epoch_length INTEGER NOT NULL,
    dao BYTEA NOT NULL,  -- 32 bytes
    nonce BYTEA NOT NULL,
    extra_hash BYTEA NOT NULL,
    extension BYTEA,
    proposals_hash BYTEA NOT NULL,
    transactions_root BYTEA NOT NULL,
    uncles_hash BYTEA NOT NULL,
    miner_lock_hash BYTEA,
    miner_message BYTEA,
    total_difficulty NUMERIC(40,0) NOT NULL DEFAULT 0,
    reward NUMERIC(20,0),
    PRIMARY KEY (number)
) PARTITION BY RANGE (number);

-- 10 partitions: 0-50M blocks (~15 years)
CREATE TABLE blocks_p00 PARTITION OF blocks FOR VALUES FROM (0) TO (5000000);
CREATE TABLE blocks_p01 PARTITION OF blocks FOR VALUES FROM (5000000) TO (10000000);
CREATE TABLE blocks_p02 PARTITION OF blocks FOR VALUES FROM (10000000) TO (15000000);
CREATE TABLE blocks_p03 PARTITION OF blocks FOR VALUES FROM (15000000) TO (20000000);
CREATE TABLE blocks_p04 PARTITION OF blocks FOR VALUES FROM (20000000) TO (25000000);
CREATE TABLE blocks_p05 PARTITION OF blocks FOR VALUES FROM (25000000) TO (30000000);
CREATE TABLE blocks_p06 PARTITION OF blocks FOR VALUES FROM (30000000) TO (35000000);
CREATE TABLE blocks_p07 PARTITION OF blocks FOR VALUES FROM (35000000) TO (40000000);
CREATE TABLE blocks_p08 PARTITION OF blocks FOR VALUES FROM (40000000) TO (45000000);
CREATE TABLE blocks_p09 PARTITION OF blocks FOR VALUES FROM (45000000) TO (50000000);

-- ---- transactions ----
CREATE TABLE transactions (
    hash BYTEA NOT NULL,
    block_number BIGINT NOT NULL,
    tx_index INTEGER NOT NULL,
    version INTEGER NOT NULL,
    inputs_count SMALLINT NOT NULL DEFAULT 0,
    outputs_count SMALLINT NOT NULL DEFAULT 0,
    witnesses_count SMALLINT NOT NULL DEFAULT 0,
    cell_deps_count SMALLINT NOT NULL DEFAULT 0,
    header_deps_count SMALLINT NOT NULL DEFAULT 0,
    total_input_capacity BIGINT NOT NULL DEFAULT 0,
    total_output_capacity BIGINT NOT NULL DEFAULT 0,
    fee BIGINT NOT NULL DEFAULT 0,
    tx_size INTEGER,
    cycles BIGINT,
    is_cellbase BOOLEAN NOT NULL DEFAULT FALSE,
    timestamp TIMESTAMPTZ NOT NULL,
    short_hash BYTEA GENERATED ALWAYS AS (substring(hash, 1, 10)) STORED,
    PRIMARY KEY (block_number, hash)
) PARTITION BY RANGE (block_number);

CREATE TABLE transactions_p00 PARTITION OF transactions FOR VALUES FROM (0) TO (5000000);
CREATE TABLE transactions_p01 PARTITION OF transactions FOR VALUES FROM (5000000) TO (10000000);
CREATE TABLE transactions_p02 PARTITION OF transactions FOR VALUES FROM (10000000) TO (15000000);
CREATE TABLE transactions_p03 PARTITION OF transactions FOR VALUES FROM (15000000) TO (20000000);
CREATE TABLE transactions_p04 PARTITION OF transactions FOR VALUES FROM (20000000) TO (25000000);
CREATE TABLE transactions_p05 PARTITION OF transactions FOR VALUES FROM (25000000) TO (30000000);
CREATE TABLE transactions_p06 PARTITION OF transactions FOR VALUES FROM (30000000) TO (35000000);
CREATE TABLE transactions_p07 PARTITION OF transactions FOR VALUES FROM (35000000) TO (40000000);
CREATE TABLE transactions_p08 PARTITION OF transactions FOR VALUES FROM (40000000) TO (45000000);
CREATE TABLE transactions_p09 PARTITION OF transactions FOR VALUES FROM (45000000) TO (50000000);

-- ---- cells ----
CREATE TABLE cells (
    id BIGINT GENERATED ALWAYS AS IDENTITY,
    tx_hash BYTEA NOT NULL,
    output_index SMALLINT NOT NULL,
    capacity BIGINT NOT NULL,  -- shannon, max ~10^18 (fits in i64)

    -- Lock Script (required)
    lock_code_hash BYTEA NOT NULL,
    lock_hash_type SMALLINT NOT NULL,
    lock_args BYTEA NOT NULL,
    lock_script_hash BYTEA NOT NULL,

    -- Type Script (optional)
    type_code_hash BYTEA,
    type_hash_type SMALLINT,
    type_args BYTEA,
    type_script_hash BYTEA,

    -- Data
    data_hash BYTEA NOT NULL,
    data_size INTEGER NOT NULL DEFAULT 0,
    data BYTEA,  -- up to 512 bytes for hex decoder preview

    -- Lifecycle
    status SMALLINT NOT NULL DEFAULT 0,  -- 0=live, 1=dead
    created_at_block BIGINT NOT NULL,
    consumed_at_block BIGINT,
    consumed_by_tx BYTEA,
    consumed_at_index SMALLINT,

    PRIMARY KEY (created_at_block, id),
    UNIQUE (created_at_block, tx_hash, output_index)
) PARTITION BY RANGE (created_at_block);

CREATE TABLE cells_p00 PARTITION OF cells FOR VALUES FROM (0) TO (5000000);
CREATE TABLE cells_p01 PARTITION OF cells FOR VALUES FROM (5000000) TO (10000000);
CREATE TABLE cells_p02 PARTITION OF cells FOR VALUES FROM (10000000) TO (15000000);
CREATE TABLE cells_p03 PARTITION OF cells FOR VALUES FROM (15000000) TO (20000000);
CREATE TABLE cells_p04 PARTITION OF cells FOR VALUES FROM (20000000) TO (25000000);
CREATE TABLE cells_p05 PARTITION OF cells FOR VALUES FROM (25000000) TO (30000000);
CREATE TABLE cells_p06 PARTITION OF cells FOR VALUES FROM (30000000) TO (35000000);
CREATE TABLE cells_p07 PARTITION OF cells FOR VALUES FROM (35000000) TO (40000000);
CREATE TABLE cells_p08 PARTITION OF cells FOR VALUES FROM (40000000) TO (45000000);
CREATE TABLE cells_p09 PARTITION OF cells FOR VALUES FROM (45000000) TO (50000000);

-- ---- live_cells (hash-partitioned OutPoint lookup table) ----
-- Only contains cells with status=0, deleted when consumed
-- Hash partitioned by tx_hash for parallel write distribution
-- During bulk sync, writes are deferred to in-memory LiveCellStore and flushed periodically
CREATE TABLE live_cells (
    tx_hash BYTEA NOT NULL,
    output_index SMALLINT NOT NULL,
    created_at_block BIGINT NOT NULL,
    capacity BIGINT NOT NULL,
    lock_script_hash BYTEA NOT NULL,
    lock_code_hash BYTEA NOT NULL,
    lock_args BYTEA NOT NULL,
    type_script_hash BYTEA,
    type_code_hash BYTEA,
    data_size INTEGER NOT NULL,
    
    PRIMARY KEY (tx_hash, output_index)
) PARTITION BY HASH (tx_hash);

-- 16 hash partitions (matches address_transactions for consistency)
CREATE TABLE live_cells_p00 PARTITION OF live_cells FOR VALUES WITH (MODULUS 16, REMAINDER 0);
CREATE TABLE live_cells_p01 PARTITION OF live_cells FOR VALUES WITH (MODULUS 16, REMAINDER 1);
CREATE TABLE live_cells_p02 PARTITION OF live_cells FOR VALUES WITH (MODULUS 16, REMAINDER 2);
CREATE TABLE live_cells_p03 PARTITION OF live_cells FOR VALUES WITH (MODULUS 16, REMAINDER 3);
CREATE TABLE live_cells_p04 PARTITION OF live_cells FOR VALUES WITH (MODULUS 16, REMAINDER 4);
CREATE TABLE live_cells_p05 PARTITION OF live_cells FOR VALUES WITH (MODULUS 16, REMAINDER 5);
CREATE TABLE live_cells_p06 PARTITION OF live_cells FOR VALUES WITH (MODULUS 16, REMAINDER 6);
CREATE TABLE live_cells_p07 PARTITION OF live_cells FOR VALUES WITH (MODULUS 16, REMAINDER 7);
CREATE TABLE live_cells_p08 PARTITION OF live_cells FOR VALUES WITH (MODULUS 16, REMAINDER 8);
CREATE TABLE live_cells_p09 PARTITION OF live_cells FOR VALUES WITH (MODULUS 16, REMAINDER 9);
CREATE TABLE live_cells_p10 PARTITION OF live_cells FOR VALUES WITH (MODULUS 16, REMAINDER 10);
CREATE TABLE live_cells_p11 PARTITION OF live_cells FOR VALUES WITH (MODULUS 16, REMAINDER 11);
CREATE TABLE live_cells_p12 PARTITION OF live_cells FOR VALUES WITH (MODULUS 16, REMAINDER 12);
CREATE TABLE live_cells_p13 PARTITION OF live_cells FOR VALUES WITH (MODULUS 16, REMAINDER 13);
CREATE TABLE live_cells_p14 PARTITION OF live_cells FOR VALUES WITH (MODULUS 16, REMAINDER 14);
CREATE TABLE live_cells_p15 PARTITION OF live_cells FOR VALUES WITH (MODULUS 16, REMAINDER 15);

CREATE INDEX idx_live_cells_lock ON live_cells(lock_script_hash);
CREATE INDEX idx_live_cells_lock_code ON live_cells(lock_code_hash);
CREATE INDEX idx_live_cells_type ON live_cells(type_script_hash) WHERE type_script_hash IS NOT NULL;
CREATE INDEX idx_live_cells_type_code ON live_cells(type_code_hash) WHERE type_code_hash IS NOT NULL;
CREATE INDEX idx_live_cells_block ON live_cells(created_at_block);

-- ---- transaction_inputs ----
CREATE TABLE transaction_inputs (
    id BIGINT GENERATED ALWAYS AS IDENTITY,
    tx_hash BYTEA NOT NULL,
    tx_block_number BIGINT NOT NULL,  -- redundant, for partition alignment
    input_index SMALLINT NOT NULL,
    previous_tx_hash BYTEA NOT NULL,
    previous_output_index SMALLINT NOT NULL,
    since BIGINT NOT NULL DEFAULT 0,

    PRIMARY KEY (tx_block_number, id),
    UNIQUE (tx_block_number, tx_hash, input_index)
) PARTITION BY RANGE (tx_block_number);

-- ---- transaction_cell_deps ----
CREATE TABLE transaction_cell_deps (
    id BIGINT GENERATED ALWAYS AS IDENTITY,
    tx_hash BYTEA NOT NULL,
    tx_block_number BIGINT NOT NULL,  -- redundant, for partition alignment
    dep_index SMALLINT NOT NULL,
    out_point_tx_hash BYTEA NOT NULL,
    out_point_index SMALLINT NOT NULL,
    dep_type SMALLINT NOT NULL,  -- 0=code, 1=dep_group

    PRIMARY KEY (tx_block_number, id),
    UNIQUE (tx_block_number, tx_hash, dep_index)
) PARTITION BY RANGE (tx_block_number);

CREATE TABLE transaction_inputs_p00 PARTITION OF transaction_inputs FOR VALUES FROM (0) TO (5000000);
CREATE TABLE transaction_inputs_p01 PARTITION OF transaction_inputs FOR VALUES FROM (5000000) TO (10000000);
CREATE TABLE transaction_inputs_p02 PARTITION OF transaction_inputs FOR VALUES FROM (10000000) TO (15000000);
CREATE TABLE transaction_inputs_p03 PARTITION OF transaction_inputs FOR VALUES FROM (15000000) TO (20000000);
CREATE TABLE transaction_inputs_p04 PARTITION OF transaction_inputs FOR VALUES FROM (20000000) TO (25000000);
CREATE TABLE transaction_inputs_p05 PARTITION OF transaction_inputs FOR VALUES FROM (25000000) TO (30000000);
CREATE TABLE transaction_inputs_p06 PARTITION OF transaction_inputs FOR VALUES FROM (30000000) TO (35000000);
CREATE TABLE transaction_inputs_p07 PARTITION OF transaction_inputs FOR VALUES FROM (35000000) TO (40000000);
CREATE TABLE transaction_inputs_p08 PARTITION OF transaction_inputs FOR VALUES FROM (40000000) TO (45000000);
CREATE TABLE transaction_inputs_p09 PARTITION OF transaction_inputs FOR VALUES FROM (45000000) TO (50000000);

CREATE TABLE transaction_cell_deps_p00 PARTITION OF transaction_cell_deps FOR VALUES FROM (0) TO (5000000);
CREATE TABLE transaction_cell_deps_p01 PARTITION OF transaction_cell_deps FOR VALUES FROM (5000000) TO (10000000);
CREATE TABLE transaction_cell_deps_p02 PARTITION OF transaction_cell_deps FOR VALUES FROM (10000000) TO (15000000);
CREATE TABLE transaction_cell_deps_p03 PARTITION OF transaction_cell_deps FOR VALUES FROM (15000000) TO (20000000);
CREATE TABLE transaction_cell_deps_p04 PARTITION OF transaction_cell_deps FOR VALUES FROM (20000000) TO (25000000);
CREATE TABLE transaction_cell_deps_p05 PARTITION OF transaction_cell_deps FOR VALUES FROM (25000000) TO (30000000);
CREATE TABLE transaction_cell_deps_p06 PARTITION OF transaction_cell_deps FOR VALUES FROM (30000000) TO (35000000);
CREATE TABLE transaction_cell_deps_p07 PARTITION OF transaction_cell_deps FOR VALUES FROM (35000000) TO (40000000);
CREATE TABLE transaction_cell_deps_p08 PARTITION OF transaction_cell_deps FOR VALUES FROM (40000000) TO (45000000);
CREATE TABLE transaction_cell_deps_p09 PARTITION OF transaction_cell_deps FOR VALUES FROM (45000000) TO (50000000);

-- ---- block_proposals ----
-- Stores proposal short IDs (10 bytes) for each block
CREATE TABLE block_proposals (
    id BIGINT GENERATED ALWAYS AS IDENTITY,
    block_number BIGINT NOT NULL,
    proposal_index SMALLINT NOT NULL,
    proposal_id BYTEA NOT NULL,  -- 10-byte short transaction ID

    PRIMARY KEY (block_number, id),
    UNIQUE (block_number, proposal_index)
) PARTITION BY RANGE (block_number);

CREATE TABLE block_proposals_p00 PARTITION OF block_proposals FOR VALUES FROM (0) TO (5000000);
CREATE TABLE block_proposals_p01 PARTITION OF block_proposals FOR VALUES FROM (5000000) TO (10000000);
CREATE TABLE block_proposals_p02 PARTITION OF block_proposals FOR VALUES FROM (10000000) TO (15000000);
CREATE TABLE block_proposals_p03 PARTITION OF block_proposals FOR VALUES FROM (15000000) TO (20000000);
CREATE TABLE block_proposals_p04 PARTITION OF block_proposals FOR VALUES FROM (20000000) TO (25000000);
CREATE TABLE block_proposals_p05 PARTITION OF block_proposals FOR VALUES FROM (25000000) TO (30000000);
CREATE TABLE block_proposals_p06 PARTITION OF block_proposals FOR VALUES FROM (30000000) TO (35000000);
CREATE TABLE block_proposals_p07 PARTITION OF block_proposals FOR VALUES FROM (35000000) TO (40000000);
CREATE TABLE block_proposals_p08 PARTITION OF block_proposals FOR VALUES FROM (40000000) TO (45000000);
CREATE TABLE block_proposals_p09 PARTITION OF block_proposals FOR VALUES FROM (45000000) TO (50000000);

-- ---- uncle_blocks ----
CREATE TABLE uncle_blocks (
    id BIGINT GENERATED ALWAYS AS IDENTITY,
    block_number BIGINT NOT NULL,
    uncle_index INTEGER NOT NULL,
    hash BYTEA NOT NULL,
    proposals_hash BYTEA NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL,
    compact_target BIGINT NOT NULL,
    epoch_number BIGINT NOT NULL,
    epoch_index INTEGER NOT NULL,
    epoch_length INTEGER NOT NULL,
    parent_hash BYTEA NOT NULL,
    transactions_root BYTEA NOT NULL,
    extra_hash BYTEA NOT NULL,
    dao BYTEA NOT NULL,
    nonce BYTEA NOT NULL,

    PRIMARY KEY (block_number, id),
    UNIQUE (block_number, uncle_index)
) PARTITION BY RANGE (block_number);

CREATE TABLE uncle_blocks_p00 PARTITION OF uncle_blocks FOR VALUES FROM (0) TO (5000000);
CREATE TABLE uncle_blocks_p01 PARTITION OF uncle_blocks FOR VALUES FROM (5000000) TO (10000000);
CREATE TABLE uncle_blocks_p02 PARTITION OF uncle_blocks FOR VALUES FROM (10000000) TO (15000000);
CREATE TABLE uncle_blocks_p03 PARTITION OF uncle_blocks FOR VALUES FROM (15000000) TO (20000000);
CREATE TABLE uncle_blocks_p04 PARTITION OF uncle_blocks FOR VALUES FROM (20000000) TO (25000000);
CREATE TABLE uncle_blocks_p05 PARTITION OF uncle_blocks FOR VALUES FROM (25000000) TO (30000000);
CREATE TABLE uncle_blocks_p06 PARTITION OF uncle_blocks FOR VALUES FROM (30000000) TO (35000000);
CREATE TABLE uncle_blocks_p07 PARTITION OF uncle_blocks FOR VALUES FROM (35000000) TO (40000000);
CREATE TABLE uncle_blocks_p08 PARTITION OF uncle_blocks FOR VALUES FROM (40000000) TO (45000000);
CREATE TABLE uncle_blocks_p09 PARTITION OF uncle_blocks FOR VALUES FROM (45000000) TO (50000000);

-- ---- cell_data ----
CREATE TABLE cell_data (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    tx_hash BYTEA NOT NULL,
    output_index SMALLINT NOT NULL,
    data BYTEA NOT NULL,

    UNIQUE(tx_hash, output_index)
);

-- ===========================================
-- 3. Pre-computed Aggregation Tables
-- ===========================================

-- ---- address_balances ----
CREATE TABLE address_balances (
    lock_script_hash BYTEA PRIMARY KEY,

    -- Balance (incremental maintenance, in shannons)
    balance NUMERIC(40,0) NOT NULL DEFAULT 0,

    -- Cell counts
    live_cells_count INTEGER NOT NULL DEFAULT 0,
    total_cells_count BIGINT NOT NULL DEFAULT 0,

    -- Transaction count (incremental maintenance)
    transactions_count BIGINT NOT NULL DEFAULT 0,

    -- Timeline
    first_seen_block BIGINT,
    first_seen_tx BYTEA,
    last_activity_block BIGINT,
    last_activity_tx BYTEA,

    updated_at TIMESTAMPTZ DEFAULT NOW()
);

-- Rich list sorting
CREATE INDEX idx_address_balances_balance ON address_balances(balance DESC)
    WHERE balance > 0;
-- Active addresses
CREATE INDEX idx_address_balances_activity ON address_balances(last_activity_block DESC);

-- ---- address_transactions ----
-- For address page transaction history, avoiding complex UNION queries
CREATE TABLE address_transactions (
    lock_script_hash BYTEA NOT NULL,
    tx_hash BYTEA NOT NULL,
    block_number BIGINT NOT NULL,
    tx_type SMALLINT NOT NULL,  -- 1=received, 2=sent, 3=both
    capacity_change NUMERIC(20,0) NOT NULL,  -- positive=income, negative=expense
    timestamp TIMESTAMPTZ NOT NULL,

    PRIMARY KEY (lock_script_hash, block_number, tx_hash)
) PARTITION BY HASH (lock_script_hash);

-- 16 hash partitions (even distribution)
CREATE TABLE address_transactions_p00 PARTITION OF address_transactions FOR VALUES WITH (MODULUS 16, REMAINDER 0);
CREATE TABLE address_transactions_p01 PARTITION OF address_transactions FOR VALUES WITH (MODULUS 16, REMAINDER 1);
CREATE TABLE address_transactions_p02 PARTITION OF address_transactions FOR VALUES WITH (MODULUS 16, REMAINDER 2);
CREATE TABLE address_transactions_p03 PARTITION OF address_transactions FOR VALUES WITH (MODULUS 16, REMAINDER 3);
CREATE TABLE address_transactions_p04 PARTITION OF address_transactions FOR VALUES WITH (MODULUS 16, REMAINDER 4);
CREATE TABLE address_transactions_p05 PARTITION OF address_transactions FOR VALUES WITH (MODULUS 16, REMAINDER 5);
CREATE TABLE address_transactions_p06 PARTITION OF address_transactions FOR VALUES WITH (MODULUS 16, REMAINDER 6);
CREATE TABLE address_transactions_p07 PARTITION OF address_transactions FOR VALUES WITH (MODULUS 16, REMAINDER 7);
CREATE TABLE address_transactions_p08 PARTITION OF address_transactions FOR VALUES WITH (MODULUS 16, REMAINDER 8);
CREATE TABLE address_transactions_p09 PARTITION OF address_transactions FOR VALUES WITH (MODULUS 16, REMAINDER 9);
CREATE TABLE address_transactions_p10 PARTITION OF address_transactions FOR VALUES WITH (MODULUS 16, REMAINDER 10);
CREATE TABLE address_transactions_p11 PARTITION OF address_transactions FOR VALUES WITH (MODULUS 16, REMAINDER 11);
CREATE TABLE address_transactions_p12 PARTITION OF address_transactions FOR VALUES WITH (MODULUS 16, REMAINDER 12);
CREATE TABLE address_transactions_p13 PARTITION OF address_transactions FOR VALUES WITH (MODULUS 16, REMAINDER 13);
CREATE TABLE address_transactions_p14 PARTITION OF address_transactions FOR VALUES WITH (MODULUS 16, REMAINDER 14);
CREATE TABLE address_transactions_p15 PARTITION OF address_transactions FOR VALUES WITH (MODULUS 16, REMAINDER 15);

-- ===========================================
-- 4. Statistics Tables
-- ===========================================

-- ---- daily_statistics ----
CREATE TABLE daily_statistics (
    date DATE PRIMARY KEY,

    blocks_count INTEGER NOT NULL DEFAULT 0,
    transactions_count INTEGER NOT NULL DEFAULT 0,
    cells_created INTEGER NOT NULL DEFAULT 0,
    cells_consumed INTEGER NOT NULL DEFAULT 0,
    capacity_transferred NUMERIC(30,0) NOT NULL DEFAULT 0,

    total_blocks BIGINT NOT NULL DEFAULT 0,
    total_transactions BIGINT NOT NULL DEFAULT 0,
    total_live_cells BIGINT NOT NULL DEFAULT 0,
    total_data_size BIGINT NOT NULL DEFAULT 0,
    cumulative_cells BIGINT NOT NULL DEFAULT 0,
    cumulative_data_size BIGINT NOT NULL DEFAULT 0,

    avg_block_time_ms INTEGER,
    avg_tx_per_block NUMERIC(10,2),
    new_addresses INTEGER NOT NULL DEFAULT 0,
    active_addresses INTEGER NOT NULL DEFAULT 0,

    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

-- ---- hourly_statistics ----
CREATE TABLE hourly_statistics (
    hour TIMESTAMPTZ PRIMARY KEY,
    blocks_count INTEGER NOT NULL DEFAULT 0,
    transactions_count INTEGER NOT NULL DEFAULT 0,
    cells_created INTEGER NOT NULL DEFAULT 0,
    cells_consumed INTEGER NOT NULL DEFAULT 0,
    capacity_transferred NUMERIC(30,0) NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX idx_hourly_stats_hour ON hourly_statistics(hour DESC);

-- ---- epoch_statistics ----
CREATE TABLE epoch_statistics (
    epoch_number BIGINT PRIMARY KEY,
    start_block BIGINT NOT NULL,
    end_block BIGINT,
    blocks_count INTEGER NOT NULL DEFAULT 0,
    length INTEGER NOT NULL,
    start_timestamp TIMESTAMPTZ NOT NULL,
    end_timestamp TIMESTAMPTZ,
    difficulty NUMERIC(40,0) NOT NULL DEFAULT 0,
    hash_rate NUMERIC(40,0),
    transactions_count INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX idx_epoch_stats_start ON epoch_statistics(start_block);

-- ---- daily_block_stats ----
CREATE TABLE daily_block_stats (
    date DATE PRIMARY KEY,
    min_block_time_ms INTEGER,
    max_block_time_ms INTEGER,
    median_block_time_ms INTEGER,
    p95_block_time_ms INTEGER,
    avg_block_time_ms INTEGER,
    avg_compact_target BIGINT,
    avg_uncle_rate FLOAT8 NOT NULL DEFAULT 0,
    block_count INTEGER NOT NULL DEFAULT 0,
    total_uncles INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

-- ---- block_time_distribution ----
CREATE TABLE block_time_distribution (
    bucket_seconds INTEGER PRIMARY KEY,
    block_count BIGINT NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

-- ---- epoch_time_distribution ----
CREATE TABLE epoch_time_distribution (
    bucket_minutes INTEGER PRIMARY KEY,
    epoch_count BIGINT NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

-- ---- miner_statistics ----
CREATE TABLE miner_statistics (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    date DATE NOT NULL,
    miner_lock_hash BYTEA NOT NULL,
    blocks_count INTEGER NOT NULL DEFAULT 0,
    total_reward NUMERIC(20,0) NOT NULL DEFAULT 0,
    last_block_number BIGINT,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    
    UNIQUE(date, miner_lock_hash)
);

CREATE INDEX idx_miner_stats_date ON miner_statistics(date DESC);
CREATE INDEX idx_miner_stats_miner ON miner_statistics(miner_lock_hash);
CREATE INDEX idx_miner_stats_lock_hash_only ON miner_statistics(miner_lock_hash);

-- ===========================================
-- 5. DAO Tables
-- ===========================================

-- ---- dao_deposits ----
CREATE TABLE dao_deposits (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    tx_hash BYTEA NOT NULL,
    output_index SMALLINT NOT NULL,
    lock_script_hash BYTEA NOT NULL,
    capacity NUMERIC(20,0) NOT NULL,

    deposit_block_number BIGINT NOT NULL,
    deposit_tx_hash BYTEA NOT NULL,
    deposit_timestamp TIMESTAMPTZ NOT NULL,
    deposit_ar NUMERIC(20,0) NOT NULL,  -- AR at deposit time

    status SMALLINT NOT NULL DEFAULT 0,  -- 0=active, 1=requesting, 2=withdrawn

    withdraw_request_block BIGINT,
    withdraw_request_tx BYTEA,
    withdraw_request_timestamp TIMESTAMPTZ,
    withdraw_request_ar NUMERIC(20,0),  -- AR at request time

    withdraw_block BIGINT,
    withdraw_tx BYTEA,
    withdraw_timestamp TIMESTAMPTZ,

    compensation NUMERIC(20,0),  -- computed and stored

    UNIQUE(tx_hash, output_index)
);

CREATE INDEX idx_dao_deposits_lock ON dao_deposits(lock_script_hash);
CREATE INDEX idx_dao_deposits_status ON dao_deposits(status) WHERE status < 2;
CREATE INDEX idx_dao_deposits_block ON dao_deposits(deposit_block_number DESC);
CREATE INDEX idx_dao_deposits_withdraw_request_tx ON dao_deposits(withdraw_request_tx) WHERE withdraw_request_tx IS NOT NULL;

-- ---- dao_statistics ----
CREATE TABLE dao_statistics (
    id INTEGER PRIMARY KEY DEFAULT 1,
    total_deposited NUMERIC(20,0) NOT NULL DEFAULT 0,
    total_depositors INTEGER NOT NULL DEFAULT 0,
    active_deposits INTEGER NOT NULL DEFAULT 0,
    total_compensation_paid NUMERIC(20,0) NOT NULL DEFAULT 0,
    unclaimed_compensation NUMERIC(20,0) NOT NULL DEFAULT 0,
    average_deposit_epochs INTEGER NOT NULL DEFAULT 0,
    estimated_apc TEXT,
    mining_reward TEXT NOT NULL DEFAULT '0',
    deposit_compensation TEXT NOT NULL DEFAULT '0',
    burnt TEXT NOT NULL DEFAULT '0',
    cumulative_burnt TEXT,
    cumulative_secondary_issuance TEXT,
    cumulative_miner_secondary TEXT,
    cumulative_dao_compensation TEXT,
    secondary_issuance NUMERIC(20,0) NOT NULL DEFAULT 0,
    last_processed_block BIGINT NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    
    CONSTRAINT dao_single_row CHECK (id = 1)
);

INSERT INTO dao_statistics (id) VALUES (1);

-- ---- block_secondary_issuance ----
CREATE TABLE block_secondary_issuance (
    block_number BIGINT PRIMARY KEY,
    block_timestamp TIMESTAMPTZ NOT NULL,
    secondary_issuance NUMERIC(20,0) NOT NULL DEFAULT 0,
    miner_secondary NUMERIC(20,0) NOT NULL DEFAULT 0,
    dao_compensation NUMERIC(20,0) NOT NULL DEFAULT 0,
    burnt NUMERIC(20,0) NOT NULL DEFAULT 0
);

CREATE INDEX idx_block_secondary_issuance_timestamp ON block_secondary_issuance(block_timestamp);

-- ---- dao_daily_snapshots ----
CREATE TABLE dao_daily_snapshots (
    date DATE PRIMARY KEY,
    
    total_deposit NUMERIC(20,0) NOT NULL DEFAULT 0,
    depositors_count INTEGER NOT NULL DEFAULT 0,
    daily_deposit NUMERIC(20,0) NOT NULL DEFAULT 0,
    daily_deposit_count INTEGER NOT NULL DEFAULT 0,
    -- total_issuance from dao field C (includes genesis burnt 33.6B at genesis)
    -- NOT circulating supply - actual circulating = total_issuance - genesis_burnt - secondary_burnt
    total_issuance NUMERIC(20,0) NOT NULL DEFAULT 0,
    dao_data BYTEA,
    secondary_issuance NUMERIC(20,0) NOT NULL DEFAULT 0,
    burnt NUMERIC(20,0) NOT NULL DEFAULT 0,
    cumulative_burnt TEXT,
    cumulative_mining_reward TEXT,
    cumulative_deposit_compensation TEXT,
    
    created_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX idx_dao_daily_snapshots_date ON dao_daily_snapshots(date DESC);

-- ===========================================
-- 6. Token Tables
-- ===========================================

-- ---- tokens ----
CREATE TABLE tokens (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    type_script_hash BYTEA NOT NULL UNIQUE,
    type_code_hash BYTEA NOT NULL,
    type_hash_type SMALLINT NOT NULL,
    type_args BYTEA NOT NULL,
    
    standard TEXT NOT NULL,  -- 'sudt' or 'xudt'
    name TEXT,
    symbol TEXT,
    decimals SMALLINT NOT NULL DEFAULT 8,
    description TEXT,
    icon_url TEXT,
    
    -- Label info from token-labels (docs/token-labels)
    published BOOLEAN NOT NULL DEFAULT FALSE,
    famous BOOLEAN NOT NULL DEFAULT FALSE,
    tags TEXT[],  -- array of tags: 'rgb++', 'layer-1-asset', 'layer-2-asset', 'supply-limited'
    udt_type TEXT,  -- 'sudt', 'xudt', 'xudt_compatible', 'omiga_inscription', 'ssri'
    manager TEXT,  -- manager address
    email TEXT,
    operator_website TEXT,
    label_updated_at TIMESTAMPTZ,  -- when label info was last synced
    
    total_supply NUMERIC(40,0) NOT NULL DEFAULT 0,
    holders_count INTEGER NOT NULL DEFAULT 0,
    transfers_count BIGINT NOT NULL DEFAULT 0,
    transfers_24h BIGINT NOT NULL DEFAULT 0,
    
    first_seen_block BIGINT NOT NULL,
    first_seen_tx BYTEA NOT NULL,
    
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX idx_tokens_code_hash ON tokens(type_code_hash);
CREATE INDEX idx_tokens_standard ON tokens(standard);
CREATE INDEX idx_tokens_holders ON tokens(holders_count DESC);
CREATE INDEX idx_tokens_transfers ON tokens(transfers_count DESC);
CREATE INDEX idx_tokens_transfers_24h ON tokens(transfers_24h DESC, holders_count DESC);
CREATE INDEX idx_tokens_published ON tokens(published) WHERE published = TRUE;
CREATE INDEX idx_tokens_famous ON tokens(famous) WHERE famous = TRUE;

-- ---- token_balances ----
CREATE TABLE token_balances (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    token_id BIGINT NOT NULL REFERENCES tokens(id) ON DELETE CASCADE,
    lock_script_hash BYTEA NOT NULL,
    
    balance NUMERIC(40,0) NOT NULL DEFAULT 0,
    
    first_tx BYTEA NOT NULL,
    last_tx BYTEA NOT NULL,
    
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    
    UNIQUE(token_id, lock_script_hash)
);

CREATE INDEX idx_token_balances_token ON token_balances(token_id);
CREATE INDEX idx_token_balances_lock ON token_balances(lock_script_hash);
CREATE INDEX idx_token_balances_balance ON token_balances(token_id, balance DESC);
CREATE INDEX idx_token_balances_pagination ON token_balances(token_id, balance DESC, lock_script_hash DESC) WHERE balance > 0;

-- ---- token_transfers ----
CREATE TABLE token_transfers (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    token_id BIGINT NOT NULL REFERENCES tokens(id) ON DELETE CASCADE,
    tx_hash BYTEA NOT NULL,
    block_number BIGINT NOT NULL,
    
    from_lock_hash BYTEA,
    to_lock_hash BYTEA NOT NULL,
    amount NUMERIC(40,0) NOT NULL,
    
    is_mint BOOLEAN NOT NULL DEFAULT FALSE,
    is_burn BOOLEAN NOT NULL DEFAULT FALSE,
    
    timestamp TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX idx_token_transfers_token ON token_transfers(token_id);
CREATE INDEX idx_token_transfers_tx ON token_transfers(tx_hash);
CREATE INDEX idx_token_transfers_from ON token_transfers(from_lock_hash) WHERE from_lock_hash IS NOT NULL;
CREATE INDEX idx_token_transfers_to ON token_transfers(to_lock_hash);
CREATE INDEX idx_token_transfers_block ON token_transfers(block_number DESC);
CREATE INDEX idx_token_transfers_timestamp ON token_transfers(timestamp DESC);
CREATE INDEX idx_token_transfers_pagination ON token_transfers(token_id, block_number DESC, id DESC);

-- ---- udt_cells ----
CREATE TABLE udt_cells (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    tx_hash BYTEA NOT NULL,
    output_index SMALLINT NOT NULL,
    
    type_script_hash BYTEA NOT NULL,
    type_code_hash BYTEA NOT NULL,
    type_hash_type SMALLINT NOT NULL,
    type_args BYTEA NOT NULL,
    
    lock_script_hash BYTEA NOT NULL,
    amount NUMERIC(40,0) NOT NULL,
    standard TEXT NOT NULL,
    
    is_live BOOLEAN NOT NULL DEFAULT TRUE,
    
    created_at_block BIGINT NOT NULL,
    consumed_at_block BIGINT,
    consumed_by_tx BYTEA,
    
    UNIQUE(tx_hash, output_index)
);

CREATE INDEX idx_udt_cells_type_script ON udt_cells(type_script_hash);
CREATE INDEX idx_udt_cells_lock ON udt_cells(lock_script_hash);
CREATE INDEX idx_udt_cells_live ON udt_cells(is_live) WHERE is_live = TRUE;
CREATE INDEX idx_udt_cells_block ON udt_cells(created_at_block DESC);

-- ===========================================
-- 7. Spore Tables
-- ===========================================

-- ---- spore_clusters ----
CREATE TABLE spore_clusters (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    cluster_id BYTEA NOT NULL UNIQUE,
    
    type_script_hash BYTEA NOT NULL,
    
    name TEXT,
    description TEXT,
    
    owner_lock_hash BYTEA NOT NULL,
    
    spores_count INTEGER NOT NULL DEFAULT 0,
    
    created_at_block BIGINT NOT NULL,
    created_at_tx BYTEA NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX idx_spore_clusters_owner ON spore_clusters(owner_lock_hash);
CREATE INDEX idx_spore_clusters_name ON spore_clusters(name) WHERE name IS NOT NULL;

-- ---- spore_cells ----
CREATE TABLE spore_cells (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    spore_id BYTEA NOT NULL UNIQUE,
    
    type_script_hash BYTEA NOT NULL,
    tx_hash BYTEA NOT NULL,
    output_index SMALLINT NOT NULL,
    
    cluster_id BYTEA,
    
    content_type TEXT NOT NULL,
    content_size INTEGER NOT NULL,
    
    owner_lock_hash BYTEA NOT NULL,
    
    is_live BOOLEAN NOT NULL DEFAULT TRUE,
    
    created_at_block BIGINT NOT NULL,
    created_at_tx BYTEA NOT NULL,
    
    consumed_at_block BIGINT,
    consumed_by_tx BYTEA,
    
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    
    UNIQUE(tx_hash, output_index)
);

CREATE INDEX idx_spore_cells_cluster ON spore_cells(cluster_id) WHERE cluster_id IS NOT NULL;
CREATE INDEX idx_spore_cells_owner ON spore_cells(owner_lock_hash);
CREATE INDEX idx_spore_cells_content_type ON spore_cells(content_type);
CREATE INDEX idx_spore_cells_live ON spore_cells(is_live) WHERE is_live = TRUE;
CREATE INDEX idx_spore_cells_created ON spore_cells(created_at_block DESC);

-- ---- spore_content ----
CREATE TABLE spore_content (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    spore_id BYTEA NOT NULL UNIQUE REFERENCES spore_cells(spore_id) ON DELETE CASCADE,
    content BYTEA NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- ===========================================
-- 7b. M-NFT Tables (DEPRECATED - legacy NFT standard)
-- ===========================================

-- ---- mnft_issuers ----
-- M-NFT Issuer cells define NFT issuers
CREATE TABLE mnft_issuers (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    issuer_id BYTEA NOT NULL UNIQUE,  -- type_script.args (first 20 bytes of lock hash)
    type_script_hash BYTEA NOT NULL,
    
    name TEXT,
    info BYTEA,  -- Raw info data from cell
    
    owner_lock_hash BYTEA NOT NULL,
    
    classes_count INTEGER NOT NULL DEFAULT 0,
    
    is_live BOOLEAN NOT NULL DEFAULT TRUE,
    
    created_at_block BIGINT NOT NULL,
    created_at_tx BYTEA NOT NULL,
    
    consumed_at_block BIGINT,
    consumed_by_tx BYTEA,
    
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX idx_mnft_issuers_owner ON mnft_issuers(owner_lock_hash);
CREATE INDEX idx_mnft_issuers_live ON mnft_issuers(is_live) WHERE is_live = TRUE;

-- ---- mnft_classes ----
-- M-NFT Class cells define NFT collections
CREATE TABLE mnft_classes (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    class_id BYTEA NOT NULL UNIQUE,  -- type_script.args (issuer_id + class_index)
    type_script_hash BYTEA NOT NULL,
    
    issuer_id BYTEA NOT NULL,
    
    name TEXT,
    description TEXT,
    renderer TEXT,  -- URL to renderer
    
    total INTEGER NOT NULL DEFAULT 0,  -- Max supply (0 = unlimited)
    issued INTEGER NOT NULL DEFAULT 0,  -- Currently minted count
    
    owner_lock_hash BYTEA NOT NULL,
    
    is_live BOOLEAN NOT NULL DEFAULT TRUE,
    
    created_at_block BIGINT NOT NULL,
    created_at_tx BYTEA NOT NULL,
    
    consumed_at_block BIGINT,
    consumed_by_tx BYTEA,
    
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX idx_mnft_classes_issuer ON mnft_classes(issuer_id);
CREATE INDEX idx_mnft_classes_owner ON mnft_classes(owner_lock_hash);
CREATE INDEX idx_mnft_classes_live ON mnft_classes(is_live) WHERE is_live = TRUE;
CREATE INDEX idx_mnft_classes_name ON mnft_classes(name) WHERE name IS NOT NULL;

-- ---- mnft_tokens ----
-- M-NFT Token cells are individual NFTs
CREATE TABLE mnft_tokens (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    token_id BYTEA NOT NULL UNIQUE,  -- type_script.args (class_id + token_index)
    type_script_hash BYTEA NOT NULL,
    tx_hash BYTEA NOT NULL,
    output_index SMALLINT NOT NULL,
    
    class_id BYTEA NOT NULL,
    token_index INTEGER NOT NULL,  -- Index within the class
    
    characteristic BYTEA,  -- Token-specific data
    configure SMALLINT NOT NULL DEFAULT 0,  -- Configuration flags
    state SMALLINT NOT NULL DEFAULT 0,  -- Token state
    
    owner_lock_hash BYTEA NOT NULL,
    
    is_live BOOLEAN NOT NULL DEFAULT TRUE,
    
    created_at_block BIGINT NOT NULL,
    created_at_tx BYTEA NOT NULL,
    
    consumed_at_block BIGINT,
    consumed_by_tx BYTEA,
    
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    
    UNIQUE(tx_hash, output_index)
);

CREATE INDEX idx_mnft_tokens_class ON mnft_tokens(class_id);
CREATE INDEX idx_mnft_tokens_owner ON mnft_tokens(owner_lock_hash);
CREATE INDEX idx_mnft_tokens_live ON mnft_tokens(is_live) WHERE is_live = TRUE;

-- ===========================================
-- 7c. .bit Tables (DAS - Decentralized Account System)
-- ===========================================

-- ---- dotbit_accounts ----
-- .bit account cells store decentralized identity accounts
CREATE TABLE dotbit_accounts (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    account_id BYTEA NOT NULL UNIQUE,  -- Account ID from cell data
    type_script_hash BYTEA NOT NULL,
    tx_hash BYTEA NOT NULL,
    output_index SMALLINT NOT NULL,
    
    account_name TEXT NOT NULL,  -- e.g. "alice.bit"
    
    owner_lock_hash BYTEA NOT NULL,
    manager_lock_hash BYTEA,
    
    registered_at BIGINT,  -- Registration timestamp
    expired_at BIGINT,  -- Expiration timestamp
    
    status SMALLINT NOT NULL DEFAULT 0,  -- Account status
    
    is_live BOOLEAN NOT NULL DEFAULT TRUE,
    
    created_at_block BIGINT NOT NULL,
    created_at_tx BYTEA NOT NULL,
    
    consumed_at_block BIGINT,
    consumed_by_tx BYTEA,
    
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    
    UNIQUE(tx_hash, output_index)
);

CREATE INDEX idx_dotbit_accounts_name ON dotbit_accounts(account_name);
CREATE INDEX idx_dotbit_accounts_owner ON dotbit_accounts(owner_lock_hash);
CREATE INDEX idx_dotbit_accounts_live ON dotbit_accounts(is_live) WHERE is_live = TRUE;
CREATE INDEX idx_dotbit_accounts_expired ON dotbit_accounts(expired_at) WHERE expired_at IS NOT NULL;

-- ===========================================
-- 7d. DOB Transfer History (Spore, DOB/0, DOB/1, did:ckb)
-- ===========================================

-- ---- dob_transfers ----
-- Transfer history for Digital Objects (Spore ecosystem)
-- Asset-centric table for DOB/Spore detail pages
CREATE TABLE dob_transfers (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    
    -- DOB identification
    dob_id BYTEA NOT NULL,             -- spore_id (type_script.args)
    cluster_id BYTEA,                   -- parent cluster (nullable)
    dob_type TEXT NOT NULL,             -- 'spore', 'dob/0', 'dob/1', 'did:ckb'
    
    -- Transaction info
    tx_hash BYTEA NOT NULL,
    block_number BIGINT NOT NULL,
    
    -- Transfer parties
    from_lock_hash BYTEA,               -- NULL = mint
    to_lock_hash BYTEA NOT NULL,
    
    -- Event type
    event_type TEXT NOT NULL,           -- 'mint', 'transfer', 'burn'
    
    -- Metadata for display
    content_type TEXT,                  -- 'image/png', 'dob/0', etc.
    
    timestamp TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX idx_dob_transfers_dob ON dob_transfers(dob_id, block_number DESC);
CREATE INDEX idx_dob_transfers_cluster ON dob_transfers(cluster_id, block_number DESC) 
    WHERE cluster_id IS NOT NULL;
CREATE INDEX idx_dob_transfers_block ON dob_transfers(block_number);
CREATE INDEX idx_dob_transfers_type ON dob_transfers(dob_type, block_number DESC);
CREATE INDEX idx_dob_transfers_to ON dob_transfers(to_lock_hash, block_number DESC);
CREATE INDEX idx_dob_transfers_from ON dob_transfers(from_lock_hash, block_number DESC) 
    WHERE from_lock_hash IS NOT NULL;

-- ===========================================
-- 7e. NFT Transfer History (M-NFT, .bit)
-- ===========================================

-- ---- nft_transfers ----
-- Transfer history for legacy NFTs and naming services
-- Asset-centric table for M-NFT/.bit detail pages
CREATE TABLE nft_transfers (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    
    -- NFT identification
    nft_id BYTEA NOT NULL,              -- token_id (M-NFT) or account_id (.bit)
    nft_type TEXT NOT NULL,             -- 'mnft', 'dotbit'
    
    -- M-NFT hierarchy (M-NFT only)
    issuer_id BYTEA,                    -- M-NFT issuer
    class_id BYTEA,                     -- M-NFT class
    
    -- Transaction info
    tx_hash BYTEA NOT NULL,
    block_number BIGINT NOT NULL,
    
    -- Transfer parties
    from_lock_hash BYTEA,               -- NULL = mint/register
    to_lock_hash BYTEA NOT NULL,
    
    -- Event type
    event_type TEXT NOT NULL,           -- 'mint', 'transfer', 'burn', 'register', 'renew'
    
    -- Metadata for display
    name TEXT,                          -- .bit: "alice.bit", M-NFT: token name
    
    timestamp TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX idx_nft_transfers_nft ON nft_transfers(nft_id, block_number DESC);
CREATE INDEX idx_nft_transfers_class ON nft_transfers(class_id, block_number DESC) 
    WHERE class_id IS NOT NULL;
CREATE INDEX idx_nft_transfers_issuer ON nft_transfers(issuer_id, block_number DESC)
    WHERE issuer_id IS NOT NULL;
CREATE INDEX idx_nft_transfers_block ON nft_transfers(block_number);
CREATE INDEX idx_nft_transfers_type ON nft_transfers(nft_type, block_number DESC);
CREATE INDEX idx_nft_transfers_name ON nft_transfers(name) WHERE name IS NOT NULL;
CREATE INDEX idx_nft_transfers_to ON nft_transfers(to_lock_hash, block_number DESC);
CREATE INDEX idx_nft_transfers_from ON nft_transfers(from_lock_hash, block_number DESC)
    WHERE from_lock_hash IS NOT NULL;

-- ===========================================
-- 7f. Address Asset Transfers (Projection Table)
-- ===========================================

-- ---- address_asset_transfers ----
-- Address-centric projection of all non-CKB asset movements
-- Optimized for Address page queries (HASH partitioned by lock_script_hash)
CREATE TABLE address_asset_transfers (
    id BIGINT GENERATED ALWAYS AS IDENTITY,
    
    -- Address association (partition key)
    lock_script_hash BYTEA NOT NULL,
    
    -- Transaction info
    tx_hash BYTEA NOT NULL,
    block_number BIGINT NOT NULL,
    tx_index INT NOT NULL,
    event_index SMALLINT NOT NULL DEFAULT 0,  -- ordering within tx
    
    -- Asset classification
    asset_category TEXT NOT NULL,       -- 'token', 'dob', 'nft', 'dao'
    asset_type TEXT NOT NULL,           -- 'sudt', 'xudt', 'spore', 'dob/0', 'mnft', 'dotbit', 'dao'
    asset_id BYTEA,                     -- type_script_hash / spore_id / nft_id / deposit_id
    
    -- Transfer semantics
    direction SMALLINT NOT NULL,        -- 1=in, 2=out
    peer_lock_hash BYTEA,               -- counterparty (nullable for mint/burn)
    amount NUMERIC(40,0),               -- quantity (NFT = 1, DAO = shannons)
    
    -- Special events (DAO etc.)
    event_type TEXT,                    -- 'deposit', 'withdraw_request', 'withdraw_complete'
    
    timestamp TIMESTAMPTZ NOT NULL,
    
    PRIMARY KEY (lock_script_hash, block_number, tx_index, event_index, id)
) PARTITION BY HASH (lock_script_hash);

-- 16 hash partitions (matches address_transactions)
CREATE TABLE address_asset_transfers_p00 PARTITION OF address_asset_transfers FOR VALUES WITH (MODULUS 16, REMAINDER 0);
CREATE TABLE address_asset_transfers_p01 PARTITION OF address_asset_transfers FOR VALUES WITH (MODULUS 16, REMAINDER 1);
CREATE TABLE address_asset_transfers_p02 PARTITION OF address_asset_transfers FOR VALUES WITH (MODULUS 16, REMAINDER 2);
CREATE TABLE address_asset_transfers_p03 PARTITION OF address_asset_transfers FOR VALUES WITH (MODULUS 16, REMAINDER 3);
CREATE TABLE address_asset_transfers_p04 PARTITION OF address_asset_transfers FOR VALUES WITH (MODULUS 16, REMAINDER 4);
CREATE TABLE address_asset_transfers_p05 PARTITION OF address_asset_transfers FOR VALUES WITH (MODULUS 16, REMAINDER 5);
CREATE TABLE address_asset_transfers_p06 PARTITION OF address_asset_transfers FOR VALUES WITH (MODULUS 16, REMAINDER 6);
CREATE TABLE address_asset_transfers_p07 PARTITION OF address_asset_transfers FOR VALUES WITH (MODULUS 16, REMAINDER 7);
CREATE TABLE address_asset_transfers_p08 PARTITION OF address_asset_transfers FOR VALUES WITH (MODULUS 16, REMAINDER 8);
CREATE TABLE address_asset_transfers_p09 PARTITION OF address_asset_transfers FOR VALUES WITH (MODULUS 16, REMAINDER 9);
CREATE TABLE address_asset_transfers_p10 PARTITION OF address_asset_transfers FOR VALUES WITH (MODULUS 16, REMAINDER 10);
CREATE TABLE address_asset_transfers_p11 PARTITION OF address_asset_transfers FOR VALUES WITH (MODULUS 16, REMAINDER 11);
CREATE TABLE address_asset_transfers_p12 PARTITION OF address_asset_transfers FOR VALUES WITH (MODULUS 16, REMAINDER 12);
CREATE TABLE address_asset_transfers_p13 PARTITION OF address_asset_transfers FOR VALUES WITH (MODULUS 16, REMAINDER 13);
CREATE TABLE address_asset_transfers_p14 PARTITION OF address_asset_transfers FOR VALUES WITH (MODULUS 16, REMAINDER 14);
CREATE TABLE address_asset_transfers_p15 PARTITION OF address_asset_transfers FOR VALUES WITH (MODULUS 16, REMAINDER 15);

-- Indexes for address_asset_transfers
-- Primary use case: fetch assets for specific txs on address page
CREATE INDEX idx_aat_addr_tx ON address_asset_transfers(lock_script_hash, tx_hash);
-- Timeline pagination (address assets tab)
CREATE INDEX idx_aat_timeline ON address_asset_transfers(lock_script_hash, block_number DESC, tx_index DESC, event_index);
-- Reorg rollback
CREATE INDEX idx_aat_block ON address_asset_transfers(block_number);
-- Filter by category
CREATE INDEX idx_aat_category ON address_asset_transfers(lock_script_hash, asset_category, block_number DESC);

-- ===========================================
-- 8. Scripts Tables
-- ===========================================

-- ---- scripts ----
CREATE TABLE scripts (
    hash BYTEA PRIMARY KEY,
    code_hash BYTEA NOT NULL,
    hash_type SMALLINT NOT NULL,
    args BYTEA NOT NULL,
    first_seen_block BIGINT NOT NULL,
    first_seen_tx BYTEA NOT NULL,
    cells_count BIGINT NOT NULL DEFAULT 0,
    live_cells_count BIGINT NOT NULL DEFAULT 0,
    capacity_sum NUMERIC(30,0) NOT NULL DEFAULT 0,
    live_capacity_sum NUMERIC(30,0) NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX idx_scripts_code_hash ON scripts(code_hash);
CREATE INDEX idx_scripts_cells_count ON scripts(cells_count DESC);

-- ---- known_scripts ----
-- Stores metadata about known CKB scripts from token-labels repository
-- Each script can have multiple deployments (mainnet/testnet, versions)
CREATE TABLE known_scripts (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    code_hash BYTEA NOT NULL,
    name VARCHAR(255) NOT NULL,
    description TEXT,
    -- NOTE: script_kind (lock/type) is NOT stored here - it's dynamically inferred
    -- from cells table at query time since upstream token-labels doesn't have this data
    
    -- Extended metadata from token-labels
    rfc TEXT,  -- RFC documentation URL
    website TEXT,  -- Project website
    source_url TEXT,  -- Source code URL
    decoder_type VARCHAR(50),  -- 'udt', 'spore', 'spore-cluster', 'dao', 'ckbfs'
    
    -- Deployment info
    network VARCHAR(20) NOT NULL DEFAULT 'mainnet',  -- 'mainnet' or 'testnet'
    hash_type VARCHAR(20),  -- 'type', 'data', 'data1', 'data2'
    data_hash BYTEA,
    type_hash BYTEA,
    tag VARCHAR(100),  -- Version tag like 'v1', 'v2', '@0000'
    deprecated BOOLEAN NOT NULL DEFAULT FALSE,
    
    -- System flag and tracking
    is_system BOOLEAN NOT NULL DEFAULT FALSE,
    label_source VARCHAR(100) DEFAULT 'token-labels',
    label_updated_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    
    -- Unique constraint per code_hash + network + tag combination
    UNIQUE(code_hash, network, tag)
);

CREATE INDEX idx_known_scripts_code_hash ON known_scripts(code_hash);
CREATE INDEX idx_known_scripts_name ON known_scripts(name);
CREATE INDEX idx_known_scripts_network ON known_scripts(network);
CREATE INDEX idx_known_scripts_decoder_type ON known_scripts(decoder_type) WHERE decoder_type IS NOT NULL;

-- ---- script_usage_stats ----
-- Precomputed usage statistics for scripts, updated incrementally by the indexer.
-- This avoids expensive full-table scans on the cells table for script usage queries.
CREATE TABLE script_usage_stats (
    code_hash BYTEA NOT NULL,
    script_kind VARCHAR(4) NOT NULL,  -- 'lock' or 'type'
    cells_count BIGINT NOT NULL DEFAULT 0,
    live_cells_count BIGINT NOT NULL DEFAULT 0,
    capacity_sum NUMERIC(30,0) NOT NULL DEFAULT 0,
    live_capacity_sum NUMERIC(30,0) NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    PRIMARY KEY (code_hash, script_kind)
);

-- Insert known system scripts (mainnet) - these are foundational CKB scripts
-- NOTE: tag must be '' (empty string) not NULL to work correctly with UNIQUE constraint
-- and to match the upsert logic in integrity/mod.rs which uses empty string for empty tags
INSERT INTO known_scripts (code_hash, name, description, network, hash_type, is_system, rfc, tag) VALUES
(decode('9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8', 'hex'), 
 'Default Lock', 'Default lock script to verify CKB transaction signature', 'mainnet', 'type', true,
 'https://github.com/nervosnetwork/rfcs/blob/master/rfcs/0024-ckb-genesis-script-list/0024-ckb-genesis-script-list.md', ''),
(decode('5c5069eb0857efc65e1bca0c07df34c31663b3622fd3876c876320fc9634e2a8', 'hex'), 
 'Default Multisig', 'Multi-signature lock script', 'mainnet', 'type', true,
 'https://github.com/nervosnetwork/rfcs/blob/master/rfcs/0024-ckb-genesis-script-list/0024-ckb-genesis-script-list.md', ''),
(decode('82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e', 'hex'), 
 'Nervos DAO', 'Nervos DAO type script for deposits and withdrawals', 'mainnet', 'type', true,
 'https://github.com/nervosnetwork/rfcs/blob/master/rfcs/0024-ckb-genesis-script-list/0024-ckb-genesis-script-list.md', ''),
(decode('5e7a36a77e68eecc013dfa2fe6a23f3b6c344b04005808694ae6dd45eea4cfd5', 'hex'), 
 'Simple UDT', 'Simple UDT type script for fungible tokens', 'mainnet', 'type', true,
 'https://github.com/nervosnetwork/rfcs/blob/master/rfcs/0025-simple-udt/0025-simple-udt.md', ''),
(decode('50bd8d6680b8b9cf98b73f3c08faf8b2a21914311954118ad6609be6e78a1b95', 'hex'),
 'xUDT', 'Extensible UDT type script', 'mainnet', 'data1', true,
 'https://github.com/nervosnetwork/rfcs/blob/master/rfcs/0052-extensible-udt/0052-extensible-udt.md', '@50bd8d66')
ON CONFLICT (code_hash, network, tag) DO NOTHING;

-- ===========================================
-- 9. BRIN Indexes (Time-series data)
-- ===========================================

-- blocks: inserted in number order
CREATE INDEX idx_blocks_number_brin ON blocks USING BRIN (number) WITH (pages_per_range = 128);
CREATE INDEX idx_blocks_timestamp_brin ON blocks USING BRIN (timestamp) WITH (pages_per_range = 128);

-- transactions: inserted in block_number order
CREATE INDEX idx_tx_block_brin ON transactions USING BRIN (block_number) WITH (pages_per_range = 128);

-- cells: inserted in created_at_block order
CREATE INDEX idx_cells_created_brin ON cells USING BRIN (created_at_block) WITH (pages_per_range = 128);

-- ===========================================
-- 10. B-tree Indexes (Lookups)
-- ===========================================

-- blocks
CREATE INDEX idx_blocks_hash ON blocks(hash);
CREATE INDEX idx_blocks_epoch ON blocks(epoch_number);
CREATE INDEX idx_blocks_miner ON blocks(miner_lock_hash) WHERE miner_lock_hash IS NOT NULL;

-- transactions
CREATE INDEX idx_tx_hash ON transactions(hash);
CREATE INDEX idx_tx_timestamp ON transactions(timestamp DESC);
CREATE INDEX idx_tx_short_hash ON transactions(short_hash, block_number);
-- Cursor pagination
CREATE INDEX idx_tx_cursor ON transactions(block_number DESC, tx_index DESC);

-- cells
CREATE INDEX idx_cells_outpoint ON cells(tx_hash, output_index);
-- Live cells query (most important)
CREATE INDEX idx_cells_lock_live ON cells(lock_script_hash, created_at_block DESC)
    WHERE status = 0;
-- Lock script details lookup (for address encoding)
CREATE INDEX idx_cells_lock_script_details ON cells(lock_script_hash)
    INCLUDE (lock_code_hash, lock_hash_type, lock_args);
CREATE INDEX idx_cells_type_live ON cells(type_script_hash, created_at_block DESC)
    WHERE status = 0 AND type_script_hash IS NOT NULL;
-- Consumed query
CREATE INDEX idx_cells_consumed_by ON cells(consumed_by_tx)
    WHERE consumed_by_tx IS NOT NULL;
-- Type script hash lookup (for UDT)
CREATE INDEX idx_cells_type_script_hash ON cells(type_script_hash) WHERE type_script_hash IS NOT NULL;
-- Code hash lookup (for script usage stats) - basic indexes
CREATE INDEX idx_cells_lock_code_hash ON cells(lock_code_hash);
CREATE INDEX idx_cells_type_code_hash ON cells(type_code_hash) WHERE type_code_hash IS NOT NULL;

-- Optimized composite indexes for script page cells query
-- Includes hash_type for exact match, ordered by created_at_block DESC for cursor pagination
CREATE INDEX idx_cells_lock_code_hash_live ON cells(lock_code_hash, lock_hash_type, created_at_block DESC, output_index DESC)
    WHERE status = 0;
CREATE INDEX idx_cells_type_code_hash_live ON cells(type_code_hash, type_hash_type, created_at_block DESC, output_index DESC)
    WHERE status = 0 AND type_code_hash IS NOT NULL;

-- transaction_inputs
CREATE INDEX idx_inputs_previous ON transaction_inputs(previous_tx_hash, previous_output_index);
CREATE INDEX idx_inputs_tx ON transaction_inputs(tx_hash);

-- transaction_cell_deps
CREATE INDEX idx_cell_deps_tx ON transaction_cell_deps(tx_hash);

-- uncle_blocks
CREATE INDEX idx_uncles_hash ON uncle_blocks(hash);

-- ===========================================
-- 11. Covering Indexes (Avoid table lookups)
-- ===========================================

-- Transaction list page common fields
CREATE INDEX idx_tx_list_covering ON transactions(block_number DESC, tx_index DESC)
    INCLUDE (hash, inputs_count, outputs_count, fee, is_cellbase, timestamp);

-- Cell list page common fields
CREATE INDEX idx_cells_list_covering ON cells(lock_script_hash, created_at_block DESC)
    INCLUDE (tx_hash, output_index, capacity, type_script_hash, data_size)
    WHERE status = 0;

-- ===========================================
-- 12. Task System
-- Background task management for ckbadger
-- ===========================================

CREATE TABLE tasks (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    task_type VARCHAR(50) NOT NULL,  -- 'cycles_backfill', 'index_rebuild', 'label_import'
    status VARCHAR(20) NOT NULL DEFAULT 'pending',  -- 'pending', 'running', 'completed', 'failed', 'cancelled', 'paused'
    priority INTEGER DEFAULT 0,
    
    -- Configuration (task-specific JSON)
    config JSONB NOT NULL DEFAULT '{}',
    
    -- Progress tracking
    progress_total BIGINT DEFAULT 0,
    progress_current BIGINT DEFAULT 0,
    progress_message TEXT,
    
    -- Result/error
    result JSONB,
    error_message TEXT,
    
    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    heartbeat_at TIMESTAMPTZ,
    
    -- Runtime metadata
    runner_id VARCHAR(100),  -- Identifies which runner instance is executing
    retry_count INTEGER DEFAULT 0,
    max_retries INTEGER DEFAULT 3,
    
    -- Rate tracking for ETA calculation
    rate_samples JSONB DEFAULT '[]',  -- Recent samples: [{ts: epoch, v: progress_current}, ...]
    rate_ema DOUBLE PRECISION,  -- Exponential moving average (items/sec)
    
    -- Log tail for TUI display
    log_tail TEXT  -- Last N lines of log output
);

CREATE INDEX idx_tasks_status ON tasks(status);
CREATE INDEX idx_tasks_type_status ON tasks(task_type, status);
CREATE INDEX idx_tasks_created_at ON tasks(created_at DESC);
CREATE INDEX idx_tasks_runner ON tasks(runner_id) WHERE runner_id IS NOT NULL;

COMMENT ON TABLE tasks IS 'Background task management for cycles backfill, index rebuild, and label import';
COMMENT ON COLUMN tasks.task_type IS 'cycles_backfill | index_rebuild | label_import';
COMMENT ON COLUMN tasks.status IS 'pending | running | completed | failed | cancelled | paused';
COMMENT ON COLUMN tasks.config IS 'Task-specific configuration JSON';
COMMENT ON COLUMN tasks.result IS 'Task result/progress details JSON (e.g., index rebuild progress)';
COMMENT ON COLUMN tasks.rate_ema IS 'Exponential moving average rate for ETA calculation';
