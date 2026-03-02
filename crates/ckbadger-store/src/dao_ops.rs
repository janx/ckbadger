//! DAO operations.

use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::{DaoDepositCacheEntry, SecondaryIssuance};

impl CkbadgerStore {
    pub fn get_dao_deposit(
        &self,
        outpoint_key: &[u8],
    ) -> anyhow::Result<Option<DaoDepositCacheEntry>> {
        match self.get_cf(self.cf_dao_deposits(), outpoint_key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_dao_deposit_direct(
        &self,
        outpoint_key: &[u8],
        entry: &DaoDepositCacheEntry,
    ) -> anyhow::Result<()> {
        let value = bincode::serialize(entry)?;
        self.put_cf(self.cf_dao_deposits(), outpoint_key, &value)
    }

    pub fn get_dao_deposit_by_withdraw_tx(
        &self,
        tx_hash: &[u8],
    ) -> anyhow::Result<Option<Vec<u8>>> {
        self.get_cf(self.cf_dao_by_withdraw_tx(), tx_hash)
    }

    pub fn get_block_issuance(&self, block_num: i64) -> anyhow::Result<Option<SecondaryIssuance>> {
        let key = keys::encode_block_num(block_num);
        match self.get_cf(self.cf_block_issuance(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    /// List all DAO deposits (prefix scan).
    pub fn list_dao_deposits(&self) -> anyhow::Result<Vec<(Vec<u8>, DaoDepositCacheEntry)>> {
        let iter = self.iterator_cf(self.cf_dao_deposits(), rocksdb::IteratorMode::Start);
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, value) = item;
            if let Ok(entry) = bincode::deserialize::<DaoDepositCacheEntry>(&value) {
                results.push((key.to_vec(), entry));
            }
        }
        Ok(results)
    }

    /// List active (status=0) DAO deposits.
    pub fn list_active_dao_deposits(&self) -> anyhow::Result<Vec<(Vec<u8>, DaoDepositCacheEntry)>> {
        let all = self.list_dao_deposits()?;
        Ok(all.into_iter().filter(|(_, e)| e.status == 0).collect())
    }
}
