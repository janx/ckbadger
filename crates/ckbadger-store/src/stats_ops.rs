//! Statistics operations.

use crate::keys::{self, stats_prefix};
use crate::store::CkbadgerStore;
use crate::types::*;

use crate::bytes_to_hex;
use ckbadger_common::dao::calculate_secondary_miner_delta;

// ---------------------------------------------------------------------------
// Activity address-set rows (ACTIVITY_DAILY_ADDR_SET / ACTIVITY_HOURLY_ADDR_SET)
//
// The persistent per-bucket address set is the dedup memory behind
// `DailyActivityStats::unique_address_count`. Its on-disk form and its count
// derivation have exactly one implementation — these functions — shared by the
// live write path (`BatchWriter::merge_persistent_addr_set`) and by reorg
// rollback repair (`reorg_ops`). Set and count must never be derived
// independently, or a rollback can leave them disagreeing forever.
// ---------------------------------------------------------------------------

/// Canonical on-disk encoding of an activity address-set row: the bucket's
/// 32-byte lock hashes, deduplicated and sorted, concatenated.
pub fn encode_activity_addr_set<I: IntoIterator<Item = [u8; 32]>>(addrs: I) -> Vec<u8> {
    let mut sorted: Vec<[u8; 32]> = addrs.into_iter().collect();
    sorted.sort_unstable();
    sorted.dedup();
    sorted.into_iter().flatten().collect()
}

/// Decode a persisted activity address-set row.
///
/// A length that is not a whole number of 32-byte hashes means the row was
/// written by something other than `encode_activity_addr_set` — fail with the
/// bucket rather than silently dropping the trailing partial hash, which would
/// undercount `unique_address_count` forever.
pub fn decode_activity_addr_set(
    raw: &[u8],
    bucket: &str,
) -> anyhow::Result<std::collections::HashSet<[u8; 32]>> {
    if !raw.len().is_multiple_of(32) {
        anyhow::bail!(
            "corrupt activity addr set row: bucket={}, len={} (not a multiple of 32)",
            bucket,
            raw.len()
        );
    }
    let mut set = std::collections::HashSet::with_capacity(raw.len() / 32);
    for chunk in raw.chunks_exact(32) {
        let mut hash = [0u8; 32];
        hash.copy_from_slice(chunk);
        set.insert(hash);
    }
    Ok(set)
}

