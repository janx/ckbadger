//! RocksDB-backed storage with multiple Column Families for:
//! - Live cells: O(1) lookup for unspent cells
//! - Consumed cells: Recently consumed cells (reduces PostgreSQL fallback)
//! - Block headers: block_number -> header + hash reverse index
//! - DAO cache: block_number -> 32-byte DAO field

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use rocksdb::{ColumnFamily, ColumnFamilyDescriptor, DBCompressionType, Options, DB};
use sqlx::PgPool;
use std::sync::RwLock;

use super::live_cell_storage::{
    CachedBlockHeader, ConsumedCellRecord, LiveCellInfo, LiveCellStorage, LiveCellStorageAsync,
    MemoryStats,
};

const KEY_SIZE: usize = 34;

const CF_LIVE_CELLS: &str = "live_cells";
const CF_CONSUMED_CELLS: &str = "consumed_cells";
const CF_BLOCK_HEADERS: &str = "block_headers";
const CF_BLOCK_HASH_INDEX: &str = "block_hash_index";
const CF_DAO_DEPOSITS: &str = "dao_deposits";
const CF_DAO_DEPOSIT_BY_WITHDRAW_TX: &str = "dao_deposit_by_withdraw_tx";

const MAX_CONSUMED_HISTORY_BLOCKS: i64 = 1000;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
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

type DaoDepositCacheList = Vec<(Vec<u8>, i16, DaoDepositCacheEntry)>;

pub struct RocksDbLiveCellStore {
    db: DB,
    /// Keep block cache alive for the lifetime of the store.
    /// Without this, the cache is dropped when `open()` returns.
    block_cache: rocksdb::Cache,
    consumed_history: RwLock<VecDeque<ConsumedCellRecord>>,
    max_history_blocks: i64,
    bulk_sync_mode: AtomicBool,
    bulk_sync_cell_cache_enabled: bool,
}

impl RocksDbLiveCellStore {
    pub fn open<P: AsRef<Path>>(
        path: P,
        bulk_sync_cell_cache_enabled: bool,
    ) -> anyhow::Result<Self> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);

        opts.set_write_buffer_size(256 * 1024 * 1024);
        opts.set_max_write_buffer_number(4);
        opts.set_level_zero_file_num_compaction_trigger(4);
        opts.set_max_bytes_for_level_base(512 * 1024 * 1024);
        opts.set_compression_type(DBCompressionType::Lz4);

        let block_cache = rocksdb::Cache::new_lru_cache(512 * 1024 * 1024);
        let mut block_opts = rocksdb::BlockBasedOptions::default();
        block_opts.set_block_size(16 * 1024);
        block_opts.set_block_cache(&block_cache);
        block_opts.set_cache_index_and_filter_blocks(true);
        block_opts.set_bloom_filter(10.0, false);
        opts.set_block_based_table_factory(&block_opts);

        let cf_descriptors = vec![
            ColumnFamilyDescriptor::new(CF_LIVE_CELLS, opts.clone()),
            ColumnFamilyDescriptor::new(CF_CONSUMED_CELLS, opts.clone()),
            ColumnFamilyDescriptor::new(CF_BLOCK_HEADERS, opts.clone()),
            ColumnFamilyDescriptor::new(CF_BLOCK_HASH_INDEX, opts.clone()),
            ColumnFamilyDescriptor::new(CF_DAO_DEPOSITS, opts.clone()),
            ColumnFamilyDescriptor::new(CF_DAO_DEPOSIT_BY_WITHDRAW_TX, opts.clone()),
        ];

        let db = DB::open_cf_descriptors(&opts, path, cf_descriptors)?;

        Ok(Self {
            db,
            block_cache,
            consumed_history: RwLock::new(VecDeque::new()),
            max_history_blocks: 36,
            bulk_sync_mode: AtomicBool::new(false),
            bulk_sync_cell_cache_enabled,
        })
    }

    fn cf_live_cells(&self) -> &ColumnFamily {
        self.db.cf_handle(CF_LIVE_CELLS).expect("CF live_cells")
    }

    fn cf_consumed_cells(&self) -> &ColumnFamily {
        self.db
            .cf_handle(CF_CONSUMED_CELLS)
            .expect("CF consumed_cells")
    }

    fn cf_block_headers(&self) -> &ColumnFamily {
        self.db
            .cf_handle(CF_BLOCK_HEADERS)
            .expect("CF block_headers")
    }

    fn cf_block_hash_index(&self) -> &ColumnFamily {
        self.db
            .cf_handle(CF_BLOCK_HASH_INDEX)
            .expect("CF block_hash_index")
    }

    fn cf_dao_deposits(&self) -> &ColumnFamily {
        self.db.cf_handle(CF_DAO_DEPOSITS).expect("CF dao_deposits")
    }

    fn cf_dao_deposit_by_withdraw_tx(&self) -> &ColumnFamily {
        self.db
            .cf_handle(CF_DAO_DEPOSIT_BY_WITHDRAW_TX)
            .expect("CF dao_deposit_by_withdraw_tx")
    }

    fn encode_cell_key(tx_hash: &[u8], output_index: i16) -> [u8; KEY_SIZE] {
        let mut key = [0u8; KEY_SIZE];
        key[..32].copy_from_slice(tx_hash);
        key[32..34].copy_from_slice(&output_index.to_be_bytes());
        key
    }

    fn decode_cell_key(key: &[u8]) -> (Vec<u8>, i16) {
        let tx_hash = key[..32].to_vec();
        let output_index = i16::from_be_bytes([key[32], key[33]]);
        (tx_hash, output_index)
    }

    fn encode_block_number_key(block_number: i64) -> [u8; 8] {
        block_number.to_be_bytes()
    }

    fn decode_block_number_key(key: &[u8]) -> i64 {
        i64::from_be_bytes(key.try_into().unwrap_or([0; 8]))
    }

    fn insert_internal(&self, tx_hash: &[u8], output_index: i16, info: &LiveCellInfo) {
        let key = Self::encode_cell_key(tx_hash, output_index);
        let value = bincode::serialize(info).expect("serialize LiveCellInfo");
        self.db
            .put_cf(self.cf_live_cells(), key, &value)
            .expect("put to live_cells CF");
    }

    fn insert_consumed_internal(&self, tx_hash: &[u8], output_index: i16, info: &LiveCellInfo) {
        let key = Self::encode_cell_key(tx_hash, output_index);
        let value = bincode::serialize(info).expect("serialize LiveCellInfo");
        self.db
            .put_cf(self.cf_consumed_cells(), key, &value)
            .expect("put to consumed_cells CF");
    }

    fn prune_old_consumed_cells(&self, current_block: i64) {
        if self.bulk_sync_cell_cache_enabled && self.bulk_sync_mode.load(Ordering::Relaxed) {
            return;
        }

        let cutoff_block = current_block - MAX_CONSUMED_HISTORY_BLOCKS;
        if cutoff_block <= 0 {
            return;
        }

        let cf = self.cf_consumed_cells();
        let mut batch = rocksdb::WriteBatch::default();
        let iter = self.db.iterator_cf(cf, rocksdb::IteratorMode::Start);

        for item in iter.flatten() {
            let (key, value) = item;
            if let Ok(info) = bincode::deserialize::<LiveCellInfo>(&value) {
                if info.created_at_block < cutoff_block {
                    batch.delete_cf(cf, &key);
                }
            }
        }

        if !batch.is_empty() {
            let _ = self.db.write(batch);
        }
    }
}

