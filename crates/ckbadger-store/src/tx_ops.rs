//! Transaction index operations.

use anyhow::{anyhow, Context};
use std::collections::HashMap;

use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::TxIndexEntry;

pub(crate) type TxByHashBatchEntry = (Vec<u8>, Option<(i64, i32, TxIndexEntry)>);
pub(crate) type CanonicalTxIdentityBatchEntry = (Vec<u8>, Option<(i64, i32, Vec<u8>)>);

use crate::bytes_to_hex;

impl CkbadgerStore {
    pub fn get_tx_index(
        &self,
        block_num: i64,
        tx_idx: i32,
    ) -> anyhow::Result<Option<TxIndexEntry>> {
        let key = keys::encode_composite(&[
            &keys::encode_block_num(block_num),
            &keys::encode_tx_idx(tx_idx),
        ]);
        match self.get_cf(self.cf_tx_index(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    /// Look up block_num and tx_idx by transaction hash.
    pub fn get_tx_location(&self, tx_hash: &[u8]) -> anyhow::Result<Option<(i64, i32)>> {
        match self.get_cf(self.cf_tx_hash_map(), tx_hash)? {
            None => Ok(None),
            Some(value) if value.len() == 12 => {
                let block_num = keys::decode_block_num(&value[..8]);
                let tx_idx = keys::decode_tx_idx(&value[8..12]);
                Ok(Some((block_num, tx_idx)))
            }
            Some(value) => {
                anyhow::bail!(
                    "tx_hash_map: corrupt value length {} (expected 12) for tx_hash=0x{}",
                    value.len(),
                    bytes_to_hex(tx_hash)
                )
            }
        }
    }

    /// Get full transaction info: location + index entry.
    pub fn get_tx_by_hash(
        &self,
        tx_hash: &[u8],
    ) -> anyhow::Result<Option<(i64, i32, TxIndexEntry)>> {
        if let Some((block_num, tx_idx)) = self.get_tx_location(tx_hash)? {
            if let Some(entry) = self.get_tx_index(block_num, tx_idx)? {
                return Ok(Some((block_num, tx_idx, entry)));
            }
        }
        Ok(None)
    }

    /// Batch-fetch transaction location + index entry by tx hash.
    pub fn get_txs_by_hash_batch(
        &self,
        tx_hashes: &[Vec<u8>],
    ) -> anyhow::Result<Vec<TxByHashBatchEntry>> {
        if tx_hashes.is_empty() {
            return Ok(Vec::new());
        }

        let tx_hash_cf = self.cf_tx_hash_map();
        let tx_index_cf = self.cf_tx_index();

        let tx_hash_cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> = tx_hashes
            .iter()
            .map(|hash| (tx_hash_cf, hash.as_slice()))
            .collect();
        let location_values = self.multi_get_cf(tx_hash_cf_keys);

        let mut locations: Vec<Option<(i64, i32)>> = vec![None; tx_hashes.len()];
        let mut present_indices = Vec::new();
        let mut tx_index_keys: Vec<Vec<u8>> = Vec::new();
        for (i, value_result) in location_values.into_iter().enumerate() {
            match value_result {
                Ok(Some(value)) => {
                    if value.len() != 12 {
                        return Err(anyhow!(
                            "invalid tx_hash_map value length in get_txs_by_hash_batch: tx_hash=0x{}, value_len={}",
                            bytes_to_hex(&tx_hashes[i]),
                            value.len()
                        ));
                    }
                    let block_num = keys::decode_block_num(&value[..8]);
                    let tx_idx = keys::decode_tx_idx(&value[8..12]);
                    locations[i] = Some((block_num, tx_idx));
                    present_indices.push(i);
                    tx_index_keys.push(keys::encode_composite(&[
                        &keys::encode_block_num(block_num),
                        &keys::encode_tx_idx(tx_idx),
                    ]));
                }
                Ok(None) => {}
                Err(e) => {
                    return Err(anyhow!(
                        "rocksdb multi_get failed in get_txs_by_hash_batch: tx_hash=0x{}, error={}",
                        bytes_to_hex(&tx_hashes[i]),
                        e
                    ));
                }
            }
        }

        let tx_index_cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> = tx_index_keys
            .iter()
            .map(|key| (tx_index_cf, key.as_slice()))
            .collect();
        let tx_index_values = self.multi_get_cf(tx_index_cf_keys);
        let mut tx_entries: HashMap<usize, TxIndexEntry> = HashMap::new();
        for (batch_idx, value_result) in tx_index_values.into_iter().enumerate() {
            let out_idx = present_indices[batch_idx];
            match value_result {
                Ok(Some(value)) => {
                    let entry = bincode::deserialize::<TxIndexEntry>(&value).map_err(|e| {
                        anyhow!(
                            "failed to deserialize tx index in get_txs_by_hash_batch: tx_hash=0x{}, error={}",
                            bytes_to_hex(&tx_hashes[out_idx]),
                            e
                        )
                    })?;
                    tx_entries.insert(out_idx, entry);
                }
                Ok(None) => {}
                Err(e) => {
                    return Err(anyhow!(
                        "rocksdb multi_get failed while reading tx_index in get_txs_by_hash_batch: tx_hash=0x{}, error={}",
                        bytes_to_hex(&tx_hashes[out_idx]),
                        e
                    ));
                }
            }
        }

        let mut out = Vec::with_capacity(tx_hashes.len());
        for (i, tx_hash) in tx_hashes.iter().enumerate() {
            let item = locations[i].and_then(|(block_num, tx_idx)| {
                tx_entries
                    .get(&i)
                    .cloned()
                    .map(|entry| (block_num, tx_idx, entry))
            });
            out.push((tx_hash.clone(), item));
        }
        Ok(out)
    }

    /// Batch-fetch canonical transaction identity by tx hash:
    /// `(block_num, tx_idx, canonical_block_hash)`.
    pub fn get_canonical_tx_identities_by_hash_batch(
        &self,
        tx_hashes: &[Vec<u8>],
    ) -> anyhow::Result<Vec<CanonicalTxIdentityBatchEntry>> {
        let tx_rows = self.get_txs_by_hash_batch(tx_hashes)?;
        if tx_rows.is_empty() {
            return Ok(Vec::new());
        }

        let mut block_numbers = Vec::new();
        let mut seen_blocks = std::collections::HashSet::new();
        for (_, row_opt) in &tx_rows {
            if let Some((block_num, _, _)) = row_opt {
                if seen_blocks.insert(*block_num) {
                    block_numbers.push(*block_num);
                }
            }
        }

        let headers_by_block = self.get_block_headers_batch(&block_numbers)?;
        let mut out = Vec::with_capacity(tx_rows.len());
        for (tx_hash, row_opt) in tx_rows {
            let identity = match row_opt {
                Some((block_num, tx_idx, _)) => {
                    let header = headers_by_block.get(&block_num).ok_or_else(|| {
                        anyhow!(
                            "missing block header while resolving canonical tx identity: tx_hash=0x{}, block_num={}, tx_idx={}",
                            bytes_to_hex(&tx_hash),
                            block_num,
                            tx_idx
                        )
                    })?;
                    Some((block_num, tx_idx, header.hash.clone()))
                }
                None => None,
            };
            out.push((tx_hash, identity));
        }
        Ok(out)
    }

    /// Update cycles for a transaction identified by tx hash.
    pub fn update_tx_cycles_by_hash(&self, tx_hash: &[u8], cycles: i64) -> anyhow::Result<()> {
        let (block_num, tx_idx) = self
            .get_tx_location(tx_hash)?
            .ok_or_else(|| anyhow!("transaction location not found"))?;

        self.update_tx_cycles(block_num, tx_idx, cycles)
            .with_context(|| {
                format!(
                    "failed to update tx cycles for block {} tx {}",
                    block_num, tx_idx
                )
            })
    }

    /// Update cycles for a transaction at a known location.
    // SAFETY: Called only from the cycles worker after batch commit completes for
    // the target block. No concurrent batch can write the same tx_index key.
    pub fn update_tx_cycles(&self, block_num: i64, tx_idx: i32, cycles: i64) -> anyhow::Result<()> {
        let mut entry = self
            .get_tx_index(block_num, tx_idx)?
            .ok_or_else(|| anyhow!("transaction index entry not found"))?;
        entry.cycles = Some(cycles);

        let key = keys::encode_composite(&[
            &keys::encode_block_num(block_num),
            &keys::encode_tx_idx(tx_idx),
        ]);
        let value = bincode::serialize(&entry).with_context(|| {
            format!(
                "failed to serialize tx index entry {}:{}",
                block_num, tx_idx
            )
        })?;

        self.put_cf(self.cf_tx_index(), &key, &value)
            .with_context(|| {
                format!(
                    "failed to write tx index entry for block {} tx {}",
                    block_num, tx_idx
                )
            })
    }

    /// List transactions for a block, ordered by tx_index.
    pub fn list_block_txs(&self, block_num: i64) -> anyhow::Result<Vec<(i32, TxIndexEntry)>> {
        let prefix = keys::encode_block_num(block_num);
        let iter = self.prefix_iterator_cf(self.cf_tx_index(), &prefix);

        let mut results = Vec::new();
        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!("failed to iterate tx_index in list_block_txs: {}", e)
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            if key.len() == 12 {
                let tx_idx = keys::decode_tx_idx(&key[8..12]);
                let entry: TxIndexEntry = bincode::deserialize(&value)?;
                results.push((tx_idx, entry));
            }
        }
        Ok(results)
    }

    /// List the highest-indexed transactions below `before_tx_idx`, in ascending order.
    ///
    /// Collects all entries with `tx_idx < before_tx_idx`, then returns the last
    /// `limit` entries (the ones closest to the cursor). This is used for
    /// descending cross-block pagination where we need the highest tx indices
    /// within a block that are below the cursor position.
    pub fn list_block_txs_before(
        &self,
        block_num: i64,
        before_tx_idx: i32,
        limit: usize,
    ) -> anyhow::Result<Vec<(i32, TxIndexEntry)>> {
        let prefix = keys::encode_block_num(block_num);
        let iter = self.prefix_iterator_cf(self.cf_tx_index(), &prefix);

        let mut all = Vec::new();
        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!("failed to iterate tx_index in list_block_txs_before: {}", e)
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            if key.len() == 12 {
                let tx_idx = keys::decode_tx_idx(&key[8..12]);
                if tx_idx >= before_tx_idx {
                    break;
                }
                let entry: TxIndexEntry = bincode::deserialize(&value)?;
                all.push((tx_idx, entry));
            }
        }
        // Return the last `limit` entries (highest tx_idx values below cursor)
        if all.len() > limit {
            Ok(all.split_off(all.len() - limit))
        } else {
            Ok(all)
        }
    }

    /// List transactions for a block with tx_idx > `after_tx_idx`, ordered ascending.
    /// Returns at most `limit` entries. Used for ascending single-block pagination.
    pub fn list_block_txs_after(
        &self,
        block_num: i64,
        after_tx_idx: i32,
        limit: usize,
    ) -> anyhow::Result<Vec<(i32, TxIndexEntry)>> {
        let prefix = keys::encode_block_num(block_num);
        let iter = self.prefix_iterator_cf(self.cf_tx_index(), &prefix);

        let mut results = Vec::new();
        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!("failed to iterate tx_index in list_block_txs_after: {}", e)
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            if key.len() == 12 {
                let tx_idx = keys::decode_tx_idx(&key[8..12]);
                if tx_idx <= after_tx_idx {
                    continue;
                }
                let entry: TxIndexEntry = bincode::deserialize(&value)?;
                results.push((tx_idx, entry));
                if results.len() >= limit {
                    break;
                }
            }
        }
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::StoreBatch;

    fn make_header(hash_byte: u8) -> crate::types::CachedBlockHeader {
        crate::types::CachedBlockHeader {
            hash: vec![hash_byte; 32],
            parent_hash: vec![0u8; 32],
            timestamp: 1_700_000_000_000,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0; 32],
            transactions_count: 1,
            uncles_count: 0,
            cycles: None,
        }
    }