/// Derive `unique_address_count` from an address-set size.
pub fn activity_addr_set_count(len: usize, bucket: &str) -> anyhow::Result<u32> {
    u32::try_from(len).map_err(|_| {
        anyhow::anyhow!(
            "unique_address_count exceeds u32: bucket={}, count={}",
            bucket,
            len
        )
    })
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

    pub fn put_dao_top_depositors(&self, top: &DaoTopDepositors) -> anyhow::Result<()> {
        let key = keys::encode_stats_key(stats_prefix::DAO_TOP_DEPOSITORS, b"latest");
        let value = bincode::serialize(top)?;
        self.put_cf(self.cf_stats_dao(), &key, &value)
    }

    pub fn get_dao_top_depositors(&self) -> anyhow::Result<Option<DaoTopDepositors>> {
        let key = keys::encode_stats_key(stats_prefix::DAO_TOP_DEPOSITORS, b"latest");
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

    // ---- Cell distribution snapshots ----

    pub fn put_cell_distribution(
        &self,
        date: &str,
        snapshot: &DailyCellDistribution,
    ) -> anyhow::Result<()> {
        let key = keys::encode_stats_key(stats_prefix::CELL_DISTRIBUTION, date.as_bytes());
        let value = bincode::serialize(snapshot)?;
        self.put_cf(self.cf_stats_hodl(), &key, &value)
    }

    pub fn get_cell_distribution(
        &self,
        date: &str,
    ) -> anyhow::Result<Option<DailyCellDistribution>> {
        let key = keys::encode_stats_key(stats_prefix::CELL_DISTRIBUTION, date.as_bytes());
        match self.get_cf(self.cf_stats_hodl(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn get_latest_cell_distribution(
        &self,
    ) -> anyhow::Result<Option<(String, DailyCellDistribution)>> {
        let prefix = [stats_prefix::CELL_DISTRIBUTION];
        let seek_key = [stats_prefix::CELL_DISTRIBUTION + 1];
        let iter = self.iterator_cf(
            self.cf_stats_hodl(),
            rocksdb::IteratorMode::From(&seek_key, rocksdb::Direction::Reverse),
        );

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate stats_hodl in get_latest_cell_distribution: {}",
                    e
                )
            })?;
            if key.first().copied().unwrap_or_default() < stats_prefix::CELL_DISTRIBUTION {
                break;
            }
            if !key.starts_with(&prefix) {
                continue;
            }
            let date_str = String::from_utf8_lossy(&key[1..]).to_string();
            let snapshot: DailyCellDistribution =
                bincode::deserialize(&value).map_err(|e| {
                    anyhow::anyhow!(
                        "failed to deserialize cell distribution in get_latest_cell_distribution: key=0x{}, error={}",
                        bytes_to_hex(&key),
                        e
                    )
                })?;
            return Ok(Some((date_str, snapshot)));
        }

        Ok(None)
    }

    // ---- Address cohort snapshots ----

    pub fn put_address_cohort(
        &self,
        date: &str,
        snapshot: &DailyAddressCohort,
    ) -> anyhow::Result<()> {
        let key = keys::encode_stats_key(stats_prefix::ADDR_COHORT, date.as_bytes());
        let value = bincode::serialize(snapshot)?;
        self.put_cf(self.cf_stats_hodl(), &key, &value)
    }

    pub fn get_address_cohort(&self, date: &str) -> anyhow::Result<Option<DailyAddressCohort>> {
        let key = keys::encode_stats_key(stats_prefix::ADDR_COHORT, date.as_bytes());
        match self.get_cf(self.cf_stats_hodl(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn get_latest_address_cohort(
        &self,
    ) -> anyhow::Result<Option<(String, DailyAddressCohort)>> {
        let prefix = [stats_prefix::ADDR_COHORT];
        let seek_key = [stats_prefix::ADDR_COHORT + 1];
        let iter = self.iterator_cf(
            self.cf_stats_hodl(),
            rocksdb::IteratorMode::From(&seek_key, rocksdb::Direction::Reverse),
        );

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate stats_hodl in get_latest_address_cohort: {}",
                    e
                )
            })?;
            if key.first().copied().unwrap_or_default() < stats_prefix::ADDR_COHORT {
                break;
            }
            if !key.starts_with(&prefix) {
                continue;
            }
            let date_str = String::from_utf8_lossy(&key[1..]).to_string();
            let snapshot: DailyAddressCohort =
                bincode::deserialize(&value).map_err(|e| {
                    anyhow::anyhow!(
                        "failed to deserialize address cohort in get_latest_address_cohort: key=0x{}, error={}",
                        bytes_to_hex(&key),
                        e
                    )
                })?;
            return Ok(Some((date_str, snapshot)));
        }

        Ok(None)
    }

    // ---- Cell distribution tracker state persistence ----

    pub fn get_cell_dist_tracker_state(
        &self,
    ) -> anyhow::Result<Option<CellDistributionTrackerState>> {
        match self.get_cf(
            self.cf_sync_meta(),
            crate::keys::sync_meta_keys::CELL_DIST_TRACKER,
        )? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_cell_dist_tracker_state(
        &self,
        state: &CellDistributionTrackerState,
    ) -> anyhow::Result<()> {
        let value = bincode::serialize(state)?;
        self.put_cf(
            self.cf_sync_meta(),
            crate::keys::sync_meta_keys::CELL_DIST_TRACKER,
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

    // ---- Hourly activity stats ----

    pub fn put_hourly_activity_stats(
        &self,
        hour_key: &str, // "YYYYMMDDHH"
        stats: &DailyActivityStats,
    ) -> anyhow::Result<()> {
        let key = keys::encode_stats_key(keys::stats_prefix::ACTIVITY_HOURLY, hour_key.as_bytes());
        let value = bincode::serialize(stats)?;
        self.put_cf(self.cf_stats_chain(), &key, &value)
    }

    pub fn get_hourly_activity_stats(
        &self,
        hour_key: &str,
    ) -> anyhow::Result<Option<DailyActivityStats>> {
        let key = keys::encode_stats_key(keys::stats_prefix::ACTIVITY_HOURLY, hour_key.as_bytes());
        match self.get_cf(self.cf_stats_chain(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    /// List hourly activity stats from `since_hour` (inclusive) onwards.
    /// `since_hour` is a "YYYYMMDDHH" string.
    pub fn list_hourly_activity_stats_since(
        &self,
        since_hour: &str,
    ) -> anyhow::Result<Vec<(String, DailyActivityStats)>> {
        let start_key =
            keys::encode_stats_key(keys::stats_prefix::ACTIVITY_HOURLY, since_hour.as_bytes());
        let prefix = [keys::stats_prefix::ACTIVITY_HOURLY];
        let iter = self.iterator_cf(
            self.cf_stats_chain(),
            rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
        );
        let mut results = Vec::new();
        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate stats_chain in list_hourly_activity_stats_since: {}",
                    e
                )
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            let hour_bytes = &key[1..];
            let hour_str = std::str::from_utf8(hour_bytes)
                .map_err(|e| {
                    anyhow::anyhow!("invalid UTF-8 hour in hourly activity stats key: {}", e)
                })?
                .to_string();
            let stats: DailyActivityStats = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize hourly activity stats: hour={}, error={}",
                    hour_str,
                    e
                )
            })?;
            results.push((hour_str, stats));
        }
        Ok(results)
    }

    // ---- Script daily deltas ----

    pub fn get_script_daily_delta(
        &self,
        code_hash: &[u8],
        hash_type: u8,
        is_type: bool,
        date_yyyymmdd: u32,
    ) -> anyhow::Result<Option<ScriptDailyDelta>> {
        let key = keys::encode_script_daily_key(code_hash, hash_type, is_type, date_yyyymmdd);
        match self.get_cf(self.cf_stats_script(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn put_script_daily_delta(
        &self,
        code_hash: &[u8],
        hash_type: u8,
        is_type: bool,
        date_yyyymmdd: u32,
        delta: &ScriptDailyDelta,
    ) -> anyhow::Result<()> {
        let key = keys::encode_script_daily_key(code_hash, hash_type, is_type, date_yyyymmdd);
        let value = bincode::serialize(delta)?;
        self.put_cf(self.cf_stats_script(), &key, &value)
    }

    pub fn list_script_daily_deltas(
        &self,
        code_hash: &[u8],
        hash_type: u8,
        is_type: bool,
    ) -> anyhow::Result<Vec<(u32, ScriptDailyDelta)>> {
        self.list_script_daily_deltas_in_range(code_hash, hash_type, is_type, None, None)
    }

    pub fn list_script_daily_deltas_in_range(
        &self,
        code_hash: &[u8],
        hash_type: u8,
        is_type: bool,
        from_date_yyyymmdd: Option<u32>,
        to_date_yyyymmdd: Option<u32>,
    ) -> anyhow::Result<Vec<(u32, ScriptDailyDelta)>> {
        let prefix = keys::encode_script_daily_prefix(code_hash, hash_type, is_type);
        let start_key = keys::encode_script_daily_key(
            code_hash,
            hash_type,
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
            let (_, _, _, date) = keys::decode_script_daily_key(&key);
            if let Some(to_date) = to_date_yyyymmdd {
                if date > to_date {
                    break;
                }
            }
            let delta: ScriptDailyDelta = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize script daily delta in list_script_daily_deltas_in_range: code_hash=0x{}, hash_type={}, is_type={}, date={}, error={}",
                    bytes_to_hex(code_hash),
                    hash_type,
                    is_type,
                    date,
                    e
                )
            })?;
            results.push((date, delta));
        }

        Ok(results)
    }

    /// List every script daily row of a code_hash across all hash_type forms
    /// and script kinds. Returns ((hash_type, is_type, date), delta) rows in
    /// key order.
    #[allow(clippy::type_complexity)]
    pub fn list_script_daily_deltas_by_code_hash(
        &self,
        code_hash: &[u8],
    ) -> anyhow::Result<Vec<((u8, bool, u32), ScriptDailyDelta)>> {
        let prefix = keys::encode_script_daily_code_hash_prefix(code_hash);
        let iter = self.iterator_cf(
            self.cf_stats_script(),
            rocksdb::IteratorMode::From(&prefix, rocksdb::Direction::Forward),
        );
        let mut results = Vec::new();

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate stats_script in list_script_daily_deltas_by_code_hash: {}",
                    e
                )
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            if key.len() != keys::SCRIPT_DAILY_KEY_SIZE {
                continue;
            }
            let (_, hash_type, is_type, date) = keys::decode_script_daily_key(&key);
            let delta: ScriptDailyDelta = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize script daily delta in list_script_daily_deltas_by_code_hash: code_hash=0x{}, hash_type={}, is_type={}, date={}, error={}",
                    bytes_to_hex(code_hash),
                    hash_type,
                    is_type,
                    date,
                    e
                )
            })?;
            results.push(((hash_type, is_type, date), delta));
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

    pub fn get_script_version(
        &self,
        version_hash: &[u8],
    ) -> anyhow::Result<Option<ScriptVersionInfo>> {
        match self.get_cf(self.cf_script_versions(), version_hash)? {
            Some(value) => {
                let info = bincode::deserialize(&value).map_err(|e| {
                    anyhow::anyhow!(
                        "failed to deserialize script version: version_hash=0x{}, error={}",
                        bytes_to_hex(version_hash),
                        e
                    )
                })?;
                Ok(Some(info))
            }
            None => Ok(None),
        }
    }

    pub fn put_script_version(
        &self,
        version_hash: &[u8],
        info: &ScriptVersionInfo,
    ) -> anyhow::Result<()> {
        let value = bincode::serialize(info)?;
        self.put_cf(self.cf_script_versions(), version_hash, &value)
    }

    pub fn list_script_versions(&self) -> anyhow::Result<Vec<(Vec<u8>, ScriptVersionInfo)>> {
        let iter = self.iterator_cf(self.cf_script_versions(), rocksdb::IteratorMode::Start);
        let mut results = Vec::new();

        for item in iter {
            let (key, value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate script_versions in list_script_versions: {}",
                    e
                )
            })?;
            let info: ScriptVersionInfo = bincode::deserialize(&value).map_err(|e| {
                anyhow::anyhow!(
                    "failed to deserialize script version in list_script_versions: version_hash=0x{}, error={}",
                    bytes_to_hex(&key),
                    e
                )
            })?;
            results.push((key.to_vec(), info));
        }

        Ok(results)
    }

    pub fn insert_script_version_by_label(
        &self,
        label_key: &str,
        version_hash: &[u8],
    ) -> anyhow::Result<()> {
        let key = keys::encode_script_version_by_label_key(label_key, version_hash);
        self.put_cf(self.cf_script_versions_by_label(), &key, &[])
    }

    pub fn delete_script_version_by_label(
        &self,
        label_key: &str,
        version_hash: &[u8],
    ) -> anyhow::Result<()> {
        let key = keys::encode_script_version_by_label_key(label_key, version_hash);
        self.delete_cf(self.cf_script_versions_by_label(), &key)
    }

    pub fn list_script_version_hashes_by_label(
        &self,
        label_key: &str,
    ) -> anyhow::Result<Vec<Vec<u8>>> {
        let prefix = keys::encode_script_version_by_label_prefix(label_key);
        let iter = self.prefix_iterator_cf(self.cf_script_versions_by_label(), &prefix);
        let mut results = Vec::new();

        for item in iter {
            let (key, _value) = item.map_err(|e| {
                anyhow::anyhow!(
                    "failed to iterate script_versions_by_label in prefix scan: {}",
                    e
                )
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            let (_label, version_hash) = keys::decode_script_version_by_label_key(&key);
            results.push(version_hash);
        }

        Ok(results)
    }

    /// Recompute the DAO daily snapshot for a single date from the canonical
    /// `dao_deposits` CF plus `block_headers` CF. Used by reorg rollback to
    /// rebuild the cutoff-date snapshot after a partial-day rollback.
    ///
    /// `date` is the target UTC+8 date (e.g., 2026-04-08).
    /// `end_block_inclusive` is the highest block number to include (typically
    /// the rollback target for partial-day reorgs, or the last block of the
    /// day for cross-day reorgs).
    ///
    /// Assumes:
    /// - `dao_deposits` CF is in its post-rollback normalized state (i.e.,
    ///   `repair_and_rebuild_dao_indexes` has already run).
    /// - `block_headers` CF still contains all headers for blocks up to
    ///   `end_block_inclusive`.
    pub fn recompute_dao_daily_snapshot_for_date(
        &self,
        date: chrono::NaiveDate,
        end_block_inclusive: i64,
        batch: &mut crate::batch::StoreBatch,
    ) -> anyhow::Result<()> {
        use crate::types::DaoDailySnapshot;
        use chrono::{FixedOffset, TimeZone};
        use ckbadger_common::CKB_UTC8_OFFSET;
        use std::collections::{HashMap, HashSet};

        // 1. Compute the UTC ms bounds of the target UTC+8 date.
        let utc8 = FixedOffset::east_opt(CKB_UTC8_OFFSET).unwrap();
        let day_start_naive = date
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| anyhow::anyhow!("invalid midnight for date {}", date))?;
        let day_start_utc = utc8
            .from_local_datetime(&day_start_naive)
            .single()
            .ok_or_else(|| anyhow::anyhow!("ambiguous day start for {}", date))?;
        let day_end_utc = day_start_utc + chrono::Duration::days(1);
        let day_start_ms = day_start_utc.timestamp_millis();
        let day_end_ms = day_end_utc.timestamp_millis();

        // 2. Locate the first block whose timestamp >= day_start_ms, bounded
        //    above by end_block_inclusive.
        let Some(day_start_block) = self.find_first_block_at_or_after_ms(day_start_ms)? else {
            return Ok(()); // no blocks at or after this date — nothing to recompute
        };
        // NOTE: find_first_block_at_or_after_ms reads the live DB (pre-rollback-
        // committed state), so day_start_block may be > end_block_inclusive if
        // the first block on this date is itself being rolled back. This early
        // return correctly handles that case — do NOT remove this guard.
        if day_start_block > end_block_inclusive {
            return Ok(()); // day_start is after our upper bound
        }

        // Walk forward to find the last block on this date <= end_block_inclusive.
        let mut day_end_block = day_start_block;
        {
            let mut bn = day_start_block + 1;
            while bn <= end_block_inclusive {
                let Some(h) = self.get_block_header(bn)? else {
                    break;
                };
                if h.timestamp >= day_end_ms {
                    break;
                }
                day_end_block = bn;
                bn += 1;
            }
        }

        // 3. Load previous day's snapshot as the starting baseline.
        let prev_date = date - chrono::Duration::days(1);
        let prev_date_key = prev_date.format("%Y%m%d").to_string();
        let prev_snap = self.get_dao_daily_snapshot(&prev_date_key)?;

        let (
            mut running_total_deposited,
            mut running_protocol_deposited,
            mut running_new_deposits,
            mut running_withdrawals,
            mut running_cumulative_deposit,
            mut running_cum_miner,
            mut running_total_depositors,
            mut running_cumulative_depositors,
        ) = match prev_snap.as_ref() {
            Some(p) => (
                p.total_deposited,
                p.protocol_deposited.unwrap_or(p.total_deposited),
                p.new_deposits,
                p.withdrawals,
                p.cumulative_deposit_amount,
                p.cum_miner_secondary,
                p.depositors_count,
                p.cumulative_depositors,
            ),
            None => (0i128, 0i128, 0i64, 0i64, 0i128, 0i128, 0i64, 0i64),
        };

        // RFC-0023 defines block N's issuance distribution from the DAO state
        // at the end of block N-1. Read that header directly instead of using
        // the previous daily snapshot as an approximate boundary value.
        let mut prev_dao_csu = if day_start_block > 0 {
            let previous_block = day_start_block - 1;
            let header = self.get_block_header(previous_block)?.ok_or_else(|| {
                anyhow::anyhow!(
                    "missing previous block header during DAO snapshot recompute: target_date={}, previous_block={}",
                    date,
                    previous_block
                )
            })?;
            Some(extract_dao_csu(&header.dao).ok_or_else(|| {
                anyhow::anyhow!(
                    "invalid previous DAO field during snapshot recompute: target_date={}, previous_block={}, dao_len={}",
                    date,
                    previous_block,
                    header.dao.len()
                )
            })?)
        } else {
            None
        };

        // 4. Scan dao_deposits CF once. Group entries by (deposit_block_number,
        //    withdraw_request_block, withdraw_block) for block-by-block walk.
        //    Also build the "prior_ever_deposited" set for cumulative_depositors.
        let mut by_deposit_block: HashMap<i64, Vec<crate::types::DaoDepositCacheEntry>> =
            HashMap::new();
        let mut by_phase1_block: HashMap<i64, Vec<crate::types::DaoDepositCacheEntry>> =
            HashMap::new();
        let mut by_phase2_block: HashMap<i64, Vec<crate::types::DaoDepositCacheEntry>> =
            HashMap::new();
        let mut prior_ever_deposited: HashSet<Vec<u8>> = HashSet::new();

        self.scan_dao_deposits(|_key, entry| {
            // For cumulative_depositors delta: any deposit with deposit_block_number
            // strictly before our day_start_block counts as "previously seen".
            if entry.deposit_block_number < day_start_block {
                prior_ever_deposited.insert(entry.lock_script_hash.clone());
            }
            if entry.deposit_block_number >= day_start_block
                && entry.deposit_block_number <= day_end_block
            {
                by_deposit_block
                    .entry(entry.deposit_block_number)
                    .or_default()
                    .push(entry.clone());
            }
            if let Some(wrb) = entry.withdraw_request_block {
                if wrb >= day_start_block && wrb <= day_end_block {
                    by_phase1_block.entry(wrb).or_default().push(entry.clone());
                }
            }
            if let Some(wb) = entry.withdraw_block {
                if wb >= day_start_block && wb <= day_end_block {
                    by_phase2_block.entry(wb).or_default().push(entry.clone());
                }
            }
            Ok(())
        })?;

        // Track unique lock hashes for daily_depositor_addresses.
        let mut daily_depositor_locks: HashSet<Vec<u8>> = HashSet::new();

        // 5. Walk blocks day_start_block..=day_end_block, applying deltas.
        let mut last_header_ar: u64 = 0;
        let mut last_header_c: i128 = 0;
        let mut last_header_s: i128 = 0;
        let mut last_header_u: i128 = 0;
        for block_num in day_start_block..=day_end_block {
            let header = self.get_block_header(block_num)?.ok_or_else(|| {
                anyhow::anyhow!(
                    "missing block header during DAO snapshot recompute: block_num={}",
                    block_num
                )
            })?;
            let (c, s, u) = extract_dao_csu(&header.dao).ok_or_else(|| {
                anyhow::anyhow!(
                    "invalid DAO field during recompute: block_num={}, dao_len={}",
                    block_num,
                    header.dao.len()
                )
            })?;
            let ar = extract_dao_ar(&header.dao).ok_or_else(|| {
                anyhow::anyhow!(
                    "invalid AR during recompute: block_num={}, dao_len={}",
                    block_num,
                    header.dao.len()
                )
            })?;
            last_header_ar = ar;
            last_header_c = c;
            last_header_s = s;
            last_header_u = u;

            // 5a. Deposits created in this block.
            if let Some(deposits) = by_deposit_block.get(&block_num) {
                for d in deposits {
                    running_total_deposited = running_total_deposited
                        .checked_add(d.capacity as i128)
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "total_deposited overflow during recompute: block_num={}",
                                block_num
                            )
                        })?;
                    running_cumulative_deposit = running_cumulative_deposit
                        .checked_add(d.capacity as i128)
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "cumulative_deposit_amount overflow during recompute: block_num={}",
                                block_num
                            )
                        })?;
                    running_protocol_deposited = running_protocol_deposited
                        .checked_add(d.capacity as i128)
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "protocol_deposited overflow during recompute: block_num={}",
                                block_num
                            )
                        })?;
                    running_new_deposits =
                        running_new_deposits.checked_add(1).ok_or_else(|| {
                            anyhow::anyhow!(
                                "new_deposits overflow during recompute: block_num={}",
                                block_num
                            )
                        })?;
                    daily_depositor_locks.insert(d.lock_script_hash.clone());
                    if !prior_ever_deposited.contains(&d.lock_script_hash) {
                        prior_ever_deposited.insert(d.lock_script_hash.clone());
                        running_cumulative_depositors = running_cumulative_depositors
                            .checked_add(1)
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                                    "cumulative_depositors overflow during recompute: block_num={}",
                                    block_num
                                )
                            })?;
                    }
                    running_total_depositors =
                        running_total_depositors.checked_add(1).ok_or_else(|| {
                            anyhow::anyhow!(
                                "depositors_count overflow during recompute: block_num={}",
                                block_num
                            )
                        })?;
                }
            }

            // 5b. Phase-1 (withdraw request) in this block. Subtract from active
            //     total_deposited but NOT from protocol_deposited (cell still locked).
            if let Some(phase1s) = by_phase1_block.get(&block_num) {
                for p in phase1s {
                    running_total_deposited = running_total_deposited
                        .checked_sub(p.capacity as i128)
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "total_deposited phase-1 underflow during recompute: block_num={}",
                                block_num
                            )
                        })?;
                    running_total_depositors =
                        running_total_depositors.checked_sub(1).ok_or_else(|| {
                            anyhow::anyhow!(
                                "depositors_count phase-1 underflow during recompute: block_num={}",
                                block_num
                            )
                        })?;
                }
            }

            // 5c. Phase-2 (withdraw completion) in this block. Increment withdrawals
            //     count, subtract from protocol_deposited, and accumulate claimed
            //     compensation for the secondary-issuance split.
            let claimed_compensation_in_block: i128 = by_phase2_block
                .get(&block_num)
                .map(|v| {
                    v.iter()
                        .filter_map(|e| e.compensation)
                        .map(i128::from)
                        .sum()
                })
                .unwrap_or(0);
            if let Some(phase2s) = by_phase2_block.get(&block_num) {
                for p in phase2s {
                    running_protocol_deposited = running_protocol_deposited
                        .checked_sub(p.capacity as i128)
                        .ok_or_else(|| anyhow::anyhow!(
                            "protocol_deposited phase-2 underflow during recompute: block_num={}", block_num
                        ))?;
                    running_withdrawals = running_withdrawals.checked_add(1).ok_or_else(|| {
                        anyhow::anyhow!(
                            "withdrawals overflow during recompute: block_num={}",
                            block_num
                        )
                    })?;
                }
            }

            // 5d. Exact miner portion for this block. RFC-0023 uses C/U from
            // the end of block N-1. DAO compensation is computed separately
            // from each deposit's lifecycle below.
            if let Some((prev_c, prev_s, prev_u)) = prev_dao_csu {
                let s_delta = s.checked_sub(prev_s).ok_or_else(|| {
                    anyhow::anyhow!(
                        "secondary_pool s_delta overflow during recompute: block_num={}, current_s={}, previous_s={}",
                        block_num,
                        s,
                        prev_s
                    )
                })?;
                let non_miner_delta = s_delta
                    .checked_add(claimed_compensation_in_block)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "non_miner_delta overflow during recompute: block_num={}",
                            block_num
                        )
                    })?;
                if non_miner_delta != 0 {
                    let miner = calculate_secondary_miner_delta(prev_c, prev_u, non_miner_delta)?;
                    running_cum_miner = running_cum_miner.checked_add(miner).ok_or_else(|| {
                        anyhow::anyhow!(
                            "cum_miner_secondary overflow during recompute: block_num={}",
                            block_num
                        )
                    })?;
                }
            }
            prev_dao_csu = Some((c, s, u));
        }

        // 6. Exact end-of-day compensation from the normalized DAO lifecycle.
        let compensation = if last_header_ar > 0 {
            self.compute_dao_compensation_breakdown_at(day_end_block, last_header_ar)?
        } else {
            crate::types::DaoCompensationBreakdown::default()
        };
        let total_compensation = compensation.total().ok_or_else(|| {
            anyhow::anyhow!(
                "DAO total compensation overflow during recompute: target_date={}, claimed={}, unclaimed={}",
                date,
                compensation.claimed,
                compensation.unclaimed
            )
        })?;
        let cumulative_treasury = last_header_s
            .checked_sub(compensation.active_unmade)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "DAO treasury subtraction overflow during recompute: target_date={}",
                    date
                )
            })?;
        if cumulative_treasury < 0 {
            anyhow::bail!(
                "active DAO interests exceed secondary pool during recompute: target_date={}, secondary_pool={}, active_unmade={}",
                date,
                last_header_s,
                compensation.active_unmade
            );
        }

        // 7. Write the rebuilt snapshot.
        let snapshot = DaoDailySnapshot {
            date: date.format("%Y-%m-%d").to_string(),
            total_deposited: running_total_deposited,
            depositors_count: running_total_depositors,
            new_deposits: running_new_deposits,
            withdrawals: running_withdrawals,
            compensation: compensation.claimed,
            cumulative_deposit_amount: running_cumulative_deposit,
            total_issuance: last_header_c,
            secondary_pool: last_header_s,
            occupied_capacity: last_header_u,
            cum_miner_secondary: running_cum_miner,
            cum_dao_compensation: total_compensation,
            cum_treasury: cumulative_treasury,
            unmade_dao_interests: compensation.active_unmade,
            unclaimed_compensation: compensation.unclaimed,
            cumulative_depositors: running_cumulative_depositors,
            daily_depositor_addresses: daily_depositor_locks.len() as i64,
            protocol_deposited: Some(running_protocol_deposited),
        };
        let key = keys::encode_stats_key(
            keys::stats_prefix::DAO_DAILY_SNAPSHOT,
            date.format("%Y%m%d").to_string().as_bytes(),
        );
        batch.put_stats(&key, &bincode::serialize(&snapshot)?);
        Ok(())
    }

    /// Binary-search `block_headers` to find the first block whose timestamp
    /// is >= `ms`. Returns None if no such block exists.
    fn find_first_block_at_or_after_ms(&self, ms: i64) -> anyhow::Result<Option<i64>> {
        let (tip, _) = match self.get_sync_tip_block()? {
            Some(x) => x,
            None => return Ok(None),
        };
        let mut lo: i64 = 0;
        let mut hi: i64 = tip;
        let mut result: Option<i64> = None;
        while lo <= hi {
            let mid = lo + (hi - lo) / 2;
            match self.get_block_header(mid)? {
                Some(h) => {
                    if h.timestamp >= ms {
                        result = Some(mid);
                        hi = mid - 1;
                    } else {
                        lo = mid + 1;
                    }
                }
                None => {
                    // Hole in block_headers — search above (higher blocks may
                    // still exist for dense CF).
                    lo = mid + 1;
                }
            }
        }
        Ok(result)
    }
}

