//! DAO operations.

use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::{DaoDepositCacheEntry, SecondaryIssuance};

fn bytes_to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(&mut out, "{:02x}", b);
    }
    out
}

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
        withdraw_tx_hash: &[u8],
        withdraw_output_index: i16,
    ) -> anyhow::Result<Option<Vec<u8>>> {
        let key = keys::encode_outpoint(withdraw_tx_hash, withdraw_output_index);
        self.get_cf(self.cf_dao_by_withdraw_tx(), &key)
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
            let entry: DaoDepositCacheEntry = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize dao deposit entry in list_dao_deposits: outpoint_key=0x{}, error={}",
                    bytes_to_hex(&key),
                    e
                )
            })?;
            results.push((key.to_vec(), entry));
        }
        Ok(results)
    }

    /// List active (status=0) DAO deposits.
    pub fn list_active_dao_deposits(&self) -> anyhow::Result<Vec<(Vec<u8>, DaoDepositCacheEntry)>> {
        let all = self.list_dao_deposits()?;
        Ok(all.into_iter().filter(|(_, e)| e.status == 0).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_list_dao_deposits_fails_on_invalid_payload() {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open(dir.path()).unwrap();
        let outpoint = keys::encode_outpoint(&[0xAB; 32], 1);
        store
            .put_cf(store.cf_dao_deposits(), &outpoint, b"invalid-dao-deposit")
            .unwrap();

        let err = store.list_dao_deposits().unwrap_err();
        assert!(err
            .to_string()
            .contains("failed to deserialize dao deposit entry in list_dao_deposits"));
    }
}
