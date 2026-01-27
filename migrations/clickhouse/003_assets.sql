-- ClickHouse Asset Tables Schema (Phase 2)
-- DAO deposits/withdrawals, sUDT/xUDT tokens, Spore NFTs
--
-- Design Principles:
-- 1. Event-sourcing pattern (immutable inserts only, no UPDATE)
-- 2. Separate tables for different event types (deposits, withdrawals, transfers)
-- 3. FixedString(32) for all hash fields (binary serialization)
-- 4. Partitioning by block number for time-series queries
-- 5. Derive current state from event history (no status columns)
--
-- DAO Lifecycle:
-- 1. Deposit: User locks CKB in DAO cell → INSERT INTO dao_deposits
-- 2. Withdraw Request: User creates withdraw request → INSERT INTO dao_withdrawals (withdraw_request_tx)
-- 3. Withdraw Completion: User completes withdrawal → INSERT INTO dao_withdrawals (withdraw_completion_tx)
--
-- Token Lifecycle:
-- 1. Token Creation: First sUDT/xUDT cell appears → INSERT INTO tokens
-- 2. Token Transfer: Cell consumed/created → INSERT INTO token_transfers
-- 3. Token Burn: Cell consumed without output → INSERT INTO token_transfers (to_lock_hash = NULL)
--
-- NFT Lifecycle:
-- 1. Spore Creation: Spore cell created → INSERT INTO spore_cells
-- 2. Spore Transfer: Spore cell consumed/created → INSERT INTO spore_transfers
-- 3. Spore Burn: Spore cell consumed without output → spore_cells.consumed_at_block set

USE ckbadger;

-- ============================================================================
-- DAO TABLES
-- ============================================================================

-- ---- dao_deposits ----
-- Records DAO deposit events (cell creation with DAO type script)
-- Primary access pattern: Query by depositor lock hash or block range
-- Partition: 5M blocks per partition (aligned with deposit block)
-- Sort key: (deposit_block, tx_hash, output_index) - optimized for time-series queries
--
-- DAO Deposit Lifecycle:
-- 1. User locks CKB in DAO cell → INSERT INTO dao_deposits
-- 2. Cell data = [0u8; 8] (8 zero bytes indicates deposit state)
-- 3. Capacity = locked amount (shannon)
-- 4. deposit_ar = AR (accumulated rate) from block DAO field at deposit time
--
-- Query Patterns:
-- - Active deposits: SELECT * FROM dao_deposits WHERE (tx_hash, output_index) NOT IN (SELECT deposit_tx, deposit_index FROM dao_withdrawals)
-- - Depositor balance: SELECT sum(capacity) FROM dao_deposits WHERE depositor_lock_hash = unhex('...')
-- - Deposit history: SELECT * FROM dao_deposits WHERE deposit_block >= X AND deposit_block < Y

CREATE TABLE IF NOT EXISTS dao_deposits (
    -- Cell identification (OutPoint)
    tx_hash FixedString(32),            -- Transaction hash (32 bytes binary)
    output_index UInt16,                -- Output index within transaction
    
    -- Depositor information
    depositor_lock_hash FixedString(32), -- Lock script hash (identifies depositor)
    
    -- Deposit metadata
    capacity UInt64,                    -- Locked capacity in shannon (1 CKB = 10^8 shannon)
    deposit_block UInt64,               -- Block height when deposit was created
    deposit_timestamp DateTime,         -- Block timestamp (denormalized for convenience)
    deposit_ar UInt64                   -- AR (accumulated rate) at deposit time (from block DAO field)
) ENGINE = MergeTree()
PARTITION BY intDiv(deposit_block, 5000000)
ORDER BY (deposit_block, tx_hash, output_index)
PRIMARY KEY (deposit_block, tx_hash, output_index)
COMMENT 'DAO deposit events - immutable insert-only';

