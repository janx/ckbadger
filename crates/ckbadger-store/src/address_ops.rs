//! Address balance operations.

use rocksdb::IteratorMode;

use crate::store::CkbadgerStore;
use crate::types::AddressBalance;

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
    ) -> anyhow::Result<Vec<(i64, i32, Vec<u8>)>> {
        // Seek to the end of the prefix range and iterate backwards
        let mut upper_key = Vec::with_capacity(44);
        upper_key.extend_from_slice(lock_hash);
        upper_key.extend_from_slice(&[0xFF; 12]); // max block_num(8) + max tx_idx(4)

        let iter = self.iterator_cf(
            self.cf_addr_txs(),
            rocksdb::IteratorMode::From(&upper_key, rocksdb::Direction::Reverse),
        );

        let mut results = Vec::new();
        for item in iter.flatten() {
            let (key, value) = item;
            if !key.starts_with(lock_hash) {
                break;
            }
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
        use crate::types::{CompactConsumedCellInfo, LiveCellInfo};
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
                let lock_hash =
                    if let Ok(compact) = bincode::deserialize::<CompactConsumedCellInfo>(&value) {
                        Some((compact.lock_script_hash, compact.created_at_block))
                    } else if let Ok(info) = bincode::deserialize::<LiveCellInfo>(&value) {
                        Some((info.lock_script_hash, info.created_at_block))
                    } else {
                        None
                    };
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
}
