use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, NaiveDate, Utc};

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
}

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
                let mut s: HourlyStats = bincode::deserialize(&val).unwrap_or_default();
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

    pub fn update_daily_statistics(
        &self,
        date: NaiveDate,
        blocks_count: i32,
        transactions_count: i32,
        cells_created: i32,
        cells_consumed: i32,
        capacity_transferred: i128,
        occupied_capacity_created: i128,
        occupied_capacity_consumed: i128,
        data_size_added: i64,
        data_size_consumed: i64,
        dao_field: Option<&[u8]>,
        block_time: Option<(i64, i32)>, // (sum_ms, count)
        batch: &mut StoreBatch,
    ) -> Result<()> {
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
                let mut s: DailyStats = bincode::deserialize(&val).unwrap_or_default();
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
                s.occupied_capacity_created = checked_add_i128(
                    s.occupied_capacity_created,
                    occupied_capacity_created,
                    "daily.occupied_capacity_created",
                )?;
                s.occupied_capacity_consumed = checked_add_i128(
                    s.occupied_capacity_consumed,
                    occupied_capacity_consumed,
                    "daily.occupied_capacity_consumed",
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
                let net_cells = (cells_created - cells_consumed) as i64;
                let net_data_size = data_size_added - data_size_consumed;
                let knowledge_size = dao_field.and_then(calculate_knowledge_size);

                DailyStats {
                    blocks_count,
                    transactions_count,
                    cells_created,
                    cells_consumed,
                    capacity_transferred,
                    occupied_capacity_created,
                    occupied_capacity_consumed,
                    total_live_cells: net_cells,
                    total_dead_cells: cells_consumed as i64,
                    total_all_cells: cells_created as i64,
                    total_data_size: net_data_size,
                    knowledge_size,
                    avg_block_time_ms,
                }
            }
        };

        let value = bincode::serialize(&stats)?;
        batch.put_stats(&key, &value);
        Ok(())
    }

    pub fn update_daily_block_stats(
        &self,
        date: NaiveDate,
        compact_target: i64,
        uncles_count: i32,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let key = keys::encode_stats_key(
            keys::STATS_PREFIX_DAILY_BLOCK,
            date.format("%Y%m%d").to_string().as_bytes(),
        );
        let existing = self.store.get_stats_key(&key)?;

        let stats = match existing {
            Some(val) => {
                let mut s: DailyBlockStats = bincode::deserialize(&val).unwrap_or_default();
                let old_total = s.avg_compact_target * s.block_count as f64;
                s.block_count += 1;
                s.avg_compact_target = (old_total + compact_target as f64) / s.block_count as f64;
                s.total_uncles += uncles_count;
                s
            }
            None => DailyBlockStats {
                avg_compact_target: compact_target as f64,
                block_count: 1,
                total_uncles: uncles_count,
                avg_block_time_ms: None,
            },
        };

        let value = bincode::serialize(&stats)?;
        batch.put_stats(&key, &value);
        Ok(())
    }

    pub fn update_miner_statistics(
        &self,
        lock_script_hash: &[u8],
        block_number: i64,
        date: NaiveDate,
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
                let mut s: MinerStats = bincode::deserialize(&val).unwrap_or_default();
                s.blocks_count += 1;
                s.last_block_number = s.last_block_number.max(block_number);
                s
            }
            None => MinerStats {
                miner_lock_hash: lock_script_hash.to_vec(),
                blocks_count: 1,
                last_block_number: block_number,
            },
        };

        let value = bincode::serialize(&stats)?;
        batch.put_stats(&key, &value);
        Ok(())
    }

    pub fn upsert_epoch_statistics(
        &self,
        epoch_number: i64,
        block_number: i64,
        epoch_length: i32,
        timestamp: DateTime<Utc>,
        epoch_index: i32,
        transactions_count: i32,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let key = keys::encode_stats_key(keys::STATS_PREFIX_EPOCH, &epoch_number.to_be_bytes());
        let existing = self.store.get_stats_key(&key)?;

        let stats = match existing {
            Some(val) => {
                let mut s: EpochStats = bincode::deserialize(&val).unwrap_or_default();
                s.blocks_count += 1;
                s.transactions_count += transactions_count;
                if epoch_index > 0 {
                    s.end_block = Some(block_number);
                    s.end_timestamp = Some(timestamp);
                }
                s
            }
            None => EpochStats {
                epoch_number,
                start_block: block_number,
                end_block: if epoch_index > 0 {
                    Some(block_number)
                } else {
                    None
                },
                blocks_count: 1,
                length: epoch_length,
                start_timestamp: timestamp,
                end_timestamp: if epoch_index > 0 {
                    Some(timestamp)
                } else {
                    None
                },
                transactions_count,
            },
        };

        let value = bincode::serialize(&stats)?;
        batch.put_stats(&key, &value);
        Ok(())
    }

    pub fn update_daily_avg_block_time(
        &self,
        date: NaiveDate,
        block_time_ms: i64,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        if block_time_ms < 0 {
            return Ok(());
        }

        let key = keys::encode_stats_key(
            keys::STATS_PREFIX_DAILY,
            date.format("%Y%m%d").to_string().as_bytes(),
        );
        if let Some(val) = self.store.get_stats_key(&key)? {
            if let Ok(mut s) = bincode::deserialize::<DailyStats>(&val) {
                s.avg_block_time_ms = match s.avg_block_time_ms {
                    Some(existing) => {
                        let total = existing * (s.blocks_count as i64 - 1) + block_time_ms;
                        Some(total / s.blocks_count as i64)
                    }
                    None => Some(block_time_ms),
                };
                let value = bincode::serialize(&s)?;
                batch.put_stats(&key, &value);
            }
        }
        Ok(())
    }

    pub fn update_daily_avg_block_time_batch(
        &self,
        date: NaiveDate,
        avg_block_time_ms: i64,
        block_count: i32,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        if block_count <= 0 {
            return Ok(());
        }

        let key = keys::encode_stats_key(
            keys::STATS_PREFIX_DAILY,
            date.format("%Y%m%d").to_string().as_bytes(),
        );
        if let Some(val) = self.store.get_stats_key(&key)? {
            if let Ok(mut s) = bincode::deserialize::<DailyStats>(&val) {
                s.avg_block_time_ms = match s.avg_block_time_ms {
                    Some(existing) => {
                        let total = existing * (s.blocks_count as i64 - block_count as i64)
                            + avg_block_time_ms * block_count as i64;
                        Some(total / s.blocks_count as i64)
                    }
                    None => Some(avg_block_time_ms),
                };
                let value = bincode::serialize(&s)?;
                batch.put_stats(&key, &value);
            }
        }
        Ok(())
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
                let mut s: DailyBlockStats = bincode::deserialize(&val).unwrap_or_default();
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
                let mut s: MinerStats = bincode::deserialize(&val).unwrap_or_default();
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
            let mut s: EpochStats = bincode::deserialize(&val).unwrap_or_default();
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
                i32::from_le_bytes(val.try_into().unwrap_or([0; 4])) + count
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
                i32::from_le_bytes(val.try_into().unwrap_or([0; 4])) + count
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
            return Ok(Some(
                DateTime::from_timestamp_millis(header.timestamp)
                    .unwrap_or_else(DateTime::<Utc>::default),
            ));
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
        };

        let value = bincode::serialize(&snapshot)?;
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

        let collections = self.store.list_nft_collection_aggregates()?;
        let mut total_deleted = 0u64;
        for (collection_id, agg) in collections {
            if agg.standard == NftStandard::MnftClass {
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

    pub fn get_previous_epoch_duration_minutes(&self, epoch_number: i64) -> Result<Option<f64>> {
        let key = keys::encode_stats_key(keys::STATS_PREFIX_EPOCH, &epoch_number.to_be_bytes());
        if let Some(val) = self.store.get_stats_key(&key)? {
            if let Ok(s) = bincode::deserialize::<EpochStats>(&val) {
                if let Some(end_ts) = s.end_timestamp {
                    let duration = end_ts - s.start_timestamp;
                    return Ok(Some(duration.num_minutes() as f64));
                }
            }
        }
        Ok(None)
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
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());

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
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());

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
    fn test_refresh_mnft_24h_transfers_cleans_only_mnft_collections() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());

        let now_ms = chrono::Utc::now().timestamp_millis();
        let current_hour = now_ms / 3_600_000;
        let old_hour = current_hour - 100;

        let mnft_collection = vec![0x10; 32];
        let dotbit_collection = vec![0x20; 32];

        let mut seed = StoreBatch::new(&store);
        seed.put_nft_collection_aggregate(
            &mnft_collection,
            &NftCollectionAggregate {
                standard: NftStandard::MnftClass,
                ..Default::default()
            },
        );
        seed.put_nft_collection_aggregate(
            &dotbit_collection,
            &NftCollectionAggregate {
                standard: NftStandard::DotBit,
                ..Default::default()
            },
        );
        seed.put_nft_hourly_transfer(&mnft_collection, old_hour, 9);
        seed.put_nft_hourly_transfer(&mnft_collection, current_hour, 3);
        seed.put_nft_hourly_transfer(&dotbit_collection, old_hour, 7);
        seed.commit().unwrap();

        let deleted = writer.refresh_mnft_24h_transfers().unwrap();
        assert_eq!(deleted, 1);

        let mnft_old_key = keys::encode_nft_hourly_key(&mnft_collection, old_hour);
        let mnft_new_key = keys::encode_nft_hourly_key(&mnft_collection, current_hour);
        let dotbit_old_key = keys::encode_nft_hourly_key(&dotbit_collection, old_hour);

        assert!(store.get_stats_key(&mnft_old_key).unwrap().is_none());
        assert!(store.get_stats_key(&mnft_new_key).unwrap().is_some());
        assert!(store.get_stats_key(&dotbit_old_key).unwrap().is_some());
    }
}
