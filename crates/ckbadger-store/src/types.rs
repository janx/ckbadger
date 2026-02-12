//! Value types for all column families.
//!
//! All types use `bincode` serialization for compact binary storage.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================
// Group A: Core cell data (ported from LiveCellStorage)
// ============================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveCellInfo {
    pub capacity: i64,
    pub created_at_block: i64,
    pub lock_script_hash: Vec<u8>,
    pub lock_code_hash: Vec<u8>,
    pub lock_hash_type: i16,
    pub lock_args: Vec<u8>,
    pub type_script_hash: Option<Vec<u8>>,
    pub type_code_hash: Option<Vec<u8>>,
    pub data_size: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactConsumedCellInfo {
    pub capacity: i64,
    pub created_at_block: i64,
    pub lock_script_hash: Vec<u8>,
    pub lock_code_hash: Vec<u8>,
    pub lock_hash_type: i16,
    pub lock_args: Vec<u8>,
    pub type_code_hash: Option<Vec<u8>>,
    pub data_size: i32,
}

impl CompactConsumedCellInfo {
    pub fn from_live_cell_info(info: &LiveCellInfo) -> Self {
        Self {
            capacity: info.capacity,
            created_at_block: info.created_at_block,
            lock_script_hash: info.lock_script_hash.clone(),
            lock_code_hash: info.lock_code_hash.clone(),
            lock_hash_type: info.lock_hash_type,
            lock_args: info.lock_args.clone(),
            type_code_hash: info.type_code_hash.clone(),
            data_size: info.data_size,
        }
    }

