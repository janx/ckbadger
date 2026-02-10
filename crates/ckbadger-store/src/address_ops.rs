//! Address balance operations.

use rocksdb::IteratorMode;

use crate::store::CkbadgerStore;
use crate::types::AddressBalance;

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

    /// Update address balance with read-modify-write.
    pub fn update_addr_balance<F>(&self, lock_hash: &[u8], update_fn: F) -> anyhow::Result<()>
    where
        F: FnOnce(&mut AddressBalance),
    {
        let mut balance = self.get_addr_balance(lock_hash)?.unwrap_or_default();
        update_fn(&mut balance);
        self.put_addr_balance_direct(lock_hash, &balance)
    }

    /// List top addresses by balance (full scan, sorted).
    pub fn top_addresses(&self, limit: usize) -> anyhow::Result<Vec<(Vec<u8>, AddressBalance)>> {
        let iter = self.iterator_cf(self.cf_addr_balance(), IteratorMode::Start);

        let mut all: Vec<(Vec<u8>, AddressBalance)> = Vec::new();
        for item in iter.flatten() {
            let (key, value) = item;
            if let Ok(balance) = bincode::deserialize::<AddressBalance>(&value) {
                all.push((key.to_vec(), balance));
            }
        }

        all.sort_by(|a, b| b.1.balance.cmp(&a.1.balance));
        all.truncate(limit);
        Ok(all)
    }

    /// List transactions for an address.
    pub fn list_addr_txs(
        &self,
        lock_hash: &[u8],
        limit: usize,
    ) -> anyhow::Result<Vec<(i64, i32, Vec<u8>)>> {
        let iter = self.prefix_iterator_cf(self.cf_addr_txs(), lock_hash);
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, value) = item;
            if !key.starts_with(lock_hash) {
                break;
            }
            // Key: lock_hash(32) + block_num(8) + tx_idx(4) = 44
            if key.len() == 44 {
                let block_num = crate::keys::decode_block_num(&key[32..40]);
                let tx_idx = crate::keys::decode_tx_idx(&key[40..44]);
                results.push((block_num, tx_idx, value.to_vec()));
                if results.len() >= limit {
                    break;
                }
            }
        }
        Ok(results)
    }
}
