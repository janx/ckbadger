//! Cell read/write operations.
//!
//! Data lives across two physical stores:
//! - `cells` (append): outpoint → CellInfo (SSOT, never deleted)
//! - `live_cells` (default): outpoint → empty (liveness marker)
//! - `consumed_cells` (default): outpoint → consumed_at_block + consumed_by_tx (40B)
//! - `live_cells_by_lock/type/lock_code/type_code` (default): index → empty

use std::collections::HashMap;

use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::{ConsumedCellInfo, LiveCellInfo};

/// Aggregated cell statistics for a token.
#[derive(Debug, Clone, Default)]
pub struct TokenCellStats {
    pub cells_count: i64,
    pub total_capacity: i128,
    pub total_occupied_capacity: i128,
}

impl CkbadgerStore {
    /// Get a live cell's data. Checks liveness in default store, reads data from append store.
    pub fn get_cell(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> anyhow::Result<Option<LiveCellInfo>> {
        let key = keys::encode_outpoint(tx_hash, output_index);
        // Check liveness marker in default store
        if self.get_cf(self.cf_live_cells(), &key)?.is_none() {
            return Ok(None);
        }
        // Get cell data from append store
        self.get_cell_data_by_key(&key)
    }

    /// Get cell data from append store without checking liveness.
    /// Use when you already know the cell exists (e.g., iterating a live index).
    pub fn get_cell_data(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> anyhow::Result<Option<LiveCellInfo>> {
        let key = keys::encode_outpoint(tx_hash, output_index);
        self.get_cell_data_by_key(&key)
    }

    fn get_cell_data_by_key(&self, key: &[u8]) -> anyhow::Result<Option<LiveCellInfo>> {
        match self.append_get_cf(self.cf_cells(), key)? {
            Some(value) => Ok(bincode::deserialize(&value).ok()),
            None => Ok(None),
        }
    }

    /// Batch get live cells. Multi-gets from append store for cells that are live.
    pub fn get_cells_batch(
        &self,
        outpoints: &[(&[u8], i16)],
    ) -> HashMap<(Vec<u8>, i16), LiveCellInfo> {
        let mut result = HashMap::with_capacity(outpoints.len());

        let keys: Vec<[u8; keys::OUTPOINT_KEY_SIZE]> = outpoints
            .iter()
            .map(|(tx_hash, idx)| keys::encode_outpoint(tx_hash, *idx))
            .collect();

        // Check liveness in default store
        let live_cf = self.cf_live_cells();
        let live_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> =
            keys.iter().map(|k| (live_cf, k.as_slice())).collect();
        let live_results = self.multi_get_cf(live_keys);

        // Collect live outpoint indices
        let live_indices: Vec<usize> = live_results
            .into_iter()
            .enumerate()
            .filter_map(|(i, r)| if let Ok(Some(_)) = r { Some(i) } else { None })
            .collect();

        if live_indices.is_empty() {
            return result;
        }

        // Get cell data from append store only for live cells
        let cells_cf = self.cf_cells();
        let data_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> = live_indices
            .iter()
            .map(|&i| (cells_cf, keys[i].as_slice()))
            .collect();
        let data_results = self.append_multi_get_cf(data_keys);

        for (j, value_result) in data_results.into_iter().enumerate() {
            if let Ok(Some(value)) = value_result {
                if let Ok(info) = bincode::deserialize::<LiveCellInfo>(&value) {
                    let i = live_indices[j];
                    let (tx_hash, idx) = outpoints[i];
                    result.insert((tx_hash.to_vec(), idx), info);
                }
            }
        }
        result
    }

    /// Batch get cell data from append store without checking liveness.
    pub fn get_cells_data_batch(
        &self,
        outpoints: &[(&[u8], i16)],
    ) -> HashMap<(Vec<u8>, i16), LiveCellInfo> {
        let mut result = HashMap::with_capacity(outpoints.len());
        let cf = self.cf_cells();

        let keys: Vec<[u8; keys::OUTPOINT_KEY_SIZE]> = outpoints
            .iter()
            .map(|(tx_hash, idx)| keys::encode_outpoint(tx_hash, *idx))
            .collect();

        let cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> =
            keys.iter().map(|k| (cf, k.as_slice())).collect();
        let values = self.append_multi_get_cf(cf_keys);

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

    /// Get a consumed cell's data. Returns the original CellInfo if the cell was consumed.
    pub fn get_consumed_cell(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> anyhow::Result<Option<LiveCellInfo>> {
        let key = keys::encode_outpoint(tx_hash, output_index);
        // Check if consumed
        if self.get_cf(self.cf_consumed_cells(), &key)?.is_none() {
            return Ok(None);
        }
        // Get cell data from append store
        self.get_cell_data_by_key(&key)
    }

    /// Get full consumed cell info (cell data + consumption metadata).
    pub fn get_consumed_cell_info(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> anyhow::Result<Option<ConsumedCellInfo>> {
        let key = keys::encode_outpoint(tx_hash, output_index);
        // Get consumption metadata from default store
        let consumption = match self.get_cf(self.cf_consumed_cells(), &key)? {
            Some(value) if value.len() >= 40 => {
                let (consumed_at_block, consumed_by_tx) = keys::decode_consumed_cell_value(&value);
                let consumed_by = if consumed_by_tx.iter().all(|&b| b == 0) {
                    None
                } else {
                    Some(consumed_by_tx)
                };
                (consumed_at_block, consumed_by)
            }
            _ => return Ok(None),
        };
        // Get cell data from append store
        let cell = match self.get_cell_data_by_key(&key)? {
            Some(cell) => cell,
            None => return Ok(None),
        };
        Ok(Some(ConsumedCellInfo {
            cell,
            consumed_at_block: consumption.0,
            consumed_by_tx: consumption.1,
        }))
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

        // Check which are consumed
        let cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> =
            keys.iter().map(|k| (cf, k.as_slice())).collect();
        let consumed_results = self.multi_get_cf(cf_keys);

        let consumed_indices: Vec<usize> = consumed_results
            .into_iter()
            .enumerate()
            .filter_map(|(i, r)| if let Ok(Some(_)) = r { Some(i) } else { None })
            .collect();

        if consumed_indices.is_empty() {
            return result;
        }

        // Get cell data from append store
        let cells_cf = self.cf_cells();
        let data_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> = consumed_indices
            .iter()
            .map(|&i| (cells_cf, keys[i].as_slice()))
            .collect();
        let data_results = self.append_multi_get_cf(data_keys);

        for (j, value_result) in data_results.into_iter().enumerate() {
            if let Ok(Some(value)) = value_result {
                if let Ok(info) = bincode::deserialize::<LiveCellInfo>(&value) {
                    let i = consumed_indices[j];
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
        after_key: Option<&[u8]>,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, LiveCellInfo)>> {
        self.list_cells_by_hash_cf(self.cf_live_cells_by_lock(), lock_hash, limit, after_key)
    }

    /// List live cells by type script hash (prefix scan).
    pub fn list_cells_by_type(
        &self,
        type_hash: &[u8],
        limit: usize,
        after_key: Option<&[u8]>,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, LiveCellInfo)>> {
        self.list_cells_by_hash_cf(self.cf_live_cells_by_type(), type_hash, limit, after_key)
    }

    fn list_cells_by_hash_cf(
        &self,
        cf: &rocksdb::ColumnFamily,
        hash: &[u8],
        limit: usize,
        after_key: Option<&[u8]>,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, LiveCellInfo)>> {
        let mut results = Vec::new();

        let start_key = after_key
            .map(|k| k.to_vec())
            .unwrap_or_else(|| hash.to_vec());

        let iter = self.iterator_cf(
            cf,
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );

        let mut first = after_key.is_some();
        for item in iter.flatten() {
            let (key, _) = item;
            if !key.starts_with(hash) {
                break;
            }
            // Skip the cursor key itself (already returned on the previous page)
            if first {
                first = false;
                if after_key.is_some_and(|ak| key.as_ref() == ak) {
                    continue;
                }
            }
            // Key: hash(32) + block_num(8) + outpoint(34)
            if key.len() >= 74 {
                let (tx_hash, output_index) = keys::decode_outpoint(&key[40..74]);
                // Get cell data from append store (we know it's live via the index)
                if let Some(cell) = self.get_cell_data(&tx_hash, output_index)? {
                    results.push((tx_hash, output_index, cell));
                    if results.len() >= limit {
                        break;
                    }
                }
            }
        }
        Ok(results)
    }

    /// List live cells by lock code hash (prefix scan on live_cells_by_lock_code).
    pub fn list_cells_by_lock_code_hash(
        &self,
        code_hash: &[u8],
        limit: usize,
        after_key: Option<&[u8]>,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, LiveCellInfo)>> {
        self.list_cells_by_code_hash_cf(
            self.cf_live_cells_by_lock_code(),
            code_hash,
            limit,
            after_key,
        )
    }

    /// List live cells by type code hash (prefix scan on live_cells_by_type_code).
    pub fn list_cells_by_type_code_hash(
        &self,
        code_hash: &[u8],
        limit: usize,
        after_key: Option<&[u8]>,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, LiveCellInfo)>> {
        self.list_cells_by_code_hash_cf(
            self.cf_live_cells_by_type_code(),
            code_hash,
            limit,
            after_key,
        )
    }

    fn list_cells_by_code_hash_cf(
        &self,
        cf: &rocksdb::ColumnFamily,
        code_hash: &[u8],
        limit: usize,
        after_key: Option<&[u8]>,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, LiveCellInfo)>> {
        let mut results = Vec::new();

        let start_key = after_key
            .map(|k| k.to_vec())
            .unwrap_or_else(|| code_hash.to_vec());

        let iter = self.iterator_cf(
            cf,
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );

        let mut first = after_key.is_some();
        for item in iter.flatten() {
            let (key, _) = item;
            if !key.starts_with(code_hash) {
                break;
            }
            // Skip the cursor key itself (already returned on the previous page)
            if first {
                first = false;
                if after_key.is_some_and(|ak| key.as_ref() == ak) {
                    continue;
                }
            }
            // Key: code_hash(32) + block_num(8) + outpoint(34) = 74
            if key.len() >= 74 {
                let (tx_hash, output_index) = keys::decode_outpoint(&key[40..74]);
                if let Some(cell) = self.get_cell_data(&tx_hash, output_index)? {
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

    /// Backfill the live_cells_by_lock_code and live_cells_by_type_code indexes from live cells.
    pub fn backfill_code_hash_indexes(&self) -> anyhow::Result<u64> {
        let mut count = 0u64;
        let mut batch = rocksdb::WriteBatch::default();
        let batch_size = 10_000;

        // Iterate live_cells for outpoints, get data from append store
        let iter = self.iterator_cf(self.cf_live_cells(), rocksdb::IteratorMode::Start);
        for item in iter.flatten() {
            let (key, _) = item;
            if key.len() == keys::OUTPOINT_KEY_SIZE {
                let (tx_hash, output_index) = keys::decode_outpoint(&key);
                if let Some(info) = self.get_cell_data(&tx_hash, output_index)? {
                    // Index by lock code hash
                    let idx_key = keys::encode_cell_index_key(
                        &info.lock_code_hash,
                        info.created_at_block,
                        &tx_hash,
                        output_index,
                    );
                    batch.put_cf(self.cf_live_cells_by_lock_code(), idx_key, []);

                    // Index by type code hash (if present)
                    if let Some(ref type_code_hash) = info.type_code_hash {
                        let idx_key = keys::encode_cell_index_key(
                            type_code_hash,
                            info.created_at_block,
                            &tx_hash,
                            output_index,
                        );
                        batch.put_cf(self.cf_live_cells_by_type_code(), idx_key, []);
                    }

                    count += 1;
                    #[allow(clippy::manual_is_multiple_of)]
                    if count % batch_size as u64 == 0 {
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
        let iter = self.iterator_cf(
            self.cf_live_cells_by_lock_code(),
            rocksdb::IteratorMode::Start,
        );
        iter.flatten().next().is_some()
    }

    /// Aggregate cell stats for a token (by type script hash).
    pub fn aggregate_token_cell_stats(&self, type_hash: &[u8]) -> anyhow::Result<TokenCellStats> {
        let mut stats = TokenCellStats {
            cells_count: 0,
            total_capacity: 0,
            total_occupied_capacity: 0,
        };

        let cf = self.cf_live_cells_by_type();
        let iter = self.iterator_cf(
            cf,
            rocksdb::IteratorMode::From(type_hash, rocksdb::Direction::Forward),
        );

        // Collect outpoints in batches for multi-get
        let batch_size = 256;
        let mut outpoints: Vec<(Vec<u8>, i16)> = Vec::with_capacity(batch_size);

        for item in iter.flatten() {
            let (key, _) = item;
            if !key.starts_with(type_hash) {
                break;
            }
            // Key: hash(32) + block_num(8) + outpoint(34) = 74 bytes
            if key.len() >= 74 {
                let (tx_hash, output_index) = keys::decode_outpoint(&key[40..74]);
                outpoints.push((tx_hash, output_index));

                if outpoints.len() >= batch_size {
                    Self::accumulate_cell_stats(self, &outpoints, &mut stats);
                    outpoints.clear();
                }
            }
        }

        // Flush remaining
        if !outpoints.is_empty() {
            Self::accumulate_cell_stats(self, &outpoints, &mut stats);
        }

        Ok(stats)
    }

    fn accumulate_cell_stats(&self, outpoints: &[(Vec<u8>, i16)], stats: &mut TokenCellStats) {
        let refs: Vec<(&[u8], i16)> = outpoints.iter().map(|(h, i)| (h.as_slice(), *i)).collect();
        // Use data batch (no liveness check needed — index guarantees liveness)
        let cells = self.get_cells_data_batch(&refs);
        for cell in cells.values() {
            stats.cells_count += 1;
            stats.total_capacity += cell.capacity as i128;
            stats.total_occupied_capacity += cell.occupied_capacity as i128;
        }
    }

    /// Return cells created after a given block number.
    pub fn cells_created_since(&self, block_number: i64) -> Vec<(Vec<u8>, i16, LiveCellInfo)> {
        let mut result = Vec::new();
        // Iterate live_cells for outpoints, get data from append store
        let iter = self.iterator_cf(self.cf_live_cells(), rocksdb::IteratorMode::Start);
        for item in iter.flatten() {
            let (key, _) = item;
            if key.len() == keys::OUTPOINT_KEY_SIZE {
                let (tx_hash, output_index) = keys::decode_outpoint(&key);
                if let Ok(Some(info)) = self.get_cell_data(&tx_hash, output_index) {
                    if info.created_at_block > block_number {
                        result.push((tx_hash, output_index, info));
                    }
                }
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch::StoreBatch;
    use crate::store::CkbadgerStore;
    use tempfile::TempDir;

    fn test_store() -> (TempDir, CkbadgerStore) {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        (dir, store)
    }

    fn make_cell(capacity: i64, occupied: i64, type_hash: &[u8]) -> LiveCellInfo {
        LiveCellInfo {
            capacity,
            created_at_block: 100,
            lock_script_hash: vec![0xAA; 32],
            lock_code_hash: vec![0xBB; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(type_hash.to_vec()),
            type_code_hash: Some(vec![0xCC; 32]),
            type_args: Some(vec![]),
            data_size: 0,
            occupied_capacity: occupied,
            udt_amount: None,
        }
    }

    fn insert_cell(
        store: &CkbadgerStore,
        tx_hash: &[u8],
        output_index: i16,
        type_hash: &[u8],
        cell: &LiveCellInfo,
    ) {
        let mut batch = StoreBatch::new(store);
        batch.put_cell(tx_hash, output_index, cell);

        // Write to cell_by_type index
        let idx_key =
            keys::encode_cell_index_key(type_hash, cell.created_at_block, tx_hash, output_index);
        batch
            .raw_batch()
            .put_cf(store.cf_live_cells_by_type(), &idx_key, []);
        batch.commit().unwrap();
    }

    #[test]
    fn test_aggregate_token_cell_stats_empty() {
        let (_dir, store) = test_store();
        let type_hash = [0x01u8; 32];
        let stats = store.aggregate_token_cell_stats(&type_hash).unwrap();
        assert_eq!(stats.cells_count, 0);
        assert_eq!(stats.total_capacity, 0);
        assert_eq!(stats.total_occupied_capacity, 0);
    }

    #[test]
    fn test_aggregate_token_cell_stats_single_cell() {
        let (_dir, store) = test_store();
        let type_hash = [0x01u8; 32];
        let tx_hash = [0x11u8; 32];
        let cell = make_cell(200_00000000, 61_00000000, &type_hash);
        insert_cell(&store, &tx_hash, 0, &type_hash, &cell);

        let stats = store.aggregate_token_cell_stats(&type_hash).unwrap();
        assert_eq!(stats.cells_count, 1);
        assert_eq!(stats.total_capacity, 200_00000000);
        assert_eq!(stats.total_occupied_capacity, 61_00000000);
    }

    #[test]
    fn test_aggregate_token_cell_stats_multiple_cells() {
        let (_dir, store) = test_store();
        let type_hash = [0x01u8; 32];

        let tx1 = [0x11u8; 32];
        let cell1 = make_cell(200_00000000, 61_00000000, &type_hash);
        insert_cell(&store, &tx1, 0, &type_hash, &cell1);

        let tx2 = [0x22u8; 32];
        let cell2 = make_cell(300_00000000, 80_00000000, &type_hash);
        insert_cell(&store, &tx2, 0, &type_hash, &cell2);

        let tx3 = [0x33u8; 32];
        let cell3 = make_cell(150_00000000, 61_00000000, &type_hash);
        insert_cell(&store, &tx3, 1, &type_hash, &cell3);

        let stats = store.aggregate_token_cell_stats(&type_hash).unwrap();
        assert_eq!(stats.cells_count, 3);
        assert_eq!(stats.total_capacity, 650_00000000);
        assert_eq!(stats.total_occupied_capacity, 202_00000000);
    }

    #[test]
    fn test_aggregate_token_cell_stats_different_types_isolated() {
        let (_dir, store) = test_store();
        let type_a = [0x01u8; 32];
        let type_b = [0x02u8; 32];

        let tx1 = [0x11u8; 32];
        let cell1 = make_cell(200_00000000, 61_00000000, &type_a);
        insert_cell(&store, &tx1, 0, &type_a, &cell1);

        let tx2 = [0x22u8; 32];
        let cell2 = make_cell(500_00000000, 100_00000000, &type_b);
        insert_cell(&store, &tx2, 0, &type_b, &cell2);

        let stats_a = store.aggregate_token_cell_stats(&type_a).unwrap();
        assert_eq!(stats_a.cells_count, 1);
        assert_eq!(stats_a.total_capacity, 200_00000000);

        let stats_b = store.aggregate_token_cell_stats(&type_b).unwrap();
        assert_eq!(stats_b.cells_count, 1);
        assert_eq!(stats_b.total_capacity, 500_00000000);
    }

    #[test]
    fn test_get_consumed_cell_info_returns_consumer_metadata() {
        let (_dir, store) = test_store();
        let type_hash = [0x01u8; 32];
        let consumed_cell = make_cell(200_00000000, 61_00000000, &type_hash);
        let tx_hash = [0x11u8; 32];
        let consumed_by_tx = [0x22u8; 32];

        let mut batch = StoreBatch::new(&store);
        // First write cell to append store
        batch.put_cell(&tx_hash, 0, &consumed_cell);
        // Then mark as consumed
        batch.put_consumed_cell_with_consumer(
            &tx_hash,
            0,
            &consumed_cell,
            12345,
            Some(&consumed_by_tx),
        );
        batch.commit().unwrap();

        let info = store.get_consumed_cell_info(&tx_hash, 0).unwrap().unwrap();
        assert_eq!(info.consumed_at_block, 12345);
        assert_eq!(info.consumed_by_tx, Some(consumed_by_tx.to_vec()));
        assert_eq!(info.cell.capacity, consumed_cell.capacity);
    }

    #[test]
    fn test_cell_data_survives_delete() {
        let (_dir, store) = test_store();
        let tx_hash = [0x42u8; 32];
        let cell = make_cell(100_00000000, 50_00000000, &[0x01u8; 32]);

        let mut batch = StoreBatch::new(&store);
        batch.put_cell(&tx_hash, 0, &cell);
        batch.commit().unwrap();

        // Cell is live
        assert!(store.get_cell(&tx_hash, 0).unwrap().is_some());

        // Delete liveness marker
        let mut batch = StoreBatch::new(&store);
        batch.delete_cell(&tx_hash, 0);
        batch.commit().unwrap();

        // get_cell returns None (not live)
        assert!(store.get_cell(&tx_hash, 0).unwrap().is_none());
        // get_cell_data still returns data (append store)
        assert!(store.get_cell_data(&tx_hash, 0).unwrap().is_some());
    }
}
