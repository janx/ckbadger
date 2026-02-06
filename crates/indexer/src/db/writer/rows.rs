//! ClickHouse row types for batch inserts.
//!
//! These structs match the ClickHouse schema exactly and use the
//! `clickhouse::Row` derive macro for efficient serialization.
//!
//! Column naming convention:
//! - Rust fields use snake_case
//! - ClickHouse columns are snake_case (match naturally)
//!
//! Type mappings:
//! - FixedString(32) → [u8; 32]
//! - FixedString(16) → [u8; 16]
//! - DateTime64(3, 'UTC') → i64 (milliseconds since epoch)
//! - UInt256 → [u8; 32] (little-endian bytes)
//! - LowCardinality(String) → String

use clickhouse::Row;
use serde::Serialize;

/// Empty 32-byte array for NULL-like semantics in FixedString columns.
pub const EMPTY_HASH: [u8; 32] = [0u8; 32];

/// Empty 16-byte array for NULL-like semantics in FixedString(16) columns.
pub const EMPTY_NONCE: [u8; 16] = [0u8; 16];

// =============================================================================
// blocks_all
// =============================================================================

/// Row for `blocks_all` table (MergeTree, append-only).
#[derive(Debug, Clone, Row, Serialize)]
pub struct BlockRow {
    pub number: u64,
    pub hash: [u8; 32],
    pub parent_hash: [u8; 32],
    pub timestamp: i64, // milliseconds since epoch
    pub version: u32,
    pub compact_target: u64,
    pub transactions_count: u32,
    pub proposals_count: u32,
    pub uncles_count: u8,
    pub epoch_number: u64,
    pub epoch_index: u32,
    pub epoch_length: u32,
    pub dao: [u8; 32],
    pub nonce: [u8; 16],
    pub extra_hash: [u8; 32],
    pub extension: String,
    pub proposals_hash: [u8; 32],
    pub transactions_root: [u8; 32],
    pub uncles_hash: [u8; 32],
    pub miner_lock_hash: [u8; 32],
    pub miner_message: String,
    pub total_difficulty: [u8; 32], // UInt256 as LE bytes
    pub reward: u64,
}

impl Default for BlockRow {
    fn default() -> Self {
        Self {
            number: 0,
            hash: EMPTY_HASH,
            parent_hash: EMPTY_HASH,
            timestamp: 0,
            version: 0,
            compact_target: 0,
            transactions_count: 0,
            proposals_count: 0,
            uncles_count: 0,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 0,
            dao: EMPTY_HASH,
            nonce: EMPTY_NONCE,
            extra_hash: EMPTY_HASH,
            extension: String::new(),
            proposals_hash: EMPTY_HASH,
            transactions_root: EMPTY_HASH,
            uncles_hash: EMPTY_HASH,
            miner_lock_hash: EMPTY_HASH,
            miner_message: String::new(),
            total_difficulty: EMPTY_HASH,
            reward: 0,
        }
    }
}

// =============================================================================
// transactions_all
// =============================================================================

/// Row for `transactions_all` table (MergeTree, append-only).
#[derive(Debug, Clone, Row, Serialize)]
pub struct TransactionRow {
    pub hash: [u8; 32],
    pub block_number: u64,
    pub block_hash: [u8; 32],
    pub tx_index: u32,
    pub version: u32,
    pub inputs_count: u16,
    pub outputs_count: u16,
    pub witnesses_count: u16,
    pub cell_deps_count: u16,
    pub header_deps_count: u16,
    pub total_input_capacity: u64,
    pub total_output_capacity: u64,
    pub fee: u64,
    pub tx_size: u32,
    pub cycles: u64,
    pub is_cellbase: u8,
    pub timestamp: i64, // milliseconds since epoch
}

impl Default for TransactionRow {
    fn default() -> Self {
        Self {
            hash: EMPTY_HASH,
            block_number: 0,
            block_hash: EMPTY_HASH,
            tx_index: 0,
            version: 0,
            inputs_count: 0,
            outputs_count: 0,
            witnesses_count: 0,
            cell_deps_count: 0,
            header_deps_count: 0,
            total_input_capacity: 0,
            total_output_capacity: 0,
            fee: 0,
            tx_size: 0,
            cycles: 0,
            is_cellbase: 0,
            timestamp: 0,
        }
    }
}