fn extract_dao_csu(dao: &[u8]) -> Option<(i128, i128, i128)> {
    if dao.len() < 32 {
        return None;
    }
    let c = u64::from_le_bytes(dao[0..8].try_into().ok()?) as i128;
    let s = u64::from_le_bytes(dao[16..24].try_into().ok()?) as i128;
    let u = u64::from_le_bytes(dao[24..32].try_into().ok()?) as i128;
    Some((c, s, u))
}

fn extract_dao_ar(dao: &[u8]) -> Option<u64> {
    if dao.len() < 16 {
        return None;
    }
    Some(u64::from_le_bytes(dao[8..16].try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CkbadgerStore;

    #[test]
    fn test_activity_addr_set_encode_decode_roundtrip() {
        let addrs = std::collections::HashSet::from([[0xC3u8; 32], [0xA1u8; 32], [0xB2u8; 32]]);
        let encoded = encode_activity_addr_set(addrs.iter().copied());
        assert_eq!(encoded.len(), 96);
        // Deterministic sorted layout, regardless of iteration order.
        assert_eq!(&encoded[0..32], &[0xA1u8; 32]);
        assert_eq!(&encoded[32..64], &[0xB2u8; 32]);
        assert_eq!(&encoded[64..96], &[0xC3u8; 32]);
        let decoded = decode_activity_addr_set(&encoded, "20260210").unwrap();
        assert_eq!(decoded, addrs);
        assert_eq!(
            activity_addr_set_count(decoded.len(), "20260210").unwrap(),
            3
        );
    }

    #[test]
    fn test_activity_addr_set_encode_dedups_repeats() {
        let encoded = encode_activity_addr_set([[0xA1u8; 32], [0xA1u8; 32], [0xB2u8; 32]]);
        assert_eq!(
            encoded.len(),
            64,
            "repeated hashes must collapse to one row"
        );
        assert_eq!(
            decode_activity_addr_set(&encoded, "2026021015")
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn test_activity_addr_set_decode_rejects_partial_hash() {
        let mut raw = vec![0xA1u8; 32];
        raw.extend_from_slice(&[0xB2u8; 7]); // truncated trailing hash
        let err = decode_activity_addr_set(&raw, "20260210").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("corrupt activity addr set row")
                && msg.contains("20260210")
                && msg.contains("39"),
            "decode must fail fast with bucket + length context, got: {}",
            msg
        );
    }

    #[test]
    fn test_activity_addr_set_count_rejects_overflow() {
        let err = activity_addr_set_count(u32::MAX as usize + 1, "20260210").unwrap_err();
        assert!(err.to_string().contains("unique_address_count exceeds u32"));
    }

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
            last_processed_block: Some(100),
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
            owned_capacity_delta: 1_000_000_000_000,
            owned_knowledge_delta: 700_000_000_000,
        };
        let d2 = ScriptDailyDelta {
            owned_capacity_delta: -200_000_000_000,
            owned_knowledge_delta: -120_000_000_000,
        };
        store
            .put_script_daily_delta(&code_hash, 1, false, 20240115, &d1)
            .unwrap();
        store
            .put_script_daily_delta(&code_hash, 1, false, 20240116, &d2)
            .unwrap();

        let loaded = store
            .get_script_daily_delta(&code_hash, 1, false, 20240115)
            .unwrap()
            .unwrap();
        assert_eq!(loaded.owned_capacity_delta, d1.owned_capacity_delta);
        assert_eq!(loaded.owned_knowledge_delta, d1.owned_knowledge_delta);

        let listed = store
            .list_script_daily_deltas(&code_hash, 1, false)
            .unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].0, 20240115);
        assert_eq!(listed[1].0, 20240116);

        let ranged = store
            .list_script_daily_deltas_in_range(&code_hash, 1, false, Some(20240116), Some(20240116))
            .unwrap();
        assert_eq!(ranged.len(), 1);
        assert_eq!(ranged[0].0, 20240116);
    }

    #[test]
    fn test_script_daily_delta_rows_are_independent_per_hash_type() {
        // Regression (B8 root cause): two references sharing the same
        // code_hash bytes but using different hash_types must produce
        // independent daily rows — junk data-form deltas must not merge into
        // the type-form timeline.
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();
        let code_hash = vec![0x9B; 32];

        let type_form = ScriptDailyDelta {
            owned_capacity_delta: 500,
            owned_knowledge_delta: 300,
        };
        let data_form = ScriptDailyDelta {
            owned_capacity_delta: 98,
            owned_knowledge_delta: 98,
        };
        store
            .put_script_daily_delta(&code_hash, 1, false, 20240115, &type_form)
            .unwrap();
        store
            .put_script_daily_delta(&code_hash, 0, false, 20240115, &data_form)
            .unwrap();

        let type_rows = store
            .list_script_daily_deltas(&code_hash, 1, false)
            .unwrap();
        assert_eq!(type_rows.len(), 1);
        assert_eq!(type_rows[0].1.owned_capacity_delta, 500);

        let data_rows = store
            .list_script_daily_deltas(&code_hash, 0, false)
            .unwrap();
        assert_eq!(data_rows.len(), 1);
        assert_eq!(data_rows[0].1.owned_capacity_delta, 98);

        let all_rows = store
            .list_script_daily_deltas_by_code_hash(&code_hash)
            .unwrap();
        assert_eq!(all_rows.len(), 2);
        let keys: Vec<(u8, bool, u32)> = all_rows.iter().map(|(key, _)| *key).collect();
        assert!(keys.contains(&(0, false, 20240115)));
        assert!(keys.contains(&(1, false, 20240115)));
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

    #[test]
    fn test_script_version_and_label_index_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();
        let version_hash_a = vec![0x61; 32];
        let version_hash_b = vec![0x62; 32];

        let version = ScriptVersionInfo {
            version_hash: version_hash_a.clone(),
            name: Some("Default Lock".to_string()),
            category: Some("lock".to_string()),
            description: Some("mainnet default lock".to_string()),
            website: Some("https://nervos.org".to_string()),
            type_cells_count: 4,
            type_live_cells_count: 2,
            type_capacity_sum: 800,
            type_owned_capacity_sum: 400,
            type_used_capacity_sum: 500,
            type_owned_knowledge_sum: 220,
            ..Default::default()
        };

        store.put_script_version(&version_hash_a, &version).unwrap();
        store
            .insert_script_version_by_label("Default Lock", &version_hash_a)
            .unwrap();
        store
            .insert_script_version_by_label("Default Lock", &version_hash_b)
            .unwrap();

        let loaded = store.get_script_version(&version_hash_a).unwrap().unwrap();
        assert_eq!(loaded.name.as_deref(), Some("Default Lock"));
        assert_eq!(loaded.category.as_deref(), Some("lock"));

        let hashes = store
            .list_script_version_hashes_by_label("Default Lock")
            .unwrap();
        assert_eq!(hashes, vec![version_hash_a.clone(), version_hash_b.clone()]);

        store
            .delete_script_version_by_label("Default Lock", &version_hash_a)
            .unwrap();

        let hashes = store
            .list_script_version_hashes_by_label("Default Lock")
            .unwrap();
        assert_eq!(hashes, vec![version_hash_b]);

        let versions = store.list_script_versions().unwrap();
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].0, version_hash_a);
        assert_eq!(versions[0].1.name.as_deref(), Some("Default Lock"));
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
            unclaimed_compensation: 0,
            unmade_dao_interests: 0,
            cumulative_depositors: 0,
            daily_depositor_addresses: 0,
            protocol_deposited: None,
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
            unclaimed_compensation: 0,
            unmade_dao_interests: 0,
            cumulative_depositors: 0,
            daily_depositor_addresses: 0,
            protocol_deposited: None,
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
            unclaimed_compensation: 0,
            unmade_dao_interests: 0,
            cumulative_depositors: 0,
            daily_depositor_addresses: 0,
            protocol_deposited: None,
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
                unclaimed_compensation: 0,
                unmade_dao_interests: 0,
                cumulative_depositors: 0,
                daily_depositor_addresses: 0,
                protocol_deposited: None,
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
    fn test_dao_top_depositors_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();
        let depositors = DaoTopDepositors {
            tip_block_number: 100,
            depositors: vec![DaoTopDepositorEntry {
                lock_script_hash: vec![0xAA; 32],
                total_capacity: 1000_00000000,
                deposit_count: 3,
                average_deposit_ms: 5400.0,
            }],
        };
        store.put_dao_top_depositors(&depositors).unwrap();
        let loaded = store.get_dao_top_depositors().unwrap().unwrap();
        assert_eq!(loaded.tip_block_number, 100);
        assert_eq!(loaded.depositors.len(), 1);
        assert_eq!(loaded.depositors[0].total_capacity, 1000_00000000);
        assert_eq!(loaded.depositors[0].deposit_count, 3);
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
            object_count: 20,
            identity_count: 5,
            script_call_count: 0,
            unknown_count: 0,
            coinbase_count: 8640,
            unique_address_count: 500,
            total_ckb_moved: 100_000_000_000_000,
            script_counts: std::collections::HashMap::new(),
            protocol_action_counts: std::collections::HashMap::new(),
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

    #[test]
    fn test_get_hourly_activity_stats_missing_returns_none() {
        let (_dir, store) = open_test_store();
        let result = store.get_hourly_activity_stats("2026030912").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_put_and_get_hourly_activity_stats_roundtrip() {
        let (_dir, store) = open_test_store();
        let stats = DailyActivityStats {
            transfer_count: 42,
            total_ckb_moved: 100_00000000,
            unique_address_count: 5,
            ..Default::default()
        };
        store
            .put_hourly_activity_stats("2026030912", &stats)
            .unwrap();
        let got = store
            .get_hourly_activity_stats("2026030912")
            .unwrap()
            .unwrap();
        assert_eq!(got.transfer_count, 42);
        assert_eq!(got.total_ckb_moved, 100_00000000);
        assert_eq!(got.unique_address_count, 5);
    }

    #[test]
    fn test_list_hourly_activity_stats_since_returns_range() {
        let (_dir, store) = open_test_store();
        let s1 = DailyActivityStats {
            transfer_count: 10,
            ..Default::default()
        };
        let s2 = DailyActivityStats {
            transfer_count: 20,
            ..Default::default()
        };
        let s3 = DailyActivityStats {
            transfer_count: 30,
            ..Default::default()
        };
        store.put_hourly_activity_stats("2026030910", &s1).unwrap();
        store.put_hourly_activity_stats("2026030911", &s2).unwrap();
        store.put_hourly_activity_stats("2026030912", &s3).unwrap();

        // Query from hour 11 onwards
        let results = store
            .list_hourly_activity_stats_since("2026030911")
            .unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, "2026030911");
        assert_eq!(results[0].1.transfer_count, 20);
        assert_eq!(results[1].0, "2026030912");
        assert_eq!(results[1].1.transfer_count, 30);
    }
}

