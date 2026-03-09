//! Statistics operations.

use crate::keys::{self, stats_prefix};
use crate::store::CkbadgerStore;
use crate::types::*;

fn bytes_to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(&mut out, "{:02x}", b);
    }
    out
}

impl CkbadgerStore {
    // ---- Daily stats ----

    pub fn get_daily_stats(&self, date: &str) -> anyhow::Result<Option<DailyStats>> {
        let key = keys::encode_stats_key(stats_prefix::DAILY, date.as_bytes());
        match self.get_cf(self.cf_stats_chain(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_daily_stats(&self, date: &str, stats: &DailyStats) -> anyhow::Result<()> {
        let key = keys::encode_stats_key(stats_prefix::DAILY, date.as_bytes());
        let value = bincode::serialize(stats)?;
        self.put_cf(self.cf_stats_chain(), &key, &value)
    }

    pub fn list_daily_stats(&self) -> anyhow::Result<Vec<DailyStats>> {
        let prefix = [stats_prefix::DAILY];
        let iter = self.prefix_iterator_cf(self.cf_stats_chain(), &prefix);
        let mut results = Vec::new();

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!("failed to iterate stats_chain in list_daily_stats: {}", e)
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            let stats: DailyStats = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize daily stats in list_daily_stats: key=0x{}, error={}",
                    bytes_to_hex(&key),
                    e
                )
            })?;
            results.push(stats);
        }
        Ok(results)
    }

    // ---- Hourly stats ----

    pub fn get_hourly_stats(&self, hour: &str) -> anyhow::Result<Option<HourlyStats>> {
        let key = keys::encode_stats_key(stats_prefix::HOURLY, hour.as_bytes());
        match self.get_cf(self.cf_stats_chain(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_hourly_stats(&self, hour: &str, stats: &HourlyStats) -> anyhow::Result<()> {
        let key = keys::encode_stats_key(stats_prefix::HOURLY, hour.as_bytes());
        let value = bincode::serialize(stats)?;
        self.put_cf(self.cf_stats_chain(), &key, &value)
    }

    /// List daily stats with their date keys (date is in the key, not the value).
    pub fn list_daily_stats_with_dates(&self) -> anyhow::Result<Vec<(String, DailyStats)>> {
        let prefix = [stats_prefix::DAILY];
        let iter = self.prefix_iterator_cf(self.cf_stats_chain(), &prefix);
        let mut results = Vec::new();

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate stats_chain in list_daily_stats_with_dates: {}",
                    e
                )
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            let stats: DailyStats = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize daily stats in list_daily_stats_with_dates: key=0x{}, error={}",
                    bytes_to_hex(&key),
                    e
                )
            })?;
            // Key is prefix(1) + date_string
            let date = String::from_utf8_lossy(&key[1..]).to_string();
            results.push((date, stats));
        }
        Ok(results)
    }

    /// List hourly stats with their hour keys.
    pub fn list_hourly_stats_with_keys(&self) -> anyhow::Result<Vec<(String, HourlyStats)>> {
        let prefix = [stats_prefix::HOURLY];
        let iter = self.prefix_iterator_cf(self.cf_stats_chain(), &prefix);
        let mut results = Vec::new();

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate stats_chain in list_hourly_stats_with_keys: {}",
                    e
                )
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            let stats: HourlyStats = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize hourly stats in list_hourly_stats_with_keys: key=0x{}, error={}",
                    bytes_to_hex(&key),
                    e
                )
            })?;
            let hour_key = String::from_utf8_lossy(&key[1..]).to_string();
            results.push((hour_key, stats));
        }
        Ok(results)
    }

    // ---- Epoch stats ----

    pub fn get_epoch_stats(&self, epoch: i64) -> anyhow::Result<Option<EpochStats>> {
        let key = keys::encode_stats_key(stats_prefix::EPOCH, &epoch.to_be_bytes());
        match self.get_cf(self.cf_stats_chain(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_epoch_stats(&self, epoch: i64, stats: &EpochStats) -> anyhow::Result<()> {
        let key = keys::encode_stats_key(stats_prefix::EPOCH, &epoch.to_be_bytes());
        let value = bincode::serialize(stats)?;
        self.put_cf(self.cf_stats_chain(), &key, &value)
    }

    /// List all epoch stats, ordered by epoch number.
    pub fn list_epoch_stats(&self) -> anyhow::Result<Vec<EpochStats>> {
        let prefix = [stats_prefix::EPOCH];
        let iter = self.prefix_iterator_cf(self.cf_stats_chain(), &prefix);
        let mut results = Vec::new();

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!("failed to iterate stats_chain in list_epoch_stats: {}", e)
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            let stats: EpochStats = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize epoch stats in list_epoch_stats: key=0x{}, error={}",
                    bytes_to_hex(&key),
                    e
                )
            })?;
            results.push(stats);
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
        self.put_cf(self.cf_stats_chain(), &key, &value)
    }

    /// List all miner stats (aggregated across all dates).
    pub fn list_miner_stats(&self) -> anyhow::Result<Vec<MinerStats>> {
        let prefix = [stats_prefix::MINER];
        let iter = self.prefix_iterator_cf(self.cf_stats_chain(), &prefix);
        let mut results = Vec::new();

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!("failed to iterate stats_chain in list_miner_stats: {}", e)
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            let stats: MinerStats = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize miner stats in list_miner_stats: key=0x{}, error={}",
                    bytes_to_hex(&key),
                    e
                )
            })?;
            results.push(stats);
        }
        Ok(results)
    }

    // ---- Daily block stats ----

    pub fn get_daily_block_stats(&self, date: &str) -> anyhow::Result<Option<DailyBlockStats>> {
        let key = keys::encode_stats_key(stats_prefix::DAILY_BLOCK, date.as_bytes());
        match self.get_cf(self.cf_stats_chain(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_daily_block_stats(&self, date: &str, stats: &DailyBlockStats) -> anyhow::Result<()> {
        let key = keys::encode_stats_key(stats_prefix::DAILY_BLOCK, date.as_bytes());
        let value = bincode::serialize(stats)?;
        self.put_cf(self.cf_stats_chain(), &key, &value)
    }

    /// List all daily block stats with their date keys.
    pub fn list_daily_block_stats(&self) -> anyhow::Result<Vec<(String, DailyBlockStats)>> {
        let prefix = [stats_prefix::DAILY_BLOCK];
        let iter = self.prefix_iterator_cf(self.cf_stats_chain(), &prefix);
        let mut results = Vec::new();

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate stats_chain in list_daily_block_stats: {}",
                    e
                )
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            let stats: DailyBlockStats = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize daily block stats in list_daily_block_stats: key=0x{}, error={}",
                    bytes_to_hex(&key),
                    e
                )
            })?;
            let date = String::from_utf8_lossy(&key[1..]).to_string();
            results.push((date, stats));
        }
        Ok(results)
    }

    // ---- Epoch time distribution ----

    pub fn put_epoch_time_dist(&self, bucket: i32, count: i32) -> anyhow::Result<()> {
        let key = keys::encode_stats_key(stats_prefix::EPOCH_TIME_DIST, &bucket.to_be_bytes());
        self.put_cf(self.cf_stats_chain(), &key, &count.to_le_bytes())
    }

    /// List all epoch time distribution buckets.
    pub fn list_epoch_time_dist(&self) -> anyhow::Result<Vec<(i32, i32)>> {
        let prefix = [stats_prefix::EPOCH_TIME_DIST];
        let iter = self.prefix_iterator_cf(self.cf_stats_chain(), &prefix);
        let mut results = Vec::new();

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate stats_chain in list_epoch_time_dist: {}",
                    e
                )
            })?;
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
        let iter = self.prefix_iterator_cf(self.cf_stats_dao(), &prefix);
        let mut results = Vec::new();

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate stats_dao in list_dao_daily_snapshots: {}",
                    e
                )
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            let snapshot: DaoDailySnapshot = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize dao daily snapshot in list_dao_daily_snapshots: key=0x{}, error={}",
                    bytes_to_hex(&key),
                    e
                )
            })?;
            results.push(snapshot);
        }
        Ok(results)
    }

    pub fn get_dao_daily_snapshot(&self, date: &str) -> anyhow::Result<Option<DaoDailySnapshot>> {
        let key = keys::encode_stats_key(stats_prefix::DAO_DAILY_SNAPSHOT, date.as_bytes());
        match self.get_cf(self.cf_stats_dao(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn get_latest_dao_daily_snapshot(&self) -> anyhow::Result<Option<DaoDailySnapshot>> {
        let prefix = [stats_prefix::DAO_DAILY_SNAPSHOT];
        let seek_key = [stats_prefix::DAO_DAILY_SNAPSHOT + 1];
        let iter = self.iterator_cf(
            self.cf_stats_dao(),
            rocksdb::IteratorMode::From(&seek_key, rocksdb::Direction::Reverse),
        );

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate stats_dao in get_latest_dao_daily_snapshot: {}",
                    e
                )
            })?;
            if key.first().copied().unwrap_or_default() < stats_prefix::DAO_DAILY_SNAPSHOT {
                break;
            }
            if !key.starts_with(&prefix) {
                continue;
            }
            let snapshot: DaoDailySnapshot = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize dao daily snapshot in get_latest_dao_daily_snapshot: key=0x{}, error={}",
                    bytes_to_hex(&key),
                    e
                )
            })?;
            return Ok(Some(snapshot));
        }

        Ok(None)
    }

    pub fn get_latest_dao_statistics(&self) -> anyhow::Result<Option<DaoLatestStatistics>> {
        let key = keys::encode_stats_key(stats_prefix::DAO_LATEST_STATS, b"latest");
        match self.get_cf(self.cf_stats_dao(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    // ---- HODL wave snapshots ----

    pub fn put_hodl_wave(&self, date: &str, wave: &DailyHodlWave) -> anyhow::Result<()> {
        let key = keys::encode_stats_key(stats_prefix::HODL_WAVE, date.as_bytes());
        let value = bincode::serialize(wave)?;
        self.put_cf(self.cf_stats_hodl(), &key, &value)
    }

    pub fn get_hodl_wave(&self, date: &str) -> anyhow::Result<Option<DailyHodlWave>> {
        let key = keys::encode_stats_key(stats_prefix::HODL_WAVE, date.as_bytes());
        match self.get_cf(self.cf_stats_hodl(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn list_hodl_waves(&self) -> anyhow::Result<Vec<(String, DailyHodlWave)>> {
        let prefix = [stats_prefix::HODL_WAVE];
        let iter = self.prefix_iterator_cf(self.cf_stats_hodl(), &prefix);
        let mut results = Vec::new();

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!("failed to iterate stats_hodl in list_hodl_waves: {}", e)
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            let wave: DailyHodlWave = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize hodl wave in list_hodl_waves: key=0x{}, error={}",
                    bytes_to_hex(&key),
                    e
                )
            })?;
            let date = String::from_utf8_lossy(&key[1..]).to_string();
            results.push((date, wave));
        }
        Ok(results)
    }

    // ---- HODL tracker state persistence ----

    pub fn get_hodl_tracker_state(&self) -> anyhow::Result<Option<HodlTrackerState>> {
        match self.get_cf(
            self.cf_sync_meta(),
            crate::keys::sync_meta_keys::HODL_TRACKER,
        )? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_hodl_tracker_state(&self, state: &HodlTrackerState) -> anyhow::Result<()> {
        let value = bincode::serialize(state)?;
        self.put_cf(
            self.cf_sync_meta(),
            crate::keys::sync_meta_keys::HODL_TRACKER,
            &value,
        )
    }

    // ---- Daily activity stats ----

    pub fn get_daily_activity_stats(
        &self,
        date: &str,
    ) -> anyhow::Result<Option<DailyActivityStats>> {
        let key = keys::encode_stats_key(keys::stats_prefix::ACTIVITY_DAILY, date.as_bytes());
        match self.get_cf(self.cf_stats_chain(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_daily_activity_stats(
        &self,
        date: &str,
        stats: &DailyActivityStats,
    ) -> anyhow::Result<()> {
        let key = keys::encode_stats_key(keys::stats_prefix::ACTIVITY_DAILY, date.as_bytes());
        let value = bincode::serialize(stats)?;
        self.put_cf(self.cf_stats_chain(), &key, &value)
    }

    pub fn list_daily_activity_stats(&self) -> anyhow::Result<Vec<(String, DailyActivityStats)>> {
        let prefix = [keys::stats_prefix::ACTIVITY_DAILY];
        let iter = self.prefix_iterator_cf(self.cf_stats_chain(), &prefix);
        let mut results = Vec::new();

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate stats_chain in list_daily_activity_stats: {}",
                    e
                )
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            let date_bytes = &key[1..]; // skip prefix byte
            let date_str = std::str::from_utf8(date_bytes)
                .map_err(|e| {
                    anyhow::anyhow!("invalid UTF-8 date in daily activity stats key: {}", e)
                })?
                .to_string();
            let stats: DailyActivityStats = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize daily activity stats: date={}, error={}",
                    date_str,
                    e
                )
            })?;
            results.push((date_str, stats));
        }
        Ok(results)
    }

    // ---- Script daily deltas ----

    pub fn get_script_daily_delta(
        &self,
        code_hash: &[u8],
        is_type: bool,
        date_yyyymmdd: u32,
    ) -> anyhow::Result<Option<ScriptDailyDelta>> {
        let key = keys::encode_script_daily_key(code_hash, is_type, date_yyyymmdd);
        match self.get_cf(self.cf_stats_script(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_script_daily_delta(
        &self,
        code_hash: &[u8],
        is_type: bool,
        date_yyyymmdd: u32,
        delta: &ScriptDailyDelta,
    ) -> anyhow::Result<()> {
        let key = keys::encode_script_daily_key(code_hash, is_type, date_yyyymmdd);
        let value = bincode::serialize(delta)?;
        self.put_cf(self.cf_stats_script(), &key, &value)
    }

    pub fn list_script_daily_deltas(
        &self,
        code_hash: &[u8],
        is_type: bool,
    ) -> anyhow::Result<Vec<(u32, ScriptDailyDelta)>> {
        self.list_script_daily_deltas_in_range(code_hash, is_type, None, None)
    }

    pub fn list_script_daily_deltas_in_range(
        &self,
        code_hash: &[u8],
        is_type: bool,
        from_date_yyyymmdd: Option<u32>,
        to_date_yyyymmdd: Option<u32>,
    ) -> anyhow::Result<Vec<(u32, ScriptDailyDelta)>> {
        let prefix = keys::encode_script_daily_prefix(code_hash, is_type);
        let start_key = keys::encode_script_daily_key(
            code_hash,
            is_type,
            from_date_yyyymmdd.unwrap_or(u32::MIN),
        );
        let iter = self.iterator_cf(
            self.cf_stats_script(),
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );
        let mut results = Vec::new();

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate stats_script in list_script_daily_deltas_in_range: {}",
                    e
                )
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            if key.len() != keys::SCRIPT_DAILY_KEY_SIZE {
                continue;
            }
            let (_, _, date) = keys::decode_script_daily_key(&key);
            if let Some(to_date) = to_date_yyyymmdd {
                if date > to_date {
                    break;
                }
            }
            let delta: ScriptDailyDelta = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize script daily delta in list_script_daily_deltas_in_range: code_hash=0x{}, is_type={}, date={}, error={}",
                    bytes_to_hex(code_hash),
                    is_type,
                    date,
                    e
                )
            })?;
            results.push((date, delta));
        }

        Ok(results)
    }

    // ---- Script info ----

    pub fn get_script_info(&self, code_hash: &[u8]) -> anyhow::Result<Option<ScriptInfo>> {
        match self.get_cf(self.cf_script_info(), code_hash)? {
            Some(value) => {
                let info = bincode::deserialize(&value).map_err(|e| {
                    anyhow::anyhow!(
                        "failed to deserialize script info: code_hash=0x{}, error={}",
                        code_hash
                            .iter()
                            .map(|b| format!("{:02x}", b))
                            .collect::<String>(),
                        e
                    )
                })?;
                Ok(Some(info))
            }
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

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!("failed to iterate script_info in list_script_infos: {}", e)
            })?;
            let info: ScriptInfo = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize script info in list_script_infos: code_hash=0x{}, error={}",
                    bytes_to_hex(&key),
                    e
                )
            })?;
            results.push((key.to_vec(), info));
        }
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CkbadgerStore;

    #[test]
    fn test_hodl_wave_put_get_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();

        let wave = DailyHodlWave {
            band_24h: 100_000_000,
            band_1d_1w: 200_000_000,
            band_1w_1m: 300_000_000,
            band_1m_3m: 400_000_000,
            band_3m_6m: 500_000_000,
            band_6m_1y: 600_000_000,
            band_1y_3y: 700_000_000,
            band_gt_3y: 800_000_000,
            holder_count: 42_000,
        };

        store.put_hodl_wave("20240115", &wave).unwrap();

        let retrieved = store.get_hodl_wave("20240115").unwrap().unwrap();
        assert_eq!(retrieved.band_24h, 100_000_000);
        assert_eq!(retrieved.band_gt_3y, 800_000_000);
        assert_eq!(retrieved.holder_count, 42_000);

        // Non-existent date returns None
        assert!(store.get_hodl_wave("20240116").unwrap().is_none());
    }

    #[test]
    fn test_hodl_wave_list_sorted() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();

        let wave1 = DailyHodlWave {
            band_24h: 100,
            holder_count: 10,
            ..Default::default()
        };
        let wave2 = DailyHodlWave {
            band_24h: 200,
            holder_count: 20,
            ..Default::default()
        };
        let wave3 = DailyHodlWave {
            band_24h: 300,
            holder_count: 30,
            ..Default::default()
        };

        // Insert out of order
        store.put_hodl_wave("20240115", &wave2).unwrap();
        store.put_hodl_wave("20240113", &wave1).unwrap();
        store.put_hodl_wave("20240117", &wave3).unwrap();

        let waves = store.list_hodl_waves().unwrap();
        assert_eq!(waves.len(), 3);
        // RocksDB prefix scan returns sorted by key
        assert_eq!(waves[0].0, "20240113");
        assert_eq!(waves[0].1.band_24h, 100);
        assert_eq!(waves[1].0, "20240115");
        assert_eq!(waves[1].1.band_24h, 200);
        assert_eq!(waves[2].0, "20240117");
        assert_eq!(waves[2].1.band_24h, 300);
    }

    #[test]
    fn test_list_hodl_waves_fails_on_invalid_payload() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();
        let key = keys::encode_stats_key(stats_prefix::HODL_WAVE, b"20240115");
        store
            .put_cf(store.cf_stats_hodl(), &key, b"invalid-hodl-wave")
            .unwrap();

        let err = store.list_hodl_waves().unwrap_err();
        assert!(err
            .to_string()
            .contains("failed to deserialize hodl wave in list_hodl_waves"));
    }

    #[test]
    fn test_hodl_tracker_state_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();

        // Initially none
        assert!(store.get_hodl_tracker_state().unwrap().is_none());

        let state = HodlTrackerState {
            capacity_by_date: vec![
                ("20240101".to_string(), 1_000_000),
                ("20240102".to_string(), 2_000_000),
            ],
            date_transitions: vec![(0, "20240101".to_string()), (100, "20240102".to_string())],
            holder_count: 500,
            last_snapshot_date: Some("20240102".to_string()),
        };

        store.put_hodl_tracker_state(&state).unwrap();

        let retrieved = store.get_hodl_tracker_state().unwrap().unwrap();
        assert_eq!(retrieved.capacity_by_date.len(), 2);
        assert_eq!(retrieved.holder_count, 500);
        assert_eq!(retrieved.last_snapshot_date, Some("20240102".to_string()));
        assert_eq!(retrieved.date_transitions[1].0, 100);
    }

    #[test]
    fn test_script_daily_delta_roundtrip_and_list() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();
        let code_hash = vec![0xAB; 32];

        let d1 = ScriptDailyDelta {
            live_capacity_delta: 1_000_000_000_000,
            live_occupied_capacity_delta: 700_000_000_000,
        };
        let d2 = ScriptDailyDelta {
            live_capacity_delta: -200_000_000_000,
            live_occupied_capacity_delta: -120_000_000_000,
        };
        store
            .put_script_daily_delta(&code_hash, false, 20240115, &d1)
            .unwrap();
        store
            .put_script_daily_delta(&code_hash, false, 20240116, &d2)
            .unwrap();

        let loaded = store
            .get_script_daily_delta(&code_hash, false, 20240115)
            .unwrap()
            .unwrap();
        assert_eq!(loaded.live_capacity_delta, d1.live_capacity_delta);
        assert_eq!(
            loaded.live_occupied_capacity_delta,
            d1.live_occupied_capacity_delta
        );

        let listed = store.list_script_daily_deltas(&code_hash, false).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].0, 20240115);
        assert_eq!(listed[1].0, 20240116);

        let ranged = store
            .list_script_daily_deltas_in_range(&code_hash, false, Some(20240116), Some(20240116))
            .unwrap();
        assert_eq!(ranged.len(), 1);
        assert_eq!(ranged[0].0, 20240116);
    }

    #[test]
    fn test_list_script_infos_fails_on_invalid_payload() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();
        let code_hash = vec![0xAB; 32];
        store
            .put_cf(store.cf_script_info(), &code_hash, b"invalid-script-info")
            .unwrap();

        let err = store.list_script_infos().unwrap_err();
        assert!(err
            .to_string()
            .contains("failed to deserialize script info in list_script_infos"));
    }

    /// Helper: write a DaoDailySnapshot directly to the store.
    fn put_dao_snapshot(store: &CkbadgerStore, date_key: &str, snap: &DaoDailySnapshot) {
        let key = crate::keys::encode_stats_key(
            crate::keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT,
            date_key.as_bytes(),
        );
        let value = bincode::serialize(snap).unwrap();
        store.put_cf(store.cf_stats_dao(), &key, &value).unwrap();
    }

    #[test]
    fn test_dao_daily_snapshot_get_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();

        let snap = DaoDailySnapshot {
            date: "2024-01-15".to_string(),
            total_deposited: 100_000_000_000_000,
            depositors_count: 42,
            new_deposits: 100,
            withdrawals: 10,
            compensation: 50_000_000_000,
            cumulative_deposit_amount: 200_000_000_000_000,
            total_issuance: 4_000_000_000_000_000_000,
            secondary_pool: 10_000_000_000_000,
            occupied_capacity: 400_000_000_000_000_000,
            cum_miner_secondary: 1_000_000_000_000,
            cum_dao_compensation: 2_000_000_000_000,
            cum_treasury: 7_000_000_000_000,
        };

        put_dao_snapshot(&store, "20240115", &snap);

        let retrieved = store.get_dao_daily_snapshot("20240115").unwrap().unwrap();
        assert_eq!(retrieved.date, "2024-01-15");
        assert_eq!(retrieved.cum_miner_secondary, 1_000_000_000_000);
        assert_eq!(retrieved.cum_dao_compensation, 2_000_000_000_000);
        assert_eq!(retrieved.cum_treasury, 7_000_000_000_000);
        assert_eq!(retrieved.total_issuance, 4_000_000_000_000_000_000);

        // Non-existent date returns None
        assert!(store.get_dao_daily_snapshot("20240116").unwrap().is_none());
    }

    #[test]
    fn test_get_latest_dao_daily_snapshot_returns_latest_by_key() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();

        let mut snap = DaoDailySnapshot {
            date: "2024-01-15".to_string(),
            total_deposited: 100,
            depositors_count: 1,
            new_deposits: 1,
            withdrawals: 0,
            compensation: 0,
            cumulative_deposit_amount: 100,
            total_issuance: 10_000,
            secondary_pool: 100,
            occupied_capacity: 50,
            cum_miner_secondary: 1,
            cum_dao_compensation: 2,
            cum_treasury: 3,
        };
        put_dao_snapshot(&store, "20240115", &snap);

        snap.date = "2024-01-16".to_string();
        put_dao_snapshot(&store, "20240116", &snap);

        // Write another key in the same CF with a different stats prefix.
        let other_key =
            crate::keys::encode_stats_key(crate::keys::STATS_PREFIX_DAO_LATEST_STATS, b"latest");
        store
            .put_cf(store.cf_stats_dao(), &other_key, b"non-snapshot")
            .unwrap();

        let latest = store.get_latest_dao_daily_snapshot().unwrap().unwrap();
        assert_eq!(latest.date, "2024-01-16");
    }

    #[test]
    fn test_dao_snapshot_miner_secondary_nonzero() {
        // Simulates the indexer formula: daily_miner = s_delta * U / (C - U)
        // With U/C = 10%, miner should get ~11.1% of S_delta (U/(C-U) = 0.1/0.9)
        let c: i128 = 4_000_000_000_000_000_000; // 40B CKB
        let u: i128 = 400_000_000_000_000_000; // 4B CKB (10% of C)
        let prev_s: i128 = 10_000_000_000_000;
        let curr_s: i128 = 10_100_000_000_000;
        let s_delta = curr_s - prev_s; // 1000 CKB worth

        let denom = (c - u).max(1);
        let daily_miner = s_delta * u / denom;

        // Miner should be ~11.1% of s_delta (U / (C-U) = 4B / 36B ≈ 0.1111)
        assert!(daily_miner > 0, "miner secondary must be non-zero");
        let ratio = daily_miner as f64 / s_delta as f64;
        assert!(
            (ratio - 0.1111).abs() < 0.001,
            "miner ratio should be ~11.1%, got {:.4}",
            ratio
        );

        // Verify that miner + non-miner (s_delta) reconstructs correctly:
        // total_secondary = s_delta + daily_miner = s_delta * C / (C-U)
        let total_secondary = s_delta + daily_miner;
        let expected_total = s_delta * c / denom;
        assert_eq!(total_secondary, expected_total);
    }

    #[test]
    fn test_dao_snapshot_multiday_chaining() {
        // Simulates a multi-day batch where each date must chain from the
        // previous date's cumulative values (not a stale store snapshot).
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();

        // Seed an initial "day 0" snapshot in the store
        let day0 = DaoDailySnapshot {
            date: "2024-01-14".to_string(),
            total_deposited: 50_000_000_000_000,
            depositors_count: 10,
            new_deposits: 50,
            withdrawals: 5,
            compensation: 10_000_000_000,
            cumulative_deposit_amount: 100_000_000_000_000,
            total_issuance: 4_000_000_000_000_000_000,
            secondary_pool: 10_000_000_000_000,
            occupied_capacity: 400_000_000_000_000_000,
            cum_miner_secondary: 500_000_000_000,
            cum_dao_compensation: 1_000_000_000_000,
            cum_treasury: 3_500_000_000_000,
        };
        put_dao_snapshot(&store, "20240114", &day0);

        // Simulate processing two dates in one batch, mimicking the fixed
        // indexer loop that updates prev_snapshot after each iteration.
        let dates = ["2024-01-15", "2024-01-16"];
        let s_values: [i128; 2] = [10_100_000_000_000, 10_200_000_000_000];
        let c: i128 = 4_000_000_000_000_000_000;
        let u: i128 = 400_000_000_000_000_000;
        let deposited: i128 = 50_000_000_000_000;

        let mut prev = store.list_dao_daily_snapshots().unwrap().last().cloned();

        let mut snapshots = Vec::new();
        for (i, date) in dates.iter().enumerate() {
            let secondary_pool = s_values[i];
            let (cum_miner, cum_dao, cum_treasury) = if let Some(ref p) = prev {
                let s_delta = secondary_pool - p.secondary_pool;
                if s_delta >= 0 {
                    let denom = (c - u).max(1);
                    let daily_miner = s_delta * u / denom;
                    let daily_dao_share = s_delta * deposited / denom;
                    let daily_treasury_share = s_delta - daily_dao_share;
                    (
                        p.cum_miner_secondary + daily_miner,
                        p.cum_dao_compensation + daily_dao_share,
                        p.cum_treasury + daily_treasury_share,
                    )
                } else {
                    (
                        p.cum_miner_secondary,
                        p.cum_dao_compensation,
                        p.cum_treasury + s_delta,
                    )
                }
            } else {
                (0, 0, 0)
            };

            let snap = DaoDailySnapshot {
                date: date.to_string(),
                total_deposited: deposited,
                depositors_count: 10,
                new_deposits: 50,
                withdrawals: 5,
                compensation: 10_000_000_000,
                cumulative_deposit_amount: 100_000_000_000_000,
                total_issuance: c,
                secondary_pool,
                occupied_capacity: u,
                cum_miner_secondary: cum_miner,
                cum_dao_compensation: cum_dao,
                cum_treasury,
            };

            // Update prev for next iteration (the bug fix)
            prev = Some(snap.clone());
            snapshots.push(snap);
        }

        // Day 1 should build on day 0's cumulatives
        assert!(
            snapshots[0].cum_miner_secondary > day0.cum_miner_secondary,
            "day 1 miner should increase from day 0"
        );
        // Day 2 should build on day 1's cumulatives (NOT day 0's)
        assert!(
            snapshots[1].cum_miner_secondary > snapshots[0].cum_miner_secondary,
            "day 2 miner should increase from day 1"
        );
        assert!(
            snapshots[1].cum_dao_compensation > snapshots[0].cum_dao_compensation,
            "day 2 dao should increase from day 1"
        );
        assert!(
            snapshots[1].cum_treasury > snapshots[0].cum_treasury,
            "day 2 treasury should increase from day 1"
        );

        // Verify the increments are equal (same s_delta each day)
        let miner_inc_1 = snapshots[0].cum_miner_secondary - day0.cum_miner_secondary;
        let miner_inc_2 = snapshots[1].cum_miner_secondary - snapshots[0].cum_miner_secondary;
        assert_eq!(
            miner_inc_1, miner_inc_2,
            "equal s_delta should produce equal miner increments"
        );
    }

    #[test]
    fn test_dao_snapshot_negative_s_delta_protocol_upgrade() {
        // CKB's on-chain S field can decrease at protocol upgrade boundaries.
        // For user-facing cumulative charts, negative deltas are ignored to
        // keep miner/dao/treasury monotonic and avoid artificial drops.
        let c: i128 = 4_000_000_000_000_000_000; // 40B CKB
        let u: i128 = 400_000_000_000_000_000; // 4B CKB
        let deposited: i128 = 50_000_000_000_000;
        let denom = (c - u).max(1);

        // S values: day 1 = +100, day 2 = -30 (upgrade drop), day 3 = +100
        let s0: i128 = 10_000_000_000_000;
        let s1: i128 = 10_100_000_000_000; // +100 CKB
        let s2: i128 = 10_070_000_000_000; // -30 CKB (protocol upgrade drop)
        let s3: i128 = 10_170_000_000_000; // +100 CKB

        let s_values = [s1, s2, s3];
        let mut prev_s = s0;
        let mut cum_miner: i128 = 0;
        let mut cum_dao: i128 = 0;
        let mut cum_treasury: i128 = 0;

        for &s in &s_values {
            let s_delta = s - prev_s;
            if s_delta > 0 {
                let miner = s_delta * u / denom;
                let dao = s_delta * deposited / denom;
                let treasury = s_delta - dao;
                cum_miner += miner;
                cum_dao += dao;
                cum_treasury += treasury;
            }
            prev_s = s;
        }

        let positive_s_change = (s1 - s0) + (s3 - s2); // only positive deltas
        let cum_non_miner = cum_dao + cum_treasury;

        // Non-miner tracks only positive S growth.
        assert_eq!(
            cum_non_miner, positive_s_change,
            "cum_dao + cum_treasury must equal sum(positive s_delta)"
        );

        // Miner and dao must be non-negative (monotonic).
        assert!(cum_miner >= 0, "cum_miner must be non-negative");
        assert!(cum_dao >= 0, "cum_dao must be non-negative");
    }

    #[test]
    fn test_dao_snapshot_negative_s_delta_batch_boundary() {
        // Regression test: negative S deltas are ignored even across batch
        // boundaries, while positive deltas still accumulate normally.
        let c: i128 = 4_000_000_000_000_000_000;
        let u: i128 = 400_000_000_000_000_000;
        let deposited: i128 = 50_000_000_000_000;

        let s_prev_day: i128 = 10_000_000_000_000; // end of previous day
        let s_batch_end: i128 = 9_980_000_000_000; // mid-day after S drop (batch boundary)
        let s_day_end: i128 = 10_080_000_000_000; // actual end of day

        // Batch N processes partial day: s_delta = s_batch_end - s_prev_day < 0
        let s_delta_batch_n = s_batch_end - s_prev_day; // -20
        assert!(s_delta_batch_n < 0);

        let (miner_n, dao_n, treas_n) = (0i128, 0i128, 0i128);

        // Batch N+1 processes rest of day: s_delta = s_day_end - s_batch_end > 0
        let s_delta_batch_n1 = s_day_end - s_batch_end; // +100
        assert!(s_delta_batch_n1 > 0);
        let denom = (c - u).max(1);
        let miner_n1 = s_delta_batch_n1 * u / denom;
        let dao_n1 = s_delta_batch_n1 * deposited / denom;
        let treas_n1 = s_delta_batch_n1 - dao_n1;

        // Total for the day
        let total_miner = miner_n + miner_n1;
        let total_dao = dao_n + dao_n1;
        let total_treas = treas_n + treas_n1;
        let total_non_miner = total_dao + total_treas;

        // Negative segment is ignored; only positive segment contributes.
        let actual_positive_change = s_delta_batch_n1; // +100
        assert_eq!(
            total_non_miner, actual_positive_change,
            "batch-split non-miner must equal positive segment: got {} expected {}",
            total_non_miner, actual_positive_change
        );

        // Miner should only account for the positive portion
        assert!(total_miner >= 0);
        assert!(total_dao >= 0);
    }
}

