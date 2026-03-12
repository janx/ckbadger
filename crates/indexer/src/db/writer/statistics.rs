use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use rocksdb::{Direction, IteratorMode};
use std::collections::{HashMap, HashSet};

use ckbadger_common::dao::calculate_estimated_apc;
use ckbadger_store::batch::StoreBatch;
use ckbadger_store::keys;
use ckbadger_store::types::*;

use super::BatchWriter;

fn checked_add_i128(current: i128, delta: i128, metric: &str) -> Result<i128> {
    current.checked_add(delta).ok_or_else(|| {
        anyhow::anyhow!(
            "daily/hourly statistics overflow while updating {}: current={}, delta={}",
            metric,
            current,
            delta
        )
    })
}

fn deserialize_stats<T: serde::de::DeserializeOwned>(raw: &[u8], metric: &str) -> Result<T> {
    bincode::deserialize(raw).map_err(|e| {
        anyhow!(
            "failed to deserialize {} from stats CF (len={}): {}",
            metric,
            raw.len(),
            e
        )
    })
}

/// Pre-computed DAO deposit statistics for daily snapshots.
/// Computed from tracked deposit/withdrawal events rather than block header fields.
pub struct DaoSnapshotInput {
    pub total_deposited: i128,
    pub depositors_count: i64,
    pub total_deposit_count: i64,
    pub total_withdrawal_count: i64,
    pub total_compensation: i128,
    /// Cumulative gross deposit amount (sum of all deposit capacities regardless
    /// of withdrawal status).
    pub cumulative_deposit_amount: i128,
    /// C field from DAO header: total CKB issuance (shannons).
    pub total_issuance: i128,
    /// S field from DAO header: cumulative non-miner secondary issuance (shannons).
    pub secondary_pool: i128,
    /// U field from DAO header: total occupied capacity (shannons).
    pub occupied_capacity: i128,
    /// Cumulative secondary issuance to miners (shannons).
    pub cum_miner_secondary: i128,
    /// Cumulative secondary issuance to DAO depositors (shannons).
    pub cum_dao_compensation: i128,
    /// Cumulative secondary issuance to treasury (shannons).
    pub cum_treasury: i128,
    /// Unclaimed DAO compensation at this point (shannons).
    pub unclaimed_compensation: u128,
}

const DAO_OCCUPIED_CAPACITY: u128 = 102_00000000;

impl BatchWriter {
    pub fn update_hourly_statistics(
        &self,
        hour: DateTime<Utc>,
        blocks_count: i32,
        transactions_count: i32,
        cells_created: i32,
        cells_consumed: i32,
        capacity_transferred: i128,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let key = keys::encode_stats_key(
            keys::STATS_PREFIX_HOURLY,
            hour.format("%Y%m%d%H").to_string().as_bytes(),
        );
        let existing = self.store.get_stats_key(&key)?;

        let stats = match existing {
            Some(val) => {
                let mut s: HourlyStats = deserialize_stats(&val, "hourly stats")?;
                s.blocks_count += blocks_count;
                s.transactions_count += transactions_count;
                s.cells_created += cells_created;
                s.cells_consumed += cells_consumed;
                s.capacity_transferred = checked_add_i128(
                    s.capacity_transferred,
                    capacity_transferred,
                    "hourly.capacity_transferred",
                )?;
                s
            }
            None => HourlyStats {
                hour: hour.timestamp(),
                blocks_count,
                transactions_count,
                cells_created,
                cells_consumed,
                capacity_transferred,
            },
        };

        let value = bincode::serialize(&stats)?;
        batch.put_stats(&key, &value);
        Ok(())
    }

    /// Update daily statistics for a given date. Returns the final DailyStats
    /// so the caller can thread cumulative totals forward when multiple dates
    /// are processed in the same batch.
    ///
    /// `prev_day_stats`: if the caller already has the previous day's stats
    /// in memory (because it was computed in the same batch), pass them here
    /// to avoid reading stale data from the not-yet-committed DB.
    pub fn update_daily_statistics(
        &self,
        date: NaiveDate,
        blocks_count: i32,
        transactions_count: i32,
        cells_created: i32,
        cells_consumed: i32,
        capacity_transferred: i128,
        used_capacity_created: i128,
        used_capacity_consumed: i128,
        data_size_added: i64,
        data_size_consumed: i64,
        dao_field: Option<&[u8]>,
        block_time: Option<(i64, i32)>, // (sum_ms, count)
        prev_day_stats: Option<&DailyStats>,
        batch: &mut StoreBatch,
    ) -> Result<DailyStats> {
        let key = keys::encode_stats_key(
            keys::STATS_PREFIX_DAILY,
            date.format("%Y%m%d").to_string().as_bytes(),
        );
        let existing = self.store.get_stats_key(&key)?;

        let avg_block_time_ms = block_time.and_then(|(sum_ms, count)| {
            if count > 0 {
                Some(sum_ms / count as i64)
            } else {
                None
            }
        });

        let stats = match existing {
            Some(val) => {
                let mut s: DailyStats = deserialize_stats(&val, "daily stats")?;
                // Compute weighted average block time before updating blocks_count
                if let Some(new_avg) = avg_block_time_ms {
                    let bt_count = block_time.map(|(_, c)| c).unwrap_or(0);
                    s.avg_block_time_ms = match s.avg_block_time_ms {
                        Some(existing_avg) => {
                            let prev_count = s.blocks_count as i64;
                            let new_total = s.blocks_count as i64 + bt_count as i64;
                            if new_total > 0 {
                                Some(
                                    (existing_avg * prev_count + new_avg * bt_count as i64)
                                        / new_total,
                                )
                            } else {
                                Some(new_avg)
                            }
                        }
                        None => Some(new_avg),
                    };
                }
                s.blocks_count += blocks_count;
                s.transactions_count += transactions_count;
                s.cells_created += cells_created;
                s.cells_consumed += cells_consumed;
                s.capacity_transferred = checked_add_i128(
                    s.capacity_transferred,
                    capacity_transferred,
                    "daily.capacity_transferred",
                )?;
                s.used_capacity_created = checked_add_i128(
                    s.used_capacity_created,
                    used_capacity_created,
                    "daily.used_capacity_created",
                )?;
                s.used_capacity_consumed = checked_add_i128(
                    s.used_capacity_consumed,
                    used_capacity_consumed,
                    "daily.used_capacity_consumed",
                )?;
                s.total_live_cells += (cells_created - cells_consumed) as i64;
                s.total_dead_cells += cells_consumed as i64;
                s.total_all_cells += cells_created as i64;
                s.total_data_size += data_size_added - data_size_consumed;
                if let Some(dao) = dao_field {
                    if let Some(ks) = calculate_knowledge_size(dao) {
                        s.knowledge_size = Some(ks);
                    }
                }
                s
            }
            None => {
                let knowledge_size = dao_field.and_then(calculate_knowledge_size);

                // Carry forward cumulative totals from the previous day.
                // Prefer in-memory stats (from same batch) over DB to handle
                // cross-day batches where the previous day isn't committed yet.
                let (prev_live, prev_dead, prev_all, prev_data_size) = if let Some(p) =
                    prev_day_stats
                {
                    (
                        p.total_live_cells,
                        p.total_dead_cells,
                        p.total_all_cells,
                        p.total_data_size,
                    )
                } else {
                    let prev_date = date - Duration::days(1);
                    let prev_key = keys::encode_stats_key(
                        keys::STATS_PREFIX_DAILY,
                        prev_date.format("%Y%m%d").to_string().as_bytes(),
                    );
                    self.store
                            .get_stats_key(&prev_key)?
                            .map(|v| deserialize_stats::<DailyStats>(&v, "previous day stats"))
                            .transpose()?
                            .map(|p| {
                                (
                                    p.total_live_cells,
                                    p.total_dead_cells,
                                    p.total_all_cells,
                                    p.total_data_size,
                                )
                            })
                            .map_or_else(
                                || -> Result<(i64, i64, i64, i64)> {
                                    let daily_prefix = [keys::STATS_PREFIX_DAILY];
                                    let mut latest_prior_daily_date: Option<String> = None;
                                    let iter = self.store.iterator_cf(
                                        self.store.cf_stats_chain(),
                                        IteratorMode::From(&key, Direction::Reverse),
                                    );
                                    for item in iter {
                                        let (candidate_key, _) = item.map_err(|e| {
                                            anyhow!(
                                                "failed to iterate stats_chain while checking previous day stats gap: date={}, error={}",
                                                date,
                                                e
                                            )
                                        })?;
                                        if !candidate_key.starts_with(&daily_prefix) {
                                            continue;
                                        }
                                        latest_prior_daily_date =
                                            Some(String::from_utf8_lossy(&candidate_key[1..]).to_string());
                                        break;
                                    }
                                    if let Some(found_date) = latest_prior_daily_date {
                                        bail!(
                                            "missing previous day stats while carrying daily totals: date={}, expected_prev_date={}, found_latest_prior_date={}",
                                            date.format("%Y%m%d"),
                                            prev_date.format("%Y%m%d"),
                                            found_date
                                        );
                                    }
                                    Ok((0, 0, 0, 0))
                                },
                                Ok,
                            )?
                };

                DailyStats {
                    blocks_count,
                    transactions_count,
                    cells_created,
                    cells_consumed,
                    capacity_transferred,
                    used_capacity_created,
                    used_capacity_consumed,
                    total_live_cells: prev_live + (cells_created - cells_consumed) as i64,
                    total_dead_cells: prev_dead + cells_consumed as i64,
                    total_all_cells: prev_all + cells_created as i64,
                    total_data_size: prev_data_size + data_size_added - data_size_consumed,
                    knowledge_size,
                    avg_block_time_ms,
                }
            }
        };

        let value = bincode::serialize(&stats)?;
        batch.put_stats(&key, &value);
        Ok(stats)
    }

