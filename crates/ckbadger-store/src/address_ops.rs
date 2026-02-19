//! Address balance operations.

use rocksdb::IteratorMode;

use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::{AddressBalance, LiveCellInfo};

impl CkbadgerStore {
    pub fn get_addr_balance(&self, lock_hash: &[u8]) -> anyhow::Result<Option<AddressBalance>> {
        match self.get_cf(self.cf_addr_balance(), lock_hash)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_addr_balance_direct(
        &self,
        lock_hash: &[u8],
        balance: &AddressBalance,
    ) -> anyhow::Result<()> {
        let value = bincode::serialize(balance)?;
        self.put_cf(self.cf_addr_balance(), lock_hash, &value)
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
        let iter = self.iterator_cf(self.cf_addr_balance(), IteratorMode::Start);

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
        use crate::keys;
        use crate::types::{decode_consumed_cell_info, LiveCellInfo};
        use std::collections::{HashMap, HashSet};

        let mut count = 0u64;
        let mut batch = rocksdb::WriteBatch::default();
        let batch_size = 10_000u64;

        // Collect (tx_hash, lock_script_hash) pairs from live cells
        // Group by tx_hash to batch tx_idx lookups
        let mut tx_addresses: HashMap<Vec<u8>, HashSet<Vec<u8>>> = HashMap::new();
        let mut tx_blocks: HashMap<Vec<u8>, i64> = HashMap::new();

        // Phase 1: Scan live cells
        let iter = self.iterator_cf(self.cf_live_cells(), rocksdb::IteratorMode::Start);
        for item in iter.flatten() {
            let (key, value) = item;
            if key.len() == keys::OUTPOINT_KEY_SIZE {
                if let Ok(info) = bincode::deserialize::<LiveCellInfo>(&value) {
                    let tx_hash = key[..32].to_vec();
                    tx_addresses
                        .entry(tx_hash.clone())
                        .or_default()
                        .insert(info.lock_script_hash);
                    tx_blocks.entry(tx_hash).or_insert(info.created_at_block);
                }
            }
        }

        // Phase 2: Scan consumed cells
        let iter = self.iterator_cf(self.cf_consumed_cells(), rocksdb::IteratorMode::Start);
        for item in iter.flatten() {
            let (key, value) = item;
            if key.len() == keys::OUTPOINT_KEY_SIZE {
                let lock_hash = decode_consumed_cell_info(&value)
                    .map(|c| (c.cell.lock_script_hash, c.cell.created_at_block));
                if let Some((lock_hash, created_at_block)) = lock_hash {
                    let tx_hash = key[..32].to_vec();
                    tx_addresses
                        .entry(tx_hash.clone())
                        .or_default()
                        .insert(lock_hash);
                    tx_blocks.entry(tx_hash).or_insert(created_at_block);
                }
            }
        }

        // Phase 3: Batch lookup tx_idx for each tx_hash
        let tx_hashes: Vec<Vec<u8>> = tx_addresses.keys().cloned().collect();
        let cf_tx_hash_map = self.cf_tx_hash_map();

        for chunk in tx_hashes.chunks(5000) {
            let cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> = chunk
                .iter()
                .map(|h| (cf_tx_hash_map, h.as_slice()))
                .collect();
            let results = self.multi_get_cf(cf_keys);

            for (i, result) in results.into_iter().enumerate() {
                if let Ok(Some(value)) = result {
                    if value.len() == 12 {
                        let block_num = keys::decode_block_num(&value[..8]);
                        let tx_idx = keys::decode_tx_idx(&value[8..12]);
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
    ///
    /// Other fields are re-initialized conservatively.
    pub fn rebuild_addr_balances_from_live_cells(&self) -> anyhow::Result<usize> {
        use std::collections::HashMap;

        struct Agg {
            balance: i128,
            occupied_capacity: i128,
            live_cells_count: i32,
            first_seen_block: i64,
            first_seen_tx: Vec<u8>,
            last_activity_block: i64,
            last_activity_tx: Vec<u8>,
        }

        let mut agg_by_lock: HashMap<Vec<u8>, Agg> = HashMap::new();
        let iter = self.iterator_cf(self.cf_live_cells(), rocksdb::IteratorMode::Start);
        for item in iter.flatten() {
            let (key, value) = item;
            if key.len() != keys::OUTPOINT_KEY_SIZE {
                continue;
            }
            let info: LiveCellInfo = match bincode::deserialize(&value) {
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

        // Clear existing addr_balance CF
        let mut clear_batch = rocksdb::WriteBatch::default();
        let mut cleared = 0usize;
        let iter = self.iterator_cf(self.cf_addr_balance(), IteratorMode::Start);
        for item in iter.flatten() {
            let (key, _) = item;
            clear_batch.delete_cf(self.cf_addr_balance(), &key);
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
            let balance = AddressBalance {
                balance: agg.balance,
                occupied_capacity: agg.occupied_capacity.max(0),
                live_cells_count: agg.live_cells_count.max(0),
                total_cells_count: agg.live_cells_count.max(0) as i64,
                txs_count: 0,
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
            data_size: 0,
            occupied_capacity: occupied,
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
}