impl LiveCellStorage for RocksDbLiveCellStore {
    fn insert(&self, tx_hash: Vec<u8>, output_index: i16, info: LiveCellInfo) {
        self.insert_internal(&tx_hash, output_index, &info);
    }

    fn get(&self, tx_hash: &[u8], output_index: i16) -> Option<LiveCellInfo> {
        let key = Self::encode_cell_key(tx_hash, output_index);
        match self.db.get_cf(self.cf_live_cells(), key) {
            Ok(Some(value)) => bincode::deserialize(&value).ok(),
            _ => None,
        }
    }

    fn remove(&self, tx_hash: &[u8], output_index: i16) -> Option<LiveCellInfo> {
        let key = Self::encode_cell_key(tx_hash, output_index);
        let cf = self.cf_live_cells();

        let existing = match self.db.get_cf(cf, key) {
            Ok(Some(value)) => bincode::deserialize(&value).ok(),
            _ => None,
        };

        if let Some(ref info) = existing {
            let _ = self.db.delete_cf(cf, key);
            self.insert_consumed_internal(tx_hash, output_index, info);
        }

        existing
    }

    fn get_batch(&self, outpoints: &[(&[u8], i16)]) -> HashMap<(Vec<u8>, i16), LiveCellInfo> {
        let mut result = HashMap::with_capacity(outpoints.len());

        let keys: Vec<[u8; KEY_SIZE]> = outpoints
            .iter()
            .map(|(tx_hash, idx)| Self::encode_cell_key(tx_hash, *idx))
            .collect();

        let cf = self.cf_live_cells();
        let cf_keys: Vec<(&ColumnFamily, &[u8])> =
            keys.iter().map(|k| (cf, k.as_slice())).collect();
        let values = self.db.multi_get_cf(cf_keys);

        for (i, value_result) in values.into_iter().enumerate() {
            if let Ok(Some(value)) = value_result {
                if let Ok(info) = bincode::deserialize::<LiveCellInfo>(&value) {
                    let (tx_hash, idx) = outpoints[i];
                    result.insert((tx_hash.to_vec(), idx), info);
                }
            }
        }

        result
    }

    fn len(&self) -> usize {
        let mut count = 0;
        let iter = self
            .db
            .iterator_cf(self.cf_live_cells(), rocksdb::IteratorMode::Start);
        for _ in iter.flatten() {
            count += 1;
        }
        count
    }

    fn clear(&self) {
        let cf = self.cf_live_cells();
        let mut batch = rocksdb::WriteBatch::default();
        let iter = self.db.iterator_cf(cf, rocksdb::IteratorMode::Start);
        for item in iter.flatten() {
            batch.delete_cf(cf, &item.0);
        }
        let _ = self.db.write(batch);
    }

    fn record_consumption(
        &self,
        tx_hash: Vec<u8>,
        output_index: i16,
        info: LiveCellInfo,
        consumed_at_block: i64,
    ) {
        let record = ConsumedCellRecord {
            tx_hash,
            output_index,
            info,
            consumed_at_block,
        };

        let mut history = self.consumed_history.write().unwrap();
        history.push_back(record);

        let skip_prune =
            self.bulk_sync_cell_cache_enabled && self.bulk_sync_mode.load(Ordering::Relaxed);

        if !skip_prune {
            while let Some(oldest) = history.front() {
                if consumed_at_block - oldest.consumed_at_block > self.max_history_blocks {
                    history.pop_front();
                } else {
                    break;
                }
            }
        }

        drop(history);

        if consumed_at_block % 100 == 0 {
            self.prune_old_consumed_cells(consumed_at_block);
        }
    }

    fn rollback_to_block(&self, rollback_to: i64) -> (usize, usize) {
        let mut removed = 0;
        let mut restored = 0;

        let cf = self.cf_live_cells();
        let mut to_remove = Vec::new();
        let iter = self.db.iterator_cf(cf, rocksdb::IteratorMode::Start);
        for item in iter.flatten() {
            let (key, value) = item;
            if let Ok(info) = bincode::deserialize::<LiveCellInfo>(&value) {
                if info.created_at_block > rollback_to {
                    to_remove.push(key.to_vec());
                }
            }
        }

        let mut batch = rocksdb::WriteBatch::default();
        for key in to_remove {
            batch.delete_cf(cf, &key);
            removed += 1;
        }
        let _ = self.db.write(batch);

        {
            let history = self
                .consumed_history
                .read()
                .expect("consumed_history lock poisoned");
            let to_restore: Vec<_> = history
                .iter()
                .filter(|r| r.consumed_at_block > rollback_to)
                .cloned()
                .collect();
            drop(history);

            for record in to_restore {
                self.insert_internal(&record.tx_hash, record.output_index, &record.info);
                restored += 1;
            }
        }

        {
            let mut history = self
                .consumed_history
                .write()
                .expect("consumed_history lock poisoned");
            history.retain(|r| r.consumed_at_block <= rollback_to);
        }

        (removed, restored)
    }