-- ---- dao_withdrawals ----
-- Records DAO withdrawal events (withdraw request + completion)
-- Primary access pattern: Query by deposit OutPoint or withdrawal block range
-- Partition: 5M blocks per partition (aligned with withdraw request block)
-- Sort key: (withdraw_request_block, deposit_tx, deposit_index) - optimized for time-series queries
--
-- DAO Withdrawal Lifecycle:
-- 1. Withdraw Request: User creates withdraw request cell → INSERT INTO dao_withdrawals (withdraw_request_tx, withdraw_request_block)
--    - Cell data = deposit_block_number (8 bytes, little-endian u64)
--    - Links back to original deposit via deposit_tx + deposit_index
-- 2. Withdraw Completion: User completes withdrawal → INSERT INTO dao_withdrawals (withdraw_completion_tx, withdraw_completion_block, compensation)
--    - Compensation = (free_capacity * ar_withdraw / ar_deposit) - free_capacity
--    - free_capacity = capacity - occupied_capacity (102 CKB for DAO cell)
--
-- Query Patterns:
-- - Pending withdrawals: SELECT * FROM dao_withdrawals WHERE withdraw_completion_tx IS NULL
-- - Completed withdrawals: SELECT * FROM dao_withdrawals WHERE withdraw_completion_tx IS NOT NULL
-- - Withdrawal by deposit: SELECT * FROM dao_withdrawals WHERE deposit_tx = unhex('...') AND deposit_index = 0

CREATE TABLE IF NOT EXISTS dao_withdrawals (
    -- Original deposit identification (links to dao_deposits)
    deposit_tx FixedString(32),         -- Original deposit transaction hash
    deposit_index UInt16,               -- Original deposit output index
    
    -- Withdraw request metadata
    withdraw_request_tx FixedString(32), -- Withdraw request transaction hash
    withdraw_request_block UInt64,      -- Block height when withdraw request was created
    withdraw_request_timestamp DateTime, -- Block timestamp (denormalized)
    withdraw_request_ar UInt64,         -- AR at withdraw request time (from block DAO field)
    
    -- Withdraw completion metadata (NULL until completed)
    withdraw_completion_tx Nullable(FixedString(32)), -- Withdraw completion transaction hash
    withdraw_completion_block Nullable(UInt64),       -- Block height when withdrawal was completed
    withdraw_completion_timestamp Nullable(DateTime), -- Block timestamp (denormalized)
    compensation Nullable(UInt64)       -- Compensation amount in shannon (calculated)
) ENGINE = MergeTree()
PARTITION BY intDiv(withdraw_request_block, 5000000)
ORDER BY (withdraw_request_block, deposit_tx, deposit_index)
PRIMARY KEY (withdraw_request_block, deposit_tx, deposit_index)
COMMENT 'DAO withdrawal events (request + completion) - immutable insert-only';

-- ============================================================================
-- TOKEN TABLES (sUDT/xUDT)
-- ============================================================================

-- ---- tokens ----
-- Records token metadata (relatively static, updated infrequently)
-- Primary access pattern: Query by type_script_hash or list all tokens
-- No partitioning (small table, ~1000s of tokens)
-- Sort key: (type_script_hash) - optimized for token lookup
--
-- Token Standards:
-- - sUDT: Simple UDT (type script with amount in data)
-- - xUDT: Extended UDT (additional metadata in data)
--
-- Token Metadata Sources:
-- 1. On-chain: type_script_hash, type_code_hash, type_hash_type, type_args
-- 2. Off-chain (token-labels): name, symbol, decimals, description, icon_url, tags
-- 3. Computed: total_supply, holders_count, transfers_count (from token_transfers)
--
-- Query Patterns:
-- - Token by hash: SELECT * FROM tokens WHERE type_script_hash = unhex('...')
-- - Token list: SELECT * FROM tokens ORDER BY holders_count DESC LIMIT 100
-- - Token search: SELECT * FROM tokens WHERE name LIKE '%CKB%'

CREATE TABLE IF NOT EXISTS tokens (
    -- Token identification (type script)
    type_script_hash FixedString(32),   -- Type script hash (unique identifier)
    type_code_hash FixedString(32),     -- Type script code hash
    type_hash_type UInt8,               -- Type script hash type (0=data, 1=type, 2=data1)
    type_args String,                   -- Type script args (hex-encoded, variable length)
    
    -- Token standard
    standard String,                    -- 'sudt' or 'xudt'
    
    -- Token metadata (from token-labels or on-chain)
    name Nullable(String),              -- Token name (e.g., "Nervos DAO")
    symbol Nullable(String),            -- Token symbol (e.g., "DAO")
    decimals UInt8,                     -- Decimal places (default: 8)
    description Nullable(String),       -- Token description
    icon_url Nullable(String),          -- Token icon URL
    
    -- Token statistics (computed from token_transfers)
    total_supply String,                -- Total supply (UInt128 as string, may exceed UInt64)
    holders_count UInt32,               -- Number of unique holders
    transfers_count UInt64,             -- Total number of transfers
    
    -- Discovery metadata
    first_seen_block UInt64,            -- Block height when token was first seen
    first_seen_tx FixedString(32)       -- Transaction hash where token was first seen
) ENGINE = MergeTree()
ORDER BY (type_script_hash)
PRIMARY KEY (type_script_hash)
COMMENT 'Token metadata (sUDT/xUDT) - relatively static';