#[cfg(test)]
mod cell_distribution_tests {
    use super::*;
    use crate::CkbadgerStore;

    #[test]
    fn test_cell_distribution_put_get_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();

        let dist = DailyCellDistribution {
            size_bucket_counts: [10, 20, 30, 40, 50, 60],
            size_bucket_capacities: [1000, 2000, 3000, 4000, 5000, 6000],
        };

        store.put_cell_distribution("20240115", &dist).unwrap();

        let retrieved = store.get_cell_distribution("20240115").unwrap().unwrap();
        assert_eq!(retrieved.size_bucket_counts, [10, 20, 30, 40, 50, 60]);
        assert_eq!(
            retrieved.size_bucket_capacities,
            [1000, 2000, 3000, 4000, 5000, 6000]
        );

        // Non-existent date returns None
        assert!(store.get_cell_distribution("20240116").unwrap().is_none());
    }

    #[test]
    fn test_get_latest_cell_distribution_returns_most_recent() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();

        // Empty store returns None
        assert!(store.get_latest_cell_distribution().unwrap().is_none());

        let d1 = DailyCellDistribution {
            size_bucket_counts: [1, 0, 0, 0, 0, 0],
            ..Default::default()
        };
        let d2 = DailyCellDistribution {
            size_bucket_counts: [2, 0, 0, 0, 0, 0],
            ..Default::default()
        };
        let d3 = DailyCellDistribution {
            size_bucket_counts: [3, 0, 0, 0, 0, 0],
            ..Default::default()
        };

        // Insert out of order
        store.put_cell_distribution("20240115", &d2).unwrap();
        store.put_cell_distribution("20240113", &d1).unwrap();
        store.put_cell_distribution("20240117", &d3).unwrap();

        let (date, latest) = store.get_latest_cell_distribution().unwrap().unwrap();
        assert_eq!(date, "20240117");
        assert_eq!(latest.size_bucket_counts, [3, 0, 0, 0, 0, 0]); // most recent by key sort
    }

    #[test]
    fn test_address_cohort_put_get_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();

        let cohort = DailyAddressCohort {
            cohorts: vec![
                AddressCohortEntry {
                    cohort_month: "2024-01".to_string(),
                    used_capacity: 1_000_000,
                    total_balance: 5_000_000,
                },
                AddressCohortEntry {
                    cohort_month: "2024-02".to_string(),
                    used_capacity: 2_000_000,
                    total_balance: 8_000_000,
                },
            ],
        };

        store.put_address_cohort("20240215", &cohort).unwrap();

        let retrieved = store.get_address_cohort("20240215").unwrap().unwrap();
        assert_eq!(retrieved.cohorts.len(), 2);
        assert_eq!(retrieved.cohorts[0].cohort_month, "2024-01");
        assert_eq!(retrieved.cohorts[0].used_capacity, 1_000_000);
        assert_eq!(retrieved.cohorts[1].total_balance, 8_000_000);

        // Non-existent date returns None
        assert!(store.get_address_cohort("20240216").unwrap().is_none());
    }

    #[test]
    fn test_get_latest_address_cohort_returns_most_recent() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();

        // Empty store returns None
        assert!(store.get_latest_address_cohort().unwrap().is_none());

        let c1 = DailyAddressCohort {
            cohorts: vec![AddressCohortEntry {
                cohort_month: "2024-01".to_string(),
                used_capacity: 100,
                total_balance: 500,
            }],
        };
        let c2 = DailyAddressCohort {
            cohorts: vec![AddressCohortEntry {
                cohort_month: "2024-02".to_string(),
                used_capacity: 200,
                total_balance: 800,
            }],
        };

        store.put_address_cohort("20240115", &c1).unwrap();
        store.put_address_cohort("20240215", &c2).unwrap();

        let (date, latest) = store.get_latest_address_cohort().unwrap().unwrap();
        assert_eq!(date, "20240215");
        assert_eq!(latest.cohorts.len(), 1);
        assert_eq!(latest.cohorts[0].cohort_month, "2024-02");
    }

    #[test]
    fn test_cell_dist_tracker_state_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();

        // Initially none
        assert!(store.get_cell_dist_tracker_state().unwrap().is_none());

        let state = CellDistributionTrackerState {
            count_by_bucket: [10, 20, 30, 40, 50, 60],
            total_capacity_by_bucket: [1000, 2000, 3000, 4000, 5000, 6000],
            date_transitions: vec![(0, "20240101".to_string()), (100, "20240102".to_string())],
            last_snapshot_date: Some("20240102".to_string()),
            cohort_accum: vec![],
            last_processed_block: Some(100),
        };

        store.put_cell_dist_tracker_state(&state).unwrap();

        let retrieved = store.get_cell_dist_tracker_state().unwrap().unwrap();
        assert_eq!(retrieved.count_by_bucket, [10, 20, 30, 40, 50, 60]);
        assert_eq!(
            retrieved.total_capacity_by_bucket,
            [1000, 2000, 3000, 4000, 5000, 6000]
        );
        assert_eq!(retrieved.last_snapshot_date, Some("20240102".to_string()));
    }
}

