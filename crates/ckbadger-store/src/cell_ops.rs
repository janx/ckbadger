//! Cell read/write operations.

use std::collections::HashMap;

use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::{
    decode_consumed_cell_info, decode_consumed_cell_meta, ConsumedCellInfo, LiveCellInfo,
};

fn bytes_to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(&mut out, "{:02x}", b);
    }
    out
}

/// Aggregated cell statistics for a token.
#[derive(Debug, Clone, Default)]
pub struct TokenCellStats {
    pub cells_count: i64,
    pub total_capacity: i128,
    pub total_occupied_capacity: i128,
}

impl CkbadgerStore {
    pub fn get_cell_by_outpoint_key(
        &self,
        outpoint_key: &[u8],
    ) -> anyhow::Result<Option<LiveCellInfo>> {
        match self.get_cf(self.cf_cells(), outpoint_key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn get_live_cell_by_outpoint_key(
        &self,
        outpoint_key: &[u8],
    ) -> anyhow::Result<Option<LiveCellInfo>> {
        if self.get_cf(self.cf_live_cells(), outpoint_key)?.is_none() {
            return Ok(None);
        }
        self.get_cell_by_outpoint_key(outpoint_key)
    }

    pub fn get_cell(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> anyhow::Result<Option<LiveCellInfo>> {
        let key = keys::encode_outpoint(tx_hash, output_index);
        self.get_live_cell_by_outpoint_key(&key)
    }

    pub fn get_cells_batch(
        &self,
        outpoints: &[(&[u8], i16)],
    ) -> anyhow::Result<HashMap<(Vec<u8>, i16), LiveCellInfo>> {
        let mut result = HashMap::with_capacity(outpoints.len());
        let live_cf = self.cf_live_cells();
        let cells_cf = self.cf_cells();

        let keys: Vec<[u8; keys::OUTPOINT_KEY_SIZE]> = outpoints
            .iter()
            .map(|(tx_hash, idx)| keys::encode_outpoint(tx_hash, *idx))
            .collect();

        let live_cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> =
            keys.iter().map(|k| (live_cf, k.as_slice())).collect();
        let live_values = self.multi_get_cf(live_cf_keys);

        let mut present_indices = Vec::new();
        let mut cell_cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> = Vec::new();
        for (i, marker_result) in live_values.into_iter().enumerate() {
            match marker_result {
                Ok(Some(_)) => {
                    present_indices.push(i);
                    cell_cf_keys.push((cells_cf, keys[i].as_slice()));
                }
                Ok(None) => {}
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "rocksdb multi_get failed while reading live marker in get_cells_batch: outpoint=0x{}, error={}",
                        bytes_to_hex(&keys[i]),
                        e
                    ));
                }
            }
        }

        let cell_values = self.multi_get_cf(cell_cf_keys);
        for (batch_idx, value_result) in cell_values.into_iter().enumerate() {
            let outpoint_idx = present_indices[batch_idx];
            let outpoint_key = &keys[outpoint_idx];
            match value_result {
                Ok(Some(value)) => {
                    let info = bincode::deserialize::<LiveCellInfo>(&value).map_err(|e| {
                        anyhow::anyhow!(
                            "failed to deserialize canonical cell in get_cells_batch: outpoint=0x{}, error={}",
                            bytes_to_hex(outpoint_key),
                            e
                        )
                    })?;
                    let (tx_hash, idx) = outpoints[outpoint_idx];
                    result.insert((tx_hash.to_vec(), idx), info);
                }
                Ok(None) => {
                    return Err(anyhow::anyhow!(
                        "missing canonical cell for live marker in get_cells_batch: outpoint=0x{}",
                        bytes_to_hex(outpoint_key)
                    ));
                }
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "rocksdb multi_get failed while reading canonical cell in get_cells_batch: outpoint=0x{}, error={}",
                        bytes_to_hex(outpoint_key),
                        e
                    ));
                }
            }
        }
        Ok(result)
    }

    pub fn get_consumed_cell(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> anyhow::Result<Option<LiveCellInfo>> {
        Ok(self
            .get_consumed_cell_info(tx_hash, output_index)?
            .map(|c| c.to_live_cell_info()))
    }

    pub fn get_consumed_cell_info(
        &self,
        tx_hash: &[u8],
        output_index: i16,
    ) -> anyhow::Result<Option<ConsumedCellInfo>> {
        let key = keys::encode_outpoint(tx_hash, output_index);
        let Some(value) = self.get_cf(self.cf_consumed_cells(), &key)? else {
            return Ok(None);
        };

        // Legacy schema stored full ConsumedCellInfo in consumed_cells.
        if let Some(info) = decode_consumed_cell_info(&value) {
            return Ok(Some(info));
        }

        let meta = decode_consumed_cell_meta(&value).ok_or_else(|| {
            anyhow::anyhow!(
                "failed to decode consumed cell meta: outpoint=0x{}:{}",
                bytes_to_hex(tx_hash),
                output_index
            )
        })?;
        let cell = self.get_cell_by_outpoint_key(&key)?.ok_or_else(|| {
            anyhow::anyhow!(
                "missing canonical cell for consumed outpoint: outpoint=0x{}:{}",
                bytes_to_hex(tx_hash),
                output_index
            )
        })?;
        Ok(Some(ConsumedCellInfo {
            cell,
            consumed_at_block: meta.consumed_at_block,
            consumed_by_tx: meta.consumed_by_tx,
        }))
    }

    pub fn get_consumed_cells_batch(
        &self,
        outpoints: &[(&[u8], i16)],
    ) -> anyhow::Result<HashMap<(Vec<u8>, i16), LiveCellInfo>> {
        let mut result = HashMap::with_capacity(outpoints.len());
        for (tx_hash, idx) in outpoints {
            if let Some(info) = self.get_consumed_cell(tx_hash, *idx)? {
                result.insert((tx_hash.to_vec(), *idx), info);
            }
        }
        Ok(result)
    }

    /// List live cells by lock script hash (prefix scan).
    /// `after_key` is the full 74-byte cell index key of the last returned entry (for pagination).
    pub fn list_cells_by_lock(
        &self,
        lock_hash: &[u8],
        limit: usize,
        after_key: Option<&[u8]>,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, LiveCellInfo)>> {
        self.list_cells_by_hash_cf(self.cf_cell_by_lock(), lock_hash, limit, after_key)
    }

    /// List live cells by type script hash (prefix scan).
    /// `after_key` is the full 74-byte cell index key of the last returned entry (for pagination).
    pub fn list_cells_by_type(
        &self,
        type_hash: &[u8],
        limit: usize,
        after_key: Option<&[u8]>,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, LiveCellInfo)>> {
        self.list_cells_by_hash_cf(self.cf_cell_by_type(), type_hash, limit, after_key)
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
        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate cell index in list_cells_by_hash_cf: {}",
                    e
                )
            })?;
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
    /// `after_key` is the full 74-byte cell index key of the last returned entry (for pagination).
    pub fn list_cells_by_lock_code_hash(
        &self,
        code_hash: &[u8],
        limit: usize,
        after_key: Option<&[u8]>,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, LiveCellInfo)>> {
        self.list_cells_by_code_hash_cf(self.cf_cell_by_lock_code(), code_hash, limit, after_key)
    }

    /// List live cells by type code hash (prefix scan on cell_by_type_code).
    /// `after_key` is the full 74-byte cell index key of the last returned entry (for pagination).
    pub fn list_cells_by_type_code_hash(
        &self,
        code_hash: &[u8],
        limit: usize,
        after_key: Option<&[u8]>,
    ) -> anyhow::Result<Vec<(Vec<u8>, i16, LiveCellInfo)>> {
        self.list_cells_by_code_hash_cf(self.cf_cell_by_type_code(), code_hash, limit, after_key)
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
        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate code-hash cell index in list_cells_by_code_hash_cf: {}",
                    e
                )
            })?;
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
        for item in iter {
            match item {
                Ok(_) => count += 1,
                Err(e) => panic!("failed to iterate live_cells in live_cells_count: {}", e),
            }
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
        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate live_cells in backfill_code_hash_indexes: {}",
                    e
                )
            })?;
            if key.len() == keys::OUTPOINT_KEY_SIZE {
                let Some(info) = self.get_cell_by_outpoint_key(&key)? else {
                    continue;
                };
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
                #[allow(clippy::manual_is_multiple_of)]
                if count % batch_size as u64 == 0 {
                    self.write_batch(std::mem::take(&mut batch))?;
                    batch = rocksdb::WriteBatch::default();
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
        let mut iter = self.iterator_cf(self.cf_cell_by_lock_code(), rocksdb::IteratorMode::Start);
        match iter.next() {
            Some(Ok(_)) => true,
            Some(Err(e)) => panic!(
                "failed to iterate cell_by_lock_code in code_hash_indexes_populated: {}",
                e
            ),
            None => false,
        }
    }

    /// Aggregate cell stats for a token (by type script hash).
    /// Prefix-scans `cell_by_type` and multi-gets each cell's capacity/occupied_capacity.
    pub fn aggregate_token_cell_stats(&self, type_hash: &[u8]) -> anyhow::Result<TokenCellStats> {
        let mut stats = TokenCellStats {
            cells_count: 0,
            total_capacity: 0,
            total_occupied_capacity: 0,
        };

        let cf = self.cf_cell_by_type();
        let iter = self.iterator_cf(
            cf,
            rocksdb::IteratorMode::From(type_hash, rocksdb::Direction::Forward),
        );

        // Collect outpoints in batches for multi-get
        let batch_size = 256;
        let mut outpoints: Vec<(Vec<u8>, i16)> = Vec::with_capacity(batch_size);

        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate cell_by_type in aggregate_token_cell_stats: {}",
                    e
                )
            })?;
            if !key.starts_with(type_hash) {
                break;
            }
            // Key: hash(32) + block_num(8) + outpoint(34) = 74 bytes
            if key.len() >= 74 {
                let (tx_hash, output_index) = keys::decode_outpoint(&key[40..74]);
                outpoints.push((tx_hash, output_index));

                if outpoints.len() >= batch_size {
                    Self::accumulate_cell_stats(self, &outpoints, &mut stats)?;
                    outpoints.clear();
                }
            }
        }

        // Flush remaining
        if !outpoints.is_empty() {
            Self::accumulate_cell_stats(self, &outpoints, &mut stats)?;
        }

        Ok(stats)
    }

    fn accumulate_cell_stats(
        &self,
        outpoints: &[(Vec<u8>, i16)],
        stats: &mut TokenCellStats,
    ) -> anyhow::Result<()> {
        let refs: Vec<(&[u8], i16)> = outpoints.iter().map(|(h, i)| (h.as_slice(), *i)).collect();
        let cells = self.get_cells_batch(&refs)?;
        for cell in cells.values() {
            stats.cells_count += 1;
            stats.total_capacity += cell.capacity as i128;
            stats.total_occupied_capacity += cell.occupied_capacity as i128;
        }
        Ok(())
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
        // Write canonical payload + live marker
        let outpoint_key = keys::encode_outpoint(tx_hash, output_index);
        let value = bincode::serialize(cell).unwrap();
        store
            .put_cf(store.cf_cells(), &outpoint_key, &value)
            .unwrap();
        store
            .put_cf(store.cf_live_cells(), &outpoint_key, &[])
            .unwrap();

        // Write to cell_by_type index
        let idx_key =
            keys::encode_cell_index_key(type_hash, cell.created_at_block, tx_hash, output_index);
        store
            .put_cf(store.cf_cell_by_type(), &idx_key, &[])
            .unwrap();
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
        batch.put_cell(&tx_hash, 0, &consumed_cell);
        batch.put_consumed_cell_with_consumer(
            &tx_hash,
            0,
            &consumed_cell,
            12345,
            Some(&consumed_by_tx),
        );
        batch.delete_cell(&tx_hash, 0);
        batch.commit().unwrap();

        let info = store.get_consumed_cell_info(&tx_hash, 0).unwrap().unwrap();
        assert_eq!(info.consumed_at_block, 12345);
        assert_eq!(info.consumed_by_tx, Some(consumed_by_tx.to_vec()));
        assert_eq!(info.cell.capacity, consumed_cell.capacity);
    }

    #[test]
    fn test_get_cells_batch_fails_when_live_marker_has_no_canonical_cell() {
        let (_dir, store) = test_store();
        let tx_hash = [0xAB; 32];
        let outpoint_key = keys::encode_outpoint(&tx_hash, 0);
        store
            .put_cf(store.cf_live_cells(), &outpoint_key, b"")
            .unwrap();

        let refs: Vec<(&[u8], i16)> = vec![(&tx_hash, 0)];
        let err = store.get_cells_batch(&refs).unwrap_err();
        assert!(err
            .to_string()
            .contains("missing canonical cell for live marker in get_cells_batch"));
    }

    #[test]
    fn test_get_consumed_cells_batch_fails_on_invalid_consumed_payload() {
        let (_dir, store) = test_store();
        let tx_hash = [0xCD; 32];
        let outpoint_key = keys::encode_outpoint(&tx_hash, 0);
        store
            .put_cf(
                store.cf_consumed_cells(),
                &outpoint_key,
                b"invalid-consumed-payload",
            )
            .unwrap();

        let refs: Vec<(&[u8], i16)> = vec![(&tx_hash, 0)];
        let err = store.get_consumed_cells_batch(&refs).unwrap_err();
        assert!(err
            .to_string()
            .contains("failed to decode consumed cell meta"));
    }
}
