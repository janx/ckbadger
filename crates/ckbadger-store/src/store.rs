//! Core RocksDB store with dual-DB architecture.
//!
//! Two physical RocksDB instances:
//! - **default**: Mutable state (indices, aggregates, sync meta). Rolled back on reorg.
//! - **append**: Immutable history (cells, tx_meta, block_meta, activities). Never deleted.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tracing::{info, warn};

use rocksdb::{
    ColumnFamily, ColumnFamilyDescriptor, DBCompactionStyle, DBCompressionType, IteratorMode,
    Options, UniversalCompactOptions, WriteBatch, WriteBufferManager, DB,
};

use crate::types::MemoryStats;

/// Type alias for RocksDB iterator items to avoid complex type lint.
pub type KvResult = Result<(Box<[u8]>, Box<[u8]>), rocksdb::Error>;

// ============================================================
// Column family name constants — DEFAULT store (mutable state)
// ============================================================

/// Existence index for live cells. Key: outpoint(34B), Value: empty.
pub const CF_LIVE_CELLS: &str = "live_cells";
/// Live cell index by lock script hash. Key: lock_hash(32B) + block_num(8B) + outpoint(34B).
pub const CF_LIVE_CELLS_BY_LOCK: &str = "live_cells_by_lock";
/// Live cell index by type script hash.
pub const CF_LIVE_CELLS_BY_TYPE: &str = "live_cells_by_type";
/// Live cell index by lock code_hash.
pub const CF_LIVE_CELLS_BY_LOCK_CODE: &str = "live_cells_by_lock_code";
/// Live cell index by type code_hash.
pub const CF_LIVE_CELLS_BY_TYPE_CODE: &str = "live_cells_by_type_code";
/// Consumption metadata. Key: outpoint(34B), Value: consumed_at_block(8B) + consumed_by_tx(32B).
pub const CF_CONSUMED_CELLS: &str = "consumed_cells";
/// Block number → tx_hash mapping. Key: block_num(8B) + tx_idx(4B), Value: tx_hash(32B).
pub const CF_TX_INDEX: &str = "tx_index";
/// Block number → block_hash mapping. Key: block_num(8B), Value: block_hash(32B).
pub const CF_BLOCK_INDEX: &str = "block_index";
/// Asset metadata (FT and NFT collections). Key: script_hash(32B).
pub const CF_ASSET_META: &str = "asset_meta";
/// Unified NFT item metadata. Key: nft_type(1B) + nft_id(32B).
pub const CF_NFT_ITEM_META: &str = "nft_item_meta";
/// Outpoint → NFT identity reverse index. Key: outpoint(34B), Value: nft_type(1B) + nft_id(32B).
pub const CF_NFT_OUTPOINTS: &str = "nft_outpoints";
/// NFT items by collection. Key: nft_type(1B) + collection_id(32B) + nft_id(32B).
pub const CF_NFT_ITEM_BY_COLLECTION: &str = "nft_item_by_collection";
/// Outpoint → FT identity reverse index. Key: outpoint(34B), Value: ft_type(1B) + script_hash(32B).
pub const CF_FT_OUTPOINTS: &str = "ft_outpoints";
/// DAO deposit tracking. Key: deposit outpoint(34B).
pub const CF_DAO_DEPOSITS: &str = "dao_deposits";
/// DAO withdraw tx → deposit outpoint reverse index.
pub const CF_DAO_WITHDRAW_INDEX: &str = "dao_withdraw_index";
/// Per-block secondary issuance breakdown. Key: block_num(8B).
pub const CF_BLOCK_ISSUANCE: &str = "block_issuance";
/// Address aggregate stats. Key: lock_hash(32B). Threshold-based materialization.
pub const CF_ADDR_STATS: &str = "addr_stats";
/// FT aggregate stats. Key: script_hash(32B). Threshold-based materialization.
pub const CF_FT_STATS: &str = "ft_stats";
/// FT holder balances (hot tokens only). Key: script_hash(32B) + lock_hash(32B), Value: amount(16B).
pub const CF_FT_HOLDERS: &str = "ft_holders";
/// NFT collection aggregate stats. Key: nft_type(1B) + collection_id(32B).
pub const CF_NFT_COLLECTION_STATS: &str = "nft_collection_stats";
/// Address transaction history. Key: lock_hash(32B) + block_num(8B) + tx_idx(4B).
pub const CF_ADDR_TXS: &str = "addr_txs";
/// Address → activity index. Key: lock_hash(32B) + inverted_activity_id(14B).
pub const CF_ADDR_ACTIVITIES: &str = "addr_activities";
/// NFT collection → activity index. Key: nft_type(1B) + collection_id(32B) + inverted_activity_id(14B).
pub const CF_NFT_COLLECTION_ACTIVITIES: &str = "nft_collection_activities";
/// FT → activity index. Key: ft_type(1B) + script_hash(32B) + inverted_activity_id(14B).
pub const CF_FT_ACTIVITIES: &str = "ft_activities";
/// Multi-purpose stats with prefix-multiplexed sub-namespaces.
pub const CF_STATS: &str = "stats";

// ============================================================
// Column family name constants — APPEND store (immutable history)
// ============================================================

/// Full cell data (SSOT). Key: outpoint(34B), Value: CellInfo (bincode).
pub const CF_CELLS: &str = "cells";
/// Transaction metadata. Key: tx_hash(32B), Value: TxMeta (bincode).
pub const CF_TX_META: &str = "tx_meta";
/// Block metadata. Key: block_hash(32B), Value: BlockMeta (bincode).
pub const CF_BLOCK_META: &str = "block_meta";
/// NFT item historical outpoints. Key: nft_type(1B) + nft_id(32B) + outpoint(34B).
pub const CF_NFT_ITEM_INDEX: &str = "nft_item_index";
/// FT historical outpoints. Key: ft_type(1B) + script_hash(32B) + outpoint(34B).
pub const CF_FT_INDEX: &str = "ft_index";
/// Global activity records. Key: block_num(8B) + tx_idx(4B) + seq(2B).
pub const CF_ACTIVITIES: &str = "activities";

// ============================================================
// Backward-compatible CF aliases (deprecated, for migration)
// ============================================================