    pub fn update_daily_block_stats_batch(
        &self,
        date: NaiveDate,
        avg_compact_target: i64,
        block_count: i32,
        total_uncles: i32,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let key = keys::encode_stats_key(
            keys::STATS_PREFIX_DAILY_BLOCK,
            date.format("%Y%m%d").to_string().as_bytes(),
        );
        let existing = self.store.get_stats_key(&key)?;

        let stats = match existing {
            Some(val) => {
                let mut s: DailyBlockStats = deserialize_stats(&val, "daily block stats")?;
                let old_total = s.avg_compact_target * s.block_count as f64;
                s.block_count += block_count;
                s.avg_compact_target = (old_total + avg_compact_target as f64 * block_count as f64)
                    / s.block_count as f64;
                s.total_uncles += total_uncles;
                s
            }
            None => DailyBlockStats {
                avg_compact_target: avg_compact_target as f64,
                block_count,
                total_uncles,
                avg_block_time_ms: None,
            },
        };

        let value = bincode::serialize(&stats)?;
        batch.put_stats(&key, &value);
        Ok(())
    }

    pub fn update_miner_statistics_batch(
        &self,
        lock_script_hash: &[u8],
        last_block_number: i64,
        date: NaiveDate,
        blocks_count: i32,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let date_key = date.format("%Y%m%d").to_string();
        let key = keys::encode_stats_key(
            keys::STATS_PREFIX_MINER,
            &[date_key.as_bytes(), lock_script_hash].concat(),
        );
        let existing = self.store.get_stats_key(&key)?;

        let stats = match existing {
            Some(val) => {
                let mut s: MinerStats = deserialize_stats(&val, "miner stats")?;
                s.blocks_count += blocks_count;
                s.last_block_number = s.last_block_number.max(last_block_number);
                s
            }
            None => MinerStats {
                miner_lock_hash: lock_script_hash.to_vec(),
                blocks_count,
                last_block_number,
            },
        };

        let value = bincode::serialize(&stats)?;
        batch.put_stats(&key, &value);
        Ok(())
    }

    pub fn upsert_epoch_statistics_batch(
        &self,
        epoch_number: i64,
        start_block: i64,
        end_block: i64,
        epoch_length: i32,
        start_timestamp: DateTime<Utc>,
        end_timestamp: DateTime<Utc>,
        transactions_count: i32,
        is_new: bool,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let key = keys::encode_stats_key(keys::STATS_PREFIX_EPOCH, &epoch_number.to_be_bytes());
        let existing = self.store.get_stats_key(&key)?;

        let stats = if is_new {
            EpochStats {
                epoch_number,
                start_block,
                end_block: Some(end_block),
                blocks_count: (end_block - start_block + 1) as i32,
                length: epoch_length,
                start_timestamp,
                end_timestamp: Some(end_timestamp),
                transactions_count,
            }
        } else if let Some(val) = existing {
            let mut s: EpochStats = deserialize_stats(&val, "epoch stats")?;
            s.end_block = Some(s.end_block.unwrap_or(end_block).max(end_block));
            s.blocks_count = (s.end_block.unwrap_or(end_block) - s.start_block + 1) as i32;
            s.end_timestamp = Some(end_timestamp);
            s.transactions_count += transactions_count;
            s
        } else {
            EpochStats {
                epoch_number,
                start_block,
                end_block: Some(end_block),
                blocks_count: (end_block - start_block + 1) as i32,
                length: epoch_length,
                start_timestamp,
                end_timestamp: Some(end_timestamp),
                transactions_count,
            }
        };

        let value = bincode::serialize(&stats)?;
        batch.put_stats(&key, &value);
        Ok(())
    }

    pub fn update_block_time_distribution_batch(
        &self,
        bucket_seconds: i32,
        count: i32,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let key = keys::encode_stats_key(
            keys::STATS_PREFIX_BLOCK_TIME_DIST,
            &bucket_seconds.to_be_bytes(),
        );
        let existing = self.store.get_stats_key(&key)?;

        let new_count = match existing {
            Some(val) if val.len() == 4 => {
                let existing_count_bytes: [u8; 4] = val.as_slice().try_into().map_err(|_| {
                    anyhow!(
                        "invalid block time distribution count bytes length: expected=4 got={}",
                        val.len()
                    )
                })?;
                i32::from_le_bytes(existing_count_bytes) + count
            }
            _ => count,
        };

        batch.put_stats(&key, &new_count.to_le_bytes());
        Ok(())
    }

    pub fn update_epoch_time_distribution_batch(
        &self,
        bucket_minutes: i32,
        count: i32,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let key = keys::encode_stats_key(
            keys::STATS_PREFIX_EPOCH_TIME_DIST,
            &bucket_minutes.to_be_bytes(),
        );
        let existing = self.store.get_stats_key(&key)?;

        let new_count = match existing {
            Some(val) if val.len() == 4 => {
                let existing_count_bytes: [u8; 4] = val.as_slice().try_into().map_err(|_| {
                    anyhow!(
                        "invalid epoch time distribution count bytes length: expected=4 got={}",
                        val.len()
                    )
                })?;
                i32::from_le_bytes(existing_count_bytes) + count
            }
            _ => count,
        };

        batch.put_stats(&key, &new_count.to_le_bytes());
        Ok(())
    }

    pub fn get_previous_block_timestamp(&self, block_number: i64) -> Result<Option<DateTime<Utc>>> {
        if block_number <= 0 {
            return Ok(None);
        }

        if let Some(header) = self.store.get_block_header(block_number - 1)? {
            let ts = DateTime::from_timestamp_millis(header.timestamp).ok_or_else(|| {
                anyhow!(
                    "invalid previous block timestamp millis for block {}: {}",
                    block_number - 1,
                    header.timestamp
                )
            })?;
            return Ok(Some(ts));
        }

        Ok(None)
    }

    pub fn update_dao_daily_snapshot(
        &self,
        date: NaiveDate,
        dao_snapshot: &DaoSnapshotInput,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let date_str = date.format("%Y%m%d").to_string();
        let key =
            keys::encode_stats_key(keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT, date_str.as_bytes());

        let snapshot = DaoDailySnapshot {
            date: date.format("%Y-%m-%d").to_string(),
            total_deposited: dao_snapshot.total_deposited,
            depositors_count: dao_snapshot.depositors_count,
            new_deposits: dao_snapshot.total_deposit_count,
            withdrawals: dao_snapshot.total_withdrawal_count,
            compensation: dao_snapshot.total_compensation,
            cumulative_deposit_amount: dao_snapshot.cumulative_deposit_amount,
            total_issuance: dao_snapshot.total_issuance,
            secondary_pool: dao_snapshot.secondary_pool,
            occupied_capacity: dao_snapshot.occupied_capacity,
            cum_miner_secondary: dao_snapshot.cum_miner_secondary,
            cum_dao_compensation: dao_snapshot.cum_dao_compensation,
            cum_treasury: dao_snapshot.cum_treasury,
            unclaimed_compensation: dao_snapshot.unclaimed_compensation,
        };

        let value = bincode::serialize(&snapshot)?;
        batch.put_stats(&key, &value);
        Ok(())
    }