#[cfg(test)]
mod dao_daily_snapshot_recompute_tests {
    use super::*;
    use crate::CkbadgerStore;

    /// Build a 32-byte DAO header field: C | AR | S | U, little-endian u64s.
    fn dao_field(c: u64, ar: u64, s: u64, u: u64) -> Vec<u8> {
        let mut dao = vec![0u8; 32];
        dao[0..8].copy_from_slice(&c.to_le_bytes());
        dao[8..16].copy_from_slice(&ar.to_le_bytes());
        dao[16..24].copy_from_slice(&s.to_le_bytes());
        dao[24..32].copy_from_slice(&u.to_le_bytes());
        dao
    }

    /// UTC millis for a UTC+8 wall-clock time on `date`.
    fn utc8_ms(date: chrono::NaiveDate, hour: u32, minute: u32) -> i64 {
        use chrono::{FixedOffset, TimeZone};
        FixedOffset::east_opt(ckbadger_common::CKB_UTC8_OFFSET)
            .unwrap()
            .from_local_datetime(&date.and_hms_opt(hour, minute, 0).unwrap())
            .single()
            .unwrap()
            .timestamp_millis()
    }

    fn header_with_dao(block: i64, timestamp: i64, dao: Vec<u8>) -> CachedBlockHeader {
        CachedBlockHeader {
            hash: vec![block as u8; 32],
            parent_hash: vec![block.saturating_sub(1) as u8; 32],
            timestamp,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1800,
            dao,
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
        }
    }

