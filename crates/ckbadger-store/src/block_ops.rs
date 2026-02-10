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

        for item in iter.flatten() {
            let (key, value) = item;
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
        for item in iter.flatten() {
            let (key, value) = item;
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
        let cf = self.cf_block_headers();

        let keys: Vec<[u8; 8]> = block_numbers
            .iter()
            .map(|n| keys::encode_block_num(*n))
            .collect();

        let cf_keys: Vec<(&rocksdb::ColumnFamily, &[u8])> =
            keys.iter().map(|k| (cf, k.as_slice())).collect();
        let values = self.multi_get_cf(cf_keys);

        for (i, value_result) in values.into_iter().enumerate() {
            if let Ok(Some(value)) = value_result {
                if let Ok(header) = bincode::deserialize::<CachedBlockHeader>(&value) {
                    result.insert(block_numbers[i], header.dao);
                }
            }
        }
        Ok(result)
    }

    pub fn block_headers_count(&self) -> usize {
        let mut count = 0;
        let iter = self.iterator_cf(self.cf_block_headers(), IteratorMode::Start);
        for _ in iter.flatten() {
            count += 1;
        }
        count
    }
}