    #[test]
    fn test_update_tx_cycles_by_hash() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let tx_hash = [0x11u8; 32];
        let block_num = 123;
        let tx_idx = 2;

        let mut batch = StoreBatch::new(&store);
        batch.put_tx_hash_map(&tx_hash, block_num, tx_idx);
        batch.put_tx_index(
            block_num,
            tx_idx,
            &TxIndexEntry {
                is_cellbase: false,
                timestamp: 1_700_000_000_000,
                inputs_count: 1,
                outputs_count: 1,
                fee: 1_000,
                tx_size: 200,
                cycles: None,
                semantic_tags: 0,
            },
        );
        batch.commit().unwrap();

        store.update_tx_cycles_by_hash(&tx_hash, 12_345).unwrap();

        let (_, _, updated) = store.get_tx_by_hash(&tx_hash).unwrap().unwrap();
        assert_eq!(updated.cycles, Some(12_345));
    }

    #[test]
    fn test_update_tx_cycles_by_hash_not_found() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let tx_hash = [0x22u8; 32];
        let err = store.update_tx_cycles_by_hash(&tx_hash, 9_999).unwrap_err();
        assert!(err.to_string().contains("transaction location not found"));
    }

    #[test]
    fn test_get_txs_by_hash_batch_reads_multiple_entries() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let tx_hash_a = [0x11u8; 32];
        let tx_hash_b = [0x22u8; 32];
        let tx_hash_missing = [0x33u8; 32];

        let entry_a = TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_000_000,
            inputs_count: 1,
            outputs_count: 1,
            fee: 1_000,
            tx_size: 200,
            cycles: Some(10),
            semantic_tags: 0,
        };
        let entry_b = TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_000_111,
            inputs_count: 2,
            outputs_count: 2,
            fee: 2_000,
            tx_size: 300,
            cycles: Some(20),
            semantic_tags: 0,
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_tx_hash_map(&tx_hash_a, 100, 1);
        batch.put_tx_index(100, 1, &entry_a);
        batch.put_tx_hash_map(&tx_hash_b, 101, 2);
        batch.put_tx_index(101, 2, &entry_b);
        batch.commit().unwrap();

        let rows = store
            .get_txs_by_hash_batch(&[
                tx_hash_a.to_vec(),
                tx_hash_b.to_vec(),
                tx_hash_missing.to_vec(),
            ])
            .unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].0, tx_hash_a.to_vec());
        assert_eq!(rows[0].1.as_ref().unwrap().0, 100);
        assert_eq!(rows[0].1.as_ref().unwrap().1, 1);
        assert_eq!(rows[0].1.as_ref().unwrap().2.cycles, Some(10));
        assert_eq!(rows[1].0, tx_hash_b.to_vec());
        assert_eq!(rows[1].1.as_ref().unwrap().0, 101);
        assert_eq!(rows[1].1.as_ref().unwrap().1, 2);
        assert_eq!(rows[1].1.as_ref().unwrap().2.cycles, Some(20));
        assert!(rows[2].1.is_none());
    }

    #[test]
    fn test_get_txs_by_hash_batch_fails_on_invalid_location_payload() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let tx_hash = [0x44u8; 32];
        store
            .put_cf(store.cf_tx_hash_map(), &tx_hash, b"short")
            .unwrap();

        let err = store
            .get_txs_by_hash_batch(&[tx_hash.to_vec()])
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("invalid tx_hash_map value length in get_txs_by_hash_batch"));
    }

    #[test]
    fn test_list_block_txs_before_returns_highest_below_cursor() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let block_num = 500i64;
        let mut batch = StoreBatch::new(&store);
        for i in 0..5 {
            batch.put_tx_index(
                block_num,
                i,
                &TxIndexEntry {
                    is_cellbase: i == 0,
                    timestamp: 1_700_000_000_000,
                    inputs_count: 1,
                    outputs_count: 1,
                    fee: (i as i64) * 100,
                    tx_size: 200,
                    cycles: None,
                    semantic_tags: 0,
                },
            );
        }
        batch.commit().unwrap();

        // before_tx_idx=3 should return txs 0,1,2
        let results = store.list_block_txs_before(block_num, 3, 100).unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].0, 0);
        assert_eq!(results[1].0, 1);
        assert_eq!(results[2].0, 2);

        // before_tx_idx=i32::MAX should return all 5
        let all = store
            .list_block_txs_before(block_num, i32::MAX, 100)
            .unwrap();
        assert_eq!(all.len(), 5);

        // limit=2 should return the HIGHEST 2 below cursor (3, 4)
        let limited = store.list_block_txs_before(block_num, i32::MAX, 2).unwrap();
        assert_eq!(limited.len(), 2);
        assert_eq!(limited[0].0, 3);
        assert_eq!(limited[1].0, 4);

        // limit=2, before_tx_idx=4 should return (2, 3) — highest 2 below 4
        let limited2 = store.list_block_txs_before(block_num, 4, 2).unwrap();
        assert_eq!(limited2.len(), 2);
        assert_eq!(limited2[0].0, 2);
        assert_eq!(limited2[1].0, 3);

        // before_tx_idx=0 should return empty
        let empty = store.list_block_txs_before(block_num, 0, 100).unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn test_list_block_txs_after_returns_ascending() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();

        let block_num = 600i64;
        let mut batch = StoreBatch::new(&store);
        for i in 0..5 {
            batch.put_tx_index(
                block_num,
                i,
                &TxIndexEntry {
                    is_cellbase: i == 0,
                    timestamp: 1_700_000_000_000,
                    inputs_count: 1,
                    outputs_count: 1,
                    fee: (i as i64) * 100,
                    tx_size: 200,
                    cycles: None,
                    semantic_tags: 0,
                },
            );
        }
        batch.commit().unwrap();

        // after_tx_idx=-1 should return all from start
        let all = store.list_block_txs_after(block_num, -1, 100).unwrap();
        assert_eq!(all.len(), 5);
        assert_eq!(all[0].0, 0);
        assert_eq!(all[4].0, 4);

        // after_tx_idx=1 should return txs 2,3,4
        let results = store.list_block_txs_after(block_num, 1, 100).unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].0, 2);
        assert_eq!(results[1].0, 3);
        assert_eq!(results[2].0, 4);

        // limit caps results
        let limited = store.list_block_txs_after(block_num, -1, 2).unwrap();
        assert_eq!(limited.len(), 2);
        assert_eq!(limited[0].0, 0);
        assert_eq!(limited[1].0, 1);

        // after last tx returns empty
        let empty = store.list_block_txs_after(block_num, 4, 100).unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn test_get_canonical_tx_identities_by_hash_batch_reads_block_hashes() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let tx_hash = [0x55u8; 32];
        let missing_tx = [0x66u8; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_tx_hash_map(&tx_hash, 200, 3);
        batch.put_tx_index(
            200,
            3,
            &TxIndexEntry {
                is_cellbase: false,
                timestamp: 1_700_000_000_000,
                inputs_count: 1,
                outputs_count: 1,
                fee: 0,
                tx_size: 100,
                cycles: None,
                semantic_tags: 0,
            },
        );
        batch.put_block_header(200, &make_header(0xAB));
        batch.commit().unwrap();

        let rows = store
            .get_canonical_tx_identities_by_hash_batch(&[tx_hash.to_vec(), missing_tx.to_vec()])
            .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, tx_hash.to_vec());
        assert_eq!(rows[0].1, Some((200, 3, vec![0xAB; 32])));
        assert_eq!(rows[1], (missing_tx.to_vec(), None));
    }
}
