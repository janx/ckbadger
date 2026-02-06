//! Live cell storage traits and types.
//!
//! This module defines the abstraction layer for storing live (unspent) cells
//! during blockchain synchronization. The storage provides O(1) lookups for
//! resolving transaction inputs without querying the database.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────┐     ┌──────────────────┐
//! │  Block Parser   │────▶│ LiveCellStorage  │
//! └─────────────────┘     └──────────────────┘
//!         │                        │
//!         │ new cells              │ resolve inputs
//!         ▼                        ▼
//! ┌─────────────────┐     ┌──────────────────┐
//! │   insert()      │     │   get_batch()    │
//! └─────────────────┘     └──────────────────┘
//! ```
//!
//! # Implementation
//!
//! Uses RocksDB with multiple Column Families for:
//! - Live cells: O(1) lookup for unspent cells
//! - Consumed cells: Recently consumed cells for lookup (reduces DB queries)
//! - DAO cache: Block number -> DAO field (32 bytes)
//! - Block headers: Block number -> header info + hash index

use std::collections::HashMap;
use std::sync::Arc;

use sqlx::PgPool;

/// Metadata for a live (unspent) cell.
///
/// Contains essential information needed to resolve transaction inputs
/// and track cell lifecycle during synchronization.
///
/// # Serialization
///
/// Uses `bincode` for efficient binary serialization when stored in RocksDB.
/// Typical serialized size: ~150-200 bytes per cell.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LiveCellInfo {
    /// Cell capacity in shannons (1 CKB = 10^8 shannons).
    pub capacity: i64,
    /// Block number where this cell was created.
    pub created_at_block: i64,
    /// Blake2b hash of the lock script (32 bytes).
    pub lock_script_hash: Vec<u8>,
    /// Code hash from the lock script (32 bytes).
    pub lock_code_hash: Vec<u8>,
    /// Args from the lock script (variable length).
    pub lock_args: Vec<u8>,
    /// Blake2b hash of the type script, if present.
    pub type_script_hash: Option<Vec<u8>>,
    /// Code hash from the type script, if present.
    pub type_code_hash: Option<Vec<u8>>,
    /// Size of cell data in bytes.
    pub data_size: i32,
}

impl LiveCellInfo {
    pub fn memory_size(&self) -> usize {
        let fixed_fields = std::mem::size_of::<i64>() * 2 + std::mem::size_of::<i32>();
        let vec_overhead = 24;
        fixed_fields
            + vec_overhead
            + self.lock_script_hash.len()
            + vec_overhead
            + self.lock_code_hash.len()
            + vec_overhead
            + self.lock_args.len()
            + vec_overhead
            + self.type_script_hash.as_ref().map(|v| v.len()).unwrap_or(0)
            + vec_overhead
            + self.type_code_hash.as_ref().map(|v| v.len()).unwrap_or(0)
    }
}

/// Record of a consumed cell, kept for reorg rollback support.
#[derive(Debug, Clone)]
pub struct ConsumedCellRecord {
    pub tx_hash: Vec<u8>,
    pub output_index: i16,
    pub info: LiveCellInfo,
    pub consumed_at_block: i64,
}

/// Compact representation of consumed cell info for RocksDB storage.
///
/// Only stores fields that are actually queried:
/// - `get_cells_info_batch`: capacity, created_at_block, lock_script_hash, data_size
/// - `get_cells_code_hashes_batch`: lock_code_hash, type_code_hash
///
/// Omits unused fields to reduce memory by ~41%:
/// - lock_args (20-52 bytes) - never queried
/// - type_script_hash (32 bytes) - never queried
///
/// Size comparison per cell:
/// - LiveCellInfo: ~197 bytes
/// - CompactConsumedCellInfo: ~116 bytes
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompactConsumedCellInfo {
    /// Cell capacity in shannons.
    pub capacity: i64,
    /// Block number where this cell was created.
    pub created_at_block: i64,
    /// Blake2b hash of the lock script (32 bytes).
    pub lock_script_hash: Vec<u8>,
    /// Code hash from the lock script (32 bytes).
    pub lock_code_hash: Vec<u8>,
    /// Code hash from the type script, if present.
    pub type_code_hash: Option<Vec<u8>>,
    /// Size of cell data in bytes.
    pub data_size: i32,
}

impl CompactConsumedCellInfo {
    /// Create from full LiveCellInfo, dropping unused fields.
    pub fn from_live_cell_info(info: &LiveCellInfo) -> Self {
        Self {
            capacity: info.capacity,
            created_at_block: info.created_at_block,
            lock_script_hash: info.lock_script_hash.clone(),
            lock_code_hash: info.lock_code_hash.clone(),
            type_code_hash: info.type_code_hash.clone(),
            data_size: info.data_size,
        }
    }

