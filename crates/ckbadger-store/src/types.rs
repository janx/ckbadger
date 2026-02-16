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
    #[serde(default)]
    pub occupied_capacity: i64,
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
            occupied_capacity: 0,
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
    #[serde(default)]
    pub occupied_capacity: i128,
    pub live_cells_count: i32,
    pub total_cells_count: i64,
    pub txs_count: i64,
    pub first_seen_block: i64,
    pub first_seen_tx: Vec<u8>,
    pub last_activity_block: i64,
    pub last_activity_tx: Vec<u8>,
}

// ============================================
// Group D: DAO
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
    /// Cumulative gross deposit amount (sum of all deposit capacities, never
    /// decreased by withdrawals). Used to compute daily gross deposits via
    /// deltas between consecutive snapshots.
    #[serde(default)]
    pub cumulative_deposit_amount: i128,
    /// C field from DAO header: total CKB issuance up to this date (shannons).
    #[serde(default)]
    pub total_issuance: i128,
    /// S field from DAO header: cumulative non-miner secondary issuance (shannons).
    #[serde(default)]
    pub secondary_pool: i128,
    /// U field from DAO header: total occupied capacity (shannons).
    #[serde(default)]
    pub occupied_capacity: i128,
    /// Cumulative secondary issuance to miners (shannons).
    #[serde(default)]
    pub cum_miner_secondary: i128,
    /// Cumulative secondary issuance to DAO depositors (shannons).
    #[serde(default)]
    pub cum_dao_compensation: i128,
    /// Cumulative secondary issuance to treasury (shannons).
    #[serde(default)]
    pub cum_treasury: i128,
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
    #[serde(default)]
    pub transfers_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenTransferRecord {
    pub tx_hash: Vec<u8>,
    pub block_number: i64,
    pub from_lock_hash: Option<Vec<u8>>,
    pub to_lock_hash: Vec<u8>,
    pub amount: u128,
    pub is_mint: bool,
    pub is_burn: bool,
    pub timestamp: i64,
}

/// DOB (Digital Object) standard identifier.
///
/// DOB is an asset type on CKB. Each variant represents a specific standard
/// or entity type within the DOB ecosystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DobStandard {
    /// A Spore item (individual DOB).
    #[default]
    Spore,
    /// A Spore cluster (collection of Spores).
    SporeCluster,
    /// A did:ckb decentralized identity (single-collection DOB standard).
    DidCkb,
}

impl DobStandard {
    /// Wire-level name for logging/debugging.
    pub fn as_str(&self) -> &'static str {
        match self {
            DobStandard::Spore => "spore",
            DobStandard::SporeCluster => "spore_cluster",
            DobStandard::DidCkb => "did_ckb",
        }
    }

    /// Asset-level standard name for API grouping (collapses cluster → "spore").
    pub fn asset_standard(&self) -> &'static str {
        match self {
            DobStandard::Spore | DobStandard::SporeCluster => "spore",
            DobStandard::DidCkb => "did_ckb",
        }
    }

    /// Returns `true` for collection-level entries (clusters), `false` for items.
    pub fn is_cluster(&self) -> bool {
        matches!(self, DobStandard::SporeCluster)
    }
}

/// Standard-specific data for DOB entries, stored inline via bincode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DobExtra {
    /// Spore item: MIME content type and content byte length.
    Spore {
        content_type: String,
        content_length: i64,
    },
    /// Spore cluster: no extra fields (name/description live on `DobEntry`).
    SporeCluster,
    /// did:ckb identity: reserved for future fields.
    DidCkb,
}

/// A DOB (Digital Object) entry stored in the `spore_data` column family.
///
/// Covers all DOB standards: Spore, Spore clusters, did:ckb.
/// Standard-specific data lives in `extra`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DobEntry {
    pub standard: DobStandard,
    /// Parent collection. `Some(id)` = belongs to that cluster/collection.
    /// `None` = default collection for this standard (grouped by standard name in API).
    pub collection_id: Option<Vec<u8>>,
    pub owner_lock_hash: Option<Vec<u8>>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub is_live: bool,
    pub created_at_block: i64,
    pub created_at_tx: Vec<u8>,
    /// Standard-specific payload (bincode-serialized, no JSON).
    pub extra: DobExtra,
}