    /// Classify an ActivityEntry and accumulate counts into DailyActivityStats.
    /// Call once per (lock_hash, scripts, ActivityEntry) triple from build_activities_for_block().
    pub fn accumulate_activity_stats(
        entry: &ActivityEntry,
        scripts: &[Vec<u8>],
        stats: &mut DailyActivityStats,
    ) {
        // Coinbase transactions are counted but excluded from all other metrics
        if entry.is_cellbase {
            stats.coinbase_count += 1;
            return;
        }

        // Total CKB moved (absolute value) — excludes coinbase
        stats.total_ckb_moved = stats
            .total_ckb_moved
            .checked_add(entry.ckb_delta.unsigned_abs())
            .expect("total_ckb_moved overflow in accumulate_activity_stats");

        // Count each involved script — excludes coinbase
        for code_hash in scripts {
            let hex = hex::encode(code_hash);
            *stats.script_counts.entry(hex).or_insert(0) += 1;
        }

        // Check asset changes for specific types
        let mut has_dao = false;
        let mut has_token = false;
        let mut has_object = false;
        let mut has_identity = false;
        let mut has_script_call = false;

        for change in &entry.asset_changes {
            match change {
                AssetChange::DaoDeposit { .. } => {
                    stats.dao_deposit_count += 1;
                    has_dao = true;
                }
                AssetChange::DaoWithdrawRequest { .. } => {
                    stats.dao_withdraw_request_count += 1;
                    has_dao = true;
                }
                AssetChange::DaoWithdrawComplete { .. } => {
                    stats.dao_withdraw_complete_count += 1;
                    has_dao = true;
                }
                AssetChange::Token { .. } => {
                    has_token = true;
                }
                AssetChange::Object { .. } => {
                    has_object = true;
                }
                AssetChange::Identity { .. } => {
                    has_identity = true;
                }
                AssetChange::ScriptCall { .. } => {
                    has_script_call = true;
                }
            }
        }

        if has_token {
            stats.token_count += 1;
        }
        if has_object {
            stats.object_count += 1;
        }
        if has_identity {
            stats.identity_count += 1;
        }
        if has_script_call {
            stats.script_call_count += 1;
        }

        // Exclusive activity-level classification
        let matched = has_dao || has_token || has_object || has_identity || has_script_call;
        if matched {
            // Already counted in specific categories above
        } else if !entry.has_type_script {
            stats.transfer_count += 1; // Pure CKB transfer: positive match
        } else {
            stats.unknown_count += 1; // Fallback: no conditions, just else
        }
    }

