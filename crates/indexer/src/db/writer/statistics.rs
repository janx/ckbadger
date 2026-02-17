use anyhow::Result;
use chrono::{DateTime, NaiveDate, Utc};

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::keys;
use ckbadger_store::types::*;

use super::BatchWriter;

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
        capacity_transferred: i64,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let key = keys::encode_stats_key(
            keys::STATS_PREFIX_HOURLY,
            hour.format("%Y%m%d%H").to_string().as_bytes(),
        );
        let existing = self.store.get_cf(self.store.cf_stats(), &key)?;

        let stats = match existing {
            Some(val) => {
                let mut s: HourlyStats = bincode::deserialize(&val).unwrap_or_default();
                s.blocks_count += blocks_count;
                s.transactions_count += transactions_count;
                s.cells_created += cells_created;
                s.cells_consumed += cells_consumed;
                s.capacity_transferred += capacity_transferred;
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
        capacity_transferred: i64,
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
        let existing = self.store.get_cf(self.store.cf_stats(), &key)?;

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
                s.capacity_transferred += capacity_transferred;
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
        let existing = self.store.get_cf(self.store.cf_stats(), &key)?;

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
        let existing = self.store.get_cf(self.store.cf_stats(), &key)?;

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
        let existing = self.store.get_cf(self.store.cf_stats(), &key)?;

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
        if let Some(val) = self.store.get_cf(self.store.cf_stats(), &key)? {
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
        if let Some(val) = self.store.get_cf(self.store.cf_stats(), &key)? {
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

    pub fn update_block_time_distribution(&self, _block_time_seconds: i64) -> Result<()> {
        // No-op: distribution is not maintained incrementally yet.
        Ok(())
    }

    pub fn update_epoch_time_distribution(
        &self,
        _epoch_number: i64,
        _epoch_duration_minutes: f64,
        _batch: &mut StoreBatch,
    ) -> Result<()> {
        // No-op: distribution is not maintained incrementally yet.
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
        let existing = self.store.get_cf(self.store.cf_stats(), &key)?;

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
        let existing = self.store.get_cf(self.store.cf_stats(), &key)?;

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
        let existing = self.store.get_cf(self.store.cf_stats(), &key)?;

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
        let existing = self.store.get_cf(self.store.cf_stats(), &key)?;

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
        let existing = self.store.get_cf(self.store.cf_stats(), &key)?;

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
        // No-op placeholder for future MNFT transfer-window maintenance.
        Ok(0)
    }

    pub fn rebuild_mnft_statistics(&self) -> Result<u64> {
        // No-op placeholder for future full MNFT statistics rebuild.
        Ok(0)
    }

    pub fn rebuild_all_statistics(&self) -> Result<()> {
        // No-op placeholder for future full statistics rebuild.
        Ok(())
    }

    pub fn get_dao_deposits_at_block(&self, _block_number: i64) -> Result<u128> {
        // Not implemented: requires full DAO deposit aggregation at historical height.
        Ok(0)
    }

    pub fn get_previous_epoch_duration_minutes(&self, epoch_number: i64) -> Result<Option<f64>> {
        let key = keys::encode_stats_key(keys::STATS_PREFIX_EPOCH, &epoch_number.to_be_bytes());
        if let Some(val) = self.store.get_cf(self.store.cf_stats(), &key)? {
            if let Ok(s) = bincode::deserialize::<EpochStats>(&val) {
                if let Some(end_ts) = s.end_timestamp {
                    let duration = end_ts - s.start_timestamp;
                    return Ok(Some(duration.num_minutes() as f64));
                }
            }
        }
        Ok(None)
    }

    pub fn get_last_epoch_start(&self, _before_block: i64) -> Result<Option<(i64, DateTime<Utc>)>> {
        // This is complex to implement efficiently without an epoch->block index
        // The caller should track epoch boundaries during sync
        Ok(None)
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
}