-- ---- token_transfers ----
-- Records token transfer events (mint, transfer, burn)
-- Primary access pattern: Query by type_script_hash, lock_hash, or block range
-- Partition: 5M blocks per partition (aligned with block number)
-- Sort key: (block_number, type_script_hash, tx_hash) - optimized for time-series queries
--
-- Transfer Types:
-- 1. Mint: from_lock_hash = NULL, to_lock_hash = recipient
-- 2. Transfer: from_lock_hash = sender, to_lock_hash = recipient
-- 3. Burn: from_lock_hash = sender, to_lock_hash = NULL
--
-- Query Patterns:
-- - Token transfers: SELECT * FROM token_transfers WHERE type_script_hash = unhex('...')
-- - Address transfers: SELECT * FROM token_transfers WHERE from_lock_hash = unhex('...') OR to_lock_hash = unhex('...')
-- - Recent transfers: SELECT * FROM token_transfers WHERE block_number >= X ORDER BY block_number DESC

CREATE TABLE IF NOT EXISTS token_transfers (
    -- Token identification
    type_script_hash FixedString(32),   -- Type script hash (identifies token)
    
    -- Transfer participants
    from_lock_hash Nullable(FixedString(32)), -- Sender lock script hash (NULL for mint)
    to_lock_hash Nullable(FixedString(32)),   -- Recipient lock script hash (NULL for burn)
    
    -- Transfer metadata
    amount String,                      -- Transfer amount (UInt128 as string, may exceed UInt64)
    block_number UInt64,                -- Block height when transfer occurred
    tx_hash FixedString(32),            -- Transaction hash
    tx_index UInt32,                    -- Transaction index within block
    timestamp DateTime                  -- Block timestamp (denormalized)
) ENGINE = MergeTree()
PARTITION BY intDiv(block_number, 5000000)
ORDER BY (block_number, type_script_hash, tx_hash)
PRIMARY KEY (block_number, type_script_hash, tx_hash)
COMMENT 'Token transfer events (mint/transfer/burn) - immutable insert-only';

-- ============================================================================
-- NFT TABLES (Spore Protocol)
-- ============================================================================

-- ---- spore_cells ----
-- Records Spore NFT metadata (creation + consumption)
-- Primary access pattern: Query by spore_id, cluster_id, or owner lock hash
-- Partition: 5M blocks per partition (aligned with creation block)
-- Sort key: (created_at_block, tx_hash, output_index) - optimized for time-series queries
--
-- Spore Protocol:
-- - Spore ID: Unique identifier (type_script.args, 32 bytes)
-- - Cluster ID: Optional collection identifier (32 bytes)
-- - Content: NFT data (image, text, etc.) stored in cell data
-- - Content Type: MIME type (e.g., "image/png", "text/plain")
--
-- Spore Lifecycle:
-- 1. Creation: Spore cell created → INSERT INTO spore_cells (consumed_at_block = NULL)
-- 2. Transfer: Spore cell consumed/created → INSERT INTO spore_transfers + UPDATE spore_cells.consumed_at_block
-- 3. Burn: Spore cell consumed without output → UPDATE spore_cells.consumed_at_block
--
-- Query Patterns:
-- - Live spores: SELECT * FROM spore_cells WHERE consumed_at_block IS NULL
-- - Spore by ID: SELECT * FROM spore_cells WHERE tx_hash = unhex('...') AND output_index = 0
-- - Spores by cluster: SELECT * FROM spore_cells WHERE cluster_id = unhex('...')
-- - Spores by owner: SELECT * FROM spore_cells WHERE owner_lock_hash = unhex('...')

