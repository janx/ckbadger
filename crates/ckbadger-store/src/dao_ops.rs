//! DAO operations.

use std::collections::BTreeMap;

use chrono::DateTime;

use crate::keys;
use crate::store::CkbadgerStore;
use crate::types::{CachedBlockHeader, DaoDepositCacheEntry, SecondaryIssuance};

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

    /// Aggregate secondary issuance by date.
    ///
    /// Iterates the block index to enumerate all blocks, then looks up each
    /// block's metadata (via `cf_block_meta`) and per-block secondary issuance,
    /// returning cumulative (dao_reward, miner_reward, treasury) per date sorted
    /// chronologically.
    pub fn list_daily_secondary_issuance(&self) -> anyhow::Result<Vec<(String, i128, i128, i128)>> {
        let iter = self.iterator_cf(self.cf_block_index(), rocksdb::IteratorMode::Start);
        // date -> (dao_reward_sum, miner_reward_sum, treasury_sum)
        let mut daily: BTreeMap<String, (i128, i128, i128)> = BTreeMap::new();

        for item in iter.flatten() {
            let (key, block_hash) = item;
            if key.len() != 8 {
                continue;
            }
            let block_num = keys::decode_block_num(&key);

            // cf_block_index value is the block_hash; look up the header from cf_block_meta
            let header: CachedBlockHeader =
                match self.append_get_cf(self.cf_block_meta(), &block_hash) {
                    Ok(Some(meta_bytes)) => match bincode::deserialize(&meta_bytes) {
                        Ok(h) => h,
                        Err(_) => continue,
                    },
                    _ => continue,
                };

            let issuance = match self.get_cf(self.cf_block_issuance(), &key)? {
                Some(v) => match bincode::deserialize::<SecondaryIssuance>(&v) {
                    Ok(i) => i,
                    Err(_) => continue,
                },
                // genesis / early blocks may have no issuance entry
                None if block_num == 0 => continue,
                None => continue,
            };

            let date = DateTime::from_timestamp_millis(header.timestamp)
                .map(|dt| dt.format("%Y-%m-%d").to_string())
                .unwrap_or_default();
            if date.is_empty() {
                continue;
            }

            let entry = daily.entry(date).or_insert((0, 0, 0));
            entry.0 += i128::from(issuance.dao_reward);
            entry.1 += i128::from(issuance.miner_reward);
            entry.2 += i128::from(issuance.treasury);
        }

        // Convert to cumulative
        let mut cum_dao: i128 = 0;
        let mut cum_miner: i128 = 0;
        let mut cum_treasury: i128 = 0;
        let results: Vec<_> = daily
            .into_iter()
            .map(|(date, (d, m, t))| {
                cum_dao += d;
                cum_miner += m;
                cum_treasury += t;
                (date, cum_dao, cum_miner, cum_treasury)
            })
            .collect();

        Ok(results)
    }
}
