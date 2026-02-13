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

    /// List live cells by lock code hash (prefix scan on cell_by_lock_code).
    pub fn list_cells_by_lock_code_hash(
        &self,
        code_hash: &[u8],
        limit: usize,
        cursor_block: Option<i64>,
        cursor_output_index: Option<i16>,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, LiveCellInfo)>> {
        self.list_cells_by_code_hash_cf(
            self.cf_cell_by_lock_code(),
            code_hash,
            limit,
            cursor_block,
            cursor_output_index,
        )
    }

    /// List live cells by type code hash (prefix scan on cell_by_type_code).
    pub fn list_cells_by_type_code_hash(
        &self,
        code_hash: &[u8],
        limit: usize,
        cursor_block: Option<i64>,
        cursor_output_index: Option<i16>,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, LiveCellInfo)>> {
        self.list_cells_by_code_hash_cf(
            self.cf_cell_by_type_code(),
            code_hash,
            limit,
            cursor_block,
            cursor_output_index,
        )
    }

    fn list_cells_by_code_hash_cf(
        &self,
        cf: &rocksdb::ColumnFamily,
        code_hash: &[u8],
        limit: usize,
        cursor_block: Option<i64>,
        cursor_output_index: Option<i16>,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, LiveCellInfo)>> {
        let mut results = Vec::new();

        // Build start key: when cursor is provided, seek past it; otherwise start at prefix
        let start_key = if let (Some(block), Some(idx)) = (cursor_block, cursor_output_index) {
            keys::encode_cell_index_key(code_hash, block, &[0xffu8; 32], idx)
        } else {
            code_hash.to_vec()
        };

        let iter = self.iterator_cf(
            cf,
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );

        for item in iter.flatten() {
            let (key, _) = item;
            if !key.starts_with(code_hash) {
                break;
            }
            // Key: code_hash(32) + block_num(8) + outpoint(34) = 74
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

    /// Backfill the cell_by_lock_code and cell_by_type_code indexes from live_cells.
    /// Call once after adding the new column families to populate them from existing data.
    /// Returns the number of index entries written.
    pub fn backfill_code_hash_indexes(&self) -> anyhow::Result<u64> {
        let mut count = 0u64;
        let mut batch = rocksdb::WriteBatch::default();
        let batch_size = 10_000;

        let iter = self.iterator_cf(self.cf_live_cells(), rocksdb::IteratorMode::Start);
        for item in iter.flatten() {
            let (key, value) = item;
            if let Ok(info) = bincode::deserialize::<LiveCellInfo>(&value) {
                if key.len() == keys::OUTPOINT_KEY_SIZE {
                    let (tx_hash, output_index) = keys::decode_outpoint(&key);

                    // Index by lock code hash
                    let idx_key = keys::encode_cell_index_key(
                        &info.lock_code_hash,
                        info.created_at_block,
                        &tx_hash,
                        output_index,
                    );
                    batch.put_cf(self.cf_cell_by_lock_code(), idx_key, []);

                    // Index by type code hash (if present)
                    if let Some(ref type_code_hash) = info.type_code_hash {
                        let idx_key = keys::encode_cell_index_key(
                            type_code_hash,
                            info.created_at_block,
                            &tx_hash,
                            output_index,
                        );
                        batch.put_cf(self.cf_cell_by_type_code(), idx_key, []);
                    }

                    count += 1;
                    if count.is_multiple_of(batch_size as u64) {
                        self.write_batch(std::mem::take(&mut batch))?;
                        batch = rocksdb::WriteBatch::default();
                    }
                }
            }
        }

        if !batch.is_empty() {
            self.write_batch(batch)?;
        }

        Ok(count)
    }

    /// Check if the code_hash indexes have been populated.
    pub fn code_hash_indexes_populated(&self) -> bool {
        // Check if cell_by_lock_code has any entries
        let iter = self.iterator_cf(self.cf_cell_by_lock_code(), rocksdb::IteratorMode::Start);
        iter.flatten().next().is_some()
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
