//! Block header operations.
//!
//! Data lives in two CFs across two physical stores:
//! - `block_meta` (append): block_hash → BlockMeta (SSOT, immutable)
//! - `block_index` (default): block_number → block_hash (thin index, reorg-safe)

use rocksdb::IteratorMode;

use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::CachedBlockHeader;

impl CkbadgerStore {
    pub fn get_block_header(&self, block_number: i64) -> anyhow::Result<Option<CachedBlockHeader>> {
        // block_index (default): block_number → block_hash
        let key = keys::encode_block_num(block_number);
        let block_hash = match self.get_cf(self.cf_block_index(), &key)? {
            Some(hash) => hash,
            None => return Ok(None),
        };
        // block_meta (append): block_hash → BlockMeta
        match self.append_get_cf(self.cf_block_meta(), &block_hash)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn get_block_number_by_hash(&self, hash: &[u8]) -> anyhow::Result<Option<i64>> {
        // block_meta (append): block_hash → BlockMeta → block_number
        match self.append_get_cf(self.cf_block_meta(), hash)? {
            Some(value) => {
                let meta: CachedBlockHeader = bincode::deserialize(&value)?;
                Ok(Some(meta.block_number))
            }
            None => Ok(None),
        }
    }

    /// Get the latest block number (sync tip).
    pub fn get_sync_tip_block(&self) -> anyhow::Result<Option<(i64, CachedBlockHeader)>> {
        let iter = self.iterator_cf(self.cf_block_index(), IteratorMode::End);

        for item in iter.flatten() {
            let (key, hash) = item;
            if key.len() == 8 {
                let block_num = keys::decode_block_num(&key);
                if let Some(value) = self.append_get_cf(self.cf_block_meta(), &hash)? {
                    let header: CachedBlockHeader = bincode::deserialize(&value)?;
                    return Ok(Some((block_num, header)));
                }
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
            self.cf_block_index(),
            IteratorMode::From(&start_key, rocksdb::Direction::Reverse),
        );

        let mut results = Vec::with_capacity(limit);
        for item in iter.flatten() {
            let (key, hash) = item;
            if key.len() == 8 {
                let block_num = keys::decode_block_num(&key);
                if let Some(value) = self.append_get_cf(self.cf_block_meta(), &hash)? {
                    let header: CachedBlockHeader = bincode::deserialize(&value)?;
                    results.push((block_num, header));
                    if results.len() >= limit {
                        break;
                    }
                }
            }
        }
        Ok(results)
    }

    /// Get DAO field for a block.
    pub fn get_dao_field(&self, block_number: i64) -> anyhow::Result<Option<Vec<u8>>> {
        Ok(self.get_block_header(block_number)?.map(|h| h.dao))
    }

    /// Batch get DAO fields.
    pub fn get_dao_fields_batch(
        &self,
        block_numbers: &[i64],
    ) -> anyhow::Result<std::collections::HashMap<i64, Vec<u8>>> {
        let mut result = std::collections::HashMap::with_capacity(block_numbers.len());
        for &bn in block_numbers {
            if let Some(header) = self.get_block_header(bn)? {
                result.insert(bn, header.dao);
            }
        }
        Ok(result)
    }

    pub fn block_headers_count(&self) -> usize {
        let mut count = 0;
        let iter = self.iterator_cf(self.cf_block_index(), IteratorMode::Start);
        for _ in iter.flatten() {
            count += 1;
        }
        count
    }

    /// Find the first missing block number in `block_index` if there is an internal gap.
    pub fn find_first_block_header_gap(&self) -> anyhow::Result<Option<i64>> {
        let iter = self.iterator_cf(self.cf_block_index(), IteratorMode::Start);
        let mut expected: Option<i64> = None;

        for item in iter.flatten() {
            let (key, _) = item;
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
            block_number,
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
    fn test_get_block_header_roundtrip() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();

        let header = make_header(42);
        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(42, &header);
        batch.commit().unwrap();

        let result = store.get_block_header(42).unwrap().unwrap();
        assert_eq!(result.block_number, 42);
        assert_eq!(result.hash, header.hash);
    }

    #[test]
    fn test_get_block_number_by_hash() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();

        let header = make_header(99);
        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(99, &header);
        batch.commit().unwrap();

        assert_eq!(
            store.get_block_number_by_hash(&header.hash).unwrap(),
            Some(99)
        );
        assert_eq!(store.get_block_number_by_hash(&[0xFF; 32]).unwrap(), None);
    }

    #[test]
    fn test_list_blocks_desc() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();

        let mut batch = StoreBatch::new(&store);
        for n in 0..5 {
            batch.put_block_header(n, &make_header(n));
        }
        batch.commit().unwrap();

        let blocks = store.list_blocks_desc(None, 3).unwrap();
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].0, 4);
        assert_eq!(blocks[1].0, 3);
        assert_eq!(blocks[2].0, 2);
    }
}