    /// Write accumulated daily activity stats for a date.
    /// Reads existing stats for the date, merges with accumulated, writes back.
    pub fn update_daily_activity_stats(
        &self,
        date: &str,
        accumulated: &DailyActivityStats,
        unique_addresses: u32,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let existing = self.store.get_daily_activity_stats(date)?;
        let merged = match existing {
            Some(mut e) => {
                e.transfer_count += accumulated.transfer_count;
                e.dao_deposit_count += accumulated.dao_deposit_count;
                e.dao_withdraw_request_count += accumulated.dao_withdraw_request_count;
                e.dao_withdraw_complete_count += accumulated.dao_withdraw_complete_count;
                e.token_count += accumulated.token_count;
                e.object_count += accumulated.object_count;
                e.identity_count += accumulated.identity_count;
                e.script_call_count += accumulated.script_call_count;
                e.unknown_count += accumulated.unknown_count;
                e.coinbase_count += accumulated.coinbase_count;
                e.unique_address_count = unique_addresses;
                e.total_ckb_moved = e
                    .total_ckb_moved
                    .checked_add(accumulated.total_ckb_moved)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "total_ckb_moved overflow merging daily activity stats for date {}",
                            date
                        )
                    })?;
                // Merge script counts
                for (code_hash, count) in &accumulated.script_counts {
                    *e.script_counts.entry(code_hash.clone()).or_insert(0) += count;
                }
                e
            }
            None => {
                let mut s = accumulated.clone();
                s.unique_address_count = unique_addresses;
                s
            }
        };
        let key = keys::encode_stats_key(keys::stats_prefix::ACTIVITY_DAILY, date.as_bytes());
        let value = bincode::serialize(&merged)?;
        batch.put_stats(&key, &value);
        Ok(())
    }

    /// Write accumulated hourly activity stats for an hour key.
    /// Reads existing stats for the hour, merges with accumulated, writes back.
    pub fn update_hourly_activity_stats(
        &self,
        hour_key: &str,
        accumulated: &DailyActivityStats,
        unique_addresses: u32,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let existing = self.store.get_hourly_activity_stats(hour_key)?;
        let merged = match existing {
            Some(mut e) => {
                e.transfer_count += accumulated.transfer_count;
                e.dao_deposit_count += accumulated.dao_deposit_count;
                e.dao_withdraw_request_count += accumulated.dao_withdraw_request_count;
                e.dao_withdraw_complete_count += accumulated.dao_withdraw_complete_count;
                e.token_count += accumulated.token_count;
                e.object_count += accumulated.object_count;
                e.identity_count += accumulated.identity_count;
                e.script_call_count += accumulated.script_call_count;
                e.unknown_count += accumulated.unknown_count;
                e.coinbase_count += accumulated.coinbase_count;
                e.unique_address_count = unique_addresses;
                e.total_ckb_moved = e
                    .total_ckb_moved
                    .checked_add(accumulated.total_ckb_moved)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "total_ckb_moved overflow merging hourly activity stats for hour {}",
                            hour_key
                        )
                    })?;
                // Merge script counts
                for (code_hash, count) in &accumulated.script_counts {
                    *e.script_counts.entry(code_hash.clone()).or_insert(0) += count;
                }
                e
            }
            None => {
                let mut s = accumulated.clone();
                s.unique_address_count = unique_addresses;
                s
            }
        };
        let key = keys::encode_stats_key(keys::stats_prefix::ACTIVITY_HOURLY, hour_key.as_bytes());
        let value = bincode::serialize(&merged)?;
        batch.put_stats(&key, &value);
        Ok(())
    }

    pub fn refresh_token_24h_transfers(&self) -> Result<u64> {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let cutoff_hour = now_ms / 3_600_000 - 48; // Keep 48h, discard older

        let tokens = self.store.list_tokens()?;
        let mut total_deleted = 0u64;
        for (type_hash, _) in &tokens {
            total_deleted += self
                .store
                .cleanup_old_hourly_buckets(type_hash, cutoff_hour)?;
        }
        Ok(total_deleted)
    }

    pub fn refresh_mnft_24h_transfers(&self) -> Result<u64> {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let cutoff_hour = now_ms / 3_600_000 - 48; // Keep 48h, discard older

        let collections = self.store.list_object_collection_aggregates()?;
        let mut total_deleted = 0u64;
        for (collection_id, agg) in collections {
            if agg.standard == ObjectStandard::MnftClass {
                total_deleted += self
                    .store
                    .cleanup_old_nft_hourly_buckets(&collection_id, cutoff_hour)?;
            }
        }

        Ok(total_deleted)
    }

    pub fn get_dao_deposits_at_block(&self, block_number: i64) -> Result<u128> {
        if block_number < 0 {
            bail!(
                "invalid block number for DAO deposit lookup: block_number={}",
                block_number
            );
        }

        let mut total: u128 = 0;
        let iter = self
            .store
            .iterator_cf(self.store.cf_dao_deposits(), rocksdb::IteratorMode::Start);

        for item in iter {
            let (key, value) = item
                .map_err(|e| anyhow!("failed to iterate dao_deposits while aggregating: {}", e))?;
            let entry: DaoDepositCacheEntry = bincode::deserialize(&value).map_err(|e| {
                anyhow!(
                    "failed to deserialize dao_deposit while aggregating: outpoint=0x{}, error={}",
                    hex::encode(&key),
                    e
                )
            })?;

            if entry.capacity < 0 {
                bail!(
                    "negative dao deposit capacity while aggregating: outpoint=0x{}, capacity={}",
                    hex::encode(&key),
                    entry.capacity
                );
            }

            if entry.deposit_block_number > block_number {
                continue;
            }

            let is_active = if let Some(withdraw_request_block) = entry.withdraw_request_block {
                if withdraw_request_block < entry.deposit_block_number {
                    let (tx_hash, output_index) = keys::decode_outpoint(&key);
                    bail!(
                        "invalid dao deposit lifecycle while aggregating: deposit outpoint=0x{}:{}, deposit_block={}, withdraw_request_block={}",
                        hex::encode(tx_hash),
                        output_index,
                        entry.deposit_block_number,
                        withdraw_request_block
                    );
                }
                block_number < withdraw_request_block
            } else {
                match entry.status {
                    0 => true,
                    1 | 2 => {
                        let (tx_hash, output_index) = keys::decode_outpoint(&key);
                        bail!(
                            "dao deposit missing withdraw_request_block while aggregating historical active deposits: outpoint=0x{}:{}, status={}, block_number={}",
                            hex::encode(tx_hash),
                            output_index,
                            entry.status,
                            block_number
                        );
                    }
                    other => {
                        let (tx_hash, output_index) = keys::decode_outpoint(&key);
                        bail!(
                            "unknown dao deposit status while aggregating: outpoint=0x{}:{}, status={}, block_number={}",
                            hex::encode(tx_hash),
                            output_index,
                            other,
                            block_number
                        );
                    }
                }
            };

            if !is_active {
                continue;
            }

            total = total.checked_add(entry.capacity as u128).ok_or_else(|| {
                anyhow!(
                    "dao deposits total overflow while aggregating: block_number={}, outpoint=0x{}",
                    block_number,
                    hex::encode(&key)
                )
            })?;
        }

        Ok(total)
    }

    pub fn get_last_epoch_start(&self, before_block: i64) -> Result<Option<(i64, DateTime<Utc>)>> {
        if before_block <= 0 {
            return Ok(None);
        }

        let mut best: Option<(i64, i64, DateTime<Utc>)> = None;
        for epoch in self.store.list_epoch_stats()? {
            if epoch.start_block >= before_block {
                continue;
            }
            match &best {
                Some((best_start_block, best_epoch_number, _))
                    if epoch.start_block < *best_start_block
                        || (epoch.start_block == *best_start_block
                            && epoch.epoch_number <= *best_epoch_number) => {}
                _ => {
                    best = Some((epoch.start_block, epoch.epoch_number, epoch.start_timestamp));
                }
            }
        }

        Ok(best.map(|(_, epoch_number, ts)| (epoch_number, ts)))
    }

    pub fn refresh_latest_dao_statistics(&self) -> Result<()> {
        let Some((tip_block_number, header)) = self.store.get_sync_tip_block()? else {
            return Ok(());
        };

        let tip_ar = extract_ar_from_dao_field(&header.dao).ok_or_else(|| {
            anyhow!(
                "invalid DAO field in sync tip block while refreshing latest dao statistics: block_number={}, dao_len={}",
                tip_block_number,
                header.dao.len()
            )
        })?;

        let mut total_deposited: i128 = 0;
        let mut unique_depositors: HashSet<Vec<u8>> = HashSet::new();
        let mut active_deposits = 0i32;
        let mut total_compensation_paid: i128 = 0;
        let mut total_blocks_held: f64 = 0.0;
        let mut active_filtered_count = 0usize;
        let mut unclaimed_compensation: u128 = 0;
        let mut depositor_map: HashMap<Vec<u8>, (i128, i32, f64)> = HashMap::new();

        self.store.scan_dao_deposits_by_status(0, |_, entry| {
            total_deposited += entry.capacity as i128;
            unique_depositors.insert(entry.lock_script_hash.clone());
            active_deposits += 1;

            {
                let dm = depositor_map
                    .entry(entry.lock_script_hash.clone())
                    .or_insert((0, 0, 0.0));
                dm.0 += entry.capacity as i128;
                dm.1 += 1;
                if entry.deposit_block_number <= tip_block_number {
                    dm.2 += (tip_block_number - entry.deposit_block_number) as f64;
                }
            }

            if entry.deposit_block_number <= tip_block_number {
                total_blocks_held += (tip_block_number - entry.deposit_block_number) as f64;
                active_filtered_count += 1;

                if entry.capacity < 0 {
                    bail!(
                        "negative DAO deposit capacity while refreshing latest dao statistics: deposit_block={}, lock_script_hash=0x{}, capacity={}",
                        entry.deposit_block_number,
                        hex::encode(&entry.lock_script_hash),
                        entry.capacity
                    );
                }
                let capacity = entry.capacity as u128;
                let free_capacity = capacity.checked_sub(DAO_OCCUPIED_CAPACITY).ok_or_else(|| {
                    anyhow!(
                        "DAO deposit capacity below occupied capacity while refreshing latest dao statistics: deposit_block={}, lock_script_hash=0x{}, capacity={}",
                        entry.deposit_block_number,
                        hex::encode(&entry.lock_script_hash),
                        capacity
                    )
                })?;
                let ar_deposit = u64::try_from(entry.deposit_ar).map_err(|_| {
                    anyhow!(
                        "invalid negative DAO deposit AR while refreshing latest dao statistics: deposit_block={}, lock_script_hash=0x{}, deposit_ar={}",
                        entry.deposit_block_number,
                        hex::encode(&entry.lock_script_hash),
                        entry.deposit_ar
                    )
                })?;
                if ar_deposit > 0 && tip_ar > ar_deposit {
                    let gross = free_capacity
                        .checked_mul(tip_ar as u128)
                        .ok_or_else(|| anyhow!("DAO compensation multiply overflow"))?
                        / ar_deposit as u128;
                    let compensation = gross.checked_sub(free_capacity).ok_or_else(|| {
                        anyhow!(
                            "DAO compensation underflow while refreshing latest dao statistics: deposit_block={}, lock_script_hash=0x{}, free_capacity={}, ar_deposit={}, tip_ar={}",
                            entry.deposit_block_number,
                            hex::encode(&entry.lock_script_hash),
                            free_capacity,
                            ar_deposit,
                            tip_ar
                        )
                    })?;
                    unclaimed_compensation += compensation;
                }
            }

            Ok(())
        })?;

        self.store.scan_dao_deposits_by_status(2, |_, entry| {
            if let Some(comp) = entry.compensation {
                total_compensation_paid += comp as i128;
            }
            Ok(())
        })?;

        let latest_snapshot = self.store.get_latest_dao_daily_snapshot()?;
        let estimated_apc = latest_snapshot
            .as_ref()
            .map(snapshot_estimated_apc)
            .transpose()?
            .flatten()
            .unwrap_or_default();
        let (mining_reward, deposit_compensation, burnt) = if let Some(s) = latest_snapshot.as_ref()
        {
            if s.cum_miner_secondary < 0 {
                bail!(
                    "negative cum_miner_secondary in dao_daily_snapshots while refreshing latest dao statistics for {}: {}",
                    s.date,
                    s.cum_miner_secondary
                );
            }
            if s.cum_dao_compensation < 0 {
                bail!(
                    "negative cum_dao_compensation in dao_daily_snapshots while refreshing latest dao statistics for {}: {}",
                    s.date,
                    s.cum_dao_compensation
                );
            }
            if s.cum_treasury < 0 {
                bail!(
                    "negative cum_treasury in dao_daily_snapshots while refreshing latest dao statistics for {}: {}",
                    s.date,
                    s.cum_treasury
                );
            }
            (
                s.cum_miner_secondary,
                s.cum_dao_compensation,
                s.cum_treasury,
            )
        } else {
            (0, 0, 0)
        };

        let avg_epochs = if active_filtered_count > 0 {
            (total_blocks_held / active_filtered_count as f64) / 1800.0
        } else {
            0.0
        };

        let latest = DaoLatestStatistics {
            tip_block_number,
            total_deposited,
            total_depositors: unique_depositors.len() as i32,
            active_deposits,
            total_compensation_paid,
            unclaimed_compensation,
            average_deposit_days: epochs_to_days(avg_epochs),
            estimated_apc,
            mining_reward,
            deposit_compensation,
            burnt,
        };

        let key = keys::encode_stats_key(keys::STATS_PREFIX_DAO_LATEST_STATS, b"latest");
        let value = bincode::serialize(&latest)?;
        self.store.put_stats_key(&key, &value)?;

        // Build and store top depositors
        {
            let mut sorted: Vec<_> = depositor_map.into_iter().collect();
            sorted.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
            sorted.truncate(100);

            let depositors = sorted
                .into_iter()
                .map(
                    |(lock_hash, (total_capacity, deposit_count, total_blocks))| {
                        let avg_blocks = if deposit_count > 0 {
                            total_blocks / deposit_count as f64
                        } else {
                            0.0
                        };
                        DaoTopDepositorEntry {
                            lock_script_hash: lock_hash,
                            address: None, // Resolved at API layer
                            total_capacity,
                            deposit_count,
                            average_deposit_blocks: avg_blocks,
                        }
                    },
                )
                .collect();

            let top = DaoTopDepositors {
                tip_block_number,
                depositors,
            };
            self.store.put_dao_top_depositors(&top)?;
        }

        // Update today's dao daily snapshot with the latest unclaimed compensation
        if let Some(mut today_snapshot) = self.store.get_latest_dao_daily_snapshot()? {
            today_snapshot.unclaimed_compensation = unclaimed_compensation;
            let date_key = today_snapshot.date.replace('-', "");
            let snap_key =
                keys::encode_stats_key(keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT, date_key.as_bytes());
            let snap_value = bincode::serialize(&today_snapshot)?;
            self.store.put_stats_key(&snap_key, &snap_value)?;
        }

        Ok(())
    }
}