// TODO(data-refactor): Use CF_LIVE_CELLS_BY_LOCK
pub const CF_CELL_BY_LOCK: &str = "live_cells_by_lock";
// TODO(data-refactor): Use CF_LIVE_CELLS_BY_TYPE
pub const CF_CELL_BY_TYPE: &str = "live_cells_by_type";
// TODO(data-refactor): Use CF_LIVE_CELLS_BY_LOCK_CODE
pub const CF_CELL_BY_LOCK_CODE: &str = "live_cells_by_lock_code";
// TODO(data-refactor): Use CF_LIVE_CELLS_BY_TYPE_CODE
pub const CF_CELL_BY_TYPE_CODE: &str = "live_cells_by_type_code";
// TODO(data-refactor): Use CF_ADDR_STATS
pub const CF_ADDR_BALANCE: &str = "addr_stats";
// TODO(data-refactor): Use CF_DAO_WITHDRAW_INDEX
pub const CF_DAO_BY_WITHDRAW_TX: &str = "dao_withdraw_index";
// TODO(data-refactor): Use CF_ASSET_META
pub const CF_TOKENS: &str = "asset_meta";
// TODO(data-refactor): Use CF_FT_HOLDERS
pub const CF_TOKEN_HOLDERS: &str = "ft_holders";
// Legacy CFs that still physically exist in the DEFAULT store during migration.
// These have DIFFERENT names from the new append-store CFs to avoid collision.
// They will be removed once ops files are fully rewritten (Phase 1e).
// TODO(data-refactor): Use CF_BLOCK_META (append store) + CF_BLOCK_INDEX
pub const CF_BLOCK_HEADERS: &str = "block_headers";
// TODO(data-refactor): Use CF_BLOCK_INDEX
pub const CF_BLOCK_HASH_INDEX: &str = "block_hash_index";
// TODO(data-refactor): Use CF_TX_META (append store) + CF_TX_INDEX
pub const CF_TX_HASH_MAP: &str = "tx_hash_map";
// TODO(data-refactor): Use CF_NFT_ITEM_META
pub const CF_SPORE_DATA: &str = "spore_data";
// TODO(data-refactor): Use CF_NFT_ITEM_BY_COLLECTION
pub const CF_SPORE_BY_CLUSTER: &str = "spore_by_cluster";
// TODO(data-refactor): Use CF_NFT_ITEM_META
pub const CF_NFT_DATA: &str = "nft_data";
// TODO(data-refactor): Use CF_NFT_COLLECTION_STATS
pub const CF_CLUSTER_AGG: &str = "cluster_agg";
// TODO(data-refactor): Use CF_NFT_COLLECTION_STATS
pub const CF_NFT_COLLECTION_AGG: &str = "nft_collection_agg";
// TODO(data-refactor): Merged into CF_STATS with prefix 0x21
pub const CF_SCRIPT_INFO: &str = "script_info";
// TODO(data-refactor): Merged into CF_STATS with prefix 0xF0
pub const CF_SYNC_META: &str = "sync_meta";
// TODO(data-refactor): Use CF_FT_ACTIVITIES
pub const CF_TOKEN_TRANSFERS: &str = "token_transfers";
// TODO(data-refactor): Merged into CF_STATS with prefix 0x20
pub const CF_ADDR_DAILY_STATS: &str = "addr_daily_stats";

// ============================================================
// CF arrays
// ============================================================

/// All column families in the DEFAULT (mutable) store.
pub const DEFAULT_CFS: &[&str] = &[
    // New canonical CFs (25)
    CF_LIVE_CELLS,
    CF_LIVE_CELLS_BY_LOCK,
    CF_LIVE_CELLS_BY_TYPE,
    CF_LIVE_CELLS_BY_LOCK_CODE,
    CF_LIVE_CELLS_BY_TYPE_CODE,
    CF_CONSUMED_CELLS,
    CF_TX_INDEX,
    CF_BLOCK_INDEX,
    CF_ASSET_META,
    CF_NFT_ITEM_META,
    CF_NFT_OUTPOINTS,
    CF_NFT_ITEM_BY_COLLECTION,
    CF_FT_OUTPOINTS,
    CF_DAO_DEPOSITS,
    CF_DAO_WITHDRAW_INDEX,
    CF_BLOCK_ISSUANCE,
    CF_ADDR_STATS,
    CF_FT_STATS,
    CF_FT_HOLDERS,
    CF_NFT_COLLECTION_STATS,
    CF_ADDR_TXS,
    CF_ADDR_ACTIVITIES,
    CF_NFT_COLLECTION_ACTIVITIES,
    CF_FT_ACTIVITIES,
    CF_STATS,
    // Legacy CFs (kept during migration, removed in Phase 1e)
    CF_BLOCK_HEADERS,
    CF_BLOCK_HASH_INDEX,
    CF_TX_HASH_MAP,
    CF_SPORE_DATA,
    CF_SPORE_BY_CLUSTER,
    CF_NFT_DATA,
    CF_CLUSTER_AGG,
    CF_NFT_COLLECTION_AGG,
    CF_SCRIPT_INFO,
    CF_SYNC_META,
    CF_TOKEN_TRANSFERS,
    CF_ADDR_DAILY_STATS,
];

/// All column families in the APPEND (immutable) store.
pub const APPEND_CFS: &[&str] = &[
    CF_CELLS,
    CF_TX_META,
    CF_BLOCK_META,
    CF_NFT_ITEM_INDEX,
    CF_FT_INDEX,
    CF_ACTIVITIES,
];

/// Combined list of all unique CFs across both stores.
/// Includes both new canonical CFs and legacy CFs kept during migration.
pub const ALL_CFS: &[&str] = &[
    // Default store — new canonical (25)
    CF_LIVE_CELLS,
    CF_LIVE_CELLS_BY_LOCK,
    CF_LIVE_CELLS_BY_TYPE,
    CF_LIVE_CELLS_BY_LOCK_CODE,
    CF_LIVE_CELLS_BY_TYPE_CODE,
    CF_CONSUMED_CELLS,
    CF_TX_INDEX,
    CF_BLOCK_INDEX,
    CF_ASSET_META,
    CF_NFT_ITEM_META,
    CF_NFT_OUTPOINTS,
    CF_NFT_ITEM_BY_COLLECTION,
    CF_FT_OUTPOINTS,
    CF_DAO_DEPOSITS,
    CF_DAO_WITHDRAW_INDEX,
    CF_BLOCK_ISSUANCE,
    CF_ADDR_STATS,
    CF_FT_STATS,
    CF_FT_HOLDERS,
    CF_NFT_COLLECTION_STATS,
    CF_ADDR_TXS,
    CF_ADDR_ACTIVITIES,
    CF_NFT_COLLECTION_ACTIVITIES,
    CF_FT_ACTIVITIES,
    CF_STATS,
    // Default store — legacy (12, removed in Phase 1e)
    CF_BLOCK_HEADERS,
    CF_BLOCK_HASH_INDEX,
    CF_TX_HASH_MAP,
    CF_SPORE_DATA,
    CF_SPORE_BY_CLUSTER,
    CF_NFT_DATA,
    CF_CLUSTER_AGG,
    CF_NFT_COLLECTION_AGG,
    CF_SCRIPT_INFO,
    CF_SYNC_META,
    CF_TOKEN_TRANSFERS,
    CF_ADDR_DAILY_STATS,
    // Append store (6)
    CF_CELLS,
    CF_TX_META,
    CF_BLOCK_META,
    CF_NFT_ITEM_INDEX,
    CF_FT_INDEX,
    CF_ACTIVITIES,
];

fn consumed_cf_storage_bytes(
    live_data_bytes: Option<u64>,
    sst_files_bytes: Option<u64>,
    memtable_bytes: Option<u64>,
) -> (usize, &'static str) {
    let (base, source) = match live_data_bytes {
        Some(v) if v > 0 => (v, "live"),
        _ => match sst_files_bytes {
            Some(v) if v > 0 => (v, "sst"),
            _ => match memtable_bytes {
                Some(v) if v > 0 => (0, "mem"),
                _ => (0, "none"),
            },
        },
    };
    let total = base.saturating_add(memtable_bytes.unwrap_or(0));
    (usize::try_from(total).unwrap_or(usize::MAX), source)
}

/// Dual-RocksDB store: default (mutable) + append (immutable history).
pub struct CkbadgerStore {
    /// Mutable state: indices, aggregates, sync meta.
    default_db: DB,
    /// Immutable history: cells, tx_meta, block_meta, activities.
    append_db: DB,
    /// Keep block cache alive for the lifetime of the store.
    block_cache: Mutex<rocksdb::Cache>,
    /// Global memtable memory budget.
    write_buffer_manager: WriteBufferManager,
    bulk_sync_mode: AtomicBool,
    is_secondary: bool,
    /// Path to the default store (for reference).
    default_path: PathBuf,
    /// Path to the append store (for reference).
    append_path: PathBuf,
}