// =============================================================================
// cell_outputs_all
// =============================================================================

/// Row for `cell_outputs_all` table (MergeTree, append-only).
#[derive(Debug, Clone, Row, Serialize)]
pub struct CellOutputRow {
    pub tx_hash: [u8; 32],
    pub output_index: u16,
    pub block_number: u64,
    pub block_hash: [u8; 32],
    pub capacity: u64,
    pub lock_code_hash: [u8; 32],
    pub lock_hash_type: u8,
    pub lock_args: String, // Variable length hex or bytes
    pub lock_script_hash: [u8; 32],
    pub type_code_hash: [u8; 32], // Empty if no type script
    pub type_hash_type: u8,
    pub type_args: String,
    pub type_script_hash: [u8; 32], // Empty if no type script
    pub data_hash: [u8; 32],
    pub data_size: u32,
    pub data: String, // Up to 512 bytes for preview
}

impl Default for CellOutputRow {
    fn default() -> Self {
        Self {
            tx_hash: EMPTY_HASH,
            output_index: 0,
            block_number: 0,
            block_hash: EMPTY_HASH,
            capacity: 0,
            lock_code_hash: EMPTY_HASH,
            lock_hash_type: 0,
            lock_args: String::new(),
            lock_script_hash: EMPTY_HASH,
            type_code_hash: EMPTY_HASH,
            type_hash_type: 0,
            type_args: String::new(),
            type_script_hash: EMPTY_HASH,
            data_hash: EMPTY_HASH,
            data_size: 0,
            data: String::new(),
        }
    }
}

// =============================================================================
// cell_inputs_all
// =============================================================================

/// Row for `cell_inputs_all` table (MergeTree, append-only).
#[derive(Debug, Clone, Row, Serialize)]
pub struct CellInputRow {
    pub tx_hash: [u8; 32],
    pub tx_block_number: u64,
    pub input_index: u16,
    pub previous_tx_hash: [u8; 32],
    pub previous_output_index: u16,
    pub since: u64,
}

impl Default for CellInputRow {
    fn default() -> Self {
        Self {
            tx_hash: EMPTY_HASH,
            tx_block_number: 0,
            input_index: 0,
            previous_tx_hash: EMPTY_HASH,
            previous_output_index: 0,
            since: 0,
        }
    }
}

// =============================================================================
// activities_all
// =============================================================================

/// Row for `activities_all` table (MergeTree, append-only).
#[derive(Debug, Clone, Row, Serialize)]
pub struct ActivityRow {
    pub activity_id: [u8; 32],
    pub activity_type: String,     // LowCardinality in CH
    pub activity_category: String, // LowCardinality in CH
    pub block_number: u64,
    pub tx_hash: [u8; 32],
    pub tx_index: u32,
    pub activity_index: u16,
    pub from_lock_hash: [u8; 32], // Empty for mint/cellbase
    pub to_lock_hash: [u8; 32],   // Empty for burn
    pub amount: [u8; 32],         // UInt256 as LE bytes
    pub asset_id: [u8; 32],       // Empty if N/A
    pub metadata: String,         // JSON string
    pub timestamp: i64,           // milliseconds since epoch
}

impl Default for ActivityRow {
    fn default() -> Self {
        Self {
            activity_id: EMPTY_HASH,
            activity_type: String::new(),
            activity_category: String::new(),
            block_number: 0,
            tx_hash: EMPTY_HASH,
            tx_index: 0,
            activity_index: 0,
            from_lock_hash: EMPTY_HASH,
            to_lock_hash: EMPTY_HASH,
            amount: EMPTY_HASH,
            asset_id: EMPTY_HASH,
            metadata: String::new(),
            timestamp: 0,
        }
    }
}

// =============================================================================
// canonical_blocks
// =============================================================================

/// Row for `canonical_blocks` table (ReplacingMergeTree).
/// Higher `canon_version` wins on merge.
#[derive(Debug, Clone, Row, Serialize)]
pub struct CanonicalBlockRow {
    pub number: u64,
    pub block_hash: [u8; 32],
    pub canon_version: u64,
}

impl Default for CanonicalBlockRow {
    fn default() -> Self {
        Self {
            number: 0,
            block_hash: EMPTY_HASH,
            canon_version: 0,
        }
    }
}