    fn active_dao_deposit(
        capacity: i64,
        occupied_capacity: i64,
        deposit_block_number: i64,
        deposit_ar: i64,
        lock_script_hash: Vec<u8>,
    ) -> DaoDepositCacheEntry {
        DaoDepositCacheEntry {
            capacity,
            occupied_capacity,
            deposit_block_number,
            deposit_timestamp: 0,
            lock_script_hash,
            deposit_ar,
            status: 0,
            withdraw_request_tx: None,
            withdraw_request_output_index: None,
            withdraw_request_block: None,
            withdraw_request_ar: None,
            withdraw_block: None,
            withdraw_tx: None,
            withdraw_to_output_index: None,
            compensation: None,
        }
    }

    const TEST_C: u64 = 100_000_000_000_000;
    const TEST_U: u64 = 20_000_000_000;

    #[test]
    fn test_recompute_dao_daily_snapshot_for_date_handles_day_start_block_zero() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();

        let date = chrono::NaiveDate::from_ymd_opt(2026, 3, 10).unwrap();
        let ar_deposit: u64 = 10_000_000_000_000_000;
        let ar_end: u64 = 10_100_000_000_000_000;
        let s: u64 = 50_000_000_000;

        // The whole day starts at block 0, so there is no previous block header
        // and no C/U baseline for the miner split.
        let mut batch = crate::batch::StoreBatch::new(&store);
        batch.put_block_header(
            0,
            &header_with_dao(
                0,
                utc8_ms(date, 0, 10),
                dao_field(TEST_C, ar_deposit, s, TEST_U),
            ),
        );
        batch.put_block_header(
            1,
            &header_with_dao(
                1,
                utc8_ms(date, 12, 0),
                dao_field(TEST_C, ar_deposit, s, TEST_U),
            ),
        );
        batch.put_block_header(
            2,
            &header_with_dao(
                2,
                utc8_ms(date, 23, 50),
                dao_field(TEST_C, ar_end, s, TEST_U),
            ),
        );
        let capacity = 100_000_000_000i64;
        let occupied = 10_200_000_000i64;
        batch.put_dao_deposit(
            &keys::encode_outpoint(&[0xAA; 32], 0),
            &active_dao_deposit(capacity, occupied, 1, ar_deposit as i64, vec![0xA1; 32]),
        );
        batch.commit().unwrap();
        store
            .set_sync_status(&SyncStatus {
                tip_block_number: 2,
                tip_block_hash: vec![2u8; 32],
                ..Default::default()
            })
            .unwrap();