    pub fn to_live_cell_info(&self) -> LiveCellInfo {
        LiveCellInfo {
            capacity: self.capacity,
            created_at_block: self.created_at_block,
            lock_script_hash: self.lock_script_hash.clone(),
            lock_code_hash: self.lock_code_hash.clone(),
            lock_hash_type: self.lock_hash_type,
            lock_args: self.lock_args.clone(),
            type_script_hash: None,
            type_code_hash: self.type_code_hash.clone(),
            data_size: self.data_size,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedBlockHeader {
    pub hash: Vec<u8>,
    pub timestamp: i64,
    pub epoch_number: i64,
    pub epoch_index: i32,
    pub epoch_length: i32,
    pub dao: Vec<u8>,
    pub transactions_count: i32,
}

// ============================================
// Group B: Transaction indexes
// ============================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxIndexEntry {
    pub is_cellbase: bool,
    pub timestamp: i64,
    pub inputs_count: i16,
    pub outputs_count: i16,
    pub fee: i64,
    pub tx_size: i32,
    pub cycles: Option<i64>,
}

// ============================================
// Group C: Address data
// ============================================

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AddressBalance {
    pub balance: i128,
    pub live_cells_count: i32,
    pub total_cells_count: i64,
    pub txs_count: i64,
    pub first_seen_block: i64,
    pub first_seen_tx: Vec<u8>,
    pub last_activity_block: i64,
    pub last_activity_tx: Vec<u8>,
}

// ============================================
// Group D: Activities
// ============================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityEntry {
    pub activity_type: String,
    pub category: String,
    pub tx_hash: Vec<u8>,
    pub tx_idx: i32,
    pub from_lock: Option<Vec<u8>>,
    pub to_lock: Option<Vec<u8>>,
    pub amount: Option<i128>,
    pub asset_id: Option<Vec<u8>>,
    pub metadata: Option<serde_json::Value>,
    pub timestamp: i64,
}

// ============================================
// Group E: DAO
// ============================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DaoDepositCacheEntry {
    pub capacity: i64,
    pub deposit_block_number: i64,
    pub lock_script_hash: Vec<u8>,
    pub deposit_ar: i64,
    pub status: i16,
    pub withdraw_request_tx: Option<Vec<u8>>,
    pub withdraw_request_block: Option<i64>,
    pub withdraw_request_ar: Option<i64>,
    pub withdraw_block: Option<i64>,
    pub withdraw_tx: Option<Vec<u8>>,
    pub compensation: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DaoStats {
    pub total_deposited: i128,
    pub total_depositors: i64,
    pub total_compensation: i128,
    pub total_deposits: i64,
    pub total_withdrawals: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaoDailySnapshot {
    pub date: String,
    pub total_deposited: i128,
    pub depositors_count: i64,
    pub new_deposits: i64,
    pub withdrawals: i64,
    pub compensation: i128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecondaryIssuance {
    pub miner_reward: i64,
    pub dao_reward: i64,
    pub treasury: i64,
}

// ============================================
// Group F: Tokens & NFTs
// ============================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInfo {
    pub type_code_hash: Vec<u8>,
    pub hash_type: u8,
    pub type_args: Vec<u8>,
    pub standard: String,
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub decimals: Option<i32>,
    pub total_supply: Option<i128>,
    pub holders_count: i64,
    pub first_seen_block: i64,
    pub icon_url: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SporeEntry {
    pub cluster_id: Option<Vec<u8>>,
    pub content_type: Option<String>,
    pub content_length: Option<i64>,
    pub owner_lock_hash: Option<Vec<u8>>,
    pub is_live: bool,
    pub created_at_block: i64,
    pub created_at_tx: Vec<u8>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NftEntry {
    pub standard: String,
    pub collection_id: Option<Vec<u8>>,
    pub token_id: Option<Vec<u8>>,
    pub owner_lock_hash: Option<Vec<u8>>,
    pub name: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub is_live: bool,
    pub created_at_block: i64,
}

// ============================================
// Group G: Statistics
// ============================================

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DailyStats {
    pub blocks_count: i32,
    pub transactions_count: i32,
    pub cells_created: i32,
    pub cells_consumed: i32,
    pub capacity_transferred: i64,
    pub total_live_cells: i64,
    pub total_dead_cells: i64,
    pub total_all_cells: i64,
    pub total_data_size: i64,
    pub knowledge_size: Option<i128>,
    pub avg_block_time_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HourlyStats {
    pub hour: i64,
    pub blocks_count: i32,
    pub transactions_count: i32,
    pub cells_created: i32,
    pub cells_consumed: i32,
    pub capacity_transferred: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EpochStats {
    pub epoch_number: i64,
    pub start_block: i64,
    pub end_block: Option<i64>,
    pub blocks_count: i32,
    pub length: i32,
    pub start_timestamp: DateTime<Utc>,
    pub end_timestamp: Option<DateTime<Utc>>,
    pub transactions_count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MinerStats {
    pub miner_lock_hash: Vec<u8>,
    pub blocks_count: i32,
    pub last_block_number: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DailyBlockStats {
    pub avg_compact_target: f64,
    pub block_count: i32,
    pub total_uncles: i32,
    pub avg_block_time_ms: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScriptInfo {
    pub code_hash: Vec<u8>,
    pub hash_type: u8,
    pub name: Option<String>,
    pub category: Option<String>,
    pub website: Option<String>,
    pub description: Option<String>,
    pub cells_count: i64,
    pub capacity_used: i128,
    // Per-kind usage stats (lock vs type)
    pub lock_cells_count: i64,
    pub lock_live_cells_count: i64,
    pub lock_capacity_sum: i64,
    pub lock_live_capacity_sum: i64,
    pub type_cells_count: i64,
    pub type_live_cells_count: i64,
    pub type_capacity_sum: i64,
    pub type_live_capacity_sum: i64,
    /// type_script_hash of the deployment cell (from label data).
    /// Used to find the code cell for hash_type="data"/"data1"/"data2" scripts.
    #[serde(default)]
    pub dep_type_hash: Option<Vec<u8>>,
    /// data_hash of the deployment cell (from label data).
    /// Used as fallback when dep_type_hash is absent (e.g. genesis cells).
    #[serde(default)]
    pub dep_data_hash: Option<Vec<u8>>,
    /// Pre-resolved code cell outpoint (resolved during label import).
    /// Only populated for scripts where runtime lookup is expensive
    /// (data/data1/data2 without dep_type_hash).
    #[serde(default)]
    pub code_cell_tx_hash: Option<Vec<u8>>,
    #[serde(default)]
    pub code_cell_output_index: Option<u32>,
}

// ============================================
// Group H: System
// ============================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncStatus {
    pub tip_block_number: i64,
    pub tip_block_hash: Vec<u8>,
    pub total_transactions: i64,
    pub total_cells_created: i64,
    pub total_cells_consumed: i64,
    pub last_synced_at: i64,
    pub activities_deferred: bool,
    pub address_balances_deferred: bool,
    pub deep_fork_detected: bool,
    pub deep_fork_info: Option<DeepForkInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepForkInfo {
    pub db_tip: i64,
    pub db_tip_hash: Vec<u8>,
    pub chain_tip: i64,
    pub chain_tip_hash: Vec<u8>,
    pub depth: i32,
    pub fork_point: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskEntry {
    pub id: Uuid,
    pub task_type: String,
    pub status: String,
    pub priority: i32,
    /// JSON-encoded config string (bincode cannot round-trip serde_json::Value).
    pub config: String,
    pub progress_total: Option<i64>,
    pub progress_current: Option<i64>,
    pub progress_message: Option<String>,
    /// JSON-encoded result string.
    pub result: Option<String>,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub heartbeat_at: Option<DateTime<Utc>>,
    pub runner_id: Option<String>,
    pub retry_count: i32,
    pub max_retries: i32,
    /// JSON-encoded rate samples string.
    pub rate_samples: Option<String>,
    pub rate_ema: Option<f64>,
    pub log_tail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReorgEvent {
    pub detected_at: i64,
    pub rollback_from: i64,
    pub rollback_to: i64,
    pub depth: i32,
}

/// Memory/storage statistics for monitoring.
#[derive(Debug, Clone, Default)]
pub struct MemoryStats {
    pub cells_count: usize,
    pub memory_bytes: usize,
    pub memtable_bytes: usize,
    pub block_cache_bytes: usize,
    pub table_readers_bytes: usize,
    /// Estimated bytes pending compaction across all CFs
    pub compaction_pending_bytes: u64,
    /// Number of currently running compactions
    pub num_running_compactions: u64,
    /// Total SST file size on disk (all CFs)
    pub sst_files_size: u64,
    /// Total L0 files across all CFs (high values indicate compaction backlog / write stall risk)
    pub l0_files_count: u64,
    /// Top column families by estimated live data size: (name, bytes)
    pub top_cf_sizes: Vec<(String, u64)>,
}

impl MemoryStats {
    pub fn total_mb(&self) -> usize {
        self.memory_bytes / (1024 * 1024)
    }
}

/// Cursor for pagination over prefix iterators
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cursor {
    pub last_key: Vec<u8>,
}