    /// Convert to LiveCellInfo with dummy values for omitted fields.
    ///
    /// The dummy values (empty lock_args, None type_script_hash) are safe because
    /// these fields are never accessed when querying consumed cells.
    pub fn to_live_cell_info(&self) -> LiveCellInfo {
        LiveCellInfo {
            capacity: self.capacity,
            created_at_block: self.created_at_block,
            lock_script_hash: self.lock_script_hash.clone(),
            lock_code_hash: self.lock_code_hash.clone(),
            lock_args: Vec::new(),
            type_script_hash: None,
            type_code_hash: self.type_code_hash.clone(),
            data_size: self.data_size,
        }
    }
}

/// Memory/storage statistics for monitoring.
#[derive(Debug, Clone, Default)]
pub struct MemoryStats {
    pub cells_count: usize,
    pub memory_bytes: usize,
    pub memtable_bytes: usize,
    pub block_cache_bytes: usize,
    pub table_readers_bytes: usize,
    pub fragmentation_ratio: f64,
}

impl MemoryStats {
    pub fn total_mb(&self) -> usize {
        self.memory_bytes / (1024 * 1024)
    }
}

/// Synchronous operations for live cell storage.
///
/// Provides O(1) cell lookups by outpoint (tx_hash, output_index).
/// All methods are thread-safe.
pub trait LiveCellStorage: Send + Sync {
    fn insert(&self, tx_hash: Vec<u8>, output_index: i16, info: LiveCellInfo);
    fn get(&self, tx_hash: &[u8], output_index: i16) -> Option<LiveCellInfo>;
    fn remove(&self, tx_hash: &[u8], output_index: i16) -> Option<LiveCellInfo>;
    fn get_batch(&self, outpoints: &[(&[u8], i16)]) -> HashMap<(Vec<u8>, i16), LiveCellInfo>;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn clear(&self);
    fn record_consumption(
        &self,
        tx_hash: Vec<u8>,
        output_index: i16,
        info: LiveCellInfo,
        consumed_at_block: i64,
    );
    fn rollback_to_block(&self, rollback_to: i64) -> (usize, usize);
    fn cells_created_since(&self, block_number: i64) -> Vec<(Vec<u8>, i16, LiveCellInfo)>;
    fn memory_stats(&self) -> MemoryStats;
    fn backend_name(&self) -> &'static str;

    fn insert_block_header(&self, _block_number: i64, _header: CachedBlockHeader) {}
    fn get_block_header(&self, _block_number: i64) -> Option<CachedBlockHeader> {
        None
    }
    fn get_block_number_by_hash(&self, _hash: &[u8]) -> Option<i64> {
        None
    }
    fn get_dao_field(&self, _block_number: i64) -> Option<Vec<u8>> {
        None
    }
    fn get_dao_fields_batch(&self, _block_numbers: &[i64]) -> HashMap<i64, Vec<u8>> {
        HashMap::new()
    }
    fn get_consumed_cell(&self, _tx_hash: &[u8], _output_index: i16) -> Option<LiveCellInfo> {
        None
    }
    fn get_consumed_cells_batch(
        &self,
        _outpoints: &[(&[u8], i16)],
    ) -> HashMap<(Vec<u8>, i16), LiveCellInfo> {
        HashMap::new()
    }
    fn rollback_block_cache(&self, _rollback_to: i64) {}

    fn set_bulk_sync_mode(&self, _enabled: bool) {}
    fn is_bulk_sync_mode(&self) -> bool {
        false
    }
    fn cleanup_consumed_cells(&self) -> usize {
        0
    }
    fn consumed_cells_stats(&self) -> (usize, usize) {
        (0, 0)
    }

    fn block_headers_count(&self) -> usize {
        0
    }

    fn is_bulk_sync_cell_cache_enabled(&self) -> bool {
        false
    }
}

/// Async operations for database synchronization.
#[async_trait::async_trait]
pub trait LiveCellStorageAsync: LiveCellStorage {
    /// Flush pending changes to PostgreSQL `live_cells` table.
    /// Returns (inserts, deletes) count.
    async fn flush_to_db(&self, pool: &PgPool) -> anyhow::Result<(usize, usize)>;

    /// Rebuild storage from PostgreSQL (for in-memory backends).
    /// RocksDB backend skips this as data is already persisted.
    async fn rebuild_from_db(&self, pool: &PgPool) -> anyhow::Result<()>;
}

/// Type alias for dynamic dispatch of live cell storage.
pub type DynLiveCellStorage = Arc<dyn LiveCellStorageAsync>;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CachedBlockHeader {
    pub hash: Vec<u8>,
    pub timestamp: i64,
    pub epoch_number: i64,
    pub epoch_index: i32,
    pub epoch_length: i32,
    pub dao: Vec<u8>,
    pub transactions_count: i32,
}
