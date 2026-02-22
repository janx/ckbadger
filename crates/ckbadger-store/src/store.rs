//! Core RocksDB store.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{info, warn};

use rocksdb::{
    ColumnFamily, ColumnFamilyDescriptor, DBCompressionType, IteratorMode, Options, WriteBatch, DB,
};

use crate::types::MemoryStats;

/// Type alias for RocksDB iterator items to avoid complex type lint.
pub type KvResult = Result<(Box<[u8]>, Box<[u8]>), rocksdb::Error>;

// Column family name constants
pub const CF_LIVE_CELLS: &str = "live_cells";
pub const CF_CONSUMED_CELLS: &str = "consumed_cells";
pub const CF_BLOCK_HEADERS: &str = "block_headers";
pub const CF_BLOCK_HASH_INDEX: &str = "block_hash_index";
pub const CF_CELL_BY_LOCK: &str = "cell_by_lock";
pub const CF_CELL_BY_TYPE: &str = "cell_by_type";
pub const CF_TX_INDEX: &str = "tx_index";
pub const CF_TX_HASH_MAP: &str = "tx_hash_map";
pub const CF_ADDR_BALANCE: &str = "addr_balance";
pub const CF_ADDR_TXS: &str = "addr_txs";
pub const CF_DAO_DEPOSITS: &str = "dao_deposits";
pub const CF_DAO_BY_WITHDRAW_TX: &str = "dao_by_withdraw_tx";
pub const CF_DAO_STATS: &str = "dao_stats";
pub const CF_BLOCK_ISSUANCE: &str = "block_issuance";
pub const CF_TOKENS: &str = "tokens";
pub const CF_TOKEN_HOLDERS: &str = "token_holders";
pub const CF_SPORE_DATA: &str = "spore_data";
pub const CF_NFT_DATA: &str = "nft_data";
pub const CF_NFT_BY_COLLECTION: &str = "nft_by_collection";
pub const CF_STATS: &str = "stats";
pub const CF_SCRIPT_INFO: &str = "script_info";
pub const CF_SYNC_META: &str = "sync_meta";
pub const CF_SPORE_BY_CLUSTER: &str = "spore_by_cluster";
pub const CF_CELL_BY_LOCK_CODE: &str = "cell_by_lock_code";
pub const CF_CELL_BY_TYPE_CODE: &str = "cell_by_type_code";
pub const CF_TOKEN_TRANSFERS: &str = "token_transfers";
pub const CF_ACTIVITIES: &str = "activities";
pub const CF_ADDR_DAILY_STATS: &str = "addr_daily_stats";
pub const CF_CLUSTER_AGG: &str = "cluster_agg";
pub const CF_NFT_COLLECTION_AGG: &str = "nft_collection_agg";

