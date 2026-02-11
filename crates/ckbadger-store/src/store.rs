//! Core RocksDB store with 25 column families.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::info;

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
pub const CF_ACTIVITIES: &str = "activities";
pub const CF_ACTIVITIES_BY_ADDR: &str = "activities_by_addr";
pub const CF_DAO_DEPOSITS: &str = "dao_deposits";
pub const CF_DAO_BY_WITHDRAW_TX: &str = "dao_by_withdraw_tx";
pub const CF_DAO_STATS: &str = "dao_stats";
pub const CF_BLOCK_ISSUANCE: &str = "block_issuance";
pub const CF_TOKENS: &str = "tokens";
pub const CF_TOKEN_HOLDERS: &str = "token_holders";
pub const CF_SPORE_DATA: &str = "spore_data";
pub const CF_NFT_DATA: &str = "nft_data";
pub const CF_STATS: &str = "stats";
pub const CF_SCRIPT_INFO: &str = "script_info";
pub const CF_SYNC_META: &str = "sync_meta";
pub const CF_TASKS: &str = "tasks";
pub const CF_TASKS_INDEX: &str = "tasks_index";

/// All column family names, used during DB open.
pub const ALL_CFS: &[&str] = &[
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
    CF_ACTIVITIES,
    CF_ACTIVITIES_BY_ADDR,
    CF_DAO_DEPOSITS,
    CF_DAO_BY_WITHDRAW_TX,
    CF_DAO_STATS,
    CF_BLOCK_ISSUANCE,
    CF_TOKENS,
    CF_TOKEN_HOLDERS,
    CF_SPORE_DATA,
    CF_NFT_DATA,
    CF_STATS,
    CF_SCRIPT_INFO,
    CF_SYNC_META,
    CF_TASKS,
    CF_TASKS_INDEX,
];

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
    pub fn open<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let (opts, block_cache) = Self::default_options();

        let cf_descriptors: Vec<ColumnFamilyDescriptor> = ALL_CFS
            .iter()
            .map(|name| ColumnFamilyDescriptor::new(*name, Self::cf_options(name, &block_cache)))
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

        let cf_names: Vec<&str> = ALL_CFS.to_vec();
        let db = DB::open_cf_as_secondary(&opts, primary_path, secondary_path, cf_names)?;

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
        CF_ACTIVITIES,
        CF_ACTIVITIES_BY_ADDR,
        CF_DAO_DEPOSITS,
    ];

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

        // Background jobs: 16 threads shared across 25 CFs for flush + compaction
        // With 7 writer threads (T1-T7), RocksDB needs more background threads
        // for concurrent flush + compaction across all CFs on 24-core machines
        opts.set_max_background_jobs(16);
        // Allow large compaction jobs to use multiple threads
        opts.set_max_subcompactions(4);

        // Bypass OS page cache for flush/compaction to avoid cache pollution
        opts.set_use_direct_io_for_flush_and_compaction(true);

        // Pipeline WAL write and memtable insert for concurrent writers
        opts.set_enable_pipelined_write(true);

        // 8 GB block cache — system has 93 GB RAM; 2 GB only covered ~17% of SST data
        let block_cache = rocksdb::Cache::new_lru_cache(8 * 1024 * 1024 * 1024);
        let block_opts = Self::default_block_options(&block_cache);
        opts.set_block_based_table_factory(&block_opts);

        (opts, block_cache)
    }

    /// Per-CF options: low-write CFs get smaller write buffers (32 MB) to reduce
    /// memtable overhead and free memory for the block cache.
    fn cf_options(name: &str, block_cache: &rocksdb::Cache) -> Options {
        let mut opts = Options::default();

        if Self::is_high_write_cf(name) {
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
    pub fn cf_activities(&self) -> &ColumnFamily {
        self.cf(CF_ACTIVITIES)
    }
    pub fn cf_activities_by_addr(&self) -> &ColumnFamily {
        self.cf(CF_ACTIVITIES_BY_ADDR)
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
    pub fn cf_stats(&self) -> &ColumnFamily {
        self.cf(CF_STATS)
    }
    pub fn cf_script_info(&self) -> &ColumnFamily {
        self.cf(CF_SCRIPT_INFO)
    }
    pub fn cf_sync_meta(&self) -> &ColumnFamily {
        self.cf(CF_SYNC_META)
    }
    pub fn cf_tasks(&self) -> &ColumnFamily {
        self.cf(CF_TASKS)
    }
    pub fn cf_tasks_index(&self) -> &ColumnFamily {
        self.cf(CF_TASKS_INDEX)
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
            write_buffer_high_mb = 128,
            write_buffer_low_mb = 32,
            max_write_buffers_high = 4,
            max_write_buffers_low = 2,
            l0_slowdown = 12,
            l0_stop = 24,
            l0_slowdown_bulk = 64,
            l0_stop_bulk = 128,
            max_background_jobs = 10,
            max_subcompactions = 3,
            block_cache_gb = 8,
            direct_io_compaction = true,
            pipelined_write = true,
            high_write_cfs = Self::HIGH_WRITE_CFS.len(),
            column_families = ALL_CFS.len(),
            "RocksDB configuration"
        );
    }

    pub fn is_secondary(&self) -> bool {
        self.is_secondary
    }

    /// Set relaxed L0 thresholds for bulk sync while keeping compaction enabled.
    /// This prevents write stalls by raising slowdown/stop triggers, while still
    /// allowing background compaction threads to drain L0 files.
    pub fn set_bulk_sync_compaction_options(&self) {
        for cf_name in ALL_CFS {
            if let Some(cf) = self.db.cf_handle(cf_name) {
                let _ = self.db.set_options_cf(
                    cf,
                    &[
                        ("level0_slowdown_writes_trigger", "64"),
                        ("level0_stop_writes_trigger", "128"),
                    ],
                );
            }
        }
        info!("Bulk sync compaction options set: l0_slowdown=64, l0_stop=128");
    }

    /// Restore normal L0 thresholds after bulk sync completes.
    pub fn restore_normal_compaction_options(&self) {
        for cf_name in ALL_CFS {
            if let Some(cf) = self.db.cf_handle(cf_name) {
                let _ = self.db.set_options_cf(
                    cf,
                    &[
                        ("level0_slowdown_writes_trigger", "12"),
                        ("level0_stop_writes_trigger", "24"),
                    ],
                );
            }
        }
        info!("Normal compaction options restored: l0_slowdown=12, l0_stop=24");
    }

    /// Disable auto-compactions on all column families.
    /// Call during bulk sync to avoid compaction competing with writes.
    pub fn disable_auto_compactions(&self) {
        for cf_name in ALL_CFS {
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
        let mut compaction_pending_bytes = 0u64;
        let mut num_running_compactions = 0u64;
        let mut sst_files_size = 0u64;
        let mut l0_files_count = 0u64;
        let mut cf_sizes: Vec<(String, u64)> = Vec::new();

        for cf_name in ALL_CFS {
            if let Some(cf) = self.db.cf_handle(cf_name) {
                if let Ok(Some(v)) = self
                    .db
                    .property_int_value_cf(cf, "rocksdb.cur-size-all-mem-tables")
                {
                    memtable_bytes += v as usize;
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
                    .property_int_value_cf(cf, "rocksdb.num-running-compactions")
                {
                    num_running_compactions += v;
                }
                if let Ok(Some(v)) = self
                    .db
                    .property_int_value_cf(cf, "rocksdb.total-sst-files-size")
                {
                    sst_files_size += v;
                }
                // L0 file count — tracks compaction backlog / write stall risk
                if let Ok(Some(v)) = self
                    .db
                    .property_int_value_cf(cf, "rocksdb.num-files-at-level0")
                {
                    l0_files_count += v;
                }
                // Per-CF live data size for top-N display
                if let Ok(Some(v)) = self
                    .db
                    .property_int_value_cf(cf, "rocksdb.estimate-live-data-size")
                {
                    if v > 0 {
                        cf_sizes.push((cf_name.to_string(), v));
                    }
                }
            }
        }

        // Sort by size descending and keep top 5
        cf_sizes.sort_by(|a, b| b.1.cmp(&a.1));
        cf_sizes.truncate(5);

        let block_cache_bytes = self.block_cache.get_usage();
        let memory_bytes = memtable_bytes + block_cache_bytes + table_readers_bytes;

        MemoryStats {
            cells_count: 0,
            memory_bytes,
            memtable_bytes,
            block_cache_bytes,
            table_readers_bytes,
            compaction_pending_bytes,
            num_running_compactions,
            sst_files_size,
            l0_files_count,
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
        let stats = store.memory_stats();
        // Just verify it doesn't panic and returns reasonable values
        let _ = stats; // Just verify it doesn't panic
    }
}
