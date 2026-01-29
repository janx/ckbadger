//! RocksDB-backed live cell storage.
//!
//! Provides persistent storage for ~20M+ live cells with O(1) lookups.
//! Data survives indexer restarts without rebuild from PostgreSQL.
//!
//! # Storage Format
//!
//! - **Key**: 34 bytes = tx_hash (32) + output_index (2, big-endian)
//! - **Value**: bincode-serialized `LiveCellInfo` with LZ4 compression
//!
//! # Performance Tuning
//!
//! Configured for write-heavy workload during initial sync:
//! - 256MB write buffer, 4 buffers max
//! - LZ4 compression (fast, ~50% reduction)
//! - 10-bit bloom filters for negative lookups

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::RwLock;

use rocksdb::{DBCompressionType, Options, DB};
use sqlx::PgPool;

use super::live_cell_storage::{
    ConsumedCellRecord, LiveCellInfo, LiveCellStorage, LiveCellStorageAsync, MemoryStats,
};

/// Key size: 32 bytes tx_hash + 2 bytes output_index.
const KEY_SIZE: usize = 34;

/// RocksDB-backed implementation of [`LiveCellStorage`].
///
/// Thread-safe via RocksDB's internal synchronization.
/// Consumed cell history uses `RwLock` for rollback support.
pub struct RocksDbLiveCellStore {
    db: DB,
    consumed_history: RwLock<VecDeque<ConsumedCellRecord>>,
    max_history_blocks: i64,
}

impl RocksDbLiveCellStore {
    pub fn open<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let mut opts = Options::default();
        opts.create_if_missing(true);

        opts.set_write_buffer_size(256 * 1024 * 1024);
        opts.set_max_write_buffer_number(4);
        opts.set_level_zero_file_num_compaction_trigger(4);
        opts.set_max_bytes_for_level_base(512 * 1024 * 1024);

        opts.set_compression_type(DBCompressionType::Lz4);

        let mut block_opts = rocksdb::BlockBasedOptions::default();
        block_opts.set_block_size(16 * 1024);
        block_opts.set_cache_index_and_filter_blocks(true);
        block_opts.set_bloom_filter(10.0, false);
        opts.set_block_based_table_factory(&block_opts);

        let db = DB::open(&opts, path)?;

        Ok(Self {
            db,
            consumed_history: RwLock::new(VecDeque::new()),
            max_history_blocks: 36,
        })
    }

    fn encode_key(tx_hash: &[u8], output_index: i16) -> [u8; KEY_SIZE] {
        let mut key = [0u8; KEY_SIZE];
        key[..32].copy_from_slice(tx_hash);
        key[32..34].copy_from_slice(&output_index.to_be_bytes());
        key
    }

    fn decode_key(key: &[u8]) -> (Vec<u8>, i16) {
        let tx_hash = key[..32].to_vec();
        let output_index = i16::from_be_bytes([key[32], key[33]]);
        (tx_hash, output_index)
    }

    fn insert_internal(&self, tx_hash: &[u8], output_index: i16, info: &LiveCellInfo) {
        let key = Self::encode_key(tx_hash, output_index);
        let value = bincode::serialize(info).expect("failed to serialize LiveCellInfo");
        self.db
            .put(key, &value)
            .expect("failed to write to RocksDB");
    }
}

impl LiveCellStorage for RocksDbLiveCellStore {
    fn insert(&self, tx_hash: Vec<u8>, output_index: i16, info: LiveCellInfo) {
        self.insert_internal(&tx_hash, output_index, &info);
    }

    fn get(&self, tx_hash: &[u8], output_index: i16) -> Option<LiveCellInfo> {
        let key = Self::encode_key(tx_hash, output_index);
        match self.db.get(key) {
            Ok(Some(value)) => bincode::deserialize(&value).ok(),
            _ => None,
        }
    }

    fn remove(&self, tx_hash: &[u8], output_index: i16) -> Option<LiveCellInfo> {
        let key = Self::encode_key(tx_hash, output_index);
        let existing = match self.db.get(key) {
            Ok(Some(value)) => bincode::deserialize(&value).ok(),
            _ => None,
        };

        if existing.is_some() {
            let _ = self.db.delete(key);
        }

        existing
    }

