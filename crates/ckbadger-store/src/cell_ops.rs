//! Cell read/write operations.

use std::collections::HashMap;

use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::{CompactConsumedCellInfo, LiveCellInfo};

impl CkbadgerStore {
    pub fn get_cell(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> anyhow::Result<Option<LiveCellInfo>> {
        let key = keys::encode_outpoint(tx_hash, output_index);
        match self.get_cf(self.cf_live_cells(), &key)? {
            Some(value) => Ok(bincode::deserialize(&value).ok()),
            None => Ok(None),
        }
    }

    pub fn get_cells_batch(
        &self,
        outpoints: &[(&[u8], i16)],
    ) -> HashMap<(Vec<u8>, i16), LiveCellInfo> {
        let mut result = HashMap::with_capacity(outpoints.len());
        let cf = self.cf_live_cells();

        let keys: Vec<[u8; keys::OUTPOINT_KEY_SIZE]> = outpoints
            .iter()
            .map(|(tx_hash, idx)| keys::encode_outpoint(tx_hash, *idx))
            .collect();

        let cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> =
            keys.iter().map(|k| (cf, k.as_slice())).collect();
        let values = self.multi_get_cf(cf_keys);

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

    pub fn get_consumed_cell(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> anyhow::Result<Option<LiveCellInfo>> {
        let key = keys::encode_outpoint(tx_hash, output_index);
        match self.get_cf(self.cf_consumed_cells(), &key)? {
            Some(value) => {
                if let Ok(compact) = bincode::deserialize::<CompactConsumedCellInfo>(&value) {
                    return Ok(Some(compact.to_live_cell_info()));
                }
                Ok(bincode::deserialize::<LiveCellInfo>(&value).ok())
            }
            None => Ok(None),
        }
    }

    pub fn get_consumed_cells_batch(
        &self,
        outpoints: &[(&[u8], i16)],
    ) -> HashMap<(Vec<u8>, i16), LiveCellInfo> {
        let mut result = HashMap::with_capacity(outpoints.len());
        let cf = self.cf_consumed_cells();

        let keys: Vec<[u8; keys::OUTPOINT_KEY_SIZE]> = outpoints
            .iter()
            .map(|(tx_hash, idx)| keys::encode_outpoint(tx_hash, *idx))
            .collect();

        let cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> =
            keys.iter().map(|k| (cf, k.as_slice())).collect();
        let values = self.multi_get_cf(cf_keys);

        for (i, value_result) in values.into_iter().enumerate() {
            if let Ok(Some(value)) = value_result {
                let info =
                    if let Ok(compact) = bincode::deserialize::<CompactConsumedCellInfo>(&value) {
                        Some(compact.to_live_cell_info())
                    } else {
                        bincode::deserialize::<LiveCellInfo>(&value).ok()
                    };
                if let Some(info) = info {
                    let (tx_hash, idx) = outpoints[i];
                    result.insert((tx_hash.to_vec(), idx), info);
                }
            }
        }
        result
    }

    /// List live cells by lock script hash (prefix scan).
    pub fn list_cells_by_lock(
        &self,
        lock_hash: &[u8],
        limit: usize,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, LiveCellInfo)>> {
        let mut results = Vec::new();
        let iter = self.prefix_iterator_cf(self.cf_cell_by_lock(), lock_hash);

        for item in iter.flatten() {
            let (key, _) = item;
            if !key.starts_with(lock_hash) {
                break;
            }
            // Key: lock_hash(32) + block_num(8) + outpoint(34)
            if key.len() >= 74 {
                let (tx_hash, output_index) = keys::decode_outpoint(&key[40..74]);
                if let Some(cell) = self.get_cell(&tx_hash, output_index)? {
                    results.push((tx_hash, output_index, cell));
                    if results.len() >= limit {
                        break;
                    }
                }
            }
        }
        Ok(results)
    }

    /// List live cells by type script hash (prefix scan).
    pub fn list_cells_by_type(
        &self,
        type_hash: &[u8],
        limit: usize,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, LiveCellInfo)>> {
        let mut results = Vec::new();
        let iter = self.prefix_iterator_cf(self.cf_cell_by_type(), type_hash);

        for item in iter.flatten() {
            let (key, _) = item;
            if !key.starts_with(type_hash) {
                break;
            }
            if key.len() >= 74 {
                let (tx_hash, output_index) = keys::decode_outpoint(&key[40..74]);
                if let Some(cell) = self.get_cell(&tx_hash, output_index)? {
                    results.push((tx_hash, output_index, cell));
                    if results.len() >= limit {
                        break;
                    }
                }
            }
        }
        Ok(results)
    }

    pub fn live_cells_count(&self) -> usize {
        let mut count = 0;
        let iter = self.iterator_cf(self.cf_live_cells(), rocksdb::IteratorMode::Start);
        for _ in iter.flatten() {
            count += 1;
        }
        count
    }

    /// Return cells created after a given block number.
    pub fn cells_created_since(&self, block_number: i64) -> Vec<(Vec<u8>, i16, LiveCellInfo)> {
        let mut result = Vec::new();
        let iter = self.iterator_cf(self.cf_live_cells(), rocksdb::IteratorMode::Start);
        for item in iter.flatten() {
            let (key, value) = item;
            if let Ok(info) = bincode::deserialize::<LiveCellInfo>(&value) {
                if info.created_at_block > block_number {
                    let (tx_hash, output_index) = keys::decode_outpoint(&key);
                    result.push((tx_hash, output_index, info));
                }
            }
        }
        result
    }
}
