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

const MAX_CONSUMED_HISTORY_BLOCKS: i64 = 1000;

pub struct RocksDbLiveCellStore {
    db: DB,
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
        ];

        let db = DB::open_cf_descriptors(&opts, path, cf_descriptors)?;

        Ok(Self {
            db,
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

        let memory_bytes = self
            .db
            .property_int_value("rocksdb.cur-size-all-mem-tables")
            .ok()
            .flatten()
            .unwrap_or(0) as usize
            + self
                .db
                .property_int_value("rocksdb.block-cache-usage")
                .ok()
                .flatten()
                .unwrap_or(0) as usize;

        MemoryStats {
            cells_count,
            memory_bytes,
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