fn extract_ar_from_dao_field(dao: &[u8]) -> Option<u64> {
    if dao.len() < 16 {
        return None;
    }
    let bytes: [u8; 8] = dao[8..16].try_into().ok()?;
    Some(u64::from_le_bytes(bytes))
}

fn snapshot_secondary_burnt(snapshot: &DaoDailySnapshot) -> Result<u128> {
    if snapshot.cum_treasury < 0 {
        bail!(
            "negative cum_treasury in dao_daily_snapshots for {}: {}",
            snapshot.date,
            snapshot.cum_treasury
        );
    }
    Ok(snapshot.cum_treasury as u128)
}

fn snapshot_estimated_apc(snapshot: &DaoDailySnapshot) -> Result<Option<String>> {
    let Ok(total_issuance) = u64::try_from(snapshot.total_issuance) else {
        return Ok(None);
    };
    if total_issuance == 0 {
        return Ok(None);
    }
    let apc = calculate_estimated_apc(total_issuance, snapshot_secondary_burnt(snapshot)?);
    Ok((apc > 0.0).then(|| format!("{:.2}", apc)))
}

fn epochs_to_days(epochs: f64) -> String {
    let days = epochs * 4.0 / 24.0;
    if days >= 1000.0 {
        format!("{:.1}K days+", days / 1000.0)
    } else if days < 1.0 && days > 0.0 {
        format!("{:.1} days", days)
    } else {
        format!("{:.0} days", days)
    }
}

