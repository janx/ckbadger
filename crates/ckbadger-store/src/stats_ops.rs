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

    // ---- Average block time rebuild ----

    /// Rebuild avg_block_time_ms in DailyStats by iterating all block headers
    /// and computing the mean block time per day.
    #[allow(clippy::manual_is_multiple_of)]
    pub fn rebuild_avg_block_times(&self) -> anyhow::Result<usize> {
        use std::collections::BTreeMap;

        // Iterate all block headers in order, compute block time diffs
        let mut daily_times: BTreeMap<chrono::NaiveDate, (i64, i64)> = BTreeMap::new(); // (sum_ms, count)
        let mut prev_timestamp: Option<i64> = None;

        let iter = self.iterator_cf(self.cf_block_headers(), rocksdb::IteratorMode::Start);
        for item in iter.flatten() {
            let (_, value) = item;
            if let Ok(header) = bincode::deserialize::<CachedBlockHeader>(&value) {
                if let Some(prev_ts) = prev_timestamp {
                    let block_time_ms = header.timestamp - prev_ts;
                    if block_time_ms >= 0 {
                        let ts_secs = header.timestamp / 1000;
                        if let Some(dt) = chrono::DateTime::from_timestamp(ts_secs, 0) {
                            let date = dt.date_naive();
                            let entry = daily_times.entry(date).or_insert((0, 0));
                            entry.0 += block_time_ms;
                            entry.1 += 1;
                        }
                    }
                }
                prev_timestamp = Some(header.timestamp);
            }
        }

        // Update each day's DailyStats with the computed average
        let mut updated = 0usize;
        let mut batch = crate::batch::StoreBatch::new(self);

        for (date, (sum_ms, count)) in &daily_times {
            if *count <= 0 {
                continue;
            }
            let date_str = date.format("%Y%m%d").to_string();
            let key = crate::keys::encode_stats_key(stats_prefix::DAILY, date_str.as_bytes());
            if let Some(val) = self.get_cf(self.cf_stats(), &key)? {
                if let Ok(mut s) = bincode::deserialize::<DailyStats>(&val) {
                    s.avg_block_time_ms = Some(*sum_ms / *count);
                    let value = bincode::serialize(&s)?;
                    batch.put_stats(&key, &value);
                    updated += 1;

                    if updated % 1000 == 0 {
                        batch.commit()?;
                        batch = crate::batch::StoreBatch::new(self);
                    }
                }
            }
        }

        if updated % 1000 != 0 {
            batch.commit()?;
        }

        Ok(updated)
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
    #[allow(clippy::manual_is_multiple_of)]
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

        // 5. Collect last DAO field (C, S, U) per date AND compute per-block
        //    secondary issuance breakdown from consecutive block headers.
        //    CKB primary issuance: 191,780,821,917,808 shannons per epoch in era 0,
        //    halving every 8760 epochs.
        const PRIMARY_PER_EPOCH_ERA0: i128 = 191_780_821_917_808;
        const HALVING_INTERVAL: i64 = 8760;

        // (C, S, U) per date — last block of each day wins
        let mut daily_dao_csu: std::collections::HashMap<chrono::NaiveDate, (i128, i128, i128)> =
            std::collections::HashMap::new();
        // Per-day secondary issuance accumulators: (miner, S_delta)
        // S_delta = non-miner = dao + treasury
        let mut daily_secondary: BTreeMap<chrono::NaiveDate, (i128, i128)> = BTreeMap::new();
        {
            let iter = self.iterator_cf(self.cf_block_headers(), rocksdb::IteratorMode::Start);
            let mut prev_c: Option<i128> = None;
            let mut prev_s: Option<i128> = None;
            for item in iter.flatten() {
                let (_, value) = item;
                if let Ok(header) = bincode::deserialize::<CachedBlockHeader>(&value) {
                    let ts_secs = header.timestamp / 1000;
                    if let Some(dt) = chrono::DateTime::from_timestamp(ts_secs, 0) {
                        let d = dt.date_naive();
                        if header.dao.len() >= 32 {
                            let c =
                                u64::from_le_bytes(header.dao[0..8].try_into().unwrap_or([0; 8]))
                                    as i128;
                            let s =
                                u64::from_le_bytes(header.dao[16..24].try_into().unwrap_or([0; 8]))
                                    as i128;
                            let u =
                                u64::from_le_bytes(header.dao[24..32].try_into().unwrap_or([0; 8]))
                                    as i128;
                            daily_dao_csu.insert(d, (c, s, u));

                            // Compute per-block secondary issuance breakdown
                            if let (Some(pc), Some(ps)) = (prev_c, prev_s) {
                                let c_delta = c - pc;
                                let s_delta = (s - ps).max(0);
                                // primary_per_block = primary_per_epoch / epoch_length
                                let era = (header.epoch_number / HALVING_INTERVAL) as u32;
                                let primary_per_epoch = PRIMARY_PER_EPOCH_ERA0 >> era;
                                let epoch_len = header.epoch_length.max(1) as i128;
                                let primary = primary_per_epoch / epoch_len;
                                let secondary = (c_delta - primary).max(0);
                                let miner = (secondary - s_delta).max(0);
                                let entry = daily_secondary.entry(d).or_insert((0, 0));
                                entry.0 += miner;
                                entry.1 += s_delta;
                            }
                            prev_c = Some(c);
                            prev_s = Some(s);
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
        for date in daily_dao_csu.keys() {
            daily_events.entry(*date).or_default();
        }

        let mut running_total: i128 = 0;
        let mut cumulative_deposit_amount: i128 = 0;
        let mut active_depositors: std::collections::HashMap<Vec<u8>, i64> =
            std::collections::HashMap::new(); // lock_hash -> count of active deposits
        let mut cumulative_deposits: i64 = 0;
        let mut cumulative_withdrawals: i64 = 0;
        let mut cumulative_compensation: i128 = 0;
        let mut cum_miner: i128 = 0;
        let mut cum_dao: i128 = 0;
        let mut cum_treasury: i128 = 0;
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

            // Accumulate secondary issuance breakdown for this day
            if let Some(&(daily_miner, daily_non_miner)) = daily_secondary.get(date) {
                cum_miner += daily_miner;
                // Split non-miner into dao and treasury using dao_ratio
                let (c, _s, u) = daily_dao_csu.get(date).copied().unwrap_or((0, 0, 0));
                let denom = (c - u).max(1);
                let deposited = running_total.max(0);
                let daily_dao_share = daily_non_miner * deposited / denom;
                let daily_treasury_share = daily_non_miner - daily_dao_share;
                cum_dao += daily_dao_share;
                cum_treasury += daily_treasury_share;
            }

            let depositors_count = active_depositors.len() as i64;
            let date_str = date.format("%Y%m%d").to_string();
            let key = crate::keys::encode_stats_key(
                crate::keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT,
                date_str.as_bytes(),
            );

            let (total_issuance, secondary_pool, occupied_capacity) =
                daily_dao_csu.get(date).copied().unwrap_or((0, 0, 0));

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
                occupied_capacity,
                cum_miner_secondary: cum_miner,
                cum_dao_compensation: cum_dao,
                cum_treasury,
            };

            let value = bincode::serialize(&snapshot)?;
            batch.put_stats(&key, &value);
            written += 1;

            // Commit in batches of 1000
            if written % 1000 == 0 {
                batch.commit()?;
                batch = crate::batch::StoreBatch::new(self);
            }
        }

        if written % 1000 != 0 {
            batch.commit()?;
        }

        Ok(written)
    }

    // ---- HODL wave snapshots ----

    pub fn put_hodl_wave(&self, date: &str, wave: &DailyHodlWave) -> anyhow::Result<()> {
        let key = keys::encode_stats_key(stats_prefix::HODL_WAVE, date.as_bytes());
        let value = bincode::serialize(wave)?;
        self.put_cf(self.cf_stats(), &key, &value)
    }

    pub fn get_hodl_wave(&self, date: &str) -> anyhow::Result<Option<DailyHodlWave>> {
        let key = keys::encode_stats_key(stats_prefix::HODL_WAVE, date.as_bytes());
        match self.get_cf(self.cf_stats(), &key)? {
            Some(value) => Ok(Some(bincode::deserialize(&value)?)),
            None => Ok(None),
        }
    }

    pub fn list_hodl_waves(&self) -> anyhow::Result<Vec<(String, DailyHodlWave)>> {
        let prefix = [stats_prefix::HODL_WAVE];
        let iter = self.prefix_iterator_cf(self.cf_stats(), &prefix);
        let mut results = Vec::new();

        for item in iter.flatten() {
            let (key, value) = item;
            if !key.starts_with(&prefix) {
                break;
            }
            if let Ok(wave) = bincode::deserialize::<DailyHodlWave>(&value) {
                let date = String::from_utf8_lossy(&key[1..]).to_string();
                results.push((date, wave));
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CkbadgerStore;

    #[test]
    fn test_hodl_wave_put_get_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path().to_str().unwrap()).unwrap();

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
        let store = CkbadgerStore::open(dir.path().to_str().unwrap()).unwrap();

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
    fn test_hodl_tracker_state_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path().to_str().unwrap()).unwrap();

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
}