#[cfg(test)]
mod daily_activity_stats_tests {
    use super::*;
    use tempfile::TempDir;

    fn open_test_store() -> (TempDir, CkbadgerStore) {
        let dir = TempDir::new().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path()).unwrap();
        (dir, store)
    }

    #[test]
    fn test_get_daily_activity_stats_missing_returns_none() {
        let (_dir, store) = open_test_store();
        let result = store.get_daily_activity_stats("20260309").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_put_and_get_daily_activity_stats_roundtrip() {
        let (_dir, store) = open_test_store();
        let stats = DailyActivityStats {
            transfer_count: 100,
            dao_deposit_count: 10,
            dao_withdraw_request_count: 3,
            dao_withdraw_complete_count: 2,
            token_count: 50,
            nft_count: 20,
            coinbase_count: 8640,
            unique_address_count: 500,
            total_ckb_moved: 100_000_000_000_000,
        };
        store.put_daily_activity_stats("20260309", &stats).unwrap();
        let loaded = store.get_daily_activity_stats("20260309").unwrap().unwrap();
        assert_eq!(loaded.transfer_count, 100);
        assert_eq!(loaded.coinbase_count, 8640);
        assert_eq!(loaded.unique_address_count, 500);
        assert_eq!(loaded.total_ckb_moved, 100_000_000_000_000);
    }

    #[test]
    fn test_list_daily_activity_stats_returns_all_dates() {
        let (_dir, store) = open_test_store();
        let s1 = DailyActivityStats {
            transfer_count: 10,
            ..Default::default()
        };
        let s2 = DailyActivityStats {
            transfer_count: 20,
            ..Default::default()
        };
        store.put_daily_activity_stats("20260308", &s1).unwrap();
        store.put_daily_activity_stats("20260309", &s2).unwrap();

        let all = store.list_daily_activity_stats().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].0, "20260308");
        assert_eq!(all[0].1.transfer_count, 10);
        assert_eq!(all[1].0, "20260309");
        assert_eq!(all[1].1.transfer_count, 20);
    }
}