    fn cells_created_since(&self, block_number: i64) -> Vec<(Vec<u8>, i16, LiveCellInfo)> {
        let mut result = Vec::new();
        let iter = self
            .db
            .iterator_cf(self.cf_live_cells(), rocksdb::IteratorMode::Start);
        for item in iter.flatten() {
            let (key, value) = item;
            if let Ok(info) = bincode::deserialize::<LiveCellInfo>(&value) {
                if info.created_at_block > block_number {
                    let (tx_hash, output_index) = Self::decode_cell_key(&key);
                    result.push((tx_hash, output_index, info));
                }
            }
        }
        result
    }

    fn memory_stats(&self) -> MemoryStats {
        let cells_count = self.len();

        let cfs = [
            self.cf_live_cells(),
            self.cf_consumed_cells(),
            self.cf_block_headers(),
            self.cf_block_hash_index(),
            self.cf_dao_deposits(),
            self.cf_dao_deposit_by_withdraw_tx(),
        ];
        let memtable_bytes: usize = cfs
            .iter()
            .filter_map(|cf| {
                self.db
                    .property_int_value_cf(cf, "rocksdb.cur-size-all-mem-tables")
                    .ok()
                    .flatten()
            })
            .map(|v| v as usize)
            .sum();

        let block_cache_bytes = self.block_cache.get_usage();

        let table_readers_bytes: usize = cfs
            .iter()
            .filter_map(|cf| {
                self.db
                    .property_int_value_cf(cf, "rocksdb.estimate-table-readers-mem")
                    .ok()
                    .flatten()
            })
            .map(|v| v as usize)
            .sum();

        let memory_bytes = memtable_bytes + block_cache_bytes + table_readers_bytes;

        MemoryStats {
            cells_count,
            memory_bytes,
            memtable_bytes,
            block_cache_bytes,
            table_readers_bytes,
            fragmentation_ratio: 0.0,
        }
    }