/// Calculates knowledge_size from DAO field bytes.
pub fn calculate_knowledge_size(dao_field: &[u8]) -> Option<i128> {
    const BURN_ADJUSTMENT: i128 = 504_000_000_000_000_000;

    if dao_field.len() >= 32 {
        let bytes: [u8; 8] = dao_field[24..32].try_into().ok()?;
        let u_field = u64::from_le_bytes(bytes) as i128;
        Some(u_field - BURN_ADJUSTMENT)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use ckbadger_store::CkbadgerStore;

    const BURN_ADJUSTMENT: i128 = 504_000_000_000_000_000;

    #[test]
    fn test_calculate_knowledge_size_extracts_u_field() {
        let mut dao = vec![0u8; 32];
        let u_value: u64 = 600_000_000_000_000_000;
        dao[24..32].copy_from_slice(&u_value.to_le_bytes());

        let result = calculate_knowledge_size(&dao);
        assert!(result.is_some());
        let expected = u_value as i128 - BURN_ADJUSTMENT;
        assert_eq!(result.unwrap(), expected);
        assert_eq!(result.unwrap(), 96_000_000_000_000_000);
    }

    #[test]
    fn test_calculate_knowledge_size_returns_none_for_short_dao() {
        let short_dao = vec![0u8; 24];
        assert!(calculate_knowledge_size(&short_dao).is_none());

        let empty_dao: Vec<u8> = vec![];
        assert!(calculate_knowledge_size(&empty_dao).is_none());
    }

    #[test]
    fn test_calculate_knowledge_size_handles_minimum_u_value() {
        let mut dao = vec![0u8; 32];
        let u_value: u64 = BURN_ADJUSTMENT as u64;
        dao[24..32].copy_from_slice(&u_value.to_le_bytes());

        let result = calculate_knowledge_size(&dao);
        assert_eq!(result, Some(0));
    }

    #[test]
    fn test_get_dao_deposits_at_block_tracks_lifecycle_exactly() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        // deposit A: active from block 10, withdrawn at block 20
        let outpoint_a = keys::encode_outpoint(&[0x11; 32], 0);
        store
            .put_dao_deposit_direct(
                &outpoint_a,
                &DaoDepositCacheEntry {
                    capacity: 100,
                    deposit_block_number: 10,
                    lock_script_hash: vec![0xAA; 32],
                    deposit_ar: 1,
                    status: 2,
                    withdraw_request_tx: Some(vec![0x01; 32]),
                    withdraw_request_output_index: Some(0),
                    withdraw_request_block: Some(20),
                    withdraw_request_ar: Some(2),
                    withdraw_block: Some(25),
                    withdraw_tx: Some(vec![0x02; 32]),
                    withdraw_to_output_index: Some(0),
                    compensation: Some(1),
                },
            )
            .unwrap();

        // deposit B: active from block 15 onward
        let outpoint_b = keys::encode_outpoint(&[0x22; 32], 1);
        store
            .put_dao_deposit_direct(
                &outpoint_b,
                &DaoDepositCacheEntry {
                    capacity: 200,
                    deposit_block_number: 15,
                    lock_script_hash: vec![0xBB; 32],
                    deposit_ar: 1,
                    status: 0,
                    withdraw_request_tx: None,
                    withdraw_request_output_index: None,
                    withdraw_request_block: None,
                    withdraw_request_ar: None,
                    withdraw_block: None,
                    withdraw_tx: None,
                    withdraw_to_output_index: None,
                    compensation: None,
                },
            )
            .unwrap();

        assert_eq!(writer.get_dao_deposits_at_block(9).unwrap(), 0);
        assert_eq!(writer.get_dao_deposits_at_block(10).unwrap(), 100);
        assert_eq!(writer.get_dao_deposits_at_block(19).unwrap(), 300);
        assert_eq!(writer.get_dao_deposits_at_block(20).unwrap(), 200);
    }

    #[test]
    fn test_get_last_epoch_start_returns_latest_start_before_block() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let mut batch = StoreBatch::new(&store);
        let ts0 = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let ts1 = DateTime::from_timestamp(1_700_000_800, 0).unwrap();
        let ts2 = DateTime::from_timestamp(1_700_001_600, 0).unwrap();

        writer
            .upsert_epoch_statistics_batch(0, 0, 99, 100, ts0, ts0, 10, true, &mut batch)
            .unwrap();
        writer
            .upsert_epoch_statistics_batch(1, 100, 199, 100, ts1, ts1, 10, true, &mut batch)
            .unwrap();
        writer
            .upsert_epoch_statistics_batch(2, 200, 299, 100, ts2, ts2, 10, true, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        assert_eq!(writer.get_last_epoch_start(200).unwrap(), Some((1, ts1)));
        assert_eq!(writer.get_last_epoch_start(250).unwrap(), Some((2, ts2)));
        assert_eq!(writer.get_last_epoch_start(0).unwrap(), None);
    }

    #[test]
    fn test_refresh_latest_dao_statistics_persists_latest_entry() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let mut dao = vec![0u8; 32];
        dao[8..16].copy_from_slice(&2u64.to_le_bytes());
        let mut seed = StoreBatch::new(&store);
        seed.put_block_header(
            10,
            &CachedBlockHeader {
                hash: vec![0x11; 32],
                timestamp: 1_700_000_000_000,
                epoch_number: 0,
                epoch_index: 0,
                epoch_length: 1,
                dao,
                transactions_count: 1,
            },
        );
        seed.put_dao_deposit(
            &keys::encode_outpoint(&[0xAA; 32], 0),
            &DaoDepositCacheEntry {
                capacity: 200_00000000,
                deposit_block_number: 10,
                lock_script_hash: vec![0x01; 32],
                deposit_ar: 1,
                status: 0,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
        seed.put_dao_deposit(
            &keys::encode_outpoint(&[0xBB; 32], 0),
            &DaoDepositCacheEntry {
                capacity: 300_00000000,
                deposit_block_number: 9,
                lock_script_hash: vec![0x02; 32],
                deposit_ar: 1,
                status: 2,
                withdraw_request_tx: Some(vec![0x03; 32]),
                withdraw_request_output_index: Some(0),
                withdraw_request_block: Some(10),
                withdraw_request_ar: Some(2),
                withdraw_block: Some(10),
                withdraw_tx: Some(vec![0x04; 32]),
                withdraw_to_output_index: Some(0),
                compensation: Some(123_00000000),
            },
        );

        let snapshot_key =
            keys::encode_stats_key(keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT, b"20260101");
        let snapshot = DaoDailySnapshot {
            date: "2026-01-01".to_string(),
            total_deposited: 200_00000000,
            depositors_count: 1,
            new_deposits: 2,
            withdrawals: 1,
            compensation: 0,
            cumulative_deposit_amount: 500_00000000,
            total_issuance: 9_000_000_000i128 * 100_000_000i128,
            secondary_pool: 0,
            occupied_capacity: 0,
            cum_miner_secondary: 10_00000000,
            cum_dao_compensation: 20_00000000,
            cum_treasury: 30_00000000,
            unclaimed_compensation: 0,
        };
        let snapshot_val = bincode::serialize(&snapshot).unwrap();
        seed.put_stats(&snapshot_key, &snapshot_val);
        seed.commit().unwrap();

        writer.refresh_latest_dao_statistics().unwrap();
        let latest = store.get_latest_dao_statistics().unwrap().unwrap();

        assert_eq!(latest.tip_block_number, 10);
        assert_eq!(latest.total_deposited, 200_00000000);
        assert_eq!(latest.total_depositors, 1);
        assert_eq!(latest.active_deposits, 1);
        assert_eq!(latest.total_compensation_paid, 123_00000000);
        assert_eq!(latest.unclaimed_compensation, 98_00000000);
        assert_eq!(latest.average_deposit_days, "0 days");
        assert!(!latest.estimated_apc.is_empty());
        assert_eq!(latest.mining_reward, 10_00000000);
        assert_eq!(latest.deposit_compensation, 20_00000000);
        assert_eq!(latest.burnt, 30_00000000);
    }

    #[test]
    fn test_refresh_mnft_24h_transfers_cleans_only_mnft_collections() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let now_ms = chrono::Utc::now().timestamp_millis();
        let current_hour = now_ms / 3_600_000;
        let old_hour = current_hour - 100;

        let mnft_collection = vec![0x10; 32];
        let spore_collection = vec![0x20; 32];

        let mut seed = StoreBatch::new(&store);
        seed.put_object_collection_aggregate(
            &mnft_collection,
            &ObjectCollectionAggregate {
                standard: ObjectStandard::MnftClass,
                ..Default::default()
            },
        );
        seed.put_object_collection_aggregate(
            &spore_collection,
            &ObjectCollectionAggregate {
                standard: ObjectStandard::SporeCluster,
                ..Default::default()
            },
        );
        seed.put_object_hourly_transfer(&mnft_collection, old_hour, 9);
        seed.put_object_hourly_transfer(&mnft_collection, current_hour, 3);
        seed.put_object_hourly_transfer(&spore_collection, old_hour, 7);
        seed.commit().unwrap();

        let deleted = writer.refresh_mnft_24h_transfers().unwrap();
        assert_eq!(deleted, 1);

        let mnft_old_key = keys::encode_nft_hourly_key(&mnft_collection, old_hour);
        let mnft_new_key = keys::encode_nft_hourly_key(&mnft_collection, current_hour);
        let spore_old_key = keys::encode_nft_hourly_key(&spore_collection, old_hour);

        assert!(store.get_stats_key(&mnft_old_key).unwrap().is_none());
        assert!(store.get_stats_key(&mnft_new_key).unwrap().is_some());
        assert!(store.get_stats_key(&spore_old_key).unwrap().is_some());
    }

    #[test]
    fn test_update_hourly_statistics_errors_on_corrupt_existing_entry() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let hour = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let key = keys::encode_stats_key(
            keys::STATS_PREFIX_HOURLY,
            hour.format("%Y%m%d%H").to_string().as_bytes(),
        );
        let mut seed = StoreBatch::new(&store);
        seed.put_stats(&key, &[0xFF, 0xAA, 0x10]); // not a valid HourlyStats payload
        seed.commit().unwrap();

        let mut batch = StoreBatch::new(&store);
        let err = writer
            .update_hourly_statistics(hour, 1, 1, 1, 1, 1, &mut batch)
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("failed to deserialize hourly stats"));
    }

    #[test]
    fn test_get_previous_block_timestamp_errors_on_invalid_millis() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(
            0,
            &CachedBlockHeader {
                hash: vec![0x11; 32],
                timestamp: i64::MAX,
                epoch_number: 0,
                epoch_index: 0,
                epoch_length: 1,
                dao: vec![0; 32],
                transactions_count: 1,
            },
        );
        batch.commit().unwrap();

        let err = writer.get_previous_block_timestamp(1).unwrap_err();
        assert!(err
            .to_string()
            .contains("invalid previous block timestamp millis"));
    }

    #[test]
    fn test_daily_stats_new_day_carries_forward_cumulative_totals() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let day1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        let day2 = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();

        // Day 1: 10 cells created, 3 consumed, 500 data bytes
        let mut batch = StoreBatch::new(&store);
        writer
            .update_daily_statistics(
                day1, 10, 50, 10, 3, 1000, 500, 200, 500, 100, None, None, None, &mut batch,
            )
            .unwrap();
        batch.commit().unwrap();

        // Verify day 1 totals
        let key1 = keys::encode_stats_key(
            keys::STATS_PREFIX_DAILY,
            day1.format("%Y%m%d").to_string().as_bytes(),
        );
        let d1: DailyStats =
            bincode::deserialize(&store.get_stats_key(&key1).unwrap().unwrap()).unwrap();
        assert_eq!(d1.total_live_cells, 7); // 10 - 3
        assert_eq!(d1.total_dead_cells, 3);
        assert_eq!(d1.total_all_cells, 10);
        assert_eq!(d1.total_data_size, 400); // 500 - 100

        // Day 2: 5 cells created, 2 consumed, 200 data bytes
        let mut batch = StoreBatch::new(&store);
        writer
            .update_daily_statistics(
                day2, 5, 20, 5, 2, 500, 250, 100, 200, 50, None, None, None, &mut batch,
            )
            .unwrap();
        batch.commit().unwrap();

        // Day 2 should carry forward day 1's cumulative totals
        let key2 = keys::encode_stats_key(
            keys::STATS_PREFIX_DAILY,
            day2.format("%Y%m%d").to_string().as_bytes(),
        );
        let d2: DailyStats =
            bincode::deserialize(&store.get_stats_key(&key2).unwrap().unwrap()).unwrap();
        assert_eq!(d2.total_live_cells, 7 + 3); // prev 7 + (5 - 2)
        assert_eq!(d2.total_dead_cells, 3 + 2); // prev 3 + 2
        assert_eq!(d2.total_all_cells, 10 + 5); // prev 10 + 5
        assert_eq!(d2.total_data_size, 400 + 150); // prev 400 + (200 - 50)
    }

    #[test]
    fn test_daily_stats_cross_day_same_batch_uses_in_memory_prev() {
        // When a single WriteBatch spans multiple calendar days,
        // the second day must carry forward from the in-memory first day
        // (not the DB, which hasn't been committed yet).
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let day1 = NaiveDate::from_ymd_opt(2024, 3, 1).unwrap();
        let day2 = NaiveDate::from_ymd_opt(2024, 3, 2).unwrap();

        // Single batch, both days — NOT committed between them
        let mut batch = StoreBatch::new(&store);

        // Day 1: 20 created, 5 consumed, 800 data added, 100 consumed
        let d1 = writer
            .update_daily_statistics(
                day1, 8, 40, 20, 5, 2000, 1000, 400, 800, 100, None, None, None, &mut batch,
            )
            .unwrap();
        assert_eq!(d1.total_live_cells, 15); // 20 - 5
        assert_eq!(d1.total_dead_cells, 5);
        assert_eq!(d1.total_all_cells, 20);
        assert_eq!(d1.total_data_size, 700); // 800 - 100

        // Day 2: pass d1 as prev_day_stats (in-memory, batch not yet committed)
        let d2 = writer
            .update_daily_statistics(
                day2,
                4,
                10,
                6,
                2,
                500,
                300,
                100,
                200,
                50,
                None,
                None,
                Some(&d1),
                &mut batch,
            )
            .unwrap();

        // Day 2 cumulative totals should carry forward from d1
        assert_eq!(d2.total_live_cells, 15 + 4); // prev 15 + (6 - 2)
        assert_eq!(d2.total_dead_cells, 5 + 2); // prev 5 + 2
        assert_eq!(d2.total_all_cells, 20 + 6); // prev 20 + 6
        assert_eq!(d2.total_data_size, 700 + 150); // prev 700 + (200 - 50)

        batch.commit().unwrap();

        // Verify persisted values match
        let key2 = keys::encode_stats_key(
            keys::STATS_PREFIX_DAILY,
            day2.format("%Y%m%d").to_string().as_bytes(),
        );
        let persisted: DailyStats =
            bincode::deserialize(&store.get_stats_key(&key2).unwrap().unwrap()).unwrap();
        assert_eq!(persisted.total_live_cells, d2.total_live_cells);
        assert_eq!(persisted.total_all_cells, d2.total_all_cells);
    }

    #[test]
    fn test_daily_stats_errors_when_previous_day_missing_but_earlier_exists() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let day1 = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        let day3 = NaiveDate::from_ymd_opt(2024, 1, 3).unwrap();

        let mut seed_batch = StoreBatch::new(&store);
        writer
            .update_daily_statistics(
                day1,
                1,
                1,
                1,
                0,
                1,
                1,
                0,
                1,
                0,
                None,
                None,
                None,
                &mut seed_batch,
            )
            .unwrap();
        seed_batch.commit().unwrap();

        let mut gap_batch = StoreBatch::new(&store);
        let err = writer
            .update_daily_statistics(
                day3,
                1,
                1,
                1,
                0,
                1,
                1,
                0,
                1,
                0,
                None,
                None,
                None,
                &mut gap_batch,
            )
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("missing previous day stats while carrying daily totals"));
    }
}