    fn get_batch(&self, outpoints: &[(&[u8], i16)]) -> HashMap<(Vec<u8>, i16), LiveCellInfo> {
        let mut result = HashMap::with_capacity(outpoints.len());

        let keys: Vec<[u8; KEY_SIZE]> = outpoints
            .iter()
            .map(|(tx_hash, idx)| Self::encode_key(tx_hash, *idx))
            .collect();

        let key_refs: Vec<&[u8]> = keys.iter().map(|k| k.as_slice()).collect();
        let values = self.db.multi_get(&key_refs);

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
        let iter = self.db.iterator(rocksdb::IteratorMode::Start);
        for _ in iter {
            count += 1;
        }
        count
    }

    fn clear(&self) {
        let mut batch = rocksdb::WriteBatch::default();
        let iter = self.db.iterator(rocksdb::IteratorMode::Start);
        for (key, _) in iter.flatten() {
            batch.delete(&key);
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

        while let Some(oldest) = history.front() {
            if consumed_at_block - oldest.consumed_at_block > self.max_history_blocks {
                history.pop_front();
            } else {
                break;
            }
        }
    }

    fn rollback_to_block(&self, rollback_to: i64) -> (usize, usize) {
        let mut removed = 0;
        let mut restored = 0;

        let mut to_remove = Vec::new();
        let iter = self.db.iterator(rocksdb::IteratorMode::Start);
        for (key, value) in iter.flatten() {
            if let Ok(info) = bincode::deserialize::<LiveCellInfo>(&value) {
                if info.created_at_block > rollback_to {
                    to_remove.push(key.to_vec());
                }
            }
        }

        let mut batch = rocksdb::WriteBatch::default();
        for key in to_remove {
            batch.delete(&key);
            removed += 1;
        }
        let _ = self.db.write(batch);

        {
            let history = self.consumed_history.read().unwrap();
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
            let mut history = self.consumed_history.write().unwrap();
            history.retain(|r| r.consumed_at_block <= rollback_to);
        }

        (removed, restored)
    }

    fn cells_created_since(&self, block_number: i64) -> Vec<(Vec<u8>, i16, LiveCellInfo)> {
        let mut result = Vec::new();
        let iter = self.db.iterator(rocksdb::IteratorMode::Start);
        for (key, value) in iter.flatten() {
            if let Ok(info) = bincode::deserialize::<LiveCellInfo>(&value) {
                if info.created_at_block > block_number {
                    let (tx_hash, output_index) = Self::decode_key(&key);
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

    #[test]
    fn test_open_and_insert() {
        let tmp_dir = TempDir::new().unwrap();
        let store = RocksDbLiveCellStore::open(tmp_dir.path()).unwrap();

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
    fn test_remove() {
        let tmp_dir = TempDir::new().unwrap();
        let store = RocksDbLiveCellStore::open(tmp_dir.path()).unwrap();

        let tx_hash = vec![0xabu8; 32];
        let info = create_test_cell_info();

        store.insert(tx_hash.clone(), 0, info);
        assert_eq!(store.len(), 1);

        let removed = store.remove(&tx_hash, 0);
        assert!(removed.is_some());
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn test_get_batch() {
        let tmp_dir = TempDir::new().unwrap();
        let store = RocksDbLiveCellStore::open(tmp_dir.path()).unwrap();

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
    fn test_persistence() {
        let tmp_dir = TempDir::new().unwrap();
        let path = tmp_dir.path().to_path_buf();

        let tx_hash = vec![0xabu8; 32];
        let info = create_test_cell_info();

        {
            let store = RocksDbLiveCellStore::open(&path).unwrap();
            store.insert(tx_hash.clone(), 0, info.clone());
            assert_eq!(store.len(), 1);
        }

        {
            let store = RocksDbLiveCellStore::open(&path).unwrap();
            assert_eq!(store.len(), 1);
            let retrieved = store.get(&tx_hash, 0);
            assert!(retrieved.is_some());
            assert_eq!(retrieved.unwrap().capacity, info.capacity);
        }
    }

    #[test]
    fn test_rollback() {
        let tmp_dir = TempDir::new().unwrap();
        let store = RocksDbLiveCellStore::open(tmp_dir.path()).unwrap();

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
    fn test_backend_name() {
        let tmp_dir = TempDir::new().unwrap();
        let store = RocksDbLiveCellStore::open(tmp_dir.path()).unwrap();
        assert_eq!(store.backend_name(), "rocksdb");
    }
}