// =============================================================================
// cell_state
// =============================================================================

/// Row for `cell_state` table (ReplacingMergeTree).
/// Tracks cell lifecycle: live, consumed, or removed by reorg.
#[derive(Debug, Clone, Row, Serialize)]
pub struct CellStateRow {
    pub tx_hash: [u8; 32],
    pub output_index: u16,
    pub canon_version: u64,
    pub is_present: u8,           // 1 = valid cell, 0 = removed by reorg
    pub is_live: u8,              // 1 = unspent, 0 = consumed
    pub consumed_by_tx: [u8; 32], // Empty if live
    pub consumed_at_block: u64,
    pub consumed_at_index: u16,
    pub capacity: u64,
    pub lock_script_hash: [u8; 32],
    pub type_script_hash: [u8; 32], // Empty if no type script
    pub lock_code_hash: [u8; 32],
    pub type_code_hash: [u8; 32], // Empty if no type script
    pub data_size: u32,
    pub created_at_block: u64,
}

impl Default for CellStateRow {
    fn default() -> Self {
        Self {
            tx_hash: EMPTY_HASH,
            output_index: 0,
            canon_version: 0,
            is_present: 1,
            is_live: 1,
            consumed_by_tx: EMPTY_HASH,
            consumed_at_block: 0,
            consumed_at_index: 0,
            capacity: 0,
            lock_script_hash: EMPTY_HASH,
            type_script_hash: EMPTY_HASH,
            lock_code_hash: EMPTY_HASH,
            type_code_hash: EMPTY_HASH,
            data_size: 0,
            created_at_block: 0,
        }
    }
}

impl CellStateRow {
    /// Create a new live cell state row for a newly created cell output.
    pub fn new_live(
        tx_hash: [u8; 32],
        output_index: u16,
        canon_version: u64,
        capacity: u64,
        lock_script_hash: [u8; 32],
        type_script_hash: [u8; 32],
        lock_code_hash: [u8; 32],
        type_code_hash: [u8; 32],
        data_size: u32,
        created_at_block: u64,
    ) -> Self {
        Self {
            tx_hash,
            output_index,
            canon_version,
            is_present: 1,
            is_live: 1,
            consumed_by_tx: EMPTY_HASH,
            consumed_at_block: 0,
            consumed_at_index: 0,
            capacity,
            lock_script_hash,
            type_script_hash,
            lock_code_hash,
            type_code_hash,
            data_size,
            created_at_block,
        }
    }

    /// Create a consumed cell state row when a cell is spent.
    pub fn new_consumed(
        tx_hash: [u8; 32],
        output_index: u16,
        canon_version: u64,
        consumed_by_tx: [u8; 32],
        consumed_at_block: u64,
        consumed_at_index: u16,
        capacity: u64,
        lock_script_hash: [u8; 32],
        type_script_hash: [u8; 32],
        lock_code_hash: [u8; 32],
        type_code_hash: [u8; 32],
        data_size: u32,
        created_at_block: u64,
    ) -> Self {
        Self {
            tx_hash,
            output_index,
            canon_version,
            is_present: 1,
            is_live: 0,
            consumed_by_tx,
            consumed_at_block,
            consumed_at_index,
            capacity,
            lock_script_hash,
            type_script_hash,
            lock_code_hash,
            type_code_hash,
            data_size,
            created_at_block,
        }
    }

    /// Create an invalidated cell state row for reorg disconnect.
    /// Marks a cell as no longer present (is_present=0) because its creating block was disconnected.
    pub fn new_invalidated(
        tx_hash: [u8; 32],
        output_index: u16,
        canon_version: u64,
        capacity: u64,
        lock_script_hash: [u8; 32],
        type_script_hash: [u8; 32],
        lock_code_hash: [u8; 32],
        type_code_hash: [u8; 32],
        data_size: u32,
        created_at_block: u64,
    ) -> Self {
        Self {
            tx_hash,
            output_index,
            canon_version,
            is_present: 0, // Cell no longer valid
            is_live: 0,    // Not live either
            consumed_by_tx: EMPTY_HASH,
            consumed_at_block: 0,
            consumed_at_index: 0,
            capacity,
            lock_script_hash,
            type_script_hash,
            lock_code_hash,
            type_code_hash,
            data_size,
            created_at_block,
        }
    }

