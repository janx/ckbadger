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

    /// Delete all DAO daily snapshots and rebuild them from deposit history.
    ///
    /// For each deposit, creates timeline events:
    /// - deposit_block_number date: +capacity (deposit becomes active)
    /// - withdraw_request_block date: -capacity (Phase 1 withdrawal)
    ///
    /// Then walks through dates computing running totals to produce snapshots.
    pub fn rebuild_dao_daily_snapshots(&self) -> anyhow::Result<usize> {
        use std::collections::{BTreeMap, HashSet};

        // 1. Delete all existing DAO snapshots
        let prefix = [stats_prefix::DAO_DAILY_SNAPSHOT];
        let iter = self.prefix_iterator_cf(self.cf_stats(), &prefix);
        let mut delete_batch = crate::batch::StoreBatch::new(self);
        let mut deleted = 0usize;
        for item in iter.flatten() {
            let (key, _) = item;
            if !key.starts_with(&prefix) {
                break;
            }
            delete_batch.delete_stats(&key);
            deleted += 1;
        }
        if deleted > 0 {
            delete_batch.commit()?;
        }

        // 2. Load all deposits
        let all_deposits = self.list_dao_deposits()?;
        if all_deposits.is_empty() {
            return Ok(0);
        }

        // 3. Build block_number -> date cache from block headers
        let mut block_dates: std::collections::HashMap<i64, chrono::NaiveDate> =
            std::collections::HashMap::new();

        let resolve_date = |block_num: i64,
                            cache: &mut std::collections::HashMap<i64, chrono::NaiveDate>|
         -> Option<chrono::NaiveDate> {
            if let Some(d) = cache.get(&block_num) {
                return Some(*d);
            }
            None
        };

        // Pre-load all needed block headers
        let mut needed_blocks: HashSet<i64> = HashSet::new();
        for (_, entry) in &all_deposits {
            needed_blocks.insert(entry.deposit_block_number);
            if let Some(wb) = entry.withdraw_request_block {
                needed_blocks.insert(wb);
            }
        }

        for block_num in &needed_blocks {
            if let Ok(Some(header)) = self.get_block_header(*block_num) {
                let ts_secs = header.timestamp / 1000;
                if let Some(dt) = chrono::DateTime::from_timestamp(ts_secs, 0) {
                    block_dates.insert(*block_num, dt.date_naive());
                }
            }
        }

        // 4. Build timeline events: (date, capacity_delta, is_deposit, lock_script_hash)
        struct Event {
            date: chrono::NaiveDate,
            capacity_delta: i128,
            lock_script_hash: Vec<u8>,
            is_new_deposit: bool,
            is_withdrawal_complete: bool,
            compensation: i128,
        }

        let mut events: Vec<Event> = Vec::new();

        for (_, entry) in &all_deposits {
            let deposit_date = match resolve_date(entry.deposit_block_number, &mut block_dates) {
                Some(d) => d,
                None => continue,
            };

            // Deposit event: +capacity
            events.push(Event {
                date: deposit_date,
                capacity_delta: entry.capacity as i128,
                lock_script_hash: entry.lock_script_hash.clone(),
                is_new_deposit: true,
                is_withdrawal_complete: false,
                compensation: 0,
            });

            // Phase 1 withdrawal: -capacity (matches official explorer behavior)
            if entry.status >= 1 {
                if let Some(wb) = entry.withdraw_request_block {
                    let wd = match resolve_date(wb, &mut block_dates) {
                        Some(d) => d,
                        None => continue,
                    };
                    events.push(Event {
                        date: wd,
                        capacity_delta: -(entry.capacity as i128),
                        lock_script_hash: entry.lock_script_hash.clone(),
                        is_new_deposit: false,
                        is_withdrawal_complete: entry.status == 2,
                        compensation: if entry.status == 2 {
                            entry.compensation.unwrap_or(0) as i128
                        } else {
                            0
                        },
                    });
                }
            }
        }

        // Sort events by date
        events.sort_by_key(|e| e.date);

        // 5. Collect last DAO field (C, S) per date from block headers.
        // Iterate all headers in forward order; the last header per date wins.
        let mut daily_dao_cs: std::collections::HashMap<chrono::NaiveDate, (i128, i128)> =
            std::collections::HashMap::new();
        {
            let iter = self.iterator_cf(self.cf_block_headers(), rocksdb::IteratorMode::Start);
            for item in iter.flatten() {
                let (_, value) = item;
                if let Ok(header) = bincode::deserialize::<CachedBlockHeader>(&value) {
                    let ts_secs = header.timestamp / 1000;
                    if let Some(dt) = chrono::DateTime::from_timestamp(ts_secs, 0) {
                        let d = dt.date_naive();
                        if header.dao.len() >= 24 {
                            let c =
                                u64::from_le_bytes(header.dao[0..8].try_into().unwrap_or([0; 8]))
                                    as i128;
                            let s =
                                u64::from_le_bytes(header.dao[16..24].try_into().unwrap_or([0; 8]))
                                    as i128;
                            daily_dao_cs.insert(d, (c, s));
                        }
                    }
                }
            }
        }

        // 6. Walk timeline building daily snapshots
        // Group events by date
        let mut daily_events: BTreeMap<chrono::NaiveDate, Vec<&Event>> = BTreeMap::new();
        for event in &events {
            daily_events.entry(event.date).or_default().push(event);
        }

        // Also ensure dates with DAO header data but no deposit events get snapshots
        for date in daily_dao_cs.keys() {
            daily_events.entry(*date).or_default();
        }

        let mut running_total: i128 = 0;
        let mut cumulative_deposit_amount: i128 = 0;
        let mut active_depositors: std::collections::HashMap<Vec<u8>, i64> =
            std::collections::HashMap::new(); // lock_hash -> count of active deposits
        let mut cumulative_deposits: i64 = 0;
        let mut cumulative_withdrawals: i64 = 0;
        let mut cumulative_compensation: i128 = 0;
        let mut written = 0usize;

        let mut batch = crate::batch::StoreBatch::new(self);

        for (date, day_events) in &daily_events {
            for ev in day_events {
                running_total += ev.capacity_delta;

                if ev.is_new_deposit {
                    cumulative_deposits += 1;
                    cumulative_deposit_amount += ev.capacity_delta; // always positive for deposits
                    *active_depositors
                        .entry(ev.lock_script_hash.clone())
                        .or_insert(0) += 1;
                } else {
                    // Withdrawal
                    let count = active_depositors
                        .entry(ev.lock_script_hash.clone())
                        .or_insert(0);
                    *count -= 1;
                    if *count <= 0 {
                        active_depositors.remove(&ev.lock_script_hash);
                    }
                    if ev.is_withdrawal_complete {
                        cumulative_withdrawals += 1;
                        cumulative_compensation += ev.compensation;
                    }
                }
            }

            let depositors_count = active_depositors.len() as i64;
            let date_str = date.format("%Y%m%d").to_string();
            let key = crate::keys::encode_stats_key(
                crate::keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT,
                date_str.as_bytes(),
            );

            let (total_issuance, secondary_pool) =
                daily_dao_cs.get(date).copied().unwrap_or((0, 0));

            let snapshot = DaoDailySnapshot {
                date: date.format("%Y-%m-%d").to_string(),
                total_deposited: running_total,
                depositors_count,
                new_deposits: cumulative_deposits,
                withdrawals: cumulative_withdrawals,
                compensation: cumulative_compensation,
                cumulative_deposit_amount,
                total_issuance,
                secondary_pool,
            };

            let value = bincode::serialize(&snapshot)?;
            batch.put_stats(&key, &value);
            written += 1;

            // Commit in batches of 1000
            if written.is_multiple_of(1000) {
                batch.commit()?;
                batch = crate::batch::StoreBatch::new(self);
            }
        }

        if !written.is_multiple_of(1000) {
            batch.commit()?;
        }

        Ok(written)
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