/// Type alias for backward compatibility during migration.
pub type SporeEntry = DobEntry;

/// NFT standard identifier.
///
/// NFT is an asset type on CKB, separate from DOB. Each variant represents
/// a specific standard or entity type within the NFT ecosystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum NftStandard {
    /// mNFT issuer (top-level entity that creates classes).
    MnftIssuer,
    /// mNFT class (a collection of mNFT tokens).
    #[default]
    MnftClass,
    /// mNFT token (individual NFT item).
    MnftToken,
    /// .bit (DotBit) domain name account. Single-collection standard:
    /// all .bit accounts belong to one implicit ".bit" collection.
    DotBit,
}

impl NftStandard {
    /// Wire-level name for logging/debugging.
    pub fn as_str(&self) -> &'static str {
        match self {
            NftStandard::MnftIssuer => "mnft_issuer",
            NftStandard::MnftClass => "mnft_class",
            NftStandard::MnftToken => "mnft",
            NftStandard::DotBit => "dotbit",
        }
    }

    /// Asset-level standard name for API grouping.
    pub fn asset_standard(&self) -> &'static str {
        match self {
            NftStandard::MnftIssuer | NftStandard::MnftClass | NftStandard::MnftToken => "m-nft",
            NftStandard::DotBit => "dotbit",
        }
    }
}

/// Standard-specific data for NFT entries, stored inline via bincode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NftExtra {
    /// mNFT issuer metadata.
    MnftIssuer {
        class_count: u32,
        set_count: u32,
        /// Raw issuer info bytes from on-chain data.
        info: Option<Vec<u8>>,
    },
    /// mNFT class (collection) metadata.
    MnftClass {
        description: Option<String>,
        renderer: Option<String>,
        total: u32,
        issued: u32,
        configure: u8,
    },
    /// mNFT token (individual item) metadata.
    MnftToken {
        token_index: u32,
        characteristic: Vec<u8>,
        configure: u8,
        state: u8,
    },
    /// .bit account metadata.
    DotBit {
        /// Account expiration timestamp (Unix epoch seconds).
        expired_at: Option<u64>,
    },
}

/// An NFT entry stored in the `nft_data` column family.
///
/// Covers all NFT standards: mNFT (issuer/class/token), .bit (DotBit).
/// Standard-specific data lives in `extra`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NftEntry {
    pub standard: NftStandard,
    /// Parent collection. mNFT tokens → class_id, mNFT classes → issuer_id.
    /// `None` = default collection for this standard (e.g. all .bit accounts).
    pub collection_id: Option<Vec<u8>>,
    pub token_id: Option<Vec<u8>>,
    pub owner_lock_hash: Option<Vec<u8>>,
    pub name: Option<String>,
    pub is_live: bool,
    pub created_at_block: i64,
    /// Standard-specific payload (bincode-serialized, no JSON).
    pub extra: NftExtra,
}

/// Pre-aggregated cluster (DOB collection) data, maintained inline by the indexer.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClusterAggregate {
    pub name: Option<String>,
    pub description: Option<String>,
    pub total_count: i64,
    pub live_count: i64,
    pub owner_count: i64,
}

