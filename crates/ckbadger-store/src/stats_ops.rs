//! Statistics operations.

use crate::keys::{self, stats_prefix};
use crate::store::CkbadgerStore;
use crate::types::*;

impl CkbadgerStore {
    // ---- Daily stats ----

    pub fn get_daily_stats(&self, date: &str) -> anyhow::Result<Option<DailyStats>> {
        let key = keys::encode_stats_key(stats_prefix::DAILY, date.as_bytes());
        match self.get_cf(self.cf_stats(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_daily_stats(&self, date: &str, stats: &DailyStats) -> anyhow::Result<()> {
        let key = keys::encode_stats_key(stats_prefix::DAILY, date.as_bytes());
        let value = bincode::serialize(stats)?;
        self.put_cf(self.cf_stats(), &key, &value)
    }

    pub fn list_daily_stats(&self) -> anyhow::Result<Vec<DailyStats>> {
        let prefix = [stats_prefix::DAILY];
        let iter = self.prefix_iterator_cf(self.cf_stats(), &prefix);
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, value) = item;
            if !key.starts_with(&prefix) {
                break;
            }
            if let Ok(stats) = bincode::deserialize::<DailyStats>(&value) {
                results.push(stats);
            }
        }
        Ok(results)
    }

    // ---- Hourly stats ----

    pub fn get_hourly_stats(&self, hour: &str) -> anyhow::Result<Option<HourlyStats>> {
        let key = keys::encode_stats_key(stats_prefix::HOURLY, hour.as_bytes());
        match self.get_cf(self.cf_stats(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_hourly_stats(&self, hour: &str, stats: &HourlyStats) -> anyhow::Result<()> {
        let key = keys::encode_stats_key(stats_prefix::HOURLY, hour.as_bytes());
        let value = bincode::serialize(stats)?;
        self.put_cf(self.cf_stats(), &key, &value)
    }

    pub fn list_hourly_stats(&self) -> anyhow::Result<Vec<HourlyStats>> {
        let prefix = [stats_prefix::HOURLY];
        let iter = self.prefix_iterator_cf(self.cf_stats(), &prefix);
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, value) = item;
            if !key.starts_with(&prefix) {
                break;
            }
            if let Ok(stats) = bincode::deserialize::<HourlyStats>(&value) {
                results.push(stats);
            }
        }
        Ok(results)
    }

    /// List daily stats with their date keys (date is in the key, not the value).
    pub fn list_daily_stats_with_dates(&self) -> anyhow::Result<Vec<(String, DailyStats)>> {
        let prefix = [stats_prefix::DAILY];
        let iter = self.prefix_iterator_cf(self.cf_stats(), &prefix);
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, value) = item;
            if !key.starts_with(&prefix) {
                break;
            }
            if let Ok(stats) = bincode::deserialize::<DailyStats>(&value) {
                // Key is prefix(1) + date_string
                let date = String::from_utf8_lossy(&key[1..]).to_string();
                results.push((date, stats));
            }
        }
        Ok(results)
    }