    fn backend_name(&self) -> &'static str {
        "rocksdb"
    }

    fn insert_block_header(&self, block_number: i64, header: CachedBlockHeader) {
        let key = Self::encode_block_number_key(block_number);
        let value = bincode::serialize(&header).expect("serialize CachedBlockHeader");
        self.db
            .put_cf(self.cf_block_headers(), key, &value)
            .expect("put block_header");

        self.db
            .put_cf(
                self.cf_block_hash_index(),
                &header.hash,
                block_number.to_le_bytes(),
            )
            .expect("put block_hash_index");
    }

    fn get_block_header(&self, block_number: i64) -> Option<CachedBlockHeader> {
        let key = Self::encode_block_number_key(block_number);
        match self.db.get_cf(self.cf_block_headers(), key) {
            Ok(Some(value)) => bincode::deserialize(&value).ok(),
            _ => None,
        }
    }

    fn get_block_number_by_hash(&self, hash: &[u8]) -> Option<i64> {
        match self.db.get_cf(self.cf_block_hash_index(), hash) {
            Ok(Some(value)) => {
                if value.len() == 8 {
                    Some(i64::from_le_bytes(value[..8].try_into().ok()?))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn get_dao_field(&self, block_number: i64) -> Option<Vec<u8>> {
        self.get_block_header(block_number).map(|h| h.dao)
    }

    fn get_dao_fields_batch(&self, block_numbers: &[i64]) -> HashMap<i64, Vec<u8>> {
        let mut result = HashMap::with_capacity(block_numbers.len());

        let keys: Vec<[u8; 8]> = block_numbers
            .iter()
            .map(|n| Self::encode_block_number_key(*n))
            .collect();

        let cf = self.cf_block_headers();
        let cf_keys: Vec<(&ColumnFamily, &[u8])> =
            keys.iter().map(|k| (cf, k.as_slice())).collect();
        let values = self.db.multi_get_cf(cf_keys);

        for (i, value_result) in values.into_iter().enumerate() {
            if let Ok(Some(value)) = value_result {
                if let Ok(header) = bincode::deserialize::<CachedBlockHeader>(&value) {
                    result.insert(block_numbers[i], header.dao);
                }
            }
        }

        result
    }

    fn get_consumed_cell(&self, tx_hash: &[u8], output_index: i16) -> Option<LiveCellInfo> {
        let key = Self::encode_cell_key(tx_hash, output_index);
        match self.db.get_cf(self.cf_consumed_cells(), key) {
            Ok(Some(value)) => bincode::deserialize(&value).ok(),
            _ => None,
        }
    }

    fn get_consumed_cells_batch(
        &self,
        outpoints: &[(&[u8], i16)],
    ) -> HashMap<(Vec<u8>, i16), LiveCellInfo> {
        let mut result = HashMap::with_capacity(outpoints.len());

        let keys: Vec<[u8; KEY_SIZE]> = outpoints
            .iter()
            .map(|(tx_hash, idx)| Self::encode_cell_key(tx_hash, *idx))
            .collect();

        let cf = self.cf_consumed_cells();
        let cf_keys: Vec<(&ColumnFamily, &[u8])> =
            keys.iter().map(|k| (cf, k.as_slice())).collect();
        let values = self.db.multi_get_cf(cf_keys);

        for (i, value_result) in values.into_iter().enumerate() {
            if let Ok(Some(value)) = value_result {
                if let Ok(info) = bincode::deserialize::<LiveCellInfo>(&value) {
                    let (tx_hash, idx) = outpoints[i];
                    result.insert((tx_hash.to_vec(), idx), info);
                }
            }
        }

        result
    }

    fn rollback_block_cache(&self, rollback_to: i64) {
        let cf_headers = self.cf_block_headers();
        let cf_hash_idx = self.cf_block_hash_index();
        let cf_consumed = self.cf_consumed_cells();

        let mut batch = rocksdb::WriteBatch::default();

        let iter = self
            .db
            .iterator_cf(cf_headers, rocksdb::IteratorMode::Start);
        for item in iter.flatten() {
            let (key, value) = item;
            let block_num = Self::decode_block_number_key(&key);
            if block_num > rollback_to {
                batch.delete_cf(cf_headers, &key);
                if let Ok(header) = bincode::deserialize::<CachedBlockHeader>(&value) {
                    batch.delete_cf(cf_hash_idx, &header.hash);
                }
            }
        }

        let iter = self
            .db
            .iterator_cf(cf_consumed, rocksdb::IteratorMode::Start);
        for item in iter.flatten() {
            let (key, value) = item;
            if let Ok(info) = bincode::deserialize::<LiveCellInfo>(&value) {
                if info.created_at_block > rollback_to {
                    batch.delete_cf(cf_consumed, &key);
                }
            }
        }

        let _ = self.db.write(batch);
    }

    fn set_bulk_sync_mode(&self, enabled: bool) {
        let was_enabled = self.bulk_sync_mode.swap(enabled, Ordering::SeqCst);
        if was_enabled != enabled {
            if enabled {
                tracing::info!("Bulk sync cell cache: ENABLED (consumed cells prune suspended)");
            } else {
                tracing::info!("Bulk sync cell cache: DISABLED (resuming normal prune)");
            }
        }
    }

    fn is_bulk_sync_mode(&self) -> bool {
        self.bulk_sync_mode.load(Ordering::Relaxed)
    }

    fn cleanup_consumed_cells(&self) -> usize {
        let cf = self.cf_consumed_cells();
        let mut batch = rocksdb::WriteBatch::default();
        let mut count = 0;

        let iter = self.db.iterator_cf(cf, rocksdb::IteratorMode::Start);
        for item in iter.flatten() {
            batch.delete_cf(cf, &item.0);
            count += 1;
        }

        if !batch.is_empty() {
            let _ = self.db.write(batch);
        }

        let mut history = self.consumed_history.write().unwrap();
        history.clear();

        if count > 0 {
            tracing::info!("Cleaned up {} consumed cells from RocksDB", count);
        }
        count
    }

    fn consumed_cells_stats(&self) -> (usize, usize) {
        let cf = self.cf_consumed_cells();
        let mut count = 0;
        let mut bytes = 0;

        let iter = self.db.iterator_cf(cf, rocksdb::IteratorMode::Start);
        for item in iter.flatten() {
            count += 1;
            bytes += item.0.len() + item.1.len();
        }

        (count, bytes)
    }

    fn block_headers_count(&self) -> usize {
        let cf = self.cf_block_headers();
        let iter = self.db.iterator_cf(cf, rocksdb::IteratorMode::Start);
        iter.flatten().count()
    }

    fn is_bulk_sync_cell_cache_enabled(&self) -> bool {
        self.bulk_sync_cell_cache_enabled
    }
}

impl RocksDbLiveCellStore {
    /// Iterate over all live cells in batches.
    ///
    /// This is used by the `LiveCellsPopulate` task to populate the PostgreSQL
    /// `live_cells` table from RocksDB data. The callback is invoked for each
    /// batch with `(tx_hash, output_index, LiveCellInfo)` tuples.
    ///
    /// Returns the total number of cells iterated.
    pub fn iter_live_cells_batched<F>(&self, batch_size: usize, mut callback: F) -> usize
    where
        F: FnMut(Vec<(Vec<u8>, i16, LiveCellInfo)>),
    {
        let mut batch = Vec::with_capacity(batch_size);
        let mut total = 0;

        let iter = self
            .db
            .iterator_cf(self.cf_live_cells(), rocksdb::IteratorMode::Start);

        for item in iter.flatten() {
            let (key, value) = item;
            if let Ok(info) = bincode::deserialize::<LiveCellInfo>(&value) {
                let (tx_hash, output_index) = Self::decode_cell_key(&key);
                batch.push((tx_hash, output_index, info));
                total += 1;

                if batch.len() >= batch_size {
                    callback(std::mem::take(&mut batch));
                    batch = Vec::with_capacity(batch_size);
                }
            }
        }

        if !batch.is_empty() {
            callback(batch);
        }

        total
    }

    /// Get the total count of live cells (more efficient than len() for large stores).
    pub fn count_live_cells(&self) -> u64 {
        let mut count = 0u64;
        let iter = self
            .db
            .iterator_cf(self.cf_live_cells(), rocksdb::IteratorMode::Start);
        for _ in iter.flatten() {
            count += 1;
        }
        count
    }

    pub fn insert_dao_deposit(
        &self,
        tx_hash: &[u8],
        output_index: i16,
        entry: &DaoDepositCacheEntry,
    ) {
        let key = Self::encode_cell_key(tx_hash, output_index);
        let value = bincode::serialize(entry).expect("serialize DaoDepositCacheEntry");
        self.db
            .put_cf(self.cf_dao_deposits(), key, &value)
            .expect("put to dao_deposits CF");
    }

    pub fn get_dao_deposit(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> Option<DaoDepositCacheEntry> {
        let key = Self::encode_cell_key(tx_hash, output_index);
        match self.db.get_cf(self.cf_dao_deposits(), key) {
            Ok(Some(value)) => bincode::deserialize(&value).ok(),
            _ => None,
        }
    }

    pub fn get_dao_deposits_batch(
        &self,
        outpoints: &[(&[u8], i16)],
    ) -> HashMap<(Vec<u8>, i16), DaoDepositCacheEntry> {
        let mut result = HashMap::with_capacity(outpoints.len());

        let keys: Vec<[u8; KEY_SIZE]> = outpoints
            .iter()
            .map(|(tx_hash, idx)| Self::encode_cell_key(tx_hash, *idx))
            .collect();

        let cf = self.cf_dao_deposits();
        let cf_keys: Vec<(&ColumnFamily, &[u8])> =
            keys.iter().map(|k| (cf, k.as_slice())).collect();
        let values = self.db.multi_get_cf(cf_keys);

        for (i, value_result) in values.into_iter().enumerate() {
            if let Ok(Some(value)) = value_result {
                if let Ok(entry) = bincode::deserialize::<DaoDepositCacheEntry>(&value) {
                    let (tx_hash, idx) = outpoints[i];
                    result.insert((tx_hash.to_vec(), idx), entry);
                }
            }
        }

        result
    }

    pub fn get_dao_deposits_by_withdraw_tx(&self, withdraw_tx: &[u8]) -> DaoDepositCacheList {
        let cf_secondary = self.cf_dao_deposit_by_withdraw_tx();
        let cf_primary = self.cf_dao_deposits();
        let mut result = Vec::new();

        if let Ok(Some(primary_key)) = self.db.get_cf(cf_secondary, withdraw_tx) {
            if primary_key.len() == KEY_SIZE {
                if let Ok(Some(value)) = self.db.get_cf(cf_primary, &primary_key) {
                    if let Ok(entry) = bincode::deserialize::<DaoDepositCacheEntry>(&value) {
                        let (tx_hash, output_index) = Self::decode_cell_key(&primary_key);
                        result.push((tx_hash, output_index, entry));
                    }
                }
            }
        }

        result
    }

    pub fn get_dao_deposits_by_withdraw_tx_batch(
        &self,
        tx_hashes: &[Vec<u8>],
    ) -> HashMap<Vec<u8>, DaoDepositCacheList> {
        let mut result: HashMap<Vec<u8>, DaoDepositCacheList> =
            HashMap::with_capacity(tx_hashes.len());

        let cf_secondary = self.cf_dao_deposit_by_withdraw_tx();
        let cf_primary = self.cf_dao_deposits();

        let cf_keys: Vec<(&ColumnFamily, &[u8])> = tx_hashes
            .iter()
            .map(|h| (cf_secondary, h.as_slice()))
            .collect();
        let values = self.db.multi_get_cf(cf_keys);

        for (i, value_result) in values.into_iter().enumerate() {
            if let Ok(Some(primary_key)) = value_result {
                if primary_key.len() != KEY_SIZE {
                    continue;
                }
                if let Ok(Some(value)) = self.db.get_cf(cf_primary, &primary_key) {
                    if let Ok(entry) = bincode::deserialize::<DaoDepositCacheEntry>(&value) {
                        let (tx_hash, output_index) = Self::decode_cell_key(&primary_key);
                        result.entry(tx_hashes[i].clone()).or_default().push((
                            tx_hash,
                            output_index,
                            entry,
                        ));
                    }
                }
            }
        }

        result
    }

    pub fn update_dao_deposit_status(
        &self,
        tx_hash: &[u8],
        output_index: i16,
        entry: &DaoDepositCacheEntry,
    ) {
        let key = Self::encode_cell_key(tx_hash, output_index);
        let cf_primary = self.cf_dao_deposits();
        let cf_secondary = self.cf_dao_deposit_by_withdraw_tx();
        let existing = self.get_dao_deposit(tx_hash, output_index);

        let mut batch = rocksdb::WriteBatch::default();

        let value = bincode::serialize(entry).expect("serialize DaoDepositCacheEntry");
        batch.put_cf(cf_primary, key, value);

        if let Some(old_entry) = existing {
            if let Some(old_withdraw_tx) = old_entry.withdraw_request_tx {
                let should_remove = entry.status == 2
                    || entry
                        .withdraw_request_tx
                        .as_ref()
                        .map(|tx| tx.as_slice() != old_withdraw_tx.as_slice())
                        .unwrap_or(true);
                if should_remove {
                    batch.delete_cf(cf_secondary, old_withdraw_tx);
                }
            }
        }

        if entry.status == 1 {
            if let Some(ref withdraw_tx) = entry.withdraw_request_tx {
                batch.put_cf(cf_secondary, withdraw_tx, key);
            }
        }

        let _ = self.db.write(batch);
    }

    pub fn rollback_dao_deposits(&self, rollback_to: i64) -> (usize, usize) {
        let cf_primary = self.cf_dao_deposits();
        let cf_secondary = self.cf_dao_deposit_by_withdraw_tx();
        let mut removed_primary = 0;
        let mut removed_secondary = 0;

        let mut batch = rocksdb::WriteBatch::default();
        let iter = self
            .db
            .iterator_cf(cf_primary, rocksdb::IteratorMode::Start);

        for item in iter.flatten() {
            let (key, value) = item;
            if let Ok(entry) = bincode::deserialize::<DaoDepositCacheEntry>(&value) {
                if entry.deposit_block_number > rollback_to {
                    batch.delete_cf(cf_primary, &key);
                    removed_primary += 1;
                    if let Some(withdraw_tx) = entry.withdraw_request_tx {
                        batch.delete_cf(cf_secondary, withdraw_tx);
                        removed_secondary += 1;
                    }
                }
            }
        }

        let _ = self.db.write(batch);
        (removed_primary, removed_secondary)
    }

    pub fn count_dao_deposits(&self) -> usize {
        let mut count = 0;
        let iter = self
            .db
            .iterator_cf(self.cf_dao_deposits(), rocksdb::IteratorMode::Start);
        for _ in iter.flatten() {
            count += 1;
        }
        count
    }

    pub fn iter_dao_deposits_batched<F>(&self, batch_size: usize, mut callback: F) -> usize
    where
        F: FnMut(DaoDepositCacheList),
    {
        let mut batch = Vec::with_capacity(batch_size);
        let mut total = 0;

        let iter = self
            .db
            .iterator_cf(self.cf_dao_deposits(), rocksdb::IteratorMode::Start);

        for item in iter.flatten() {
            let (key, value) = item;
            if let Ok(entry) = bincode::deserialize::<DaoDepositCacheEntry>(&value) {
                let (tx_hash, output_index) = Self::decode_cell_key(&key);
                batch.push((tx_hash, output_index, entry));
                total += 1;

                if batch.len() >= batch_size {
                    callback(std::mem::take(&mut batch));
                    batch = Vec::with_capacity(batch_size);
                }
            }
        }

        if !batch.is_empty() {
            callback(batch);
        }

        total
    }
}

#[async_trait::async_trait]
impl LiveCellStorageAsync for RocksDbLiveCellStore {
    async fn flush_to_db(&self, _pool: &PgPool) -> anyhow::Result<(usize, usize)> {
        self.db.flush()?;
        Ok((0, 0))
    }

    async fn rebuild_from_db(&self, _pool: &PgPool) -> anyhow::Result<()> {
        tracing::info!(
            "RocksDbLiveCellStore: skipping rebuild_from_db (data persisted in RocksDB)"
        );
        let count = self.len();
        if count > 0 {
            tracing::info!("RocksDbLiveCellStore: loaded {} cells from disk", count);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_cell_info() -> LiveCellInfo {
        LiveCellInfo {
            capacity: 10000000000,
            created_at_block: 12345,
            lock_script_hash: vec![1u8; 32],
            lock_code_hash: vec![2u8; 32],
            lock_args: vec![3u8; 20],
            type_script_hash: Some(vec![4u8; 32]),
            type_code_hash: Some(vec![5u8; 32]),
            data_size: 100,
        }
    }

    fn create_test_block_header(block_number: i64) -> CachedBlockHeader {
        CachedBlockHeader {
            hash: vec![block_number as u8; 32],
            timestamp: 1700000000000 + block_number * 1000,
            epoch_number: block_number / 1800,
            epoch_index: (block_number % 1800) as i32,
            epoch_length: 1800,
            dao: vec![0u8; 32],
            transactions_count: 3,
        }
    }

    fn create_test_dao_deposit(block_number: i64) -> DaoDepositCacheEntry {
        DaoDepositCacheEntry {
            capacity: 10000000000,
            deposit_block_number: block_number,
            lock_script_hash: vec![9u8; 32],
            deposit_ar: 1234,
            status: 0,
            withdraw_request_tx: None,
            withdraw_request_block: None,
            withdraw_request_ar: None,
            withdraw_block: None,
            withdraw_tx: None,
            compensation: None,
        }
    }

    #[test]
    fn test_open_and_insert() {
        let tmp_dir = TempDir::new().unwrap();
        let store = RocksDbLiveCellStore::open(tmp_dir.path(), true).unwrap();

        let tx_hash = vec![0xabu8; 32];
        let output_index = 0;
        let info = create_test_cell_info();

        store.insert(tx_hash.clone(), output_index, info.clone());

        let retrieved = store.get(&tx_hash, output_index);
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.capacity, info.capacity);
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn test_remove_moves_to_consumed() {
        let tmp_dir = TempDir::new().unwrap();
        let store = RocksDbLiveCellStore::open(tmp_dir.path(), true).unwrap();

        let tx_hash = vec![0xabu8; 32];
        let info = create_test_cell_info();

        store.insert(tx_hash.clone(), 0, info.clone());
        assert_eq!(store.len(), 1);

        let removed = store.remove(&tx_hash, 0);
        assert!(removed.is_some());
        assert_eq!(store.len(), 0);

        let consumed = store.get_consumed_cell(&tx_hash, 0);
        assert!(consumed.is_some());
        assert_eq!(consumed.unwrap().capacity, info.capacity);
    }

    #[test]
    fn test_get_batch() {
        let tmp_dir = TempDir::new().unwrap();
        let store = RocksDbLiveCellStore::open(tmp_dir.path(), true).unwrap();

        let tx1 = vec![0x11u8; 32];
        let tx2 = vec![0x22u8; 32];
        let tx_missing = vec![0xffu8; 32];

        store.insert(tx1.clone(), 0, create_test_cell_info());
        store.insert(tx2.clone(), 1, create_test_cell_info());

        let outpoints = vec![
            (tx1.as_slice(), 0),
            (tx2.as_slice(), 1),
            (tx_missing.as_slice(), 99),
        ];

        let result = store.get_batch(&outpoints);
        assert_eq!(result.len(), 2);
        assert!(result.contains_key(&(tx1, 0)));
        assert!(result.contains_key(&(tx2, 1)));
    }

    #[test]
    fn test_consumed_cells_batch() {
        let tmp_dir = TempDir::new().unwrap();
        let store = RocksDbLiveCellStore::open(tmp_dir.path(), true).unwrap();

        let tx1 = vec![0x11u8; 32];
        let tx2 = vec![0x22u8; 32];

        store.insert(tx1.clone(), 0, create_test_cell_info());
        store.insert(tx2.clone(), 1, create_test_cell_info());

        store.remove(&tx1, 0);
        store.remove(&tx2, 1);

        let outpoints = vec![(tx1.as_slice(), 0i16), (tx2.as_slice(), 1i16)];
        let result = store.get_consumed_cells_batch(&outpoints);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_block_header_cache() {
        let tmp_dir = TempDir::new().unwrap();
        let store = RocksDbLiveCellStore::open(tmp_dir.path(), true).unwrap();

        let header = create_test_block_header(12345);
        store.insert_block_header(12345, header.clone());

        let retrieved = store.get_block_header(12345);
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.timestamp, header.timestamp);
        assert_eq!(retrieved.epoch_number, header.epoch_number);

        let by_hash = store.get_block_number_by_hash(&header.hash);
        assert_eq!(by_hash, Some(12345));
    }

    #[test]
    fn test_dao_cache() {
        let tmp_dir = TempDir::new().unwrap();
        let store = RocksDbLiveCellStore::open(tmp_dir.path(), true).unwrap();

        for i in 0..10 {
            let mut header = create_test_block_header(i);
            header.dao = vec![i as u8; 32];
            store.insert_block_header(i, header);
        }

        let dao = store.get_dao_field(5);
        assert!(dao.is_some());
        assert_eq!(dao.unwrap()[0], 5);

        let batch = store.get_dao_fields_batch(&[1, 3, 5, 7, 99]);
        assert_eq!(batch.len(), 4);
        assert_eq!(batch.get(&3).unwrap()[0], 3);
    }

    #[test]
    fn test_persistence() {
        let tmp_dir = TempDir::new().unwrap();
        let path = tmp_dir.path().to_path_buf();

        let tx_hash = vec![0xabu8; 32];
        let info = create_test_cell_info();

        {
            let store = RocksDbLiveCellStore::open(&path, true).unwrap();
            store.insert(tx_hash.clone(), 0, info.clone());
            store.insert_block_header(100, create_test_block_header(100));
            assert_eq!(store.len(), 1);
        }

        {
            let store = RocksDbLiveCellStore::open(&path, true).unwrap();
            assert_eq!(store.len(), 1);
            let retrieved = store.get(&tx_hash, 0);
            assert!(retrieved.is_some());
            assert_eq!(retrieved.unwrap().capacity, info.capacity);

            let header = store.get_block_header(100);
            assert!(header.is_some());
        }
    }

    #[test]
    fn test_dao_deposit_insert_and_get() {
        let tmp_dir = TempDir::new().unwrap();
        let store = RocksDbLiveCellStore::open(tmp_dir.path(), true).unwrap();

        let tx_hash = vec![0x33u8; 32];
        let entry = create_test_dao_deposit(100);

        store.insert_dao_deposit(&tx_hash, 0, &entry);
        let retrieved = store.get_dao_deposit(&tx_hash, 0).unwrap();
        assert_eq!(retrieved, entry);
    }

    #[test]
    fn test_dao_deposit_batch_get() {
        let tmp_dir = TempDir::new().unwrap();
        let store = RocksDbLiveCellStore::open(tmp_dir.path(), true).unwrap();

        let tx1 = vec![0x11u8; 32];
        let tx2 = vec![0x22u8; 32];
        let tx_missing = vec![0xffu8; 32];

        store.insert_dao_deposit(&tx1, 0, &create_test_dao_deposit(10));
        store.insert_dao_deposit(&tx2, 1, &create_test_dao_deposit(11));

        let outpoints = vec![
            (tx1.as_slice(), 0),
            (tx2.as_slice(), 1),
            (tx_missing.as_slice(), 99),
        ];

        let result = store.get_dao_deposits_batch(&outpoints);
        assert_eq!(result.len(), 2);
        assert!(result.contains_key(&(tx1, 0)));
        assert!(result.contains_key(&(tx2, 1)));
    }

    #[test]
    fn test_dao_deposit_secondary_index() {
        let tmp_dir = TempDir::new().unwrap();
        let store = RocksDbLiveCellStore::open(tmp_dir.path(), true).unwrap();

        let tx_hash = vec![0x44u8; 32];
        let withdraw_tx = vec![0x55u8; 32];
        let entry = create_test_dao_deposit(200);

        store.insert_dao_deposit(&tx_hash, 0, &entry);

        let mut updated = entry.clone();
        updated.status = 1;
        updated.withdraw_request_tx = Some(withdraw_tx.clone());
        updated.withdraw_request_block = Some(210);
        store.update_dao_deposit_status(&tx_hash, 0, &updated);

        let results = store.get_dao_deposits_by_withdraw_tx(&withdraw_tx);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, tx_hash);
        assert_eq!(results[0].1, 0);

        let batch =
            store.get_dao_deposits_by_withdraw_tx_batch(&[withdraw_tx.clone(), vec![0x99u8; 32]]);
        assert_eq!(batch.len(), 1);
        assert_eq!(batch.get(&withdraw_tx).unwrap().len(), 1);
    }

    #[test]
    fn test_dao_deposit_secondary_index_removal() {
        let tmp_dir = TempDir::new().unwrap();
        let store = RocksDbLiveCellStore::open(tmp_dir.path(), true).unwrap();

        let tx_hash = vec![0x66u8; 32];
        let withdraw_tx = vec![0x77u8; 32];
        let entry = create_test_dao_deposit(300);

        store.insert_dao_deposit(&tx_hash, 0, &entry);

        let mut updated = entry.clone();
        updated.status = 1;
        updated.withdraw_request_tx = Some(withdraw_tx.clone());
        store.update_dao_deposit_status(&tx_hash, 0, &updated);

        let mut withdrawn = updated.clone();
        withdrawn.status = 2;
        store.update_dao_deposit_status(&tx_hash, 0, &withdrawn);

        let results = store.get_dao_deposits_by_withdraw_tx(&withdraw_tx);
        assert!(results.is_empty());
    }

    #[test]
    fn test_dao_deposit_rollback() {
        let tmp_dir = TempDir::new().unwrap();
        let store = RocksDbLiveCellStore::open(tmp_dir.path(), true).unwrap();

        let tx1 = vec![0x01u8; 32];
        let tx2 = vec![0x02u8; 32];
        let withdraw_tx = vec![0x03u8; 32];

        let entry1 = create_test_dao_deposit(90);
        let entry2 = create_test_dao_deposit(110);

        store.insert_dao_deposit(&tx1, 0, &entry1);
        store.insert_dao_deposit(&tx2, 0, &entry2);

        let mut updated = entry2.clone();
        updated.status = 1;
        updated.withdraw_request_tx = Some(withdraw_tx.clone());
        store.update_dao_deposit_status(&tx2, 0, &updated);

        let (removed_primary, removed_secondary) = store.rollback_dao_deposits(100);
        assert_eq!(removed_primary, 1);
        assert_eq!(removed_secondary, 1);
        assert!(store.get_dao_deposit(&tx1, 0).is_some());
        assert!(store.get_dao_deposit(&tx2, 0).is_none());
        assert!(store
            .get_dao_deposits_by_withdraw_tx(&withdraw_tx)
            .is_empty());
    }

    #[test]
    fn test_dao_deposit_persistence() {
        let tmp_dir = TempDir::new().unwrap();
        let path = tmp_dir.path().to_path_buf();

        let tx_hash = vec![0x88u8; 32];
        let withdraw_tx = vec![0x99u8; 32];
        let entry = create_test_dao_deposit(400);

        {
            let store = RocksDbLiveCellStore::open(&path, true).unwrap();
            store.insert_dao_deposit(&tx_hash, 0, &entry);
            let mut updated = entry.clone();
            updated.status = 1;
            updated.withdraw_request_tx = Some(withdraw_tx.clone());
            store.update_dao_deposit_status(&tx_hash, 0, &updated);
        }

        {
            let store = RocksDbLiveCellStore::open(&path, true).unwrap();
            let retrieved = store.get_dao_deposit(&tx_hash, 0).unwrap();
            assert_eq!(retrieved.deposit_block_number, entry.deposit_block_number);
            let results = store.get_dao_deposits_by_withdraw_tx(&withdraw_tx);
            assert_eq!(results.len(), 1);
        }
    }

    #[test]
    fn test_dao_deposit_iter_batched() {
        let tmp_dir = TempDir::new().unwrap();
        let store = RocksDbLiveCellStore::open(tmp_dir.path(), true).unwrap();

        for i in 0..25 {
            let tx_hash = vec![i as u8; 32];
            let mut entry = create_test_dao_deposit(500 + i as i64);
            entry.capacity = (i + 1) as i64 * 10_000_000;
            store.insert_dao_deposit(&tx_hash, 0, &entry);
        }

        let mut batches_received = 0;
        let mut total_deposits = 0;

        let count = store.iter_dao_deposits_batched(10, |batch| {
            batches_received += 1;
            total_deposits += batch.len();
        });

        assert_eq!(count, 25);
        assert_eq!(total_deposits, 25);
        assert_eq!(batches_received, 3);
    }

    #[test]
    fn test_dao_deposit_count() {
        let tmp_dir = TempDir::new().unwrap();
        let store = RocksDbLiveCellStore::open(tmp_dir.path(), true).unwrap();

        assert_eq!(store.count_dao_deposits(), 0);

        for i in 0..5 {
            let tx_hash = vec![i as u8; 32];
            store.insert_dao_deposit(&tx_hash, 0, &create_test_dao_deposit(600 + i as i64));
        }

        assert_eq!(store.count_dao_deposits(), 5);
    }

    #[test]
    fn test_rollback() {
        let tmp_dir = TempDir::new().unwrap();
        let store = RocksDbLiveCellStore::open(tmp_dir.path(), true).unwrap();

        let tx1 = vec![0x11u8; 32];
        let tx2 = vec![0x22u8; 32];

        let mut info1 = create_test_cell_info();
        info1.created_at_block = 99;
        let mut info2 = create_test_cell_info();
        info2.created_at_block = 101;

        store.insert(tx1.clone(), 0, info1);
        store.insert(tx2.clone(), 0, info2);

        assert_eq!(store.len(), 2);

        let (removed, _) = store.rollback_to_block(100);

        assert_eq!(removed, 1);
        assert_eq!(store.len(), 1);
        assert!(store.get(&tx1, 0).is_some());
        assert!(store.get(&tx2, 0).is_none());
    }

    #[test]
    fn test_rollback_block_cache() {
        let tmp_dir = TempDir::new().unwrap();
        let store = RocksDbLiveCellStore::open(tmp_dir.path(), true).unwrap();

        for i in 1..=10 {
            store.insert_block_header(i, create_test_block_header(i));
        }

        store.rollback_block_cache(5);

        assert!(store.get_block_header(5).is_some());
        assert!(store.get_block_header(6).is_none());
        assert!(store.get_block_header(10).is_none());
    }

    #[test]
    fn test_backend_name() {
        let tmp_dir = TempDir::new().unwrap();
        let store = RocksDbLiveCellStore::open(tmp_dir.path(), true).unwrap();
        assert_eq!(store.backend_name(), "rocksdb");
    }

    #[test]
    fn test_iter_live_cells_batched() {
        let tmp_dir = TempDir::new().unwrap();
        let store = RocksDbLiveCellStore::open(tmp_dir.path(), true).unwrap();

        for i in 0..25 {
            let tx_hash = vec![i as u8; 32];
            let mut info = create_test_cell_info();
            info.capacity = (i + 1) as i64 * 100_000_000;
            store.insert(tx_hash, 0, info);
        }

        let mut batches_received = 0;
        let mut total_cells = 0;

        let count = store.iter_live_cells_batched(10, |batch| {
            batches_received += 1;
            total_cells += batch.len();
        });

        assert_eq!(count, 25);
        assert_eq!(total_cells, 25);
        assert_eq!(batches_received, 3); // 10 + 10 + 5
    }

    #[test]
    fn test_iter_live_cells_batched_empty() {
        let tmp_dir = TempDir::new().unwrap();
        let store = RocksDbLiveCellStore::open(tmp_dir.path(), true).unwrap();

        let mut callback_called = false;
        let count = store.iter_live_cells_batched(10, |_batch| {
            callback_called = true;
        });

        assert_eq!(count, 0);
        assert!(!callback_called);
    }

    #[test]
    fn test_count_live_cells() {
        let tmp_dir = TempDir::new().unwrap();
        let store = RocksDbLiveCellStore::open(tmp_dir.path(), true).unwrap();

        assert_eq!(store.count_live_cells(), 0);

        for i in 0..100 {
            let tx_hash = vec![i as u8; 32];
            store.insert(tx_hash, 0, create_test_cell_info());
        }

        assert_eq!(store.count_live_cells(), 100);
    }

    #[test]
    fn test_bulk_sync_mode_skips_prune() {
        let tmp_dir = TempDir::new().unwrap();
        let store = RocksDbLiveCellStore::open(tmp_dir.path(), true).unwrap();

        store.set_bulk_sync_mode(true);
        assert!(store.is_bulk_sync_mode());

        let tx_hash = vec![0xabu8; 32];
        let mut info = create_test_cell_info();
        info.created_at_block = 100;
        store.insert(tx_hash.clone(), 0, info);

        store.remove(&tx_hash, 0);
        store.record_consumption(tx_hash.clone(), 0, create_test_cell_info(), 5000);

        let (count, _) = store.consumed_cells_stats();
        assert!(
            count > 0,
            "consumed cells should be retained in bulk sync mode"
        );

        let consumed = store.get_consumed_cell(&tx_hash, 0);
        assert!(consumed.is_some());
    }

    #[test]
    fn test_bulk_sync_cell_cache_disabled_ignores_mode() {
        let tmp_dir = TempDir::new().unwrap();
        let store = RocksDbLiveCellStore::open(tmp_dir.path(), false).unwrap();

        store.set_bulk_sync_mode(true);
        assert!(store.is_bulk_sync_mode());

        let tx_hash = vec![0xabu8; 32];
        let mut info = create_test_cell_info();
        info.created_at_block = 100;
        store.insert(tx_hash.clone(), 0, info);
        store.remove(&tx_hash, 0);

        let consumed_before = store.get_consumed_cell(&tx_hash, 0);
        assert!(
            consumed_before.is_some(),
            "consumed cell should exist before prune"
        );

        store.record_consumption(vec![0xff; 32], 0, create_test_cell_info(), 5000);

        let (_, _) = store.consumed_cells_stats();
    }

    #[test]
    fn test_cleanup_consumed_cells() {
        let tmp_dir = TempDir::new().unwrap();
        let store = RocksDbLiveCellStore::open(tmp_dir.path(), true).unwrap();

        for i in 0..10 {
            let tx_hash = vec![i as u8; 32];
            store.insert(tx_hash.clone(), 0, create_test_cell_info());
            store.remove(&tx_hash, 0);
        }

        let (count_before, _) = store.consumed_cells_stats();
        assert_eq!(count_before, 10);

        let cleaned = store.cleanup_consumed_cells();
        assert_eq!(cleaned, 10);

        let (count_after, _) = store.consumed_cells_stats();
        assert_eq!(count_after, 0);
    }
}