/// All column family names, used during DB open.
pub const ALL_CFS: &[&str] = &[
    CF_LIVE_CELLS,
    CF_CONSUMED_CELLS,
    CF_BLOCK_HEADERS,
    CF_BLOCK_HASH_INDEX,
    CF_CELL_BY_LOCK,
    CF_CELL_BY_TYPE,
    CF_CELL_BY_LOCK_CODE,
    CF_CELL_BY_TYPE_CODE,
    CF_TX_INDEX,
    CF_TX_HASH_MAP,
    CF_ADDR_BALANCE,
    CF_ADDR_TXS,
    CF_DAO_DEPOSITS,
    CF_DAO_BY_WITHDRAW_TX,
    CF_DAO_STATS,
    CF_BLOCK_ISSUANCE,
    CF_TOKENS,
    CF_TOKEN_HOLDERS,
    CF_SPORE_DATA,
    CF_NFT_DATA,
    CF_NFT_BY_COLLECTION,
    CF_STATS,
    CF_SCRIPT_INFO,
    CF_SYNC_META,
    CF_SPORE_BY_CLUSTER,
    CF_TOKEN_TRANSFERS,
    CF_ACTIVITIES,
    CF_ADDR_DAILY_STATS,
    CF_CLUSTER_AGG,
    CF_NFT_COLLECTION_AGG,
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

pub struct CkbadgerStore {
    db: DB,
    /// Keep block cache alive for the lifetime of the store.
    #[allow(dead_code)]
    block_cache: rocksdb::Cache,
    bulk_sync_mode: AtomicBool,
    is_secondary: bool,
}

impl CkbadgerStore {
    /// Open as primary (read-write). Creates all column families.
    /// Also opens any legacy CFs that exist on disk but are no longer in ALL_CFS,
    /// so RocksDB doesn't error on "Column families not opened".
    pub fn open<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let (opts, block_cache) = Self::default_options();

        // Discover any CFs that exist on disk (may include legacy/removed ones)
        let existing_cfs = DB::list_cf(&opts, &path).unwrap_or_default();
        let mut cf_names: Vec<String> = ALL_CFS.iter().map(|s| s.to_string()).collect();
        for cf in &existing_cfs {
            if cf != "default" && !ALL_CFS.contains(&cf.as_str()) {
                warn!(cf = cf.as_str(), "Opening legacy column family from disk");
                cf_names.push(cf.clone());
            }
        }

        let cf_descriptors: Vec<ColumnFamilyDescriptor> = cf_names
            .iter()
            .map(|name| {
                ColumnFamilyDescriptor::new(
                    name.as_str(),
                    Self::cf_options(name.as_str(), &block_cache),
                )
            })
            .collect();

        let db = DB::open_cf_descriptors(&opts, path, cf_descriptors)?;

        Ok(Self {
            db,
            block_cache,
            bulk_sync_mode: AtomicBool::new(false),
            is_secondary: false,
        })
    }

    /// Open as secondary instance (read-only). Follows primary writes via `refresh()`.
    pub fn open_secondary<P: AsRef<Path>>(
        primary_path: P,
        secondary_path: P,
    ) -> anyhow::Result<Self> {
        let (opts, block_cache) = Self::default_options();

        let existing_cfs = DB::list_cf(&opts, &primary_path).unwrap_or_default();
        let mut cf_names: Vec<String> = ALL_CFS.iter().map(|s| s.to_string()).collect();
        for cf in &existing_cfs {
            if cf != "default" && !ALL_CFS.contains(&cf.as_str()) {
                cf_names.push(cf.clone());
            }
        }
        let cf_refs: Vec<&str> = cf_names.iter().map(|s| s.as_str()).collect();
        let db = DB::open_cf_as_secondary(&opts, primary_path, secondary_path, cf_refs)?;

        Ok(Self {
            db,
            block_cache,
            bulk_sync_mode: AtomicBool::new(false),
            is_secondary: true,
        })
    }

    /// Catch up with primary instance writes (secondary only).
    pub fn refresh(&self) -> anyhow::Result<()> {
        if self.is_secondary {
            self.db.try_catch_up_with_primary()?;
        }
        Ok(())
    }

    /// Mega-write CFs: T1's 8 CFs that receive the most data per batch.
    /// 256MB write buffers prevent memtable flush stalls during mega-blocks
    /// (block 12M has ~1.31M txs → ~162MB per CF).
    const MEGA_WRITE_CFS: &'static [&'static str] = &[
        CF_LIVE_CELLS,
        CF_CONSUMED_CELLS,
        CF_CELL_BY_LOCK,
        CF_CELL_BY_TYPE,
        CF_CELL_BY_LOCK_CODE,
        CF_CELL_BY_TYPE_CODE,
        CF_TX_INDEX,
        CF_TX_HASH_MAP,
        CF_ADDR_BALANCE,
        CF_ACTIVITIES,
    ];

    /// High-write column families that benefit from large write buffers (128 MB).
    const HIGH_WRITE_CFS: &'static [&'static str] = &[
        CF_LIVE_CELLS,
        CF_CONSUMED_CELLS,
        CF_BLOCK_HEADERS,
        CF_BLOCK_HASH_INDEX,
        CF_CELL_BY_LOCK,
        CF_CELL_BY_TYPE,
        CF_TX_INDEX,
        CF_TX_HASH_MAP,
        CF_ADDR_BALANCE,
        CF_ADDR_TXS,
        CF_DAO_DEPOSITS,
        CF_ACTIVITIES,
    ];

    fn is_mega_write_cf(name: &str) -> bool {
        Self::MEGA_WRITE_CFS.contains(&name)
    }

    fn is_high_write_cf(name: &str) -> bool {
        Self::HIGH_WRITE_CFS.contains(&name)
    }

    fn default_block_options(block_cache: &rocksdb::Cache) -> rocksdb::BlockBasedOptions {
        let mut block_opts = rocksdb::BlockBasedOptions::default();
        block_opts.set_block_size(16 * 1024);
        block_opts.set_block_cache(block_cache);
        block_opts.set_cache_index_and_filter_blocks(true);
        block_opts.set_bloom_filter(10.0, false);
        block_opts
    }

    fn default_options() -> (Options, rocksdb::Cache) {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);

        // Write buffer: 128 MB per CF (default for high-write CFs), up to 4 buffers
        opts.set_write_buffer_size(128 * 1024 * 1024);
        opts.set_max_write_buffer_number(4);

        // Compaction triggers: give L0 more headroom to avoid write stalls
        // With 5 parallel commit_no_wal() per batch, L0 files accumulate fast.
        // Wider thresholds let compaction catch up without stalling writers.
        opts.set_level_zero_file_num_compaction_trigger(4);
        opts.set_level_zero_slowdown_writes_trigger(20);
        opts.set_level_zero_stop_writes_trigger(48);
        opts.set_max_bytes_for_level_base(512 * 1024 * 1024);
        opts.set_compression_type(DBCompressionType::Lz4);

        // Background jobs shared across all column families for flush + compaction
        // With 5 parallel commit_no_wal() per batch, RocksDB needs enough background
        // threads for concurrent flush + compaction across all CFs on 24-core machines
        opts.set_max_background_jobs(24);
        // Allow large compaction jobs to use multiple threads
        opts.set_max_subcompactions(4);

        // Bypass OS page cache for flush/compaction to avoid cache pollution
        opts.set_use_direct_io_for_flush_and_compaction(true);

        // Atomic flush: when any CF's memtable triggers a flush, ALL CFs flush
        // together. This prevents cross-CF data inconsistency when using
        // commit_no_wal() during bulk sync — without it, a crash can leave
        // live_cells deletes flushed to SST while consumed_cells puts are lost
        // in memtable, creating unrecoverable "cell black holes".
        // Note: atomic_flush is incompatible with enable_pipelined_write, but
        // pipelined_write only helps pipeline WAL+memtable inserts — irrelevant
        // during bulk sync where WAL is disabled (commit_no_wal).
        opts.set_atomic_flush(true);

        // 8 GB block cache — system has 93 GB RAM; 2 GB only covered ~17% of SST data
        let block_cache = rocksdb::Cache::new_lru_cache(8 * 1024 * 1024 * 1024);
        let block_opts = Self::default_block_options(&block_cache);
        opts.set_block_based_table_factory(&block_opts);

        (opts, block_cache)
    }

    /// Per-CF options with 3 tiers:
    /// - Mega-write CFs: 256MB × 4 buffers = 1GB per CF
    /// - High-write (remaining CFs): 128MB × 4 buffers = 512MB per CF
    /// - Everything else: 32MB × 2 buffers = 64MB per CF
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

        let block_opts = Self::default_block_options(block_cache);
        opts.set_block_based_table_factory(&block_opts);

        opts
    }

    // ---- Column family accessors ----

    pub fn cf(&self, name: &str) -> &ColumnFamily {
        self.db
            .cf_handle(name)
            .unwrap_or_else(|| panic!("CF '{}' not found", name))
    }

    pub fn cf_live_cells(&self) -> &ColumnFamily {
        self.cf(CF_LIVE_CELLS)
    }
    pub fn cf_consumed_cells(&self) -> &ColumnFamily {
        self.cf(CF_CONSUMED_CELLS)
    }
    pub fn cf_block_headers(&self) -> &ColumnFamily {
        self.cf(CF_BLOCK_HEADERS)
    }
    pub fn cf_block_hash_index(&self) -> &ColumnFamily {
        self.cf(CF_BLOCK_HASH_INDEX)
    }
    pub fn cf_cell_by_lock(&self) -> &ColumnFamily {
        self.cf(CF_CELL_BY_LOCK)
    }
    pub fn cf_cell_by_type(&self) -> &ColumnFamily {
        self.cf(CF_CELL_BY_TYPE)
    }
    pub fn cf_tx_index(&self) -> &ColumnFamily {
        self.cf(CF_TX_INDEX)
    }
    pub fn cf_tx_hash_map(&self) -> &ColumnFamily {
        self.cf(CF_TX_HASH_MAP)
    }
    pub fn cf_addr_balance(&self) -> &ColumnFamily {
        self.cf(CF_ADDR_BALANCE)
    }
    pub fn cf_addr_txs(&self) -> &ColumnFamily {
        self.cf(CF_ADDR_TXS)
    }
    pub fn cf_dao_deposits(&self) -> &ColumnFamily {
        self.cf(CF_DAO_DEPOSITS)
    }
    pub fn cf_dao_by_withdraw_tx(&self) -> &ColumnFamily {
        self.cf(CF_DAO_BY_WITHDRAW_TX)
    }
    pub fn cf_dao_stats(&self) -> &ColumnFamily {
        self.cf(CF_DAO_STATS)
    }
    pub fn cf_block_issuance(&self) -> &ColumnFamily {
        self.cf(CF_BLOCK_ISSUANCE)
    }
    pub fn cf_tokens(&self) -> &ColumnFamily {
        self.cf(CF_TOKENS)
    }
    pub fn cf_token_holders(&self) -> &ColumnFamily {
        self.cf(CF_TOKEN_HOLDERS)
    }
    pub fn cf_spore_data(&self) -> &ColumnFamily {
        self.cf(CF_SPORE_DATA)
    }
    pub fn cf_nft_data(&self) -> &ColumnFamily {
        self.cf(CF_NFT_DATA)
    }
    pub fn cf_nft_by_collection(&self) -> &ColumnFamily {
        self.cf(CF_NFT_BY_COLLECTION)
    }
    pub fn cf_stats(&self) -> &ColumnFamily {
        self.cf(CF_STATS)
    }
    pub fn cf_script_info(&self) -> &ColumnFamily {
        self.cf(CF_SCRIPT_INFO)
    }
    pub fn cf_sync_meta(&self) -> &ColumnFamily {
        self.cf(CF_SYNC_META)
    }
    pub fn cf_spore_by_cluster(&self) -> &ColumnFamily {
        self.cf(CF_SPORE_BY_CLUSTER)
    }
    pub fn cf_cell_by_lock_code(&self) -> &ColumnFamily {
        self.cf(CF_CELL_BY_LOCK_CODE)
    }
    pub fn cf_cell_by_type_code(&self) -> &ColumnFamily {
        self.cf(CF_CELL_BY_TYPE_CODE)
    }
    pub fn cf_token_transfers(&self) -> &ColumnFamily {
        self.cf(CF_TOKEN_TRANSFERS)
    }
    pub fn cf_activities(&self) -> &ColumnFamily {
        self.cf(CF_ACTIVITIES)
    }
    pub fn cf_addr_daily_stats(&self) -> &ColumnFamily {
        self.cf(CF_ADDR_DAILY_STATS)
    }
    pub fn cf_cluster_agg(&self) -> &ColumnFamily {
        self.cf(CF_CLUSTER_AGG)
    }
    pub fn cf_nft_collection_agg(&self) -> &ColumnFamily {
        self.cf(CF_NFT_COLLECTION_AGG)
    }

    // ---- Raw DB operations ----

    pub fn get_cf(&self, cf: &ColumnFamily, key: &[u8]) -> anyhow::Result<Option<Vec<u8>>> {
        Ok(self.db.get_cf(cf, key)?)
    }

    pub fn put_cf(&self, cf: &ColumnFamily, key: &[u8], value: &[u8]) -> anyhow::Result<()> {
        Ok(self.db.put_cf(cf, key, value)?)
    }

    pub fn delete_cf(&self, cf: &ColumnFamily, key: &[u8]) -> anyhow::Result<()> {
        Ok(self.db.delete_cf(cf, key)?)
    }

    pub fn multi_get_cf(
        &self,
        keys: Vec<(&ColumnFamily, &[u8])>,
    ) -> Vec<Result<Option<Vec<u8>>, rocksdb::Error>> {
        self.db.multi_get_cf(keys)
    }

    pub fn write_batch(&self, batch: WriteBatch) -> anyhow::Result<()> {
        Ok(self.db.write(batch)?)
    }

    /// Write a batch with WAL disabled. Use during bulk sync where crash recovery
    /// re-syncs from the last committed block header.
    pub fn write_batch_no_wal(&self, batch: WriteBatch) -> anyhow::Result<()> {
        let mut opts = rocksdb::WriteOptions::default();
        opts.disable_wal(true);
        Ok(self.db.write_opt(batch, &opts)?)
    }

    /// Iterate over a CF starting from a specific key.
    pub fn iterator_cf(
        &self,
        cf: &ColumnFamily,
        mode: IteratorMode,
    ) -> impl Iterator<Item = KvResult> + '_ {
        self.db.iterator_cf(cf, mode)
    }

    /// Iterate over a CF with a prefix.
    pub fn prefix_iterator_cf(
        &self,
        cf: &ColumnFamily,
        prefix: &[u8],
    ) -> impl Iterator<Item = KvResult> + '_ {
        self.db.prefix_iterator_cf(cf, prefix)
    }

    /// Get the underlying DB ref for WriteBatch operations.
    pub fn raw_db(&self) -> &DB {
        &self.db
    }

    // ---- Bulk sync mode ----

    pub fn set_bulk_sync_mode(&self, enabled: bool) {
        self.bulk_sync_mode.store(enabled, Ordering::Relaxed);
    }

    pub fn is_bulk_sync_mode(&self) -> bool {
        self.bulk_sync_mode.load(Ordering::Relaxed)
    }

    /// Log the key RocksDB tuning parameters at startup.
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
            max_subcompactions = 4,
            block_cache_gb = 8,
            direct_io_compaction = true,
            pipelined_write = true,
            mega_write_cfs = Self::MEGA_WRITE_CFS.len(),
            high_write_cfs = Self::HIGH_WRITE_CFS.len(),
            column_families = ALL_CFS.len(),
            "RocksDB configuration"
        );
    }

    pub fn is_secondary(&self) -> bool {
        self.is_secondary
    }

    /// Set relaxed L0 thresholds and larger write buffers for bulk sync.
    ///
    /// During bulk sync, 5 parallel writer threads (T1-T7) each commit large
    /// WriteBatches. The default per-CF `max_write_buffer_number=4` can cause
    /// flush stalls when memtables fill faster than background flush can drain.
    /// Increasing to 8 (mega) / 6 (high) gives more headroom before stalling.
    pub fn set_bulk_sync_compaction_options(&self) {
        let mut ok = 0u32;
        let mut fail = 0u32;
        for &cf_name in ALL_CFS {
            if let Some(cf) = self.db.cf_handle(cf_name) {
                // More write buffers = more memtable headroom before flush stall
                let max_wb = if Self::is_mega_write_cf(cf_name) {
                    "8"
                } else if Self::is_high_write_cf(cf_name) {
                    "6"
                } else {
                    "4"
                };
                let result = self.db.set_options_cf(
                    cf,
                    &[
                        ("level0_slowdown_writes_trigger", "64"),
                        ("level0_stop_writes_trigger", "128"),
                        ("max_write_buffer_number", max_wb),
                        ("max_bytes_for_level_base", "2147483648"), // 2 GB
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
        info!(
            ok,
            fail,
            "Bulk sync compaction options set: l0_slowdown=64, l0_stop=128, \
             write_buffers mega=8/high=6/low=4"
        );
    }

    /// Restore normal L0 thresholds and write buffer counts after bulk sync.
    ///
    /// Reverts L0 slowdown/stop triggers to 12/24, write buffers to 4 (mega/high)
    /// or 2 (low), and `max_bytes_for_level_base` to 512 MB.
    pub fn restore_normal_compaction_options(&self) {
        for &cf_name in ALL_CFS {
            if let Some(cf) = self.db.cf_handle(cf_name) {
                let max_wb = if Self::is_mega_write_cf(cf_name) || Self::is_high_write_cf(cf_name) {
                    "4"
                } else {
                    "2"
                };
                let _ = self.db.set_options_cf(
                    cf,
                    &[
                        ("level0_slowdown_writes_trigger", "12"),
                        ("level0_stop_writes_trigger", "24"),
                        ("max_write_buffer_number", max_wb),
                        ("max_bytes_for_level_base", "536870912"), // 512 MB
                    ],
                );
            }
        }
        info!("Normal compaction options restored: l0_slowdown=12, l0_stop=24");
    }

    /// Disable auto-compactions on all column families.
    /// Call during bulk sync to avoid compaction competing with writes.
    pub fn disable_auto_compactions(&self) {
        for cf_name in ALL_CFS.iter().copied() {
            if let Some(cf) = self.db.cf_handle(cf_name) {
                if let Err(e) = self
                    .db
                    .set_options_cf(cf, &[("disable_auto_compactions", "true")])
                {
                    tracing::warn!(cf = cf_name, error = %e, "Failed to disable auto compactions");
                }
            }
        }
        info!("Auto-compactions disabled for bulk sync");
    }

    /// Re-enable auto-compactions on all column families.
    pub fn enable_auto_compactions(&self) {
        for cf_name in ALL_CFS {
            if let Some(cf) = self.db.cf_handle(cf_name) {
                if let Err(e) = self
                    .db
                    .set_options_cf(cf, &[("disable_auto_compactions", "false")])
                {
                    tracing::warn!(cf = cf_name, error = %e, "Failed to enable auto compactions");
                }
            }
        }
        info!("Auto-compactions re-enabled");
    }

    /// Trigger manual compaction on all column families.
    /// Should be called after bulk sync completes and auto-compactions are re-enabled.
    pub fn trigger_full_compaction(&self) {
        info!("Starting manual compaction across all column families");
        for cf_name in ALL_CFS {
            if let Some(cf) = self.db.cf_handle(cf_name) {
                self.db.compact_range_cf(cf, None::<&[u8]>, None::<&[u8]>);
            }
        }
        info!("Manual compaction completed");
    }

    // ---- Memory stats ----

    pub fn memory_stats(&self) -> MemoryStats {
        let mut memtable_bytes = 0usize;
        let mut table_readers_bytes = 0usize;
        let mut live_cells_count = 0usize;
        let mut consumed_cells_count = 0usize;
        let mut consumed_cf_live_data_bytes: Option<u64> = None;
        let mut consumed_cf_sst_files_bytes: Option<u64> = None;
        let mut consumed_cf_memtable_bytes: Option<u64> = None;
        let mut block_headers_count = 0usize;
        let mut addr_balance_count = 0usize;
        let mut compaction_pending_bytes = 0u64;
        let mut num_running_compactions = 0u64;
        let mut sst_files_size = 0u64;
        let mut l0_files_total = 0u64;
        let mut l0_files_max: u64 = 0;
        let mut l0_worst_cf = String::new();
        let mut immutable_memtables = 0u64;
        let mut cf_sizes: Vec<(String, u64)> = Vec::new();

        for &cf_name in ALL_CFS {
            if let Some(cf) = self.db.cf_handle(cf_name) {
                if let Ok(Some(v)) = self
                    .db
                    .property_int_value_cf(cf, "rocksdb.cur-size-all-mem-tables")
                {
                    memtable_bytes += v as usize;
                    if cf_name == CF_CONSUMED_CELLS {
                        consumed_cf_memtable_bytes = Some(v);
                    }
                }
                if let Ok(Some(v)) = self
                    .db
                    .property_int_value_cf(cf, "rocksdb.estimate-table-readers-mem")
                {
                    table_readers_bytes += v as usize;
                }
                if let Ok(Some(v)) = self
                    .db
                    .property_int_value_cf(cf, "rocksdb.estimate-pending-compaction-bytes")
                {
                    compaction_pending_bytes += v;
                }
                if let Ok(Some(v)) = self
                    .db
                    .property_int_value_cf(cf, "rocksdb.estimate-num-keys")
                {
                    match cf_name {
                        CF_LIVE_CELLS => live_cells_count = v as usize,
                        CF_CONSUMED_CELLS => consumed_cells_count = v as usize,
                        CF_BLOCK_HEADERS => block_headers_count = v as usize,
                        CF_ADDR_BALANCE => addr_balance_count = v as usize,
                        _ => {}
                    }
                }
                if let Ok(Some(v)) = self
                    .db
                    .property_int_value_cf(cf, "rocksdb.num-running-compactions")
                {
                    num_running_compactions += v;
                }
                if let Ok(Some(v)) = self
                    .db
                    .property_int_value_cf(cf, "rocksdb.total-sst-files-size")
                {
                    sst_files_size += v;
                    if cf_name == CF_CONSUMED_CELLS {
                        consumed_cf_sst_files_bytes = Some(v);
                    }
                }
                // L0 file count — track both total and per-CF max
                if let Ok(Some(v)) = self
                    .db
                    .property_int_value_cf(cf, "rocksdb.num-files-at-level0")
                {
                    l0_files_total += v;
                    if v > l0_files_max {
                        l0_files_max = v;
                        l0_worst_cf = cf_name.to_string();
                    }
                }
                // Immutable memtables waiting for flush — high values indicate
                // flush can't keep up and writes will stall when all buffers fill.
                if let Ok(Some(v)) = self
                    .db
                    .property_int_value_cf(cf, "rocksdb.num-immutable-mem-table")
                {
                    immutable_memtables += v;
                }
                // Per-CF live data size for top-N display
                if let Ok(Some(v)) = self
                    .db
                    .property_int_value_cf(cf, "rocksdb.estimate-live-data-size")
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

        // Sort by size descending and keep top 5
        cf_sizes.sort_by(|a, b| b.1.cmp(&a.1));
        cf_sizes.truncate(5);
        let (consumed_cells_bytes, consumed_cells_bytes_source) = consumed_cf_storage_bytes(
            consumed_cf_live_data_bytes,
            consumed_cf_sst_files_bytes,
            consumed_cf_memtable_bytes,
        );

        let block_cache_bytes = self.block_cache.get_usage();
        let memory_bytes = memtable_bytes + block_cache_bytes + table_readers_bytes;

        MemoryStats {
            live_cells_count,
            consumed_cells_count,
            consumed_cells_bytes,
            consumed_cells_bytes_source,
            block_headers_count,
            addr_balance_count,
            cells_count: live_cells_count,
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_open_and_close() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        assert!(!store.is_secondary());
        drop(store);
    }

    #[test]
    fn test_all_cfs_accessible() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        for cf_name in ALL_CFS {
            let _ = store.cf(cf_name);
        }
    }

    #[test]
    fn test_put_get_delete() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();

        let cf = store.cf_sync_meta();
        store.put_cf(cf, b"test_key", b"test_value").unwrap();

        let val = store.get_cf(cf, b"test_key").unwrap();
        assert_eq!(val.as_deref(), Some(b"test_value".as_slice()));

        store.delete_cf(cf, b"test_key").unwrap();
        let val = store.get_cf(cf, b"test_key").unwrap();
        assert!(val.is_none());
    }

    #[test]
    fn test_write_batch() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();

        let cf = store.cf_sync_meta();
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
        let primary_dir = TempDir::new().unwrap();
        let secondary_dir = TempDir::new().unwrap();

        let primary = CkbadgerStore::open(primary_dir.path()).unwrap();
        let cf = primary.cf_sync_meta();
        primary.put_cf(cf, b"key", b"value").unwrap();

        let secondary =
            CkbadgerStore::open_secondary(primary_dir.path(), secondary_dir.path()).unwrap();
        assert!(secondary.is_secondary());
        secondary.refresh().unwrap();

        let cf = secondary.cf_sync_meta();
        let val = secondary.get_cf(cf, b"key").unwrap();
        assert_eq!(val.as_deref(), Some(b"value".as_slice()));
    }

    #[test]
    fn test_bulk_sync_mode() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        assert!(!store.is_bulk_sync_mode());
        store.set_bulk_sync_mode(true);
        assert!(store.is_bulk_sync_mode());
        store.set_bulk_sync_mode(false);
        assert!(!store.is_bulk_sync_mode());
    }

    #[test]
    fn test_memory_stats() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        store
            .put_cf(store.cf_live_cells(), b"live-k1", b"live-v1")
            .unwrap();
        store
            .put_cf(store.cf_consumed_cells(), b"consumed-k1", b"consumed-v1")
            .unwrap();
        store
            .put_cf(store.cf_block_headers(), b"hdr-k1", b"hdr-v1")
            .unwrap();
        store
            .put_addr_balance_direct(
                b"addr-k1",
                &crate::types::AddressBalance {
                    balance: 1,
                    ..Default::default()
                },
            )
            .unwrap();

        // Flush written keys so RocksDB estimate properties have observable values.
        store.db.flush_cf(store.cf_live_cells()).unwrap();
        store.db.flush_cf(store.cf_consumed_cells()).unwrap();
        store.db.flush_cf(store.cf_block_headers()).unwrap();
        store.db.flush_cf(store.cf_addr_balance()).unwrap();

        let stats = store.memory_stats();
        assert!(stats.live_cells_count >= 1);
        assert!(stats.consumed_cells_count >= 1);
        assert!(stats.block_headers_count >= 1);
        assert!(stats.addr_balance_count >= 1);
        assert_eq!(stats.cells_count, stats.live_cells_count);
        assert!(
            matches!(
                stats.consumed_cells_bytes_source,
                "live" | "sst" | "mem" | "none"
            ),
            "unexpected source: {}",
            stats.consumed_cells_bytes_source
        );
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
    fn test_mega_write_cfs_excludes_script_info() {
        assert!(
            !CkbadgerStore::is_mega_write_cf(CF_SCRIPT_INFO),
            "CF_SCRIPT_INFO should NOT be in MEGA_WRITE_CFS"
        );
    }

    #[test]
    fn test_mega_write_cfs_expected_members() {
        let expected = &[
            CF_LIVE_CELLS,
            CF_CONSUMED_CELLS,
            CF_CELL_BY_LOCK,
            CF_CELL_BY_TYPE,
            CF_CELL_BY_LOCK_CODE,
            CF_CELL_BY_TYPE_CODE,
            CF_TX_INDEX,
            CF_TX_HASH_MAP,
            CF_ADDR_BALANCE,
            CF_ACTIVITIES,
        ];
        for cf in expected {
            assert!(
                CkbadgerStore::is_mega_write_cf(cf),
                "{cf} should be in MEGA_WRITE_CFS"
            );
        }
        assert_eq!(
            CkbadgerStore::MEGA_WRITE_CFS.len(),
            expected.len(),
            "MEGA_WRITE_CFS length mismatch"
        );
    }

    #[test]
    fn test_set_bulk_sync_compaction_options_does_not_panic() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        // Should not panic on a freshly opened store
        store.set_bulk_sync_compaction_options();
    }

    #[test]
    fn test_restore_normal_compaction_options_does_not_panic() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        // Should not panic on a freshly opened store
        store.restore_normal_compaction_options();
    }

    #[test]
    fn test_bulk_sync_then_restore_compaction_round_trip() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        // Set bulk sync options, then restore — should work without panics
        store.set_bulk_sync_compaction_options();
        store.restore_normal_compaction_options();
        // Verify DB is still operational after option changes
        let cf = store.cf_sync_meta();
        store.put_cf(cf, b"test", b"value").unwrap();
        let val = store.get_cf(cf, b"test").unwrap();
        assert_eq!(val.as_deref(), Some(b"value".as_slice()));
    }

    #[test]
    fn test_log_config_does_not_panic() {
        CkbadgerStore::log_config();
    }
}