    /// List hourly stats with their hour keys.
    pub fn list_hourly_stats_with_keys(&self) -> anyhow::Result<Vec<(String, HourlyStats)>> {
        let prefix = [stats_prefix::HOURLY];
        let iter = self.prefix_iterator_cf(self.cf_stats(), &prefix);
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, value) = item;
            if !key.starts_with(&prefix) {
                break;
            }
            if let Ok(stats) = bincode::deserialize::<HourlyStats>(&value) {
                let hour_key = String::from_utf8_lossy(&key[1..]).to_string();
                results.push((hour_key, stats));
            }
        }
        Ok(results)
    }

    // ---- Epoch stats ----

    pub fn get_epoch_stats(&self, epoch: i64) -> anyhow::Result<Option<EpochStats>> {
        let key = keys::encode_stats_key(stats_prefix::EPOCH, &epoch.to_be_bytes());
        match self.get_cf(self.cf_stats(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_epoch_stats(&self, epoch: i64, stats: &EpochStats) -> anyhow::Result<()> {
        let key = keys::encode_stats_key(stats_prefix::EPOCH, &epoch.to_be_bytes());
        let value = bincode::serialize(stats)?;
        self.put_cf(self.cf_stats(), &key, &value)
    }

    /// List all epoch stats, ordered by epoch number.
    pub fn list_epoch_stats(&self) -> anyhow::Result<Vec<EpochStats>> {
        let prefix = [stats_prefix::EPOCH];
        let iter = self.prefix_iterator_cf(self.cf_stats(), &prefix);
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, value) = item;
            if !key.starts_with(&prefix) {
                break;
            }
            if let Ok(stats) = bincode::deserialize::<EpochStats>(&value) {
                results.push(stats);
            }
        }
        Ok(results)
    }

    // ---- Miner stats ----

    pub fn put_miner_stats(
        &self,
        date: &str,
        miner_hash: &[u8],
        stats: &MinerStats,
    ) -> anyhow::Result<()> {
        let suffix = [date.as_bytes(), miner_hash].concat();
        let key = keys::encode_stats_key(stats_prefix::MINER, &suffix);
        let value = bincode::serialize(stats)?;
        self.put_cf(self.cf_stats(), &key, &value)
    }

    /// List all miner stats (aggregated across all dates).
    pub fn list_miner_stats(&self) -> anyhow::Result<Vec<MinerStats>> {
        let prefix = [stats_prefix::MINER];
        let iter = self.prefix_iterator_cf(self.cf_stats(), &prefix);
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, value) = item;
            if !key.starts_with(&prefix) {
                break;
            }
            if let Ok(stats) = bincode::deserialize::<MinerStats>(&value) {
                results.push(stats);
            }
        }
        Ok(results)
    }

    // ---- Daily block stats ----

    pub fn get_daily_block_stats(&self, date: &str) -> anyhow::Result<Option<DailyBlockStats>> {
        let key = keys::encode_stats_key(stats_prefix::DAILY_BLOCK, date.as_bytes());
        match self.get_cf(self.cf_stats(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_daily_block_stats(&self, date: &str, stats: &DailyBlockStats) -> anyhow::Result<()> {
        let key = keys::encode_stats_key(stats_prefix::DAILY_BLOCK, date.as_bytes());
        let value = bincode::serialize(stats)?;
        self.put_cf(self.cf_stats(), &key, &value)
    }

    /// List all daily block stats with their date keys.
    pub fn list_daily_block_stats(&self) -> anyhow::Result<Vec<(String, DailyBlockStats)>> {
        let prefix = [stats_prefix::DAILY_BLOCK];
        let iter = self.prefix_iterator_cf(self.cf_stats(), &prefix);
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, value) = item;
            if !key.starts_with(&prefix) {
                break;
            }
            if let Ok(stats) = bincode::deserialize::<DailyBlockStats>(&value) {
                let date = String::from_utf8_lossy(&key[1..]).to_string();
                results.push((date, stats));
            }
        }
        Ok(results)
    }

    // ---- Block time distribution ----

    pub fn put_block_time_dist(&self, bucket: i32, count: i32) -> anyhow::Result<()> {
        let key = keys::encode_stats_key(stats_prefix::BLOCK_TIME_DIST, &bucket.to_be_bytes());
        self.put_cf(self.cf_stats(), &key, &count.to_le_bytes())
    }

    pub fn get_block_time_dist(&self, bucket: i32) -> anyhow::Result<Option<i32>> {
        let key = keys::encode_stats_key(stats_prefix::BLOCK_TIME_DIST, &bucket.to_be_bytes());
        match self.get_cf(self.cf_stats(), &key)? {
            Some(value) if value.len() == 4 => {
                Ok(Some(i32::from_le_bytes(value[..4].try_into().unwrap())))
            }
            _ => Ok(None),
        }
    }

    /// List all block time distribution buckets.
    pub fn list_block_time_dist(&self) -> anyhow::Result<Vec<(i32, i32)>> {
        let prefix = [stats_prefix::BLOCK_TIME_DIST];
        let iter = self.prefix_iterator_cf(self.cf_stats(), &prefix);
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, value) = item;
            if !key.starts_with(&prefix) {
                break;
            }
            // Key is prefix(1) + bucket(4 BE)
            if key.len() == 5 && value.len() == 4 {
                let bucket = i32::from_be_bytes(key[1..5].try_into().unwrap());
                let count = i32::from_le_bytes(value[..4].try_into().unwrap());
                results.push((bucket, count));
            }
        }
        Ok(results)
    }

    // ---- Epoch time distribution ----

    pub fn put_epoch_time_dist(&self, bucket: i32, count: i32) -> anyhow::Result<()> {
        let key = keys::encode_stats_key(stats_prefix::EPOCH_TIME_DIST, &bucket.to_be_bytes());
        self.put_cf(self.cf_stats(), &key, &count.to_le_bytes())
    }

    /// List all epoch time distribution buckets.
    pub fn list_epoch_time_dist(&self) -> anyhow::Result<Vec<(i32, i32)>> {
        let prefix = [stats_prefix::EPOCH_TIME_DIST];
        let iter = self.prefix_iterator_cf(self.cf_stats(), &prefix);
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, value) = item;
            if !key.starts_with(&prefix) {
                break;
            }
            if key.len() == 5 && value.len() == 4 {
                let bucket = i32::from_be_bytes(key[1..5].try_into().unwrap());
                let count = i32::from_le_bytes(value[..4].try_into().unwrap());
                results.push((bucket, count));
            }
        }
        Ok(results)
    }

    // ---- DAO daily snapshots ----

    pub fn list_dao_daily_snapshots(&self) -> anyhow::Result<Vec<DaoDailySnapshot>> {
        let prefix = [stats_prefix::DAO_DAILY_SNAPSHOT];
        let iter = self.prefix_iterator_cf(self.cf_stats(), &prefix);
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, value) = item;
            if !key.starts_with(&prefix) {
                break;
            }
            if let Ok(snapshot) = bincode::deserialize::<DaoDailySnapshot>(&value) {
                results.push(snapshot);
            }
        }
        Ok(results)
    }

    // ---- Script info ----

    pub fn get_script_info(&self, code_hash: &[u8]) -> anyhow::Result<Option<ScriptInfo>> {
        match self.get_cf(self.cf_script_info(), code_hash)? {
            Some(value) => Ok(bincode::deserialize(&value).ok()),
            None => Ok(None),
        }
    }

    pub fn put_script_info_direct(
        &self,
        code_hash: &[u8],
        info: &ScriptInfo,
    ) -> anyhow::Result<()> {
        let value = bincode::serialize(info)?;
        self.put_cf(self.cf_script_info(), code_hash, &value)
    }

    pub fn list_script_infos(&self) -> anyhow::Result<Vec<(Vec<u8>, ScriptInfo)>> {
        let iter = self.iterator_cf(self.cf_script_info(), rocksdb::IteratorMode::Start);
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, value) = item;
            if let Ok(info) = bincode::deserialize::<ScriptInfo>(&value) {
                results.push((key.to_vec(), info));
            }
        }
        Ok(results)
    }
}