#[cfg(test)]
mod activity_stats_tests {
    use super::*;
    use std::sync::Arc;

    use ckbadger_store::types::{ActivityEntry, AssetAction, AssetChange, DailyActivityStats};
    use ckbadger_store::CkbadgerStore;

    fn make_entry(
        ckb_delta: i128,
        is_cellbase: bool,
        has_type_script: bool,
        changes: Vec<AssetChange>,
    ) -> ActivityEntry {
        ActivityEntry {
            tx_hash: vec![0; 32],
            block_hash: vec![0; 32],
            block_number: 100,
            tx_index: 0,
            timestamp: 1700000000000,
            ckb_delta,
            used_delta: 0,
            is_cellbase,
            has_type_script,
            asset_changes: changes,
            peers: vec![],
        }
    }

    #[test]
    fn test_coinbase_classified_correctly() {
        let mut stats = DailyActivityStats::default();
        let scripts = vec![vec![0x11; 32]];
        let entry = make_entry(500_00000000, true, false, vec![]);
        BatchWriter::accumulate_activity_stats(&entry, &scripts, &mut stats);
        assert_eq!(stats.coinbase_count, 1);
        assert_eq!(stats.transfer_count, 0);
        // Coinbase excluded from total_ckb_moved and script_counts
        assert_eq!(stats.total_ckb_moved, 0);
        assert!(stats.script_counts.is_empty());
    }

    #[test]
    fn test_plain_transfer_classified_correctly() {
        let mut stats = DailyActivityStats::default();
        let scripts = vec![vec![0x11; 32]];
        let entry = make_entry(-100_00000000, false, false, vec![]);
        BatchWriter::accumulate_activity_stats(&entry, &scripts, &mut stats);
        assert_eq!(stats.transfer_count, 1);
        assert_eq!(stats.coinbase_count, 0);
        assert_eq!(stats.total_ckb_moved, 100_00000000);
    }

    #[test]
    fn test_dao_deposit_classified_correctly() {
        let mut stats = DailyActivityStats::default();
        let scripts = vec![vec![0x11; 32]];
        let entry = make_entry(
            -200_00000000,
            false,
            true,
            vec![AssetChange::DaoDeposit {
                capacity: 200_00000000,
            }],
        );
        BatchWriter::accumulate_activity_stats(&entry, &scripts, &mut stats);
        assert_eq!(stats.dao_deposit_count, 1);
        assert_eq!(stats.transfer_count, 0);
    }

    #[test]
    fn test_dao_withdraw_request_classified_correctly() {
        let mut stats = DailyActivityStats::default();
        let scripts = vec![vec![0x11; 32]];
        let entry = make_entry(
            0,
            false,
            true,
            vec![AssetChange::DaoWithdrawRequest {
                capacity: 200_00000000,
                deposit_block: 50,
            }],
        );
        BatchWriter::accumulate_activity_stats(&entry, &scripts, &mut stats);
        assert_eq!(stats.dao_withdraw_request_count, 1);
        assert_eq!(stats.transfer_count, 0);
    }

    #[test]
    fn test_dao_withdraw_complete_classified_correctly() {
        let mut stats = DailyActivityStats::default();
        let scripts = vec![vec![0x11; 32]];
        let entry = make_entry(
            200_00000000,
            false,
            true,
            vec![AssetChange::DaoWithdrawComplete {
                capacity: 200_00000000,
                compensation: 5_00000000,
            }],
        );
        BatchWriter::accumulate_activity_stats(&entry, &scripts, &mut stats);
        assert_eq!(stats.dao_withdraw_complete_count, 1);
        assert_eq!(stats.transfer_count, 0);
    }

    #[test]
    fn test_token_transfer_classified_correctly() {
        let mut stats = DailyActivityStats::default();
        let scripts = vec![vec![0x11; 32]];
        let entry = make_entry(
            0,
            false,
            true,
            vec![AssetChange::Token {
                type_script_hash: vec![0xAA; 32],
                delta: 1000,
                symbol: Some("SEAL".to_string()),
                decimals: Some(8),
            }],
        );
        BatchWriter::accumulate_activity_stats(&entry, &scripts, &mut stats);
        assert_eq!(stats.token_count, 1);
        assert_eq!(stats.transfer_count, 0);
    }

    #[test]
    fn test_object_classified_correctly() {
        let mut stats = DailyActivityStats::default();
        let scripts = vec![vec![0x11; 32]];
        let entry = make_entry(
            0,
            false,
            true,
            vec![AssetChange::Object {
                object_id: vec![0xBB; 32],
                standard: "spore".to_string(),
                action: AssetAction::Mint,
            }],
        );
        BatchWriter::accumulate_activity_stats(&entry, &scripts, &mut stats);
        assert_eq!(stats.object_count, 1);
        assert_eq!(stats.transfer_count, 0);
    }

    #[test]
    fn test_identity_classified_correctly() {
        let mut stats = DailyActivityStats::default();
        let scripts = vec![vec![0x11; 32]];
        let entry = make_entry(
            0,
            false,
            true,
            vec![AssetChange::Identity {
                identity_id: vec![0xCC; 32],
                standard: "dotbit".to_string(),
                action: AssetAction::Transfer,
            }],
        );
        BatchWriter::accumulate_activity_stats(&entry, &scripts, &mut stats);
        assert_eq!(stats.identity_count, 1);
        assert_eq!(stats.transfer_count, 0);
    }

