//! Address balance operations.

use crate::store::CkbadgerStore;
use crate::types::{AddrTxValue, AddressBalance};

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
    #[allow(clippy::type_complexity)]
    pub fn list_addr_txs_recent(
        &self,
        lock_hash: &[u8],
        limit: usize,
        cursor: Option<(i64, i32)>,
    ) -> anyhow::Result<Vec<(i64, i32, Vec<u8>, AddrTxValue)>> {
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
                let addr_tx_value = if value.is_empty() {
                    anyhow::bail!(
                        "empty AddrTxValue for lock_hash=0x{}, block={}, tx_idx={} — re-sync required",
                        bytes_to_hex(lock_hash),
                        block_num,
                        tx_idx,
                    );
                } else {
                    bincode::deserialize(&value).map_err(|e| {
                        anyhow::anyhow!(
                            "failed to deserialize AddrTxValue: lock_hash=0x{}, block={}, tx_idx={}, error={}",
                            bytes_to_hex(lock_hash),
                            block_num,
                            tx_idx,
                            e
                        )
                    })?
                };
                results.push((block_num, tx_idx, tx_hash, addr_tx_value));
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
        batch.put_addr_tx(
            &lock,
            100,
            0,
            &[0x11; 32],
            &AddrTxValue::new(0, false, true),
        );
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
        batch.put_addr_tx(
            &lock,
            100,
            1,
            &[0x10; 32],
            &AddrTxValue::new(0, false, true),
        );
        batch.put_addr_tx(&lock, 99, 0, &[0x20; 32], &AddrTxValue::new(0, false, true));
        batch.commit().unwrap();

        let rows = store.list_addr_txs_recent(&lock, 10, None).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, 100);
        assert_eq!(rows[0].1, 1);
        assert_eq!(rows[0].2, vec![0x10; 32]);
        assert_eq!(rows[1].0, 99);
        assert_eq!(rows[1].1, 0);
        assert_eq!(rows[1].2, vec![0x20; 32]);
    }

    #[test]
    fn test_list_addr_txs_recent_keeps_two_rows_same_position_different_tx_hash() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let lock = [0xAB; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_addr_tx(
            &lock,
            100,
            1,
            &[0x10; 32],
            &AddrTxValue::new(0, false, true),
        );
        batch.put_addr_tx(
            &lock,
            100,
            1,
            &[0x20; 32],
            &AddrTxValue::new(0, false, true),
        );
        batch.put_addr_tx(&lock, 99, 0, &[0x30; 32], &AddrTxValue::new(0, false, true));
        batch.commit().unwrap();

        let rows = store.list_addr_txs_recent(&lock, 10, None).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].0, 100);
        assert_eq!(rows[0].1, 1);
        assert_eq!(rows[0].2, vec![0x10; 32]);
        assert_eq!(rows[1].0, 100);
        assert_eq!(rows[1].1, 1);
        assert_eq!(rows[1].2, vec![0x20; 32]);
        assert_eq!(rows[2].0, 99);
        assert_eq!(rows[2].1, 0);
        assert_eq!(rows[2].2, vec![0x30; 32]);

        let next = store
            .list_addr_txs_recent(&lock, 10, Some((100, 1)))
            .unwrap();
        assert_eq!(next.len(), 1);
        assert_eq!(next[0].0, 99);
        assert_eq!(next[0].1, 0);
        assert_eq!(next[0].2, vec![0x30; 32]);
    }

    #[test]
    fn test_list_addr_txs_recent_returns_addr_tx_value() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let lock = [0xBB; 32];
        let val_sent = AddrTxValue::new(-500, true, false);
        let val_recv = AddrTxValue::new(1000, false, true);
        let mut batch = StoreBatch::new(&store);
        batch.put_addr_tx(&lock, 200, 0, &[0xAA; 32], &val_sent);
        batch.put_addr_tx(&lock, 100, 0, &[0xBB; 32], &val_recv);
        batch.commit().unwrap();
        let rows = store.list_addr_txs_recent(&lock, 10, None).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].3, val_sent);
        assert_eq!(rows[1].3, val_recv);
    }
}
