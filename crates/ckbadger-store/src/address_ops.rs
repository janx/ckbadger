//! Address balance operations.

use rocksdb::IteratorMode;

use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::{AddressBalance, LiveCellInfo};

impl CkbadgerStore {
    /// Get address stats — checks materialized CF first, then derives from cell indices.
    pub fn get_addr_balance(&self, lock_hash: &[u8]) -> anyhow::Result<Option<AddressBalance>> {
        // Try materialized first (addresses above threshold)
        if let Some(value) = self.get_cf(self.cf_addr_stats(), lock_hash)? {
            return Ok(Some(bincode::deserialize(&value)?));
        }
        // Derive from cell indices for sub-threshold addresses
        self.derive_addr_stats(lock_hash)
    }

    /// Derive address stats at read time from cell indices.
    /// Scans CF_LIVE_CELLS_BY_LOCK for the given lock_hash prefix, batch-gets cell data
    /// from CF_CELLS (append), and aggregates balance/occupied_capacity/live_cells_count.
    fn derive_addr_stats(&self, lock_hash: &[u8]) -> anyhow::Result<Option<AddressBalance>> {
        let prefix = &lock_hash[..32];
        let iter = self.iterator_cf(
            self.cf_cell_by_lock(),
            IteratorMode::From(prefix, rocksdb::Direction::Forward),
        );

        // Collect outpoints from the cell-by-lock index
        let mut outpoint_keys: Vec<[u8; keys::OUTPOINT_KEY_SIZE]> = Vec::new();
        for item in iter.flatten() {
            let (key, _) = item;
            if !key.starts_with(prefix) {
                break;
            }
            // Key: lock_hash(32B) + block_num(8B) + outpoint(34B) = 74 bytes
            if key.len() == 74 {
                let mut outpoint = [0u8; keys::OUTPOINT_KEY_SIZE];
                outpoint.copy_from_slice(&key[40..74]);
                outpoint_keys.push(outpoint);
            }
        }

        if outpoint_keys.is_empty() {
            return Ok(None);
        }

        // Batch-get cell data from append store
        let cf = self.cf_cells();
        let cf_keys: Vec<_> = outpoint_keys.iter().map(|k| (cf, k.as_slice())).collect();
        let values = self.append_multi_get_cf(cf_keys);

        let mut balance: i128 = 0;
        let mut occupied_capacity: i128 = 0;
        let mut live_cells_count: i32 = 0;
        let mut first_seen_block = i64::MAX;
        let mut first_seen_tx = Vec::new();
        let mut last_activity_block = 0i64;
        let mut last_activity_tx = Vec::new();

        for (outpoint, value_result) in outpoint_keys.iter().zip(values) {
            if let Ok(Some(value)) = value_result {
                let info: LiveCellInfo = bincode::deserialize(&value)?;
                balance += info.capacity as i128;
                occupied_capacity += info.occupied_capacity as i128;
                live_cells_count += 1;

                if info.created_at_block < first_seen_block {
                    first_seen_block = info.created_at_block;
                    first_seen_tx = outpoint[..32].to_vec();
                }
                if info.created_at_block > last_activity_block {
                    last_activity_block = info.created_at_block;
                    last_activity_tx = outpoint[..32].to_vec();
                }
            }
        }

        if live_cells_count == 0 {
            return Ok(None);
        }

        // Derive txs_count from addr_txs index
        let mut txs_count = 0i64;
        let addr_tx_iter = self.iterator_cf(
            self.cf_addr_txs(),
            IteratorMode::From(prefix, rocksdb::Direction::Forward),
        );
        for item in addr_tx_iter.flatten() {
            let (key, _) = item;
            if !key.starts_with(prefix) {
                break;
            }
            if key.len() == 44 {
                txs_count += 1;
            }
        }

        Ok(Some(AddressBalance {
            balance,
            occupied_capacity,
            live_cells_count,
            total_cells_count: i64::from(live_cells_count),
            txs_count,
            first_seen_block,
            first_seen_tx,
            last_activity_block,
            last_activity_tx,
        }))
    }

    pub fn put_addr_balance_direct(
        &self,
        lock_hash: &[u8],
        balance: &AddressBalance,
    ) -> anyhow::Result<()> {
        let value = bincode::serialize(balance)?;
        self.put_cf(self.cf_addr_stats(), lock_hash, &value)
    }

