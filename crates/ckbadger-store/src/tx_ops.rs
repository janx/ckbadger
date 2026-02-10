//! Transaction index operations.

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

    /// List transactions for a block, ordered by tx_index.
    pub fn list_block_txs(&self, block_num: i64) -> anyhow::Result<Vec<(i32, TxIndexEntry)>> {
        let prefix = keys::encode_block_num(block_num);
        let iter = self.prefix_iterator_cf(self.cf_tx_index(), &prefix);

        let mut results = Vec::new();
        for item in iter.flatten() {
            let (key, value) = item;
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