    /// Create a restored cell state row for reorg disconnect.
    /// Restores a cell to live state (is_live=1, consumed_by_tx=empty) because its consuming block was disconnected.
    pub fn new_restored(
        tx_hash: [u8; 32],
        output_index: u16,
        canon_version: u64,
        capacity: u64,
        lock_script_hash: [u8; 32],
        type_script_hash: [u8; 32],
        lock_code_hash: [u8; 32],
        type_code_hash: [u8; 32],
        data_size: u32,
        created_at_block: u64,
    ) -> Self {
        Self {
            tx_hash,
            output_index,
            canon_version,
            is_present: 1, // Cell still exists
            is_live: 1,    // Restored to live state
            consumed_by_tx: EMPTY_HASH,
            consumed_at_block: 0,
            consumed_at_index: 0,
            capacity,
            lock_script_hash,
            type_script_hash,
            lock_code_hash,
            type_code_hash,
            data_size,
            created_at_block,
        }
    }
}

// =============================================================================
// Helper conversion functions
// =============================================================================

/// Convert a slice to a 32-byte array, padding with zeros if needed.
#[inline]
pub fn to_hash32(bytes: &[u8]) -> [u8; 32] {
    let mut arr = [0u8; 32];
    let len = bytes.len().min(32);
    arr[..len].copy_from_slice(&bytes[..len]);
    arr
}

/// Convert a slice to a 16-byte array, padding with zeros if needed.
#[inline]
pub fn to_nonce16(bytes: &[u8]) -> [u8; 16] {
    let mut arr = [0u8; 16];
    let len = bytes.len().min(16);
    arr[..len].copy_from_slice(&bytes[..len]);
    arr
}

/// Convert a u128 to a 32-byte UInt256 representation (little-endian, zero-padded).
#[inline]
pub fn u128_to_u256_bytes(val: u128) -> [u8; 32] {
    let mut arr = [0u8; 32];
    arr[..16].copy_from_slice(&val.to_le_bytes());
    arr
}

/// Convert a u64 to a 32-byte UInt256 representation (little-endian, zero-padded).
#[inline]
pub fn u64_to_u256_bytes(val: u64) -> [u8; 32] {
    let mut arr = [0u8; 32];
    arr[..8].copy_from_slice(&val.to_le_bytes());
    arr
}

/// Convert chrono DateTime to milliseconds since epoch.
#[inline]
pub fn datetime_to_millis(dt: chrono::DateTime<chrono::Utc>) -> i64 {
    dt.timestamp_millis()
}

/// Convert Unix timestamp (seconds) to milliseconds.
#[inline]
pub fn secs_to_millis(secs: u64) -> i64 {
    (secs as i64) * 1000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_hash32() {
        let short = [1u8, 2, 3];
        let result = to_hash32(&short);
        assert_eq!(result[0], 1);
        assert_eq!(result[1], 2);
        assert_eq!(result[2], 3);
        assert_eq!(result[3], 0);
        assert_eq!(result[31], 0);

        let exact: [u8; 32] = [0xab; 32];
        let result = to_hash32(&exact);
        assert_eq!(result, exact);
    }

    #[test]
    fn test_to_nonce16() {
        let short = [1u8, 2, 3];
        let result = to_nonce16(&short);
        assert_eq!(result[0], 1);
        assert_eq!(result[2], 3);
        assert_eq!(result[15], 0);
    }

    #[test]
    fn test_u128_to_u256_bytes() {
        let val = 0x123456789abcdef0_u128;
        let result = u128_to_u256_bytes(val);
        // Little-endian, so least significant byte first
        assert_eq!(result[0], 0xf0);
        assert_eq!(result[7], 0x12);
        // Upper 16 bytes should be zero
        assert_eq!(result[16], 0);
        assert_eq!(result[31], 0);
    }

    #[test]
    fn test_u64_to_u256_bytes() {
        let val = 0x0102030405060708_u64;
        let result = u64_to_u256_bytes(val);
        assert_eq!(result[0], 0x08);
        assert_eq!(result[7], 0x01);
        assert_eq!(result[8], 0);
    }
}