CREATE TABLE IF NOT EXISTS spore_cells (
    -- Cell identification (OutPoint)
    tx_hash FixedString(32),            -- Transaction hash (32 bytes binary)
    output_index UInt16,                -- Output index within transaction
    
    -- Spore identification
    spore_id FixedString(32),           -- Spore ID (type_script.args, unique identifier)
    cluster_id Nullable(FixedString(32)), -- Cluster ID (optional collection identifier)
    
    -- Spore metadata
    content_type String,                -- MIME type (e.g., "image/png", "text/plain")
    content_size UInt32,                -- Content size in bytes
    content Nullable(String),           -- Content data (hex-encoded, up to 512 bytes for preview)
    
    -- Ownership
    owner_lock_hash FixedString(32),    -- Owner lock script hash
    
    -- Lifecycle metadata
    created_at_block UInt64,            -- Block height when spore was created
    created_at_timestamp DateTime,      -- Block timestamp (denormalized)
    consumed_at_block Nullable(UInt64), -- Block height when spore was consumed (NULL if live)
    consumed_by_tx Nullable(FixedString(32)) -- Transaction hash that consumed this spore
) ENGINE = MergeTree()
PARTITION BY intDiv(created_at_block, 5000000)
ORDER BY (created_at_block, tx_hash, output_index)
PRIMARY KEY (created_at_block, tx_hash, output_index)
COMMENT 'Spore NFT cells (creation + consumption) - mutable for consumed_at_block';

-- ---- spore_transfers ----
-- Records Spore NFT transfer events
-- Primary access pattern: Query by spore_id, from/to lock hash, or block range
-- Partition: 5M blocks per partition (aligned with block number)
-- Sort key: (block_number, tx_hash, output_index) - optimized for time-series queries
--
-- Transfer Types:
-- 1. Mint: from_lock_hash = NULL, to_lock_hash = recipient
-- 2. Transfer: from_lock_hash = sender, to_lock_hash = recipient
-- 3. Burn: from_lock_hash = sender, to_lock_hash = NULL
--
-- Query Patterns:
-- - Spore transfer history: SELECT * FROM spore_transfers WHERE tx_hash = unhex('...') AND output_index = 0
-- - Address transfers: SELECT * FROM spore_transfers WHERE from_lock_hash = unhex('...') OR to_lock_hash = unhex('...')
-- - Recent transfers: SELECT * FROM spore_transfers WHERE block_number >= X ORDER BY block_number DESC

CREATE TABLE IF NOT EXISTS spore_transfers (
    -- Spore identification (OutPoint)
    tx_hash FixedString(32),            -- Original spore transaction hash
    output_index UInt16,                -- Original spore output index
    spore_id FixedString(32),           -- Spore ID (for quick lookup)
    
    -- Transfer participants
    from_lock_hash Nullable(FixedString(32)), -- Sender lock script hash (NULL for mint)
    to_lock_hash Nullable(FixedString(32)),   -- Recipient lock script hash (NULL for burn)
    
    -- Transfer metadata
    block_number UInt64,                -- Block height when transfer occurred
    transfer_tx FixedString(32),        -- Transaction hash that performed the transfer
    timestamp DateTime                  -- Block timestamp (denormalized)
) ENGINE = MergeTree()
PARTITION BY intDiv(block_number, 5000000)
ORDER BY (block_number, tx_hash, output_index)
PRIMARY KEY (block_number, tx_hash, output_index)
COMMENT 'Spore NFT transfer events - immutable insert-only';