impl CkbadgerStore {
    /// Open as primary (read-write) with a single base path.
    /// Append store is automatically placed at `{path}-append`.
    pub fn open<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let append_path = format!("{}-append", path.as_ref().display());
        Self::open_split(path.as_ref(), append_path.as_ref())
    }

    /// Open as primary (read-write) with explicit paths for both stores.
    pub fn open_split<P: AsRef<Path>>(default_path: P, append_path: P) -> anyhow::Result<Self> {
        let (opts, block_cache, write_buffer_manager) = Self::default_options();

        let default_db = Self::open_db_primary(&opts, &default_path, DEFAULT_CFS, &block_cache)?;
        let append_db = Self::open_db_primary(&opts, &append_path, APPEND_CFS, &block_cache)?;

        Ok(Self {
            default_db,
            append_db,
            block_cache: Mutex::new(block_cache),
            write_buffer_manager,
            bulk_sync_mode: AtomicBool::new(false),
            is_secondary: false,
            default_path: default_path.as_ref().to_path_buf(),
            append_path: append_path.as_ref().to_path_buf(),
        })
    }

    /// Open as secondary instance (read-only) with base paths.
    /// Append store primary is at `{primary_path}-append`, secondary at `{secondary_path}-append`.
    pub fn open_secondary<P: AsRef<Path>>(
        primary_path: P,
        secondary_path: P,
    ) -> anyhow::Result<Self> {
        let append_primary = format!("{}-append", primary_path.as_ref().display());
        let append_secondary = format!("{}-append", secondary_path.as_ref().display());
        Self::open_secondary_split(
            primary_path.as_ref(),
            secondary_path.as_ref(),
            append_primary.as_ref(),
            append_secondary.as_ref(),
        )
    }

    /// Open as secondary instance (read-only) with explicit paths for all stores.
    pub fn open_secondary_split<P: AsRef<Path>>(
        default_primary: P,
        default_secondary: P,
        append_primary: P,
        append_secondary: P,
    ) -> anyhow::Result<Self> {
        let (opts, block_cache, write_buffer_manager) = Self::default_options();

        let default_db =
            Self::open_db_secondary(&opts, &default_primary, &default_secondary, DEFAULT_CFS)?;
        let append_db =
            Self::open_db_secondary(&opts, &append_primary, &append_secondary, APPEND_CFS)?;

        Ok(Self {
            default_db,
            append_db,
            block_cache: Mutex::new(block_cache),
            write_buffer_manager,
            bulk_sync_mode: AtomicBool::new(false),
            is_secondary: true,
            default_path: default_primary.as_ref().to_path_buf(),
            append_path: append_primary.as_ref().to_path_buf(),
        })
    }

    fn open_db_primary<P: AsRef<Path>>(
        opts: &Options,
        path: P,
        cf_names: &[&str],
        block_cache: &rocksdb::Cache,
    ) -> anyhow::Result<DB> {
        let existing_cfs = DB::list_cf(opts, &path).unwrap_or_default();
        let mut all_names: Vec<String> = cf_names.iter().map(|s| s.to_string()).collect();
        for cf in &existing_cfs {
            if cf != "default" && !cf_names.contains(&cf.as_str()) {
                warn!(cf = cf.as_str(), "Opening legacy column family from disk");
                all_names.push(cf.clone());
            }
        }

        let descriptors: Vec<ColumnFamilyDescriptor> = all_names
            .iter()
            .map(|name| {
                ColumnFamilyDescriptor::new(
                    name.as_str(),
                    Self::cf_options(name.as_str(), block_cache),
                )
            })
            .collect();

        Ok(DB::open_cf_descriptors(opts, path, descriptors)?)
    }

    fn open_db_secondary<P: AsRef<Path>>(
        opts: &Options,
        primary_path: P,
        secondary_path: P,
        cf_names: &[&str],
    ) -> anyhow::Result<DB> {
        let existing_cfs = DB::list_cf(opts, &primary_path).unwrap_or_default();
        let mut all_names: Vec<String> = cf_names.iter().map(|s| s.to_string()).collect();
        for cf in &existing_cfs {
            if cf != "default" && !cf_names.contains(&cf.as_str()) {
                all_names.push(cf.clone());
            }
        }
        let refs: Vec<&str> = all_names.iter().map(|s| s.as_str()).collect();
        Ok(DB::open_cf_as_secondary(
            opts,
            primary_path,
            secondary_path,
            refs,
        )?)
    }

    /// Catch up with primary instance writes (secondary only).
    pub fn refresh(&self) -> anyhow::Result<()> {
        if self.is_secondary {
            self.default_db.try_catch_up_with_primary()?;
            self.append_db.try_catch_up_with_primary()?;
        }
        Ok(())
    }

    // ============================================================
    // Write tier classification
    // ============================================================

    /// Mega-write CFs with the heaviest per-batch write volume.
    const MEGA_WRITE_CFS: &'static [&'static str] = &[
        CF_CELLS,
        CF_LIVE_CELLS,
        CF_CONSUMED_CELLS,
        CF_LIVE_CELLS_BY_LOCK,
        CF_LIVE_CELLS_BY_TYPE,
        CF_LIVE_CELLS_BY_LOCK_CODE,
        CF_LIVE_CELLS_BY_TYPE_CODE,
        CF_TX_META,
        CF_TX_INDEX,
        CF_ADDR_STATS,
        CF_ADDR_TXS,
        CF_ACTIVITIES,
    ];

    const HIGH_WRITE_CFS: &'static [&'static str] = &[
        CF_CELLS,
        CF_LIVE_CELLS,
        CF_CONSUMED_CELLS,
        CF_LIVE_CELLS_BY_LOCK,
        CF_LIVE_CELLS_BY_TYPE,
        CF_TX_META,
        CF_TX_INDEX,
        CF_BLOCK_META,
        CF_BLOCK_INDEX,
        CF_ADDR_STATS,
        CF_ADDR_TXS,
        CF_DAO_DEPOSITS,
        CF_ACTIVITIES,
        CF_ADDR_ACTIVITIES,
    ];

    /// Historical append-heavy CFs — universal compaction.
    const HISTORICAL_APPEND_CFS: &'static [&'static str] = &[
        CF_CELLS,
        CF_TX_META,
        CF_BLOCK_META,
        CF_ACTIVITIES,
        CF_NFT_ITEM_INDEX,
        CF_FT_INDEX,
        CF_ADDR_TXS,
        CF_ADDR_ACTIVITIES,
        CF_NFT_COLLECTION_ACTIVITIES,
        CF_FT_ACTIVITIES,
    ];

    fn is_mega_write_cf(name: &str) -> bool {
        Self::MEGA_WRITE_CFS.contains(&name)
    }

    fn is_high_write_cf(name: &str) -> bool {
        Self::HIGH_WRITE_CFS.contains(&name)
    }

    fn is_historical_append_cf(name: &str) -> bool {
        Self::HISTORICAL_APPEND_CFS.contains(&name)
    }

    fn default_block_options(block_cache: &rocksdb::Cache) -> rocksdb::BlockBasedOptions {
        let mut block_opts = rocksdb::BlockBasedOptions::default();
        block_opts.set_block_size(16 * 1024);
        block_opts.set_block_cache(block_cache);
        block_opts.set_cache_index_and_filter_blocks(true);
        block_opts.set_bloom_filter(10.0, false);
        block_opts
    }

    const WBM_NORMAL_BYTES: usize = 8 * 1024 * 1024 * 1024;
    const WBM_BULK_SYNC_BYTES: usize = 16 * 1024 * 1024 * 1024;
    const BLOCK_CACHE_NORMAL_BYTES: usize = 8 * 1024 * 1024 * 1024;
    const BLOCK_CACHE_BULK_SYNC_BYTES: usize = 2 * 1024 * 1024 * 1024;

    fn default_options() -> (Options, rocksdb::Cache, WriteBufferManager) {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);

        let direct_io_reads = std::env::var("CKBADGER_DIRECT_IO_READS")
            .map(|v| v != "0" && v.to_lowercase() != "false")
            .unwrap_or(true);
        opts.set_use_direct_reads(direct_io_reads);
        opts.set_bytes_per_sync(4 * 1024 * 1024);
        opts.set_write_buffer_size(128 * 1024 * 1024);
        opts.set_max_write_buffer_number(4);
        opts.set_level_zero_file_num_compaction_trigger(4);
        opts.set_level_zero_slowdown_writes_trigger(20);
        opts.set_level_zero_stop_writes_trigger(48);
        opts.set_max_bytes_for_level_base(512 * 1024 * 1024);
        opts.set_compression_type(DBCompressionType::Lz4);
        opts.set_max_background_jobs(24);
        opts.set_max_subcompactions(8);
        opts.set_use_direct_io_for_flush_and_compaction(true);
        opts.set_atomic_flush(true);
        opts.set_unordered_write(true);

        let block_cache = rocksdb::Cache::new_lru_cache(Self::BLOCK_CACHE_NORMAL_BYTES);
        let block_opts = Self::default_block_options(&block_cache);
        opts.set_block_based_table_factory(&block_opts);

        let write_buffer_manager =
            WriteBufferManager::new_write_buffer_manager(Self::WBM_NORMAL_BYTES, true);
        opts.set_write_buffer_manager(&write_buffer_manager);

        (opts, block_cache, write_buffer_manager)
    }

    fn cf_options(name: &str, block_cache: &rocksdb::Cache) -> Options {
        let mut opts = Options::default();

        if Self::is_mega_write_cf(name) {
            opts.set_write_buffer_size(256 * 1024 * 1024);
            opts.set_max_write_buffer_number(4);
        } else if Self::is_high_write_cf(name) {
            opts.set_write_buffer_size(128 * 1024 * 1024);
            opts.set_max_write_buffer_number(4);
        } else {
            opts.set_write_buffer_size(32 * 1024 * 1024);
            opts.set_max_write_buffer_number(2);
        }

        opts.set_level_zero_file_num_compaction_trigger(4);
        opts.set_level_zero_slowdown_writes_trigger(12);
        opts.set_level_zero_stop_writes_trigger(24);
        opts.set_max_bytes_for_level_base(512 * 1024 * 1024);
        opts.set_compression_type(DBCompressionType::Lz4);

        if Self::is_historical_append_cf(name) {
            opts.set_compression_type(DBCompressionType::None);
            opts.set_compaction_style(DBCompactionStyle::Universal);
            let mut uco = UniversalCompactOptions::default();
            uco.set_size_ratio(10);
            uco.set_max_size_amplification_percent(100);
            opts.set_universal_compaction_options(&uco);
        } else {
            opts.set_level_compaction_dynamic_level_bytes(true);
        }

        let block_opts = Self::default_block_options(block_cache);
        opts.set_block_based_table_factory(&block_opts);

        opts
    }

    // ============================================================
    // CF accessor helpers — DEFAULT store
    // ============================================================

    fn default_cf(&self, name: &str) -> &ColumnFamily {
        self.default_db
            .cf_handle(name)
            .unwrap_or_else(|| panic!("Default CF '{}' not found", name))
    }

    fn append_cf(&self, name: &str) -> &ColumnFamily {
        self.append_db
            .cf_handle(name)
            .unwrap_or_else(|| panic!("Append CF '{}' not found", name))
    }

    pub fn cf_live_cells(&self) -> &ColumnFamily {
        self.default_cf(CF_LIVE_CELLS)
    }
    pub fn cf_live_cells_by_lock(&self) -> &ColumnFamily {
        self.default_cf(CF_LIVE_CELLS_BY_LOCK)
    }
    pub fn cf_live_cells_by_type(&self) -> &ColumnFamily {
        self.default_cf(CF_LIVE_CELLS_BY_TYPE)
    }
    pub fn cf_live_cells_by_lock_code(&self) -> &ColumnFamily {
        self.default_cf(CF_LIVE_CELLS_BY_LOCK_CODE)
    }
    pub fn cf_live_cells_by_type_code(&self) -> &ColumnFamily {
        self.default_cf(CF_LIVE_CELLS_BY_TYPE_CODE)
    }
    pub fn cf_consumed_cells(&self) -> &ColumnFamily {
        self.default_cf(CF_CONSUMED_CELLS)
    }
    pub fn cf_tx_index(&self) -> &ColumnFamily {
        self.default_cf(CF_TX_INDEX)
    }
    pub fn cf_block_index(&self) -> &ColumnFamily {
        self.default_cf(CF_BLOCK_INDEX)
    }
    pub fn cf_asset_meta(&self) -> &ColumnFamily {
        self.default_cf(CF_ASSET_META)
    }
    pub fn cf_nft_item_meta(&self) -> &ColumnFamily {
        self.default_cf(CF_NFT_ITEM_META)
    }
    pub fn cf_nft_outpoints(&self) -> &ColumnFamily {
        self.default_cf(CF_NFT_OUTPOINTS)
    }
    pub fn cf_nft_item_by_collection(&self) -> &ColumnFamily {
        self.default_cf(CF_NFT_ITEM_BY_COLLECTION)
    }
    pub fn cf_ft_outpoints(&self) -> &ColumnFamily {
        self.default_cf(CF_FT_OUTPOINTS)
    }
    pub fn cf_dao_deposits(&self) -> &ColumnFamily {
        self.default_cf(CF_DAO_DEPOSITS)
    }
    pub fn cf_dao_withdraw_index(&self) -> &ColumnFamily {
        self.default_cf(CF_DAO_WITHDRAW_INDEX)
    }
    pub fn cf_block_issuance(&self) -> &ColumnFamily {
        self.default_cf(CF_BLOCK_ISSUANCE)
    }
    pub fn cf_addr_stats(&self) -> &ColumnFamily {
        self.default_cf(CF_ADDR_STATS)
    }
    pub fn cf_ft_stats(&self) -> &ColumnFamily {
        self.default_cf(CF_FT_STATS)
    }
    pub fn cf_ft_holders(&self) -> &ColumnFamily {
        self.default_cf(CF_FT_HOLDERS)
    }
    pub fn cf_nft_collection_stats(&self) -> &ColumnFamily {
        self.default_cf(CF_NFT_COLLECTION_STATS)
    }
    pub fn cf_addr_txs(&self) -> &ColumnFamily {
        self.default_cf(CF_ADDR_TXS)
    }
    pub fn cf_addr_activities(&self) -> &ColumnFamily {
        self.default_cf(CF_ADDR_ACTIVITIES)
    }
    pub fn cf_nft_collection_activities(&self) -> &ColumnFamily {
        self.default_cf(CF_NFT_COLLECTION_ACTIVITIES)
    }
    pub fn cf_ft_activities(&self) -> &ColumnFamily {
        self.default_cf(CF_FT_ACTIVITIES)
    }
    pub fn cf_stats(&self) -> &ColumnFamily {
        self.default_cf(CF_STATS)
    }

    // ============================================================
    // CF accessor helpers — APPEND store
    // ============================================================

    pub fn cf_cells(&self) -> &ColumnFamily {
        self.append_cf(CF_CELLS)
    }
    pub fn cf_tx_meta(&self) -> &ColumnFamily {
        self.append_cf(CF_TX_META)
    }
    pub fn cf_block_meta(&self) -> &ColumnFamily {
        self.append_cf(CF_BLOCK_META)
    }
    pub fn cf_nft_item_index(&self) -> &ColumnFamily {
        self.append_cf(CF_NFT_ITEM_INDEX)
    }
    pub fn cf_ft_index(&self) -> &ColumnFamily {
        self.append_cf(CF_FT_INDEX)
    }
    pub fn cf_activities(&self) -> &ColumnFamily {
        self.append_cf(CF_ACTIVITIES)
    }

    // ============================================================
    // Backward-compatible CF accessors (deprecated)
    // ============================================================

    // TODO(data-refactor): Use cf_live_cells_by_lock()
    pub fn cf_cell_by_lock(&self) -> &ColumnFamily {
        self.cf_live_cells_by_lock()
    }
    // TODO(data-refactor): Use cf_live_cells_by_type()
    pub fn cf_cell_by_type(&self) -> &ColumnFamily {
        self.cf_live_cells_by_type()
    }
    // TODO(data-refactor): Use cf_live_cells_by_lock_code()
    pub fn cf_cell_by_lock_code(&self) -> &ColumnFamily {
        self.cf_live_cells_by_lock_code()
    }
    // TODO(data-refactor): Use cf_live_cells_by_type_code()
    pub fn cf_cell_by_type_code(&self) -> &ColumnFamily {
        self.cf_live_cells_by_type_code()
    }
    // TODO(data-refactor): Use cf_addr_stats()
    pub fn cf_addr_balance(&self) -> &ColumnFamily {
        self.cf_addr_stats()
    }
    // TODO(data-refactor): Use cf_dao_withdraw_index()
    pub fn cf_dao_by_withdraw_tx(&self) -> &ColumnFamily {
        self.cf_dao_withdraw_index()
    }
    // TODO(data-refactor): Use cf_asset_meta()
    pub fn cf_tokens(&self) -> &ColumnFamily {
        self.cf_asset_meta()
    }
    // TODO(data-refactor): Use cf_ft_holders()
    pub fn cf_token_holders(&self) -> &ColumnFamily {
        self.cf_ft_holders()
    }
    // Legacy accessors — these return CFs from the DEFAULT store (old physical CFs
    // kept during migration). Remove when ops files are rewritten to use new CFs.
    // TODO(data-refactor): Use cf_block_meta() (append store) + cf_block_index()
    pub fn cf_block_headers(&self) -> &ColumnFamily {
        self.default_cf(CF_BLOCK_HEADERS)
    }
    // TODO(data-refactor): Use cf_block_index()
    pub fn cf_block_hash_index(&self) -> &ColumnFamily {
        self.default_cf(CF_BLOCK_HASH_INDEX)
    }
    // TODO(data-refactor): Use cf_tx_meta() (append store) + cf_tx_index()
    pub fn cf_tx_hash_map(&self) -> &ColumnFamily {
        self.default_cf(CF_TX_HASH_MAP)
    }
    // TODO(data-refactor): Use cf_nft_item_meta()
    pub fn cf_spore_data(&self) -> &ColumnFamily {
        self.default_cf(CF_SPORE_DATA)
    }
    // TODO(data-refactor): Use cf_nft_item_by_collection()
    pub fn cf_spore_by_cluster(&self) -> &ColumnFamily {
        self.default_cf(CF_SPORE_BY_CLUSTER)
    }
    // TODO(data-refactor): Use cf_nft_item_meta()
    pub fn cf_nft_data(&self) -> &ColumnFamily {
        self.default_cf(CF_NFT_DATA)
    }
    // TODO(data-refactor): Use cf_nft_item_by_collection()
    pub fn cf_nft_by_collection(&self) -> &ColumnFamily {
        self.cf_nft_item_by_collection()
    }
    // TODO(data-refactor): Use cf_nft_collection_stats()
    pub fn cf_cluster_agg(&self) -> &ColumnFamily {
        self.default_cf(CF_CLUSTER_AGG)
    }
    // TODO(data-refactor): Use cf_nft_collection_stats()
    pub fn cf_nft_collection_agg(&self) -> &ColumnFamily {
        self.default_cf(CF_NFT_COLLECTION_AGG)
    }
    // TODO(data-refactor): Merged into cf_stats() with prefix 0x21
    pub fn cf_script_info(&self) -> &ColumnFamily {
        self.default_cf(CF_SCRIPT_INFO)
    }
    // TODO(data-refactor): Merged into cf_stats() with prefix 0xF0
    pub fn cf_sync_meta(&self) -> &ColumnFamily {
        self.default_cf(CF_SYNC_META)
    }
    // TODO(data-refactor): Use cf_ft_activities()
    pub fn cf_token_transfers(&self) -> &ColumnFamily {
        self.default_cf(CF_TOKEN_TRANSFERS)
    }
    // TODO(data-refactor): Merged into cf_stats() with prefix 0x20
    pub fn cf_addr_daily_stats(&self) -> &ColumnFamily {
        self.default_cf(CF_ADDR_DAILY_STATS)
    }

    // ============================================================
    // Raw DB operations — DEFAULT store
    // ============================================================

    pub fn get_cf(&self, cf: &ColumnFamily, key: &[u8]) -> anyhow::Result<Option<Vec<u8>>> {
        Ok(self.default_db.get_cf(cf, key)?)
    }

    pub fn put_cf(&self, cf: &ColumnFamily, key: &[u8], value: &[u8]) -> anyhow::Result<()> {
        Ok(self.default_db.put_cf(cf, key, value)?)
    }

    pub fn delete_cf(&self, cf: &ColumnFamily, key: &[u8]) -> anyhow::Result<()> {
        Ok(self.default_db.delete_cf(cf, key)?)
    }

    pub fn multi_get_cf(
        &self,
        keys: Vec<(&ColumnFamily, &[u8])>,
    ) -> Vec<Result<Option<Vec<u8>>, rocksdb::Error>> {
        self.default_db.multi_get_cf(keys)
    }

    pub fn write_batch(&self, batch: WriteBatch) -> anyhow::Result<()> {
        Ok(self.default_db.write(batch)?)
    }

    pub fn write_batch_no_wal(&self, batch: WriteBatch) -> anyhow::Result<()> {
        let mut opts = rocksdb::WriteOptions::default();
        opts.disable_wal(true);
        Ok(self.default_db.write_opt(batch, &opts)?)
    }

    pub fn iterator_cf(
        &self,
        cf: &ColumnFamily,
        mode: IteratorMode,
    ) -> impl Iterator<Item = KvResult> + '_ {
        self.default_db.iterator_cf(cf, mode)
    }

    pub fn prefix_iterator_cf(
        &self,
        cf: &ColumnFamily,
        prefix: &[u8],
    ) -> impl Iterator<Item = KvResult> + '_ {
        self.default_db.prefix_iterator_cf(cf, prefix)
    }

    pub fn raw_db(&self) -> &DB {
        &self.default_db
    }

    // ============================================================
    // Raw DB operations — APPEND store
    // ============================================================

    pub fn append_get_cf(&self, cf: &ColumnFamily, key: &[u8]) -> anyhow::Result<Option<Vec<u8>>> {
        Ok(self.append_db.get_cf(cf, key)?)
    }

    pub fn append_put_cf(&self, cf: &ColumnFamily, key: &[u8], value: &[u8]) -> anyhow::Result<()> {
        Ok(self.append_db.put_cf(cf, key, value)?)
    }

    pub fn append_multi_get_cf(
        &self,
        keys: Vec<(&ColumnFamily, &[u8])>,
    ) -> Vec<Result<Option<Vec<u8>>, rocksdb::Error>> {
        self.append_db.multi_get_cf(keys)
    }

    pub fn append_write_batch(&self, batch: WriteBatch) -> anyhow::Result<()> {
        Ok(self.append_db.write(batch)?)
    }

    pub fn append_write_batch_no_wal(&self, batch: WriteBatch) -> anyhow::Result<()> {
        let mut opts = rocksdb::WriteOptions::default();
        opts.disable_wal(true);
        Ok(self.append_db.write_opt(batch, &opts)?)
    }

    pub fn append_iterator_cf(
        &self,
        cf: &ColumnFamily,
        mode: IteratorMode,
    ) -> impl Iterator<Item = KvResult> + '_ {
        self.append_db.iterator_cf(cf, mode)
    }

    pub fn append_prefix_iterator_cf(
        &self,
        cf: &ColumnFamily,
        prefix: &[u8],
    ) -> impl Iterator<Item = KvResult> + '_ {
        self.append_db.prefix_iterator_cf(cf, prefix)
    }

    pub fn raw_append_db(&self) -> &DB {
        &self.append_db
    }

    // ============================================================
    // Bulk sync mode
    // ============================================================

    pub fn set_bulk_sync_mode(&self, enabled: bool) {
        self.bulk_sync_mode.store(enabled, Ordering::Relaxed);
    }

    pub fn is_bulk_sync_mode(&self) -> bool {
        self.bulk_sync_mode.load(Ordering::Relaxed)
    }

    pub fn log_config() {
        info!(
            write_buffer_mega_mb = 256,
            write_buffer_high_mb = 128,
            write_buffer_low_mb = 32,
            max_write_buffers_high = 4,
            max_write_buffers_low = 2,
            l0_slowdown = 20,
            l0_stop = 48,
            l0_slowdown_bulk = 64,
            l0_stop_bulk = 128,
            max_background_jobs = 24,
            max_subcompactions = 8,
            block_cache_normal_gb = Self::BLOCK_CACHE_NORMAL_BYTES / (1024 * 1024 * 1024),
            block_cache_bulk_gb = Self::BLOCK_CACHE_BULK_SYNC_BYTES / (1024 * 1024 * 1024),
            wbm_normal_gb = Self::WBM_NORMAL_BYTES / (1024 * 1024 * 1024),
            wbm_bulk_gb = Self::WBM_BULK_SYNC_BYTES / (1024 * 1024 * 1024),
            unordered_write = true,
            direct_io_reads = std::env::var("CKBADGER_DIRECT_IO_READS")
                .map(|v| v != "0" && v.to_lowercase() != "false")
                .unwrap_or(true),
            direct_io_compaction = true,
            bytes_per_sync_mb = 4,
            dynamic_level_bytes = true,
            default_cfs = DEFAULT_CFS.len(),
            append_cfs = APPEND_CFS.len(),
            total_cfs = ALL_CFS.len(),
            "RocksDB configuration (dual-store)"
        );
    }

    pub fn is_secondary(&self) -> bool {
        self.is_secondary
    }

    pub fn default_path(&self) -> &Path {
        &self.default_path
    }

    pub fn append_path(&self) -> &Path {
        &self.append_path
    }

    pub fn set_bulk_sync_compaction_options(&self) {
        if self.bulk_sync_mode.load(Ordering::Relaxed) {
            return;
        }
        self.bulk_sync_mode.store(true, Ordering::Relaxed);

        self.write_buffer_manager
            .set_buffer_size(Self::WBM_BULK_SYNC_BYTES);
        self.block_cache
            .lock()
            .expect("block_cache lock poisoned")
            .set_capacity(Self::BLOCK_CACHE_BULK_SYNC_BYTES);

        let mut ok = 0u32;
        let mut fail = 0u32;

        // Apply to both DBs
        for (db, cf_list) in [
            (&self.default_db, DEFAULT_CFS),
            (&self.append_db, APPEND_CFS),
        ] {
            for &cf_name in cf_list {
                if let Some(cf) = db.cf_handle(cf_name) {
                    let max_wb = if Self::is_mega_write_cf(cf_name) {
                        "12"
                    } else if Self::is_high_write_cf(cf_name) {
                        "8"
                    } else {
                        "6"
                    };
                    let result = db.set_options_cf(
                        cf,
                        &[
                            ("level0_slowdown_writes_trigger", "64"),
                            ("level0_stop_writes_trigger", "128"),
                            ("max_write_buffer_number", max_wb),
                            ("max_bytes_for_level_base", "2147483648"),
                            ("target_file_size_base", "268435456"),
                        ],
                    );
                    if result.is_ok() {
                        ok += 1;
                    } else {
                        warn!(
                            "Failed to set bulk sync options for CF '{}': {:?}",
                            cf_name,
                            result.err()
                        );
                        fail += 1;
                    }
                }
            }
        }
        info!(
            ok,
            fail,
            wbm_budget_gb = Self::WBM_BULK_SYNC_BYTES / (1024 * 1024 * 1024),
            block_cache_gb = Self::BLOCK_CACHE_BULK_SYNC_BYTES / (1024 * 1024 * 1024),
            "Bulk sync compaction options set (dual-store)"
        );
    }

    pub fn restore_normal_compaction_options(&self) {
        if !self.bulk_sync_mode.load(Ordering::Relaxed) {
            return;
        }
        self.bulk_sync_mode.store(false, Ordering::Relaxed);

        self.write_buffer_manager
            .set_buffer_size(Self::WBM_NORMAL_BYTES);
        self.block_cache
            .lock()
            .expect("block_cache lock poisoned")
            .set_capacity(Self::BLOCK_CACHE_NORMAL_BYTES);

        for (db, cf_list) in [
            (&self.default_db, DEFAULT_CFS),
            (&self.append_db, APPEND_CFS),
        ] {
            for &cf_name in cf_list {
                if let Some(cf) = db.cf_handle(cf_name) {
                    let max_wb =
                        if Self::is_mega_write_cf(cf_name) || Self::is_high_write_cf(cf_name) {
                            "4"
                        } else {
                            "2"
                        };
                    let _ = db.set_options_cf(
                        cf,
                        &[
                            ("level0_slowdown_writes_trigger", "12"),
                            ("level0_stop_writes_trigger", "24"),
                            ("max_write_buffer_number", max_wb),
                            ("max_bytes_for_level_base", "536870912"),
                            ("target_file_size_base", "67108864"),
                        ],
                    );
                }
            }
        }
        info!(
            wbm_budget_gb = Self::WBM_NORMAL_BYTES / (1024 * 1024 * 1024),
            block_cache_gb = Self::BLOCK_CACHE_NORMAL_BYTES / (1024 * 1024 * 1024),
            "Normal compaction options restored (dual-store)"
        );
    }

    pub fn disable_auto_compactions(&self) {
        for (db, cf_list) in [
            (&self.default_db, DEFAULT_CFS),
            (&self.append_db, APPEND_CFS),
        ] {
            for cf_name in cf_list.iter().copied() {
                if let Some(cf) = db.cf_handle(cf_name) {
                    if let Err(e) = db.set_options_cf(cf, &[("disable_auto_compactions", "true")]) {
                        tracing::warn!(cf = cf_name, error = %e, "Failed to disable auto compactions");
                    }
                }
            }
        }
        info!("Auto-compactions disabled for bulk sync (dual-store)");
    }

    pub fn enable_auto_compactions(&self) {
        for (db, cf_list) in [
            (&self.default_db, DEFAULT_CFS),
            (&self.append_db, APPEND_CFS),
        ] {
            for cf_name in cf_list {
                if let Some(cf) = db.cf_handle(cf_name) {
                    if let Err(e) = db.set_options_cf(cf, &[("disable_auto_compactions", "false")])
                    {
                        tracing::warn!(cf = cf_name, error = %e, "Failed to enable auto compactions");
                    }
                }
            }
        }
        info!("Auto-compactions re-enabled (dual-store)");
    }

    pub fn trigger_full_compaction(&self) {
        info!("Starting manual compaction across all column families (dual-store)");
        for (db, cf_list) in [
            (&self.default_db, DEFAULT_CFS),
            (&self.append_db, APPEND_CFS),
        ] {
            for cf_name in cf_list {
                if let Some(cf) = db.cf_handle(cf_name) {
                    db.compact_range_cf(cf, None::<&[u8]>, None::<&[u8]>);
                }
            }
        }
        info!("Manual compaction completed (dual-store)");
    }

    // ============================================================
    // Memory stats
    // ============================================================

    pub fn compaction_pressure(&self) -> (u64, u64, u64) {
        let mut compaction_pending_bytes = 0u64;
        let mut l0_files_max: u64 = 0;
        let mut immutable_memtables = 0u64;

        for (db, cf_list) in [
            (&self.default_db, DEFAULT_CFS),
            (&self.append_db, APPEND_CFS),
        ] {
            for &cf_name in cf_list {
                if let Some(cf) = db.cf_handle(cf_name) {
                    if let Ok(Some(v)) =
                        db.property_int_value_cf(cf, "rocksdb.estimate-pending-compaction-bytes")
                    {
                        compaction_pending_bytes += v;
                    }
                    if let Ok(Some(v)) = db.property_int_value_cf(cf, "rocksdb.num-files-at-level0")
                    {
                        l0_files_max = l0_files_max.max(v);
                    }
                    if let Ok(Some(v)) =
                        db.property_int_value_cf(cf, "rocksdb.num-immutable-mem-table")
                    {
                        immutable_memtables += v;
                    }
                }
            }
        }
        (l0_files_max, compaction_pending_bytes, immutable_memtables)
    }

    pub fn memory_stats(&self) -> MemoryStats {
        let mut memtable_bytes = 0usize;
        let mut table_readers_bytes = 0usize;
        let mut live_cells_count = 0usize;
        let mut cells_count = 0usize;
        let mut consumed_cells_count = 0usize;
        let mut consumed_cf_live_data_bytes: Option<u64> = None;
        let mut consumed_cf_sst_files_bytes: Option<u64> = None;
        let mut consumed_cf_memtable_bytes: Option<u64> = None;
        let mut block_meta_count = 0usize;
        let mut addr_stats_count = 0usize;
        let mut compaction_pending_bytes = 0u64;
        let mut num_running_compactions_fallback = 0u64;
        let mut sst_files_size = 0u64;
        let mut l0_files_total = 0u64;
        let mut l0_files_max: u64 = 0;
        let mut l0_worst_cf = String::new();
        let mut immutable_memtables = 0u64;
        let mut cf_sizes: Vec<(String, u64)> = Vec::new();

        for (db, cf_list) in [
            (&self.default_db, DEFAULT_CFS),
            (&self.append_db, APPEND_CFS),
        ] {
            for &cf_name in cf_list {
                if let Some(cf) = db.cf_handle(cf_name) {
                    if let Ok(Some(v)) =
                        db.property_int_value_cf(cf, "rocksdb.cur-size-all-mem-tables")
                    {
                        memtable_bytes += v as usize;
                        if cf_name == CF_CONSUMED_CELLS {
                            consumed_cf_memtable_bytes = Some(v);
                        }
                    }
                    if let Ok(Some(v)) =
                        db.property_int_value_cf(cf, "rocksdb.estimate-table-readers-mem")
                    {
                        table_readers_bytes += v as usize;
                    }
                    if let Ok(Some(v)) =
                        db.property_int_value_cf(cf, "rocksdb.estimate-pending-compaction-bytes")
                    {
                        compaction_pending_bytes += v;
                    }
                    if let Ok(Some(v)) = db.property_int_value_cf(cf, "rocksdb.estimate-num-keys") {
                        match cf_name {
                            CF_LIVE_CELLS => live_cells_count = v as usize,
                            CF_CELLS => cells_count = v as usize,
                            CF_CONSUMED_CELLS => consumed_cells_count = v as usize,
                            CF_BLOCK_META => block_meta_count = v as usize,
                            CF_ADDR_STATS => addr_stats_count = v as usize,
                            _ => {}
                        }
                    }
                    if let Ok(Some(v)) =
                        db.property_int_value_cf(cf, "rocksdb.num-running-compactions")
                    {
                        num_running_compactions_fallback = num_running_compactions_fallback.max(v);
                    }
                    if let Ok(Some(v)) =
                        db.property_int_value_cf(cf, "rocksdb.total-sst-files-size")
                    {
                        sst_files_size += v;
                        if cf_name == CF_CONSUMED_CELLS {
                            consumed_cf_sst_files_bytes = Some(v);
                        }
                    }
                    if let Ok(Some(v)) = db.property_int_value_cf(cf, "rocksdb.num-files-at-level0")
                    {
                        l0_files_total += v;
                        if v > l0_files_max {
                            l0_files_max = v;
                            l0_worst_cf = cf_name.to_string();
                        }
                    }
                    if let Ok(Some(v)) =
                        db.property_int_value_cf(cf, "rocksdb.num-immutable-mem-table")
                    {
                        immutable_memtables += v;
                    }
                    if let Ok(Some(v)) =
                        db.property_int_value_cf(cf, "rocksdb.estimate-live-data-size")
                    {
                        if cf_name == CF_CONSUMED_CELLS {
                            consumed_cf_live_data_bytes = Some(v);
                        }
                        if v > 0 {
                            cf_sizes.push((cf_name.to_string(), v));
                        }
                    }
                }
            }
        }

        cf_sizes.sort_by(|a, b| b.1.cmp(&a.1));
        cf_sizes.truncate(5);
        let (consumed_cells_bytes, consumed_cells_bytes_source) = consumed_cf_storage_bytes(
            consumed_cf_live_data_bytes,
            consumed_cf_sst_files_bytes,
            consumed_cf_memtable_bytes,
        );

        let block_cache_bytes = self
            .block_cache
            .lock()
            .expect("block_cache lock poisoned")
            .get_usage();
        let memory_bytes = memtable_bytes + block_cache_bytes + table_readers_bytes;

        // Try default DB first for global compaction count
        let num_running_compactions = self
            .default_db
            .property_int_value("rocksdb.num-running-compactions")
            .ok()
            .flatten()
            .unwrap_or(num_running_compactions_fallback);

        MemoryStats {
            live_cells_count,
            consumed_cells_count,
            consumed_cells_bytes,
            consumed_cells_bytes_source,
            block_headers_count: block_meta_count,
            addr_balance_count: addr_stats_count,
            cells_count,
            memory_bytes,
            memtable_bytes,
            block_cache_bytes,
            table_readers_bytes,
            compaction_pending_bytes,
            num_running_compactions,
            sst_files_size,
            l0_files_count: l0_files_total,
            l0_files_max,
            l0_worst_cf,
            immutable_memtables,
            top_cf_sizes: cf_sizes,
            wbm_usage_bytes: self.write_buffer_manager.get_usage(),
            wbm_budget_bytes: self.write_buffer_manager.get_buffer_size(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn open_test_store() -> (CkbadgerStore, TempDir, TempDir) {
        let default_dir = TempDir::new().unwrap();
        let append_dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_split(default_dir.path(), append_dir.path()).unwrap();
        (store, default_dir, append_dir)
    }

    #[test]
    fn test_open_and_close() {
        let (store, _d, _a) = open_test_store();
        assert!(!store.is_secondary());
        drop(store);
    }

    #[test]
    fn test_all_default_cfs_accessible() {
        let (store, _d, _a) = open_test_store();
        for cf_name in DEFAULT_CFS {
            let _ = store.default_cf(cf_name);
        }
    }

    #[test]
    fn test_all_append_cfs_accessible() {
        let (store, _d, _a) = open_test_store();
        for cf_name in APPEND_CFS {
            let _ = store.append_cf(cf_name);
        }
    }

    #[test]
    fn test_put_get_delete_default() {
        let (store, _d, _a) = open_test_store();
        let cf = store.cf_stats();
        store.put_cf(cf, b"test_key", b"test_value").unwrap();
        let val = store.get_cf(cf, b"test_key").unwrap();
        assert_eq!(val.as_deref(), Some(b"test_value".as_slice()));
        store.delete_cf(cf, b"test_key").unwrap();
        let val = store.get_cf(cf, b"test_key").unwrap();
        assert!(val.is_none());
    }

    #[test]
    fn test_put_get_append() {
        let (store, _d, _a) = open_test_store();
        let cf = store.cf_cells();
        store
            .raw_append_db()
            .put_cf(cf, b"cell_key", b"cell_value")
            .unwrap();
        let val = store.append_get_cf(cf, b"cell_key").unwrap();
        assert_eq!(val.as_deref(), Some(b"cell_value".as_slice()));
    }

    #[test]
    fn test_write_batch_default() {
        let (store, _d, _a) = open_test_store();
        let cf = store.cf_stats();
        let mut batch = WriteBatch::default();
        batch.put_cf(cf, b"k1", b"v1");
        batch.put_cf(cf, b"k2", b"v2");
        store.write_batch(batch).unwrap();
        assert_eq!(
            store.get_cf(cf, b"k1").unwrap().as_deref(),
            Some(b"v1".as_slice())
        );
        assert_eq!(
            store.get_cf(cf, b"k2").unwrap().as_deref(),
            Some(b"v2".as_slice())
        );
    }

    #[test]
    fn test_secondary_instance() {
        let default_primary = TempDir::new().unwrap();
        let append_primary = TempDir::new().unwrap();
        let default_secondary = TempDir::new().unwrap();
        let append_secondary = TempDir::new().unwrap();

        let primary =
            CkbadgerStore::open_split(default_primary.path(), append_primary.path()).unwrap();
        let cf = primary.cf_stats();
        primary.put_cf(cf, b"key", b"value").unwrap();

        let secondary = CkbadgerStore::open_secondary_split(
            default_primary.path(),
            default_secondary.path(),
            append_primary.path(),
            append_secondary.path(),
        )
        .unwrap();
        assert!(secondary.is_secondary());
        secondary.refresh().unwrap();

        let cf = secondary.cf_stats();
        let val = secondary.get_cf(cf, b"key").unwrap();
        assert_eq!(val.as_deref(), Some(b"value".as_slice()));
    }

    #[test]
    fn test_bulk_sync_mode() {
        let (store, _d, _a) = open_test_store();
        assert!(!store.is_bulk_sync_mode());
        store.set_bulk_sync_mode(true);
        assert!(store.is_bulk_sync_mode());
        store.set_bulk_sync_mode(false);
        assert!(!store.is_bulk_sync_mode());
    }

    #[test]
    fn test_memory_stats() {
        let (store, _d, _a) = open_test_store();
        let stats = store.memory_stats();
        assert_eq!(stats.cells_count, stats.live_cells_count);
    }

    #[test]
    fn test_set_bulk_sync_compaction_options_does_not_panic() {
        let (store, _d, _a) = open_test_store();
        store.set_bulk_sync_compaction_options();
    }

    #[test]
    fn test_restore_normal_compaction_options_does_not_panic() {
        let (store, _d, _a) = open_test_store();
        store.restore_normal_compaction_options();
    }

    #[test]
    fn test_bulk_sync_then_restore_round_trip() {
        let (store, _d, _a) = open_test_store();
        store.set_bulk_sync_compaction_options();
        store.restore_normal_compaction_options();
        let cf = store.cf_stats();
        store.put_cf(cf, b"test", b"value").unwrap();
        let val = store.get_cf(cf, b"test").unwrap();
        assert_eq!(val.as_deref(), Some(b"value".as_slice()));
    }

    #[test]
    fn test_set_bulk_compaction_is_idempotent() {
        let (store, _d, _a) = open_test_store();
        assert!(!store.is_bulk_sync_mode());
        store.set_bulk_sync_compaction_options();
        assert!(store.is_bulk_sync_mode());
        store.set_bulk_sync_compaction_options();
        assert!(store.is_bulk_sync_mode());
    }

    #[test]
    fn test_restore_normal_compaction_is_idempotent() {
        let (store, _d, _a) = open_test_store();
        store.restore_normal_compaction_options();
        assert!(!store.is_bulk_sync_mode());
        store.set_bulk_sync_compaction_options();
        assert!(store.is_bulk_sync_mode());
        store.restore_normal_compaction_options();
        assert!(!store.is_bulk_sync_mode());
        store.restore_normal_compaction_options();
        assert!(!store.is_bulk_sync_mode());
    }

    #[test]
    fn test_log_config_does_not_panic() {
        CkbadgerStore::log_config();
    }

    #[test]
    fn test_cf_count() {
        // 25 new + 12 legacy = 37 default CFs
        assert_eq!(DEFAULT_CFS.len(), 37);
        assert_eq!(APPEND_CFS.len(), 6);
        // 37 default + 6 append = 43 total
        assert_eq!(ALL_CFS.len(), 43);
    }

    #[test]
    fn test_consumed_cf_storage_bytes_prefers_live_data_estimate() {
        assert_eq!(
            consumed_cf_storage_bytes(Some(100), Some(500), Some(20)),
            (120, "live")
        );
    }

    #[test]
    fn test_consumed_cf_storage_bytes_falls_back_to_sst_when_live_missing() {
        assert_eq!(
            consumed_cf_storage_bytes(None, Some(500), Some(20)),
            (520, "sst")
        );
        assert_eq!(
            consumed_cf_storage_bytes(Some(0), Some(500), Some(20)),
            (520, "sst")
        );
    }

    #[test]
    fn test_consumed_cf_storage_bytes_returns_none_when_all_missing() {
        assert_eq!(consumed_cf_storage_bytes(None, None, None), (0, "none"));
    }

    #[test]
    fn test_consumed_cf_storage_bytes_memtable_only_source() {
        assert_eq!(consumed_cf_storage_bytes(None, None, Some(20)), (20, "mem"));
    }

    #[test]
    fn test_mega_write_cfs_contains_activities() {
        assert!(
            CkbadgerStore::is_mega_write_cf(CF_ACTIVITIES),
            "CF_ACTIVITIES should be in MEGA_WRITE_CFS"
        );
    }

    #[test]
    fn test_mega_write_cfs_excludes_asset_meta() {
        assert!(
            !CkbadgerStore::is_mega_write_cf(CF_ASSET_META),
            "CF_ASSET_META should NOT be in MEGA_WRITE_CFS"
        );
    }
}