        let mut recompute = crate::batch::StoreBatch::new(&store);
        store
            .recompute_dao_daily_snapshot_for_date(date, 2, &mut recompute)
            .unwrap();
        recompute.commit().unwrap();

        let snapshot = store.get_dao_daily_snapshot("20260310").unwrap().unwrap();
        let expected_unclaimed = i128::from(
            ckbadger_common::dao::calculate_dao_compensation_from_ar(
                capacity, occupied, ar_deposit, ar_end,
            )
            .unwrap(),
        );

        assert_eq!(snapshot.date, "2026-03-10");
        assert_eq!(snapshot.total_deposited, i128::from(capacity));
        assert_eq!(snapshot.protocol_deposited, Some(i128::from(capacity)));
        assert_eq!(snapshot.new_deposits, 1);
        assert_eq!(snapshot.withdrawals, 0);
        assert_eq!(snapshot.depositors_count, 1);
        assert_eq!(snapshot.cumulative_depositors, 1);
        assert_eq!(snapshot.daily_depositor_addresses, 1);
        assert_eq!(snapshot.total_issuance, i128::from(TEST_C));
        assert_eq!(snapshot.secondary_pool, i128::from(s));
        assert_eq!(snapshot.occupied_capacity, i128::from(TEST_U));
        // No previous block header exists, so no C/U baseline and therefore no
        // miner secondary is attributed for this day.
        assert_eq!(snapshot.cum_miner_secondary, 0);
        assert_eq!(snapshot.compensation, 0);
        assert_eq!(snapshot.unclaimed_compensation, expected_unclaimed);
        assert_eq!(snapshot.unmade_dao_interests, expected_unclaimed);
        assert_eq!(snapshot.cum_dao_compensation, expected_unclaimed);
        assert_eq!(
            snapshot.cum_treasury,
            i128::from(s) - expected_unclaimed,
            "treasury must be the end-of-day S minus active unmade interests"
        );
    }

    #[test]
    fn test_recompute_dao_daily_snapshot_for_date_uses_previous_block_csu_for_miner_split() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();

        let prev_date = chrono::NaiveDate::from_ymd_opt(2026, 3, 9).unwrap();
        let date = chrono::NaiveDate::from_ymd_opt(2026, 3, 10).unwrap();
        let ar_deposit: u64 = 10_000_000_000_000_000;
        let ar_end: u64 = 10_100_000_000_000_000;
        let s0: u64 = 50_000_000_000;
        let s1: u64 = s0 + 3_000_000;
        let s2: u64 = s1 + 5_000_000;

        let mut batch = crate::batch::StoreBatch::new(&store);
        // Block 0 lives on the previous date: it is the C/S/U baseline used by
        // block 1, not part of this day's walk.
        batch.put_block_header(
            0,
            &header_with_dao(
                0,
                utc8_ms(prev_date, 23, 50),
                dao_field(TEST_C, ar_deposit, s0, TEST_U),
            ),
        );
        batch.put_block_header(
            1,
            &header_with_dao(
                1,
                utc8_ms(date, 0, 10),
                dao_field(TEST_C, ar_deposit, s1, TEST_U),
            ),
        );
        batch.put_block_header(
            2,
            &header_with_dao(
                2,
                utc8_ms(date, 12, 0),
                dao_field(TEST_C, ar_end, s2, TEST_U),
            ),
        );
        let capacity = 100_000_000_000i64;
        let occupied = 10_200_000_000i64;
        batch.put_dao_deposit(
            &keys::encode_outpoint(&[0xBB; 32], 0),
            &active_dao_deposit(capacity, occupied, 1, ar_deposit as i64, vec![0xB1; 32]),
        );
        batch.commit().unwrap();
        store
            .set_sync_status(&SyncStatus {
                tip_block_number: 2,
                tip_block_hash: vec![2u8; 32],
                ..Default::default()
            })
            .unwrap();

        let mut recompute = crate::batch::StoreBatch::new(&store);
        store
            .recompute_dao_daily_snapshot_for_date(date, 2, &mut recompute)
            .unwrap();
        recompute.commit().unwrap();

        let snapshot = store.get_dao_daily_snapshot("20260310").unwrap().unwrap();

        // Block 1 uses block 0's C/U with S(1) - S(0); block 2 uses block 1's.
        let miner_block1 = calculate_secondary_miner_delta(
            i128::from(TEST_C),
            i128::from(TEST_U),
            i128::from(s1 - s0),
        )
        .unwrap();
        let miner_block2 = calculate_secondary_miner_delta(
            i128::from(TEST_C),
            i128::from(TEST_U),
            i128::from(s2 - s1),
        )
        .unwrap();
        assert!(miner_block1 > 0 && miner_block2 > 0);
        assert_eq!(snapshot.cum_miner_secondary, miner_block1 + miner_block2);
        assert_eq!(snapshot.secondary_pool, i128::from(s2));
        assert_eq!(snapshot.total_deposited, i128::from(capacity));

        let expected_unclaimed = i128::from(
            ckbadger_common::dao::calculate_dao_compensation_from_ar(
                capacity, occupied, ar_deposit, ar_end,
            )
            .unwrap(),
        );
        assert_eq!(snapshot.unclaimed_compensation, expected_unclaimed);
        assert_eq!(snapshot.cum_dao_compensation, expected_unclaimed);
        assert_eq!(snapshot.cum_treasury, i128::from(s2) - expected_unclaimed);
    }

    #[test]
    fn test_recompute_dao_daily_snapshot_for_date_fails_when_previous_block_header_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_test_unified(dir.path().to_str().unwrap()).unwrap();

        let date = chrono::NaiveDate::from_ymd_opt(2026, 3, 10).unwrap();
        let ar: u64 = 10_000_000_000_000_000;
        let s: u64 = 50_000_000_000;

        // No block 0: the day starts at block 1 and its C/S/U baseline is
        // unavailable. This is the fail-fast the rollback caller must not hit.
        let mut batch = crate::batch::StoreBatch::new(&store);
        batch.put_block_header(
            1,
            &header_with_dao(1, utc8_ms(date, 0, 10), dao_field(TEST_C, ar, s, TEST_U)),
        );
        batch.put_block_header(
            2,
            &header_with_dao(2, utc8_ms(date, 12, 0), dao_field(TEST_C, ar, s, TEST_U)),
        );
        batch.commit().unwrap();
        store
            .set_sync_status(&SyncStatus {
                tip_block_number: 2,
                tip_block_hash: vec![2u8; 32],
                ..Default::default()
            })
            .unwrap();

        let mut recompute = crate::batch::StoreBatch::new(&store);
        let error = store
            .recompute_dao_daily_snapshot_for_date(date, 2, &mut recompute)
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("missing previous block header during DAO snapshot recompute"),
            "unexpected error: {error}"
        );
    }
}