/// Pre-aggregated NFT collection data, maintained inline by the indexer.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NftCollectionAggregate {
    pub name: Option<String>,
    pub standard: NftStandard,
    pub total_count: i64,
    pub live_count: i64,
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
// Group G2: HODL Wave
// ============================================

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DailyHodlWave {
    pub band_24h: i128,
    pub band_1d_1w: i128,
    pub band_1w_1m: i128,
    pub band_1m_3m: i128,
    pub band_3m_6m: i128,
    pub band_6m_1y: i128,
    pub band_1y_3y: i128,
    pub band_gt_3y: i128,
    pub holder_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HodlTrackerState {
    pub capacity_by_date: Vec<(String, i128)>,
    pub date_transitions: Vec<(i64, String)>,
    pub holder_count: i64,
    pub last_snapshot_date: Option<String>,
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
    pub address_balances_deferred: bool,
    /// When `true`, activity entries were skipped during bulk sync and need
    /// to be rebuilt via `rebuild_activities` before live sync begins.
    /// Set to `true` when bulk sync starts; cleared after the rebuild completes.
    #[serde(default)]
    pub activities_deferred: bool,
    pub deep_fork_detected: bool,
    pub deep_fork_info: Option<DeepForkInfo>,
    #[serde(default)]
    pub avg_block_time_rebuilt: bool,
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
    /// Total L0 files across all CFs (sum)
    pub l0_files_count: u64,
    /// Max L0 files in any single CF (the actual write stall trigger)
    pub l0_files_max: u64,
    /// Name of the CF with the most L0 files
    pub l0_worst_cf: String,
    /// Total immutable memtables across all CFs (waiting for flush)
    pub immutable_memtables: u64,
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

// ============================================
// Group I: Activities
// ============================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityEntry {
    pub tx_hash: Vec<u8>,
    pub block_number: i64,
    pub tx_index: i32,
    pub timestamp: i64,
    /// Net CKB change in shannons.
    pub ckb_delta: i128,
    /// Net occupied capacity change in shannons.
    pub occupied_delta: i64,
    pub is_cellbase: bool,
    pub asset_changes: Vec<AssetChange>,
    /// Lock hashes of other parties in this transaction.
    pub peers: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AssetChange {
    Token {
        type_script_hash: Vec<u8>,
        delta: i128,
        symbol: Option<String>,
        decimals: Option<u8>,
    },
    Dob {
        dob_id: Vec<u8>,
        standard: String,
        action: AssetAction,
    },
    Nft {
        nft_id: Vec<u8>,
        standard: String,
        action: AssetAction,
    },
    DaoDeposit {
        capacity: i64,
    },
    DaoWithdrawRequest {
        capacity: i64,
        deposit_block: i64,
    },
    DaoWithdrawComplete {
        capacity: i64,
        compensation: i64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AssetAction {
    Mint,
    Transfer,
    Burn,
}

// ============================================
// Group J: Address Daily Stats
// ============================================

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AddressDailyStats {
    /// Number of activity entries for this address on this day.
    pub activities: i32,
    /// Number of unique transactions for this address on this day.
    pub txs: i32,
    /// Net change in live cells (created - consumed) on this day.
    pub cells_delta: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- AddressDailyStats ----

    #[test]
    fn test_address_daily_stats_roundtrip() {
        let stats = AddressDailyStats {
            activities: 42,
            txs: 10,
            cells_delta: -3,
        };
        let bytes = bincode::serialize(&stats).unwrap();
        let decoded: AddressDailyStats = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.activities, 42);
        assert_eq!(decoded.txs, 10);
        assert_eq!(decoded.cells_delta, -3);
    }

    #[test]
    fn test_address_daily_stats_default() {
        let stats = AddressDailyStats::default();
        assert_eq!(stats.activities, 0);
        assert_eq!(stats.txs, 0);
        assert_eq!(stats.cells_delta, 0);
    }

    // ---- ActivityEntry ----

    #[test]
    fn test_activity_entry_roundtrip() {
        let entry = ActivityEntry {
            tx_hash: vec![0x01u8; 32],
            block_number: 12345,
            tx_index: 3,
            timestamp: 1_700_000_000,
            ckb_delta: -500_00000000,
            occupied_delta: 610_000_000_000,
            is_cellbase: false,
            asset_changes: vec![
                AssetChange::Token {
                    type_script_hash: vec![0xAA; 32],
                    delta: 1_000_000,
                    symbol: Some("SEAL".to_string()),
                    decimals: Some(8),
                },
                AssetChange::DaoDeposit {
                    capacity: 1_000_000_000_000,
                },
            ],
            peers: vec![vec![0xBB; 32], vec![0xCC; 32]],
        };
        let bytes = bincode::serialize(&entry).unwrap();
        let decoded: ActivityEntry = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.tx_hash, entry.tx_hash);
        assert_eq!(decoded.block_number, 12345);
        assert_eq!(decoded.ckb_delta, -500_00000000);
        assert_eq!(decoded.occupied_delta, 610_000_000_000);
        assert!(!decoded.is_cellbase);
        assert_eq!(decoded.asset_changes.len(), 2);
        assert_eq!(decoded.peers.len(), 2);
    }

    #[test]
    fn test_activity_entry_all_asset_change_variants() {
        let entry = ActivityEntry {
            tx_hash: vec![0x02u8; 32],
            block_number: 100,
            tx_index: 0,
            timestamp: 1_700_000_000,
            ckb_delta: 0,
            occupied_delta: 0,
            is_cellbase: true,
            asset_changes: vec![
                AssetChange::Token {
                    type_script_hash: vec![0xAA; 32],
                    delta: -999,
                    symbol: None,
                    decimals: None,
                },
                AssetChange::Dob {
                    dob_id: vec![0xBB; 32],
                    standard: "spore".to_string(),
                    action: AssetAction::Mint,
                },
                AssetChange::Nft {
                    nft_id: vec![0xCC; 20],
                    standard: "m-nft".to_string(),
                    action: AssetAction::Transfer,
                },
                AssetChange::DaoDeposit { capacity: 500 },
                AssetChange::DaoWithdrawRequest {
                    capacity: 600,
                    deposit_block: 50,
                },
                AssetChange::DaoWithdrawComplete {
                    capacity: 700,
                    compensation: 42,
                },
            ],
            peers: vec![],
        };
        let bytes = bincode::serialize(&entry).unwrap();
        let decoded: ActivityEntry = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.asset_changes.len(), 6);
        assert!(decoded.is_cellbase);
        assert!(decoded.peers.is_empty());

        // Verify each variant survived roundtrip
        match &decoded.asset_changes[1] {
            AssetChange::Dob {
                standard, action, ..
            } => {
                assert_eq!(standard, "spore");
                assert!(matches!(action, AssetAction::Mint));
            }
            _ => panic!("expected Dob variant"),
        }
        match &decoded.asset_changes[5] {
            AssetChange::DaoWithdrawComplete {
                capacity,
                compensation,
            } => {
                assert_eq!(*capacity, 700);
                assert_eq!(*compensation, 42);
            }
            _ => panic!("expected DaoWithdrawComplete variant"),
        }
    }

    #[test]
    fn test_activity_entry_empty_roundtrip() {
        let entry = ActivityEntry {
            tx_hash: vec![0x00; 32],
            block_number: 0,
            tx_index: 0,
            timestamp: 0,
            ckb_delta: 0,
            occupied_delta: 0,
            is_cellbase: false,
            asset_changes: vec![],
            peers: vec![],
        };
        let bytes = bincode::serialize(&entry).unwrap();
        let decoded: ActivityEntry = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.ckb_delta, 0);
        assert!(decoded.asset_changes.is_empty());
        assert!(decoded.peers.is_empty());
    }

    // ---- DobStandard ----

    #[test]
    fn test_dob_standard_as_str() {
        assert_eq!(DobStandard::Spore.as_str(), "spore");
        assert_eq!(DobStandard::SporeCluster.as_str(), "spore_cluster");
        assert_eq!(DobStandard::DidCkb.as_str(), "did_ckb");
    }

    #[test]
    fn test_dob_standard_asset_standard() {
        assert_eq!(DobStandard::Spore.asset_standard(), "spore");
        assert_eq!(DobStandard::SporeCluster.asset_standard(), "spore");
        assert_eq!(DobStandard::DidCkb.asset_standard(), "did_ckb");
    }

    #[test]
    fn test_dob_standard_is_cluster() {
        assert!(!DobStandard::Spore.is_cluster());
        assert!(DobStandard::SporeCluster.is_cluster());
        assert!(!DobStandard::DidCkb.is_cluster());
    }

    // ---- NftStandard ----

    #[test]
    fn test_nft_standard_as_str() {
        assert_eq!(NftStandard::MnftIssuer.as_str(), "mnft_issuer");
        assert_eq!(NftStandard::MnftClass.as_str(), "mnft_class");
        assert_eq!(NftStandard::MnftToken.as_str(), "mnft");
        assert_eq!(NftStandard::DotBit.as_str(), "dotbit");
    }

    #[test]
    fn test_nft_standard_asset_standard() {
        assert_eq!(NftStandard::MnftIssuer.asset_standard(), "m-nft");
        assert_eq!(NftStandard::MnftClass.asset_standard(), "m-nft");
        assert_eq!(NftStandard::MnftToken.asset_standard(), "m-nft");
        assert_eq!(NftStandard::DotBit.asset_standard(), "dotbit");
    }

    // ---- Bincode roundtrip: NftEntry variants ----

    #[test]
    fn test_nft_entry_mnft_issuer_roundtrip() {
        let entry = NftEntry {
            standard: NftStandard::MnftIssuer,
            collection_id: None,
            token_id: None,
            owner_lock_hash: Some(vec![0xAA; 32]),
            name: Some("Test Issuer".to_string()),
            is_live: true,
            created_at_block: 100,
            extra: NftExtra::MnftIssuer {
                class_count: 5,
                set_count: 2,
                info: Some(vec![0x01, 0x02]),
            },
        };
        let bytes = bincode::serialize(&entry).unwrap();
        let decoded: NftEntry = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.standard, NftStandard::MnftIssuer);
        assert_eq!(decoded.name.as_deref(), Some("Test Issuer"));
        match decoded.extra {
            NftExtra::MnftIssuer {
                class_count,
                set_count,
                info,
            } => {
                assert_eq!(class_count, 5);
                assert_eq!(set_count, 2);
                assert_eq!(info, Some(vec![0x01, 0x02]));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_nft_entry_mnft_class_roundtrip() {
        let entry = NftEntry {
            standard: NftStandard::MnftClass,
            collection_id: Some(vec![0xBB; 32]),
            token_id: None,
            owner_lock_hash: Some(vec![0xCC; 32]),
            name: Some("Test Class".to_string()),
            is_live: true,
            created_at_block: 200,
            extra: NftExtra::MnftClass {
                description: Some("desc".to_string()),
                renderer: None,
                total: 100,
                issued: 42,
                configure: 0xFF,
            },
        };
        let bytes = bincode::serialize(&entry).unwrap();
        let decoded: NftEntry = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.standard, NftStandard::MnftClass);
        match decoded.extra {
            NftExtra::MnftClass {
                total,
                issued,
                configure,
                ..
            } => {
                assert_eq!(total, 100);
                assert_eq!(issued, 42);
                assert_eq!(configure, 0xFF);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_nft_entry_mnft_token_roundtrip() {
        let entry = NftEntry {
            standard: NftStandard::MnftToken,
            collection_id: Some(vec![0x11; 32]),
            token_id: Some(vec![0x22; 32]),
            owner_lock_hash: Some(vec![0x33; 32]),
            name: None,
            is_live: true,
            created_at_block: 300,
            extra: NftExtra::MnftToken {
                token_index: 7,
                characteristic: vec![0xDE, 0xAD],
                configure: 0x01,
                state: 0x02,
            },
        };
        let bytes = bincode::serialize(&entry).unwrap();
        let decoded: NftEntry = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.standard, NftStandard::MnftToken);
        match decoded.extra {
            NftExtra::MnftToken {
                token_index,
                characteristic,
                configure,
                state,
            } => {
                assert_eq!(token_index, 7);
                assert_eq!(characteristic, vec![0xDE, 0xAD]);
                assert_eq!(configure, 0x01);
                assert_eq!(state, 0x02);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_nft_entry_dotbit_roundtrip() {
        let entry = NftEntry {
            standard: NftStandard::DotBit,
            collection_id: None,
            token_id: Some(vec![0x44; 20]),
            owner_lock_hash: Some(vec![0x55; 32]),
            name: Some("test.bit".to_string()),
            is_live: true,
            created_at_block: 400,
            extra: NftExtra::DotBit {
                expired_at: Some(1_700_000_000),
            },
        };
        let bytes = bincode::serialize(&entry).unwrap();
        let decoded: NftEntry = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.standard, NftStandard::DotBit);
        assert_eq!(decoded.name.as_deref(), Some("test.bit"));
        match decoded.extra {
            NftExtra::DotBit { expired_at } => {
                assert_eq!(expired_at, Some(1_700_000_000));
            }
            _ => panic!("wrong variant"),
        }
    }

    // ---- Bincode roundtrip: DobEntry variants ----

    #[test]
    fn test_dob_entry_spore_roundtrip() {
        let entry = DobEntry {
            standard: DobStandard::Spore,
            collection_id: Some(vec![0xAA; 32]),
            owner_lock_hash: Some(vec![0xBB; 32]),
            name: None,
            description: None,
            is_live: true,
            created_at_block: 500,
            created_at_tx: vec![0xCC; 32],
            extra: DobExtra::Spore {
                content_type: "image/png".to_string(),
                content_length: 4096,
            },
        };
        let bytes = bincode::serialize(&entry).unwrap();
        let decoded: DobEntry = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.standard, DobStandard::Spore);
        match decoded.extra {
            DobExtra::Spore {
                content_type,
                content_length,
            } => {
                assert_eq!(content_type, "image/png");
                assert_eq!(content_length, 4096);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_dob_entry_cluster_roundtrip() {
        let entry = DobEntry {
            standard: DobStandard::SporeCluster,
            collection_id: None,
            owner_lock_hash: Some(vec![0xDD; 32]),
            name: Some("My Cluster".to_string()),
            description: Some("A test cluster".to_string()),
            is_live: true,
            created_at_block: 600,
            created_at_tx: vec![0xEE; 32],
            extra: DobExtra::SporeCluster,
        };
        let bytes = bincode::serialize(&entry).unwrap();
        let decoded: DobEntry = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.standard, DobStandard::SporeCluster);
        assert_eq!(decoded.name.as_deref(), Some("My Cluster"));
        assert_eq!(decoded.description.as_deref(), Some("A test cluster"));
        assert!(matches!(decoded.extra, DobExtra::SporeCluster));
    }

    #[test]
    fn test_dob_entry_did_ckb_roundtrip() {
        let entry = DobEntry {
            standard: DobStandard::DidCkb,
            collection_id: None,
            owner_lock_hash: Some(vec![0xFF; 32]),
            name: Some("did:ckb:test".to_string()),
            description: None,
            is_live: true,
            created_at_block: 700,
            created_at_tx: vec![0x11; 32],
            extra: DobExtra::DidCkb,
        };
        let bytes = bincode::serialize(&entry).unwrap();
        let decoded: DobEntry = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.standard, DobStandard::DidCkb);
        assert_eq!(decoded.name.as_deref(), Some("did:ckb:test"));
        assert!(matches!(decoded.extra, DobExtra::DidCkb));
    }

    // ---- AddressBalance ----

    #[test]
    fn test_address_balance_roundtrip() {
        let entry = AddressBalance {
            balance: 100_000_000_000,
            occupied_capacity: 610_000_000_000,
            live_cells_count: 3,
            total_cells_count: 10,
            txs_count: 7,
            first_seen_block: 100,
            first_seen_tx: vec![0x01; 32],
            last_activity_block: 500,
            last_activity_tx: vec![0x02; 32],
        };
        let bytes = bincode::serialize(&entry).unwrap();
        let decoded: AddressBalance = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.balance, 100_000_000_000);
        assert_eq!(decoded.occupied_capacity, 610_000_000_000);
        assert_eq!(decoded.live_cells_count, 3);
        assert_eq!(decoded.total_cells_count, 10);
        assert_eq!(decoded.txs_count, 7);
        assert_eq!(decoded.first_seen_block, 100);
        assert_eq!(decoded.last_activity_block, 500);
    }

    #[test]
    fn test_address_balance_default() {
        let bal = AddressBalance::default();
        assert_eq!(bal.balance, 0);
        assert_eq!(bal.occupied_capacity, 0);
        assert_eq!(bal.live_cells_count, 0);
        assert_eq!(bal.txs_count, 0);
    }
}
