//! Address balance operations.

use crate::store::CkbadgerStore;
use crate::types::AddressBalance;

use crate::bytes_to_hex;

impl CkbadgerStore {
    pub fn get_addr_balance(&self, lock_hash: &[u8]) -> anyhow::Result<Option<AddressBalance>> {
        match self.get_cf(self.cf_addr_balance(), lock_hash)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_addr_balance_direct(
        &self,
        lock_hash: &[u8],
        balance: &AddressBalance,
    ) -> anyhow::Result<()> {
        let value = bincode::serialize(balance)?;
        self.put_cf(self.cf_addr_balance(), lock_hash, &value)
    }

    /// List transactions for an address (newest first).
    pub fn list_addr_txs_recent(
        &self,
        lock_hash: &[u8],
        limit: usize,
        cursor: Option<(i64, i32)>,
    ) -> anyhow::Result<Vec<(i64, i32, Vec<u8>)>> {
        if lock_hash.len() != 32 {
            anyhow::bail!(
                "list_addr_txs_recent expects 32-byte lock_hash, got {} bytes",
                lock_hash.len()
            );
        }
        if limit == 0 {
            return Ok(Vec::new());
        }

        // Descending position keys allow a simple forward prefix scan.
        let start_key = match cursor {
            Some((block_num, tx_idx)) => {
                crate::keys::encode_addr_tx_seek_after_key(lock_hash, block_num, tx_idx)
            }
            None => lock_hash.to_vec(),
        };

        let iter = self.iterator_cf(
            self.cf_addr_txs(),
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );

        let mut results = Vec::new();
        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!("failed to iterate addr_txs in list_addr_txs_recent: {}", e)
            })?;
            if !key.starts_with(lock_hash) {
                break;
            }
            if key.len() == crate::keys::ADDR_TX_KEY_SIZE {
                let (_, block_num, tx_idx, tx_hash) = crate::keys::decode_addr_tx_key(&key);
                if !value.is_empty() {
                    anyhow::bail!(
                        "addr_txs expects empty value in list_addr_txs_recent: lock_hash=0x{}, block_num={}, tx_idx={}, value_len={}",
                        bytes_to_hex(lock_hash),
                        block_num,
                        tx_idx,
                        value.len()
                    );
                }
                results.push((block_num, tx_idx, tx_hash));
                if results.len() >= limit {
                    break;
                }
            }
        }
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch::StoreBatch;
    use tempfile::tempdir;

    #[test]
    fn test_list_addr_txs_recent_rejects_non_32_byte_lock_hash() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();

        let err = store
            .list_addr_txs_recent(&[0xAA; 31], 10, None)
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("list_addr_txs_recent expects 32-byte lock_hash"));
    }

    #[test]
    fn test_list_addr_txs_recent_limit_zero_returns_empty() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let lock = [0xAA; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_addr_tx(&lock, 100, 0, &[0x11; 32]);
        batch.commit().unwrap();

        let rows = store.list_addr_txs_recent(&lock, 0, None).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn test_list_addr_txs_recent_reads_tx_hash_from_key_with_empty_value() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let lock = [0xAC; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_addr_tx(&lock, 100, 1, &[0x10; 32]);
        batch.put_addr_tx(&lock, 99, 0, &[0x20; 32]);
        batch.commit().unwrap();

        let rows = store.list_addr_txs_recent(&lock, 10, None).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], (100, 1, vec![0x10; 32]));
        assert_eq!(rows[1], (99, 0, vec![0x20; 32]));
    }

    #[test]
    fn test_list_addr_txs_recent_keeps_two_rows_same_position_different_tx_hash() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let lock = [0xAB; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_addr_tx(&lock, 100, 1, &[0x10; 32]);
        batch.put_addr_tx(&lock, 100, 1, &[0x20; 32]);
        batch.put_addr_tx(&lock, 99, 0, &[0x30; 32]);
        batch.commit().unwrap();

        let rows = store.list_addr_txs_recent(&lock, 10, None).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0], (100, 1, vec![0x10; 32]));
        assert_eq!(rows[1], (100, 1, vec![0x20; 32]));
        assert_eq!(rows[2], (99, 0, vec![0x30; 32]));

        let next = store
            .list_addr_txs_recent(&lock, 10, Some((100, 1)))
            .unwrap();
        assert_eq!(next, vec![(99, 0, vec![0x30; 32])]);
    }
}