-- ============================================================================
-- SCHEMA DESIGN NOTES
-- ============================================================================
--
-- 1. EVENT-SOURCING PATTERN
--    - No UPDATE semantics (except spore_cells.consumed_at_block)
--    - Separate tables for different event types (deposits, withdrawals, transfers)
--    - Derive current state from event history
--    - Example: Active DAO deposits = dao_deposits NOT IN dao_withdrawals
--
-- 2. DAO LIFECYCLE TRACKING
--    - dao_deposits: Records deposit events (cell creation)
--    - dao_withdrawals: Records withdrawal events (request + completion)
--    - Compensation calculation: (free_capacity * ar_withdraw / ar_deposit) - free_capacity
--    - free_capacity = capacity - 102_00000000 (102 CKB occupied by DAO cell)
--    - AR (accumulated rate) extracted from block DAO field (bytes 8-15, u64 little-endian)
--
-- 3. TOKEN TRANSFER TRACKING
--    - tokens: Token metadata (relatively static)
--    - token_transfers: Transfer events (mint/transfer/burn)
--    - Mint: from_lock_hash = NULL
--    - Burn: to_lock_hash = NULL
--    - Amount stored as String (UInt128 may exceed UInt64 range)
--
-- 4. NFT TRANSFER TRACKING
--    - spore_cells: Spore metadata (creation + consumption)
--    - spore_transfers: Transfer events (mint/transfer/burn)
--    - Spore ID: type_script.args (32 bytes, unique identifier)
--    - Cluster ID: Optional collection identifier (32 bytes)
--    - Content: Stored in cell data (hex-encoded, up to 512 bytes for preview)
--
-- 5. PARTITIONING STRATEGY
--    - DAO tables: Partition by deposit_block / withdraw_request_block
--    - Token tables: tokens (no partition), token_transfers (partition by block_number)
--    - NFT tables: Partition by created_at_block / block_number
--    - Partition size: 5M blocks = ~18 partitions for full mainnet
--
-- 6. SORT KEYS (ORDER BY)
--    - dao_deposits: (deposit_block, tx_hash, output_index) - time-series queries
--    - dao_withdrawals: (withdraw_request_block, deposit_tx, deposit_index) - time-series queries
--    - tokens: (type_script_hash) - token lookup
--    - token_transfers: (block_number, type_script_hash, tx_hash) - time-series queries
--    - spore_cells: (created_at_block, tx_hash, output_index) - time-series queries
--    - spore_transfers: (block_number, tx_hash, output_index) - time-series queries
--
-- 7. DATA TYPES
--    - FixedString(32): All hash fields (binary storage)
--    - UInt64: Block numbers, capacity, AR values
--    - UInt32: Counts, sizes, indexes
--    - UInt16: Small indexes (output_index)
--    - UInt8: Flags, enums (hash_type, decimals)
--    - String: Variable-length data (type_args, content, amount)
--    - DateTime: Timestamps (automatic conversion from Unix epoch)
--    - Nullable(): Optional fields (type_script, cluster_id, from_lock_hash, to_lock_hash)
--
-- 8. QUERY PATTERNS
--    -- Active DAO deposits (not withdrawn)
--    SELECT d.*
--    FROM dao_deposits d
--    LEFT ANTI JOIN dao_withdrawals w
--      ON d.tx_hash = w.deposit_tx AND d.output_index = w.deposit_index
--    WHERE d.depositor_lock_hash = unhex('...');
--
--    -- Pending DAO withdrawals (not completed)
--    SELECT *
--    FROM dao_withdrawals
--    WHERE withdraw_completion_tx IS NULL;
--
--    -- Token balance (sum of live cells)
--    SELECT sum(toUInt128OrZero(amount)) AS balance
--    FROM token_transfers
--    WHERE type_script_hash = unhex('...')
--      AND to_lock_hash = unhex('...')
--      AND (tx_hash, output_index) NOT IN (
--        SELECT tx_hash, output_index FROM token_transfers WHERE from_lock_hash = unhex('...')
--      );
--
--    -- Live Spore NFTs (not consumed)
--    SELECT *
--    FROM spore_cells
--    WHERE consumed_at_block IS NULL
--      AND owner_lock_hash = unhex('...');
--
-- 9. MIGRATION FROM POSTGRESQL
--    - PostgreSQL dao_deposits.status → ClickHouse dao_deposits + dao_withdrawals (event tables)
--    - PostgreSQL tokens.holders_count → ClickHouse computed from token_transfers
--    - PostgreSQL spore_cells.is_live → ClickHouse spore_cells.consumed_at_block IS NULL
--    - PostgreSQL token_balances → ClickHouse computed from token_transfers (no separate table)
--
-- 10. FUTURE ENHANCEMENTS (Phase 3+)
--     - Materialized views for token balances (avoid recomputing from transfers)
--     - Materialized views for DAO statistics (total deposited, active deposits)
--     - Aggregating tables for daily token transfer volumes
--     - Secondary indexes for lock_script_hash, type_script_hash
--     - ReplacingMergeTree for spore_cells (avoid UPDATE for consumed_at_block)
--
-- ============================================================================
-- END OF SCHEMA
-- ============================================================================
