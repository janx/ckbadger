//! Transaction index operations.
//!
//! Data lives in two CFs across two physical stores:
//! - `tx_meta` (append): tx_hash → TxMeta (SSOT, immutable except cycles fill)
//! - `tx_index` (default): (block_num, tx_idx) → tx_hash (thin index, reorg-safe)

use anyhow::{anyhow, Context};

use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::TxIndexEntry;

impl CkbadgerStore {
    /// Get tx metadata by block position. Chains: tx_index → tx_hash → tx_meta.
    pub fn get_tx_index(
        &self,
        block_num: i64,
        tx_idx: i32,
    ) -> anyhow::Result<Option<TxIndexEntry>> {
        let key = keys::encode_composite(&[
            &keys::encode_block_num(block_num),
            &keys::encode_tx_idx(tx_idx),
        ]);
        // tx_index (default): (block_num, tx_idx) → tx_hash
        let tx_hash = match self.get_cf(self.cf_tx_index(), &key)? {
            Some(hash) => hash,
            None => return Ok(None),
        };
        // tx_meta (append): tx_hash → TxMeta
        match self.append_get_cf(self.cf_tx_meta(), &tx_hash)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    /// Look up block_num and tx_idx by transaction hash.
    pub fn get_tx_location(&self, tx_hash: &[u8]) -> anyhow::Result<Option<(i64, i32)>> {
        // tx_meta (append): tx_hash → TxMeta → (block_number, tx_index)
        match self.append_get_cf(self.cf_tx_meta(), tx_hash)? {
            Some(value) => {
                let meta: TxIndexEntry = bincode::deserialize(&value)?;
                Ok(Some((meta.block_number, meta.tx_index)))
            }
            None => Ok(None),
        }
    }

    /// Get full transaction info: location + index entry. Direct append store lookup.
    pub fn get_tx_by_hash(
        &self,
        tx_hash: &[u8],
    ) -> anyhow::Result<Option<(i64, i32, TxIndexEntry)>> {
        match self.append_get_cf(self.cf_tx_meta(), tx_hash)? {
            Some(value) => {
                let entry: TxIndexEntry = bincode::deserialize(&value)?;
                Ok(Some((entry.block_number, entry.tx_index, entry)))
            }
            None => Ok(None),
        }
    }

    /// Update cycles for a transaction identified by tx hash.
    pub fn update_tx_cycles_by_hash(&self, tx_hash: &[u8], cycles: i64) -> anyhow::Result<()> {
        let value = self
            .append_get_cf(self.cf_tx_meta(), tx_hash)?
            .ok_or_else(|| anyhow!("transaction not found"))?;
        let mut entry: TxIndexEntry =
            bincode::deserialize(&value).with_context(|| "failed to deserialize TxMeta")?;
        entry.cycles = Some(cycles);

        let updated = bincode::serialize(&entry).with_context(|| "failed to serialize TxMeta")?;
        self.append_put_cf(self.cf_tx_meta(), tx_hash, &updated)
            .with_context(|| "failed to write updated TxMeta")
    }

    /// Update cycles for a transaction at a known location.
    pub fn update_tx_cycles(&self, block_num: i64, tx_idx: i32, cycles: i64) -> anyhow::Result<()> {
        // Get tx_hash from tx_index (default)
        let key = keys::encode_composite(&[
            &keys::encode_block_num(block_num),
            &keys::encode_tx_idx(tx_idx),
        ]);
        let tx_hash = self
            .get_cf(self.cf_tx_index(), &key)?
            .ok_or_else(|| anyhow!("tx index entry not found for {}:{}", block_num, tx_idx))?;

        self.update_tx_cycles_by_hash(&tx_hash, cycles)
    }

    /// List transactions for a block, ordered by tx_index.
    pub fn list_block_txs(&self, block_num: i64) -> anyhow::Result<Vec<(i32, TxIndexEntry)>> {
        let prefix = keys::encode_block_num(block_num);
        let iter = self.prefix_iterator_cf(self.cf_tx_index(), &prefix);

        let mut results = Vec::new();
        for item in iter.flatten() {
            let (key, tx_hash) = item;
            if !key.starts_with(&prefix) {
                break;
            }
            if key.len() == 12 {
                let tx_idx = keys::decode_tx_idx(&key[8..12]);
                if let Some(value) = self.append_get_cf(self.cf_tx_meta(), &tx_hash)? {
                    let entry: TxIndexEntry = bincode::deserialize(&value)?;
                    results.push((tx_idx, entry));
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

    #[test]
    fn test_update_tx_cycles_by_hash() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();

        let tx_hash = [0x11u8; 32];
        let block_num = 123;
        let tx_idx = 2;

        let mut batch = StoreBatch::new(&store);
        batch.put_tx_index(
            block_num,
            tx_idx,
            &tx_hash,
            &TxIndexEntry {
                block_number: block_num,
                tx_index: tx_idx,
                is_cellbase: false,
                timestamp: 1_700_000_000_000,
                inputs_count: 1,
                outputs_count: 1,
                fee: 1_000,
                tx_size: 200,
                cycles: None,
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
        let store = CkbadgerStore::open(dir.path()).unwrap();

        let tx_hash = [0x22u8; 32];
        let err = store.update_tx_cycles_by_hash(&tx_hash, 9_999).unwrap_err();
        assert!(err.to_string().contains("transaction not found"));
    }

    #[test]
    fn test_get_tx_location() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();

        let tx_hash = [0x33u8; 32];
        let mut batch = StoreBatch::new(&store);
        batch.put_tx_index(
            50,
            3,
            &tx_hash,
            &TxIndexEntry {
                block_number: 50,
                tx_index: 3,
                is_cellbase: false,
                timestamp: 1_700_000_000_000,
                inputs_count: 1,
                outputs_count: 2,
                fee: 500,
                tx_size: 100,
                cycles: None,
            },
        );
        batch.commit().unwrap();

        let loc = store.get_tx_location(&tx_hash).unwrap().unwrap();
        assert_eq!(loc, (50, 3));
    }

    #[test]
    fn test_list_block_txs() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();

        let mut batch = StoreBatch::new(&store);
        for idx in 0..3 {
            let tx_hash = [idx as u8 + 1; 32];
            batch.put_tx_index(
                100,
                idx,
                &tx_hash,
                &TxIndexEntry {
                    block_number: 100,
                    tx_index: idx,
                    is_cellbase: idx == 0,
                    timestamp: 1_700_000_000_000,
                    inputs_count: 1,
                    outputs_count: 1,
                    fee: 0,
                    tx_size: 80,
                    cycles: None,
                },
            );
        }
        batch.commit().unwrap();

        let txs = store.list_block_txs(100).unwrap();
        assert_eq!(txs.len(), 3);
        assert_eq!(txs[0].0, 0);
        assert!(txs[0].1.is_cellbase);
        assert_eq!(txs[2].0, 2);
    }
}