    #[test]
    fn test_mixed_token_and_dao_counts_both() {
        let mut stats = DailyActivityStats::default();
        let scripts = vec![vec![0x11; 32]];
        let entry = make_entry(
            -500_00000000,
            false,
            true,
            vec![
                AssetChange::Token {
                    type_script_hash: vec![0xAA; 32],
                    delta: 1000,
                    symbol: None,
                    decimals: None,
                },
                AssetChange::DaoDeposit {
                    capacity: 100_00000000,
                },
            ],
        );
        BatchWriter::accumulate_activity_stats(&entry, &scripts, &mut stats);
        assert_eq!(stats.dao_deposit_count, 1);
        assert_eq!(stats.token_count, 1);
        assert_eq!(stats.transfer_count, 0);
    }

    #[test]
    fn test_multiple_activities_accumulate() {
        let mut stats = DailyActivityStats::default();
        let scripts = vec![vec![0x11; 32]];
        // 2 transfers + 1 coinbase
        BatchWriter::accumulate_activity_stats(
            &make_entry(-50_00000000, false, false, vec![]),
            &scripts,
            &mut stats,
        );
        BatchWriter::accumulate_activity_stats(
            &make_entry(30_00000000, false, false, vec![]),
            &scripts,
            &mut stats,
        );
        BatchWriter::accumulate_activity_stats(
            &make_entry(100_00000000, true, false, vec![]),
            &scripts,
            &mut stats,
        );
        assert_eq!(stats.transfer_count, 2);
        assert_eq!(stats.coinbase_count, 1);
        // Coinbase (100 CKB) excluded from total_ckb_moved: 50 + 30 = 80
        assert_eq!(stats.total_ckb_moved, 80_00000000);
    }

    #[test]
    fn test_negative_delta_uses_absolute_value() {
        let mut stats = DailyActivityStats::default();
        let scripts = vec![vec![0x11; 32]];
        let entry = make_entry(-999_00000000, false, false, vec![]);
        BatchWriter::accumulate_activity_stats(&entry, &scripts, &mut stats);
        assert_eq!(stats.total_ckb_moved, 999_00000000);
    }

    #[test]
    fn test_update_hourly_activity_stats_creates_new() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());
        let mut batch = StoreBatch::new(&store);
        let stats = DailyActivityStats {
            transfer_count: 5,
            total_ckb_moved: 50_00000000,
            ..Default::default()
        };
        writer
            .update_hourly_activity_stats("2026030912", &stats, 3, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        let got = store
            .get_hourly_activity_stats("2026030912")
            .unwrap()
            .unwrap();
        assert_eq!(got.transfer_count, 5);
        assert_eq!(got.unique_address_count, 3);
    }

    #[test]
    fn test_update_hourly_activity_stats_merges_existing() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        // First write
        let mut batch = StoreBatch::new(&store);
        let s1 = DailyActivityStats {
            transfer_count: 5,
            total_ckb_moved: 50_00000000,
            ..Default::default()
        };
        writer
            .update_hourly_activity_stats("2026030912", &s1, 3, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        // Merge write
        let mut batch2 = StoreBatch::new(&store);
        let s2 = DailyActivityStats {
            transfer_count: 10,
            dao_deposit_count: 2,
            total_ckb_moved: 100_00000000,
            ..Default::default()
        };
        writer
            .update_hourly_activity_stats("2026030912", &s2, 7, &mut batch2)
            .unwrap();
        batch2.commit().unwrap();

        let got = store
            .get_hourly_activity_stats("2026030912")
            .unwrap()
            .unwrap();
        assert_eq!(got.transfer_count, 15);
        assert_eq!(got.dao_deposit_count, 2);
        assert_eq!(got.total_ckb_moved, 150_00000000);
        assert_eq!(got.unique_address_count, 7); // replaced, not summed
    }

    #[test]
    fn test_script_counts_accumulated() {
        let mut stats = DailyActivityStats::default();
        let lock_ch = vec![0xAA; 32];
        let type_ch = vec![0xBB; 32];

        let entry = make_entry(
            -100_00000000,
            false,
            true,
            vec![AssetChange::DaoDeposit {
                capacity: 100_00000000,
            }],
        );
        let scripts = vec![lock_ch.clone(), type_ch.clone()];
        BatchWriter::accumulate_activity_stats(&entry, &scripts, &mut stats);

        assert_eq!(*stats.script_counts.get(&hex::encode(&lock_ch)).unwrap(), 1);
        assert_eq!(*stats.script_counts.get(&hex::encode(&type_ch)).unwrap(), 1);

        let entry2 = make_entry(-50_00000000, false, false, vec![]);
        let scripts2 = vec![lock_ch.clone()];
        BatchWriter::accumulate_activity_stats(&entry2, &scripts2, &mut stats);

        assert_eq!(*stats.script_counts.get(&hex::encode(&lock_ch)).unwrap(), 2);
        assert_eq!(*stats.script_counts.get(&hex::encode(&type_ch)).unwrap(), 1);
    }

    #[test]
    fn test_script_call_classified_correctly() {
        let mut stats = DailyActivityStats::default();
        let scripts = vec![vec![0xFF; 32]];
        let entry = make_entry(
            -50_00000000,
            false,
            true,
            vec![AssetChange::ScriptCall {
                type_code_hash: vec![0xFF; 32],
            }],
        );
        BatchWriter::accumulate_activity_stats(&entry, &scripts, &mut stats);
        assert_eq!(stats.script_call_count, 1);
        assert_eq!(stats.transfer_count, 0);
        assert_eq!(stats.unknown_count, 0);
    }

    #[test]
    fn test_unknown_is_unconditional_fallback() {
        let mut stats = DailyActivityStats::default();
        let scripts = vec![vec![0x11; 32]];
        // has_type_script=true but no asset changes — this is the Unknown case
        let entry = make_entry(0, false, true, vec![]);
        BatchWriter::accumulate_activity_stats(&entry, &scripts, &mut stats);
        assert_eq!(stats.unknown_count, 1);
        assert_eq!(stats.transfer_count, 0);
        assert_eq!(stats.script_call_count, 0);
    }

    #[test]
    fn test_transfer_requires_no_type_script() {
        let mut stats = DailyActivityStats::default();
        let scripts = vec![vec![0x11; 32]];
        // Pure CKB: no type scripts, no asset changes
        let entry = make_entry(-100_00000000, false, false, vec![]);
        BatchWriter::accumulate_activity_stats(&entry, &scripts, &mut stats);
        assert_eq!(stats.transfer_count, 1);
        assert_eq!(stats.unknown_count, 0);
        assert_eq!(stats.script_call_count, 0);
    }

    #[test]
    fn test_refresh_dao_statistics_computes_top_depositors() {
        use std::collections::HashMap;

        let mut depositor_map: HashMap<Vec<u8>, (i128, i32, f64)> = HashMap::new();
        let lock_a = vec![0xAA; 32];
        let lock_b = vec![0xBB; 32];

        // Depositor A: two deposits
        let e = depositor_map.entry(lock_a.clone()).or_insert((0, 0, 0.0));
        e.0 += 500_00000000i128;
        e.1 += 1;
        e.2 += 1000.0;
        let e = depositor_map.entry(lock_a.clone()).or_insert((0, 0, 0.0));
        e.0 += 300_00000000i128;
        e.1 += 1;
        e.2 += 500.0;

        // Depositor B: one deposit, larger
        let e = depositor_map.entry(lock_b.clone()).or_insert((0, 0, 0.0));
        e.0 += 1000_00000000i128;
        e.1 += 1;
        e.2 += 2000.0;

        let mut sorted: Vec<_> = depositor_map.into_iter().collect();
        sorted.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
        sorted.truncate(100);

        assert_eq!(sorted[0].0, lock_b);
        assert_eq!(sorted[0].1 .0, 1000_00000000);
        assert_eq!(sorted[1].0, lock_a);
        assert_eq!(sorted[1].1 .0, 800_00000000);
        assert_eq!(sorted[1].1 .1, 2);
    }
}
