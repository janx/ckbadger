//! Address balance operations.

use rocksdb::IteratorMode;

use crate::store::CkbadgerStore;
use crate::types::AddressBalance;

fn bytes_to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(&mut out, "{:02x}", b);
    }
    out
}

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

    /// List top addresses by balance (full scan, sorted).
    pub fn top_addresses(&self, limit: usize) -> anyhow::Result<Vec<(Vec<u8>, AddressBalance)>> {
        let iter = self.iterator_cf(self.cf_addr_balance(), IteratorMode::Start);

        let mut all: Vec<(Vec<u8>, AddressBalance)> = Vec::new();
        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!("failed to iterate addr_balance in top_addresses: {}", e)
            })?;
            let balance: AddressBalance = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize address balance in top_addresses: lock_hash=0x{}, error={}",
                    bytes_to_hex(&key),
                    e
                )
            })?;
            all.push((key.to_vec(), balance));
        }

        all.sort_by(|a, b| b.1.balance.cmp(&a.1.balance));
        all.truncate(limit);
        Ok(all)
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
                let (_, block_num, tx_idx, tx_hash_from_key) =
                    crate::keys::decode_addr_tx_key(&key);
                let tx_hash = value.to_vec();
                if tx_hash != tx_hash_from_key {
                    anyhow::bail!(
                        "addr_txs key/value tx_hash mismatch in list_addr_txs_recent: lock_hash=0x{}, block_num={}, tx_idx={}, key_tx_hash=0x{}, value_tx_hash=0x{}",
                        bytes_to_hex(lock_hash),
                        block_num,
                        tx_idx,
                        bytes_to_hex(&tx_hash_from_key),
                        bytes_to_hex(&tx_hash)
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
    fn test_top_addresses_fails_on_invalid_payload() {
        let dir = tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        let lock_hash = [0xAA; 32];
        store
            .put_cf(
                store.cf_addr_balance(),
                &lock_hash,
                b"invalid-address-balance",
            )
            .unwrap();

        let err = store.top_addresses(10).unwrap_err();
        assert!(err
            .to_string()
            .contains("failed to deserialize address balance in top_addresses"));
    }

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