    /// Update address balance with read-modify-write.
    pub fn update_addr_balance<F>(&self, lock_hash: &[u8], update_fn: F) -> anyhow::Result<()>
    where
        F: FnOnce(&mut AddressBalance),
    {
        let mut balance = self.get_addr_balance(lock_hash)?.unwrap_or_default();
        update_fn(&mut balance);
        self.put_addr_balance_direct(lock_hash, &balance)
    }

    /// List top addresses by balance (full scan, sorted).
    pub fn top_addresses(&self, limit: usize) -> anyhow::Result<Vec<(Vec<u8>, AddressBalance)>> {
        let iter = self.iterator_cf(self.cf_addr_stats(), IteratorMode::Start);

        let mut all: Vec<(Vec<u8>, AddressBalance)> = Vec::new();
        for item in iter.flatten() {
            let (key, value) = item;
            if let Ok(balance) = bincode::deserialize::<AddressBalance>(&value) {
                all.push((key.to_vec(), balance));
            }
        }

        all.sort_by(|a, b| b.1.balance.cmp(&a.1.balance));
        all.truncate(limit);
        Ok(all)
    }

    /// List transactions for an address (oldest first).
    pub fn list_addr_txs(
        &self,
        lock_hash: &[u8],
        limit: usize,
    ) -> anyhow::Result<Vec<(i64, i32, Vec<u8>)>> {
        let iter = self.prefix_iterator_cf(self.cf_addr_txs(), lock_hash);
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, value) = item;
            if !key.starts_with(lock_hash) {
                break;
            }
            // Key: lock_hash(32) + block_num(8) + tx_idx(4) = 44
            if key.len() == 44 {
                let block_num = crate::keys::decode_block_num(&key[32..40]);
                let tx_idx = crate::keys::decode_tx_idx(&key[40..44]);
                results.push((block_num, tx_idx, value.to_vec()));
                if results.len() >= limit {
                    break;
                }
            }
        }
        Ok(results)
    }

    /// List transactions for an address (newest first, reverse scan).
    pub fn list_addr_txs_recent(
        &self,
        lock_hash: &[u8],
        limit: usize,
        cursor: Option<(i64, i32)>,
    ) -> anyhow::Result<Vec<(i64, i32, Vec<u8>)>> {
        // Seek to the cursor position or end of prefix range, then iterate backwards
        let upper_key = match cursor {
            Some((block_num, tx_idx)) => {
                // Seek to cursor position; skip it in the loop
                crate::keys::encode_addr_tx_key(lock_hash, block_num, tx_idx)
            }
            None => {
                let mut key = Vec::with_capacity(44);
                key.extend_from_slice(lock_hash);
                key.extend_from_slice(&[0xFF; 12]); // max block_num(8) + max tx_idx(4)
                key
            }
        };

        let iter = self.iterator_cf(
            self.cf_addr_txs(),
            rocksdb::IteratorMode::From(&upper_key, rocksdb::Direction::Reverse),
        );

        let mut results = Vec::new();
        let mut skip_first = cursor.is_some();
        for item in iter.flatten() {
            let (key, value) = item;
            if !key.starts_with(lock_hash) {
                break;
            }
            if key.len() == 44 {
                let block_num = crate::keys::decode_block_num(&key[32..40]);
                let tx_idx = crate::keys::decode_tx_idx(&key[40..44]);
                if skip_first {
                    skip_first = false;
                    if Some((block_num, tx_idx)) == cursor {
                        continue;
                    }
                }
                results.push((block_num, tx_idx, value.to_vec()));
                if results.len() >= limit {
                    break;
                }
            }
        }
        Ok(results)
    }

    /// Check if the addr_txs index has been populated.
    pub fn addr_txs_populated(&self) -> bool {
        let iter = self.iterator_cf(self.cf_addr_txs(), rocksdb::IteratorMode::Start);
        iter.flatten().next().is_some()
    }

    /// Backfill the addr_txs index from live_cells and consumed_cells.
    /// This indexes the creation-side of transactions (cells created for each address).
    /// Returns the number of index entries written.
    pub fn backfill_addr_txs(&self) -> anyhow::Result<u64> {
        use std::collections::{HashMap, HashSet};

        let mut count = 0u64;
        let mut batch = rocksdb::WriteBatch::default();
        let batch_size = 10_000u64;

        // Collect (tx_hash, lock_script_hash) pairs from live cells
        // Group by tx_hash to batch tx_idx lookups
        let mut tx_addresses: HashMap<Vec<u8>, HashSet<Vec<u8>>> = HashMap::new();
        let mut tx_blocks: HashMap<Vec<u8>, i64> = HashMap::new();

        // Phase 1: Scan live cells (cf_live_cells has liveness markers only; cell data in cf_cells)
        let iter = self.iterator_cf(self.cf_live_cells(), rocksdb::IteratorMode::Start);
        for item in iter.flatten() {
            let (key, _value) = item;
            if key.len() == keys::OUTPOINT_KEY_SIZE {
                if let Ok(Some(cell_bytes)) = self.append_get_cf(self.cf_cells(), &key) {
                    if let Ok(info) = bincode::deserialize::<LiveCellInfo>(&cell_bytes) {
                        let tx_hash = key[..32].to_vec();
                        tx_addresses
                            .entry(tx_hash.clone())
                            .or_default()
                            .insert(info.lock_script_hash);
                        tx_blocks.entry(tx_hash).or_insert(info.created_at_block);
                    }
                }
            }
        }

        // Phase 2: Scan consumed cells (cf_consumed_cells has 40-byte metadata; cell data in cf_cells)
        let iter = self.iterator_cf(self.cf_consumed_cells(), rocksdb::IteratorMode::Start);
        for item in iter.flatten() {
            let (key, _value) = item;
            if key.len() == keys::OUTPOINT_KEY_SIZE {
                if let Ok(Some(cell_bytes)) = self.append_get_cf(self.cf_cells(), &key) {
                    if let Ok(info) = bincode::deserialize::<LiveCellInfo>(&cell_bytes) {
                        let tx_hash = key[..32].to_vec();
                        tx_addresses
                            .entry(tx_hash.clone())
                            .or_default()
                            .insert(info.lock_script_hash);
                        tx_blocks.entry(tx_hash).or_insert(info.created_at_block);
                    }
                }
            }
        }

        // Phase 3: Batch lookup tx location for each tx_hash via cf_tx_meta (append store)
        let tx_hashes: Vec<Vec<u8>> = tx_addresses.keys().cloned().collect();
        let cf_tx_meta = self.cf_tx_meta();

        for chunk in tx_hashes.chunks(5000) {
            let cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> =
                chunk.iter().map(|h| (cf_tx_meta, h.as_slice())).collect();
            let results = self.append_multi_get_cf(cf_keys);

            for (i, result) in results.into_iter().enumerate() {
                if let Ok(Some(value)) = result {
                    if let Ok(meta) = bincode::deserialize::<crate::types::TxMeta>(&value) {
                        let block_num = meta.block_number;
                        let tx_idx = meta.tx_index;
                        let tx_hash = &chunk[i];

                        if let Some(addresses) = tx_addresses.get(tx_hash) {
                            for lock_hash in addresses {
                                let key = keys::encode_addr_tx_key(lock_hash, block_num, tx_idx);
                                batch.put_cf(self.cf_addr_txs(), &key, tx_hash);
                                count += 1;

                                #[allow(clippy::manual_is_multiple_of)]
                                if count % batch_size == 0 {
                                    self.write_batch(std::mem::take(&mut batch))?;
                                    batch = rocksdb::WriteBatch::default();
                                }
                            }
                        }
                    }
                }
            }
        }

        if !batch.is_empty() {
            self.write_batch(batch)?;
        }

        Ok(count)
    }

    /// Rebuild addr_balance from live_cells.
    ///
    /// This heals historical drift caused by previously skipped input-cell consumption.
    /// Uses live_cells as the source of truth for:
    /// - balance
    /// - occupied_capacity
    /// - live_cells_count
    /// - txs_count (from addr_txs index)
    ///
    /// Other fields are re-initialized conservatively.
    pub fn rebuild_addr_balances_from_live_cells(&self) -> anyhow::Result<usize> {
        use std::collections::HashMap;

        struct Agg {
            balance: i128,
            occupied_capacity: i128,
            live_cells_count: i32,
            txs_count: i64,
            first_seen_block: i64,
            first_seen_tx: Vec<u8>,
            last_activity_block: i64,
            last_activity_tx: Vec<u8>,
        }

        let mut agg_by_lock: HashMap<Vec<u8>, Agg> = HashMap::new();
        // cf_live_cells has liveness markers only; cell data lives in cf_cells (append store)
        let iter = self.iterator_cf(self.cf_live_cells(), rocksdb::IteratorMode::Start);
        for item in iter.flatten() {
            let (key, _value) = item;
            if key.len() != keys::OUTPOINT_KEY_SIZE {
                continue;
            }
            let cell_bytes = match self.append_get_cf(self.cf_cells(), &key)? {
                Some(v) => v,
                None => continue,
            };
            let info: LiveCellInfo = match bincode::deserialize(&cell_bytes) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let tx_hash = key[..32].to_vec();

            let entry = agg_by_lock
                .entry(info.lock_script_hash.clone())
                .or_insert_with(|| Agg {
                    balance: 0,
                    occupied_capacity: 0,
                    live_cells_count: 0,
                    txs_count: 0,
                    first_seen_block: info.created_at_block,
                    first_seen_tx: tx_hash.clone(),
                    last_activity_block: info.created_at_block,
                    last_activity_tx: tx_hash.clone(),
                });

            entry.balance += info.capacity as i128;
            entry.occupied_capacity += info.occupied_capacity as i128;
            entry.live_cells_count += 1;

            if info.created_at_block < entry.first_seen_block {
                entry.first_seen_block = info.created_at_block;
                entry.first_seen_tx = tx_hash.clone();
            }
            if info.created_at_block > entry.last_activity_block {
                entry.last_activity_block = info.created_at_block;
                entry.last_activity_tx = tx_hash;
            }
        }

        // Rebuild tx count from addr_txs index.
        // Count only addresses that still have live cells after rebuild.
        let iter = self.iterator_cf(self.cf_addr_txs(), rocksdb::IteratorMode::Start);
        for item in iter.flatten() {
            let (key, _) = item;
            // Key: lock_hash(32) + block_num(8) + tx_idx(4) = 44
            if key.len() != 44 {
                continue;
            }
            if let Some(entry) = agg_by_lock.get_mut(&key[..32]) {
                entry.txs_count += 1;
            }
        }

        // Clear existing addr_stats CF
        let mut clear_batch = rocksdb::WriteBatch::default();
        let mut cleared = 0usize;
        let iter = self.iterator_cf(self.cf_addr_stats(), IteratorMode::Start);
        for item in iter.flatten() {
            let (key, _) = item;
            clear_batch.delete_cf(self.cf_addr_stats(), &key);
            cleared += 1;
            #[allow(clippy::manual_is_multiple_of)]
            if cleared % 20_000 == 0 {
                self.write_batch(std::mem::take(&mut clear_batch))?;
                clear_batch = rocksdb::WriteBatch::default();
            }
        }
        if !clear_batch.is_empty() {
            self.write_batch(clear_batch)?;
        }

        // Write rebuilt balances
        let mut write_batch = crate::batch::StoreBatch::new(self);
        let mut written = 0usize;
        for (lock_hash, agg) in agg_by_lock {
            if agg.balance < 0 || agg.occupied_capacity < 0 || agg.live_cells_count < 0 {
                anyhow::bail!(
                    "rebuild addr_balance negative aggregate: lock_hash={:?}, balance={}, occupied_capacity={}, live_cells_count={}",
                    lock_hash,
                    agg.balance,
                    agg.occupied_capacity,
                    agg.live_cells_count
                );
            }
            let balance = AddressBalance {
                balance: agg.balance,
                occupied_capacity: agg.occupied_capacity,
                live_cells_count: agg.live_cells_count,
                total_cells_count: i64::from(agg.live_cells_count),
                txs_count: agg.txs_count,
                first_seen_block: agg.first_seen_block,
                first_seen_tx: agg.first_seen_tx,
                last_activity_block: agg.last_activity_block,
                last_activity_tx: agg.last_activity_tx,
            };
            write_batch.put_addr_balance(&lock_hash, &balance);
            written += 1;
            #[allow(clippy::manual_is_multiple_of)]
            if written % 20_000 == 0 {
                write_batch.commit()?;
                write_batch = crate::batch::StoreBatch::new(self);
            }
        }
        #[allow(clippy::manual_is_multiple_of)]
        if written % 20_000 != 0 {
            write_batch.commit()?;
        }

        Ok(written)
    }

    /// Delete addr_stats entries for addresses below the materialization threshold.
    /// Called after bulk sync completes: during bulk sync all stats are written eagerly,
    /// then this pass prunes sub-threshold entries so they'll be derived at read time.
    pub fn cleanup_sub_threshold_addr_stats(&self) -> anyhow::Result<u64> {
        use crate::store::ADDR_STATS_THRESHOLD;

        let mut deleted = 0u64;
        let mut batch = rocksdb::WriteBatch::default();
        let batch_size = 20_000u64;

        let iter = self.iterator_cf(self.cf_addr_stats(), IteratorMode::Start);
        for item in iter.flatten() {
            let (key, value) = item;
            if let Ok(stats) = bincode::deserialize::<AddressBalance>(&value) {
                if stats.live_cells_count < ADDR_STATS_THRESHOLD {
                    batch.delete_cf(self.cf_addr_stats(), &key);
                    deleted += 1;

                    #[allow(clippy::manual_is_multiple_of)]
                    if deleted % batch_size == 0 {
                        self.write_batch(std::mem::take(&mut batch))?;
                        batch = rocksdb::WriteBatch::default();
                    }
                }
            }
        }

        if !batch.is_empty() {
            self.write_batch(batch)?;
        }

        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch::StoreBatch;
    use tempfile::tempdir;

    fn make_cell(
        lock_hash: Vec<u8>,
        created_at: i64,
        capacity: i64,
        occupied: i64,
    ) -> LiveCellInfo {
        LiveCellInfo {
            capacity,
            created_at_block: created_at,
            lock_script_hash: lock_hash,
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: occupied,
            udt_amount: None,
        }
    }

    #[test]
    fn test_rebuild_addr_balances_from_live_cells() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let lock_a = vec![0xAA; 32];
        let lock_b = vec![0xBB; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_cell(&[0x01; 32], 0, &make_cell(lock_a.clone(), 10, 100, 120));
        batch.put_cell(&[0x02; 32], 0, &make_cell(lock_a.clone(), 11, 300, 320));
        batch.put_cell(&[0x03; 32], 0, &make_cell(lock_b.clone(), 12, 50, 60));
        batch.commit().unwrap();

        let rebuilt = store.rebuild_addr_balances_from_live_cells().unwrap();
        assert_eq!(rebuilt, 2);

        let a = store.get_addr_balance(&lock_a).unwrap().unwrap();
        assert_eq!(a.balance, 400);
        assert_eq!(a.occupied_capacity, 440);
        assert_eq!(a.live_cells_count, 2);

        let b = store.get_addr_balance(&lock_b).unwrap().unwrap();
        assert_eq!(b.balance, 50);
        assert_eq!(b.occupied_capacity, 60);
        assert_eq!(b.live_cells_count, 1);
    }

    #[test]
    fn test_rebuild_addr_balances_restores_txs_count_from_addr_txs() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let lock_a = vec![0xAA; 32];
        let lock_b = vec![0xBB; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_cell(&[0x01; 32], 0, &make_cell(lock_a.clone(), 10, 100, 120));
        batch.put_cell(&[0x02; 32], 0, &make_cell(lock_a.clone(), 11, 300, 320));
        batch.put_cell(&[0x03; 32], 0, &make_cell(lock_b.clone(), 12, 50, 60));
        batch.put_addr_tx(&lock_a, 10, 0, &[0x11; 32]);
        batch.put_addr_tx(&lock_a, 11, 1, &[0x22; 32]);
        batch.put_addr_tx(&lock_b, 12, 0, &[0x33; 32]);
        // This address has no live cells; rebuild should not materialize addr_balance for it.
        batch.put_addr_tx(&[0xCC; 32], 9, 0, &[0x44; 32]);
        batch.commit().unwrap();

        store.rebuild_addr_balances_from_live_cells().unwrap();

        let a = store.get_addr_balance(&lock_a).unwrap().unwrap();
        assert_eq!(a.txs_count, 2);

        let b = store.get_addr_balance(&lock_b).unwrap().unwrap();
        assert_eq!(b.txs_count, 1);

        assert!(store.get_addr_balance(&[0xCC; 32]).unwrap().is_none());
    }

    #[test]
    fn test_rebuild_addr_balances_errors_on_negative_aggregate() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let lock_a = vec![0xAA; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_cell(&[0x01; 32], 0, &make_cell(lock_a.clone(), 10, -1, -1));
        batch.commit().unwrap();

        let err = store.rebuild_addr_balances_from_live_cells().unwrap_err();
        assert!(err.to_string().contains("negative aggregate"));
    }

    #[test]
    fn test_derive_addr_stats_from_cells() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let lock_a = vec![0xAA; 32];

        // Put cells + cell-by-lock index (simulating what indexer does)
        let mut batch = StoreBatch::new(&store);
        batch.put_cell(&[0x01; 32], 0, &make_cell(lock_a.clone(), 10, 100, 60));
        batch.put_cell_by_lock(&lock_a, 10, &[0x01; 32], 0);
        batch.put_cell(&[0x02; 32], 0, &make_cell(lock_a.clone(), 20, 200, 80));
        batch.put_cell_by_lock(&lock_a, 20, &[0x02; 32], 0);
        batch.commit().unwrap();

        // No materialized stats — get_addr_balance should derive from cells
        let stats = store.get_addr_balance(&lock_a).unwrap().unwrap();
        assert_eq!(stats.balance, 300);
        assert_eq!(stats.occupied_capacity, 140);
        assert_eq!(stats.live_cells_count, 2);
    }

    #[test]
    fn test_derive_addr_stats_returns_none_for_unknown_address() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let lock = vec![0xFF; 32];

        assert!(store.get_addr_balance(&lock).unwrap().is_none());
    }

    #[test]
    fn test_derive_addr_stats_includes_txs_count() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let lock_a = vec![0xAA; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_cell(&[0x01; 32], 0, &make_cell(lock_a.clone(), 10, 100, 60));
        batch.put_cell_by_lock(&lock_a, 10, &[0x01; 32], 0);
        batch.put_addr_tx(&lock_a, 10, 0, &[0x01; 32]);
        batch.put_addr_tx(&lock_a, 20, 1, &[0x02; 32]);
        batch.commit().unwrap();

        let stats = store.get_addr_balance(&lock_a).unwrap().unwrap();
        assert_eq!(stats.txs_count, 2);
    }

    #[test]
    fn test_materialized_stats_preferred_over_derive() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let lock_a = vec![0xAA; 32];

        // Write both materialized and cell data
        let mut batch = StoreBatch::new(&store);
        batch.put_cell(&[0x01; 32], 0, &make_cell(lock_a.clone(), 10, 100, 60));
        batch.put_cell_by_lock(&lock_a, 10, &[0x01; 32], 0);
        let materialized = AddressBalance {
            balance: 999,
            occupied_capacity: 500,
            live_cells_count: 200,
            total_cells_count: 300,
            txs_count: 50,
            first_seen_block: 1,
            first_seen_tx: vec![0xFF; 32],
            last_activity_block: 100,
            last_activity_tx: vec![0xEE; 32],
        };
        batch.put_addr_balance(&lock_a, &materialized);
        batch.commit().unwrap();

        // Should return materialized, not derived
        let stats = store.get_addr_balance(&lock_a).unwrap().unwrap();
        assert_eq!(stats.balance, 999);
        assert_eq!(stats.live_cells_count, 200);
    }

    #[test]
    fn test_cleanup_sub_threshold_addr_stats() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let lock_small = vec![0xAA; 32];
        let lock_big = vec![0xBB; 32];

        let mut batch = StoreBatch::new(&store);
        // Small address (below threshold)
        batch.put_addr_balance(
            &lock_small,
            &AddressBalance {
                balance: 100,
                live_cells_count: 5,
                ..Default::default()
            },
        );
        // Big address (at threshold)
        batch.put_addr_balance(
            &lock_big,
            &AddressBalance {
                balance: 10000,
                live_cells_count: crate::store::ADDR_STATS_THRESHOLD,
                ..Default::default()
            },
        );
        batch.commit().unwrap();

        let deleted = store.cleanup_sub_threshold_addr_stats().unwrap();
        assert_eq!(deleted, 1);

        // Small should be gone from materialized
        assert!(store
            .get_cf(store.cf_addr_stats(), &lock_small)
            .unwrap()
            .is_none());
        // Big should remain
        assert!(store
            .get_cf(store.cf_addr_stats(), &lock_big)
            .unwrap()
            .is_some());
    }
}
