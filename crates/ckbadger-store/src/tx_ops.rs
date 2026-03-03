//! Transaction index operations.

use anyhow::{anyhow, Context};

use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::TxIndexEntry;

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
            Some(value) if value.len() == 12 => {
                let block_num = keys::decode_block_num(&value[..8]);
                let tx_idx = keys::decode_tx_idx(&value[8..12]);
                Ok(Some((block_num, tx_idx)))
            }
            _ => Ok(None),
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
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::StoreBatch;

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
}
