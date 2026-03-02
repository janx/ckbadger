//! Block header operations.

use rocksdb::IteratorMode;

use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::CachedBlockHeader;

impl CkbadgerStore {
    pub fn get_block_header(&self, block_number: i64) -> anyhow::Result<Option<CachedBlockHeader>> {
        let key = keys::encode_block_num(block_number);
        match self.get_cf(self.cf_block_headers(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn get_block_number_by_hash(&self, hash: &[u8]) -> anyhow::Result<Option<i64>> {
        match self.get_cf(self.cf_block_hash_index(), hash)? {
            Some(value) if value.len() == 8 => {
                Ok(Some(i64::from_le_bytes(value[..8].try_into().unwrap())))
            }
            _ => Ok(None),
        }
    }

    /// Get the latest block number (sync tip).
    pub fn get_sync_tip_block(&self) -> anyhow::Result<Option<(i64, CachedBlockHeader)>> {
        let iter = self.iterator_cf(self.cf_block_headers(), IteratorMode::End);

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate block_headers in get_sync_tip_block: {}",
                    e
                )
            })?;
            if key.len() == 8 {
                let block_num = keys::decode_block_num(&key);
                let header: CachedBlockHeader = bincode::deserialize(&value)?;
                return Ok(Some((block_num, header)));
            }
        }
        Ok(None)
    }

    /// List blocks in descending order.
    pub fn list_blocks_desc(
        &self,
        from_block: Option<i64>,
        limit: usize,
    ) -> anyhow::Result<Vec<(i64, CachedBlockHeader)>> {
        let start_key = match from_block {
            Some(n) => keys::encode_block_num(n),
            None => keys::encode_block_num(i64::MAX),
        };

        let iter = self.iterator_cf(
            self.cf_block_headers(),
            IteratorMode::From(&start_key, rocksdb::Direction::Reverse),
        );

        let mut results = Vec::with_capacity(limit);
        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!("failed to iterate block_headers in list_blocks_desc: {}", e)
            })?;
            if key.len() == 8 {
                let block_num = keys::decode_block_num(&key);
                let header: CachedBlockHeader = bincode::deserialize(&value)?;
                results.push((block_num, header));
                if results.len() >= limit {
                    break;
                }
            }
        }
        Ok(results)
    }

    /// Batch get DAO fields.
    pub fn get_dao_fields_batch(
        &self,
        block_numbers: &[i64],
    ) -> anyhow::Result<std::collections::HashMap<i64, Vec<u8>>> {
        let mut result = std::collections::HashMap::with_capacity(block_numbers.len());
        let cf = self.cf_block_headers();

        let keys: Vec<[u8; 8]> = block_numbers
            .iter()
            .map(|n| keys::encode_block_num(*n))
            .collect();

        let cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> =
            keys.iter().map(|k| (cf, k.as_slice())).collect();
        let values = self.multi_get_cf(cf_keys);

        for (i, value_result) in values.into_iter().enumerate() {
            match value_result {
                Ok(Some(value)) => {
                    let header: CachedBlockHeader =
                        bincode::deserialize(&value).map_err(|e| {
                            anyhow::anyhow!(
                                "failed to deserialize block header in get_dao_fields_batch: block_number={}, error={}",
                                block_numbers[i],
                                e
                            )
                        })?;
                    result.insert(block_numbers[i], header.dao);
                }
                Ok(None) => {}
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "rocksdb multi_get failed in get_dao_fields_batch: block_number={}, error={}",
                        block_numbers[i],
                        e
                    ));
                }
            }
        }
        Ok(result)
    }

    pub fn block_headers_count(&self) -> usize {
        let mut count = 0;
        let iter = self.iterator_cf(self.cf_block_headers(), IteratorMode::Start);
        for item in iter {
            match item {
                Ok(_) => count += 1,
                Err(e) => panic!(
                    "failed to iterate block_headers in block_headers_count: {}",
                    e
                ),
            }
        }
        count
    }

    /// Find the first missing block number in `block_headers` if there is an internal gap.
    ///
    /// Returns:
    /// - `Some(n)` when block `n` is missing while later blocks exist.
    /// - `None` when headers are contiguous from 0..tip, or there are no headers.
    pub fn find_first_block_header_gap(&self) -> anyhow::Result<Option<i64>> {
        let iter = self.iterator_cf(self.cf_block_headers(), IteratorMode::Start);
        let mut expected: Option<i64> = None;

        for item in iter {
            let (key, _) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate block_headers in find_first_block_header_gap: {}",
                    e
                )
            })?;
            if key.len() != 8 {
                continue;
            }
            let block_num = keys::decode_block_num(&key);

            match expected {
                None => {
                    if block_num != 0 {
                        return Ok(Some(0));
                    }
                    expected = Some(1);
                }
                Some(exp) => {
                    if block_num != exp {
                        return Ok(Some(exp));
                    }
                    expected = Some(exp + 1);
                }
            }
        }

        Ok(None)
    }

    /// Find the first block number of the UTC+8 day containing `block_number`.
    /// If a predecessor header is missing, stops at the first available block.
    pub fn find_day_start_block(&self, block_number: i64) -> anyhow::Result<Option<i64>> {
        let Some(header) = self.get_block_header(block_number)? else {
            return Ok(None);
        };
        let Some(dt) = chrono::DateTime::from_timestamp(header.timestamp / 1000, 0) else {
            return Ok(Some(block_number));
        };
        let target_date = ckbadger_common::block_date(dt);

        let mut cursor = block_number;
        while cursor > 0 {
            let prev_num = cursor - 1;
            let Some(prev_header) = self.get_block_header(prev_num)? else {
                break;
            };
            let Some(prev_dt) = chrono::DateTime::from_timestamp(prev_header.timestamp / 1000, 0)
            else {
                break;
            };
            let d = ckbadger_common::block_date(prev_dt);
            if d == target_date {
                cursor = prev_num;
                continue;
            }
            break;
        }

        Ok(Some(cursor))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch::StoreBatch;
    use tempfile::tempdir;

    fn make_header(block_number: i64) -> CachedBlockHeader {
        let mut hash = vec![0u8; 32];
        hash[..8].copy_from_slice(&block_number.to_le_bytes());
        CachedBlockHeader {
            hash,
            timestamp: block_number,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao: vec![0u8; 32],
            transactions_count: 1,
        }
    }

    #[test]
    fn test_find_first_block_header_gap_none_when_contiguous() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();

        let mut batch = StoreBatch::new(&store);
        for n in 0..=5 {
            batch.put_block_header(n, &make_header(n));
        }
        batch.commit().unwrap();

        assert_eq!(store.find_first_block_header_gap().unwrap(), None);
    }

    #[test]
    fn test_find_first_block_header_gap_detects_internal_gap() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(0, &make_header(0));
        batch.put_block_header(1, &make_header(1));
        batch.put_block_header(3, &make_header(3));
        batch.commit().unwrap();

        assert_eq!(store.find_first_block_header_gap().unwrap(), Some(2));
    }

    #[test]
    fn test_find_first_block_header_gap_detects_missing_genesis() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(5, &make_header(5));
        batch.commit().unwrap();

        assert_eq!(store.find_first_block_header_gap().unwrap(), Some(0));
    }

    #[test]
    fn test_find_day_start_block() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();

        // 2026-02-17 00:00:00 UTC in ms
        let day_start_ts = 1_771_286_400_000i64;

        let mut h0 = make_header(0);
        h0.timestamp = day_start_ts + 10_000;
        let mut h1 = make_header(1);
        h1.timestamp = day_start_ts + 20_000;
        let mut h2 = make_header(2);
        h2.timestamp = day_start_ts + 30_000;
        let mut h3 = make_header(3);
        h3.timestamp = day_start_ts + 86_400_000 + 10_000; // next day

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(0, &h0);
        batch.put_block_header(1, &h1);
        batch.put_block_header(2, &h2);
        batch.put_block_header(3, &h3);
        batch.commit().unwrap();

        assert_eq!(store.find_day_start_block(2).unwrap(), Some(0));
        assert_eq!(store.find_day_start_block(3).unwrap(), Some(3));
        assert_eq!(store.find_day_start_block(999).unwrap(), None);
    }

    #[test]
    fn test_get_dao_fields_batch_fails_on_invalid_block_header_payload() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let key = keys::encode_block_num(42);
        store
            .put_cf(store.cf_block_headers(), &key, b"invalid-header-payload")
            .unwrap();

        let err = store.get_dao_fields_batch(&[42]).unwrap_err();
        assert!(err
            .to_string()
            .contains("failed to deserialize block header in get_dao_fields_batch"));
    }
}
