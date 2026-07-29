use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use rocksdb::{Direction, IteratorMode};
use std::collections::{HashMap, HashSet};

use ckbadger_common::dao::{calculate_estimated_apc, extract_s_from_dao};
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

/// Accumulate activity stats from a single TxActions into DailyActivityStats.
///
/// This is called once per transaction. Stats are accumulated per-tx:
/// - DAO counts: from tx_actions.protocol_actions (protocol == "dao")
/// - Token/Object/Identity counts: from participant.tags bitmask
/// - Protocol action counts: from tx_actions.protocol_actions (TX-level)
/// - Script counts: from tx_actions.type_calls + lock_calls code_hashes (TX-level)
/// - CKB moved: sum |participant.ckb_delta| for non-cellbase
/// - Transfer count: participant has no asset/protocol/script flags
/// - Coinbase: tx_actions.is_cellbase
fn accumulate_tx_actions_stats(tx_actions: &TxActions, stats: &mut DailyActivityStats) {
    stats.accumulate_from_tx_actions(tx_actions);
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
    /// AR-based compensation sum for active (status-0) deposits (shannons).
    pub unmade_dao_interests: i128,
    /// Unclaimed DAO compensation at this point (shannons).
    pub unclaimed_compensation: i128,
    /// Cumulative count of unique addresses that have ever deposited.
    pub cumulative_depositors: i64,
    /// Unique addresses that deposited on this specific day (including repeat depositors).
    pub daily_depositor_addresses: i64,
    /// Protocol-level total deposited (includes status=1 cells still locked in DAO).
    /// Used for secondary issuance split instead of display `total_deposited`.
    pub protocol_deposited: Option<i128>,
}

/// Boundary of a calendar day that a batch has fully written: the date and the
/// final block on it. Completed days are evaluated at this block's AR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DaoSnapshotBoundary {
    pub(crate) date: NaiveDate,
    pub(crate) end_block: i64,
}

/// Exact per-deposit DAO lifecycle result at one observation point.
///
/// The same value materializes a completed day's snapshot (evaluated at that
/// day's final block and AR, inside the atomic batch that writes the day) and
/// the live tip's snapshot (evaluated at the sync tip after that commit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ExactDaoSnapshotCompensation {
    breakdown: DaoCompensationBreakdown,
    total: i128,
    treasury: i128,
    secondary_pool: i128,
}

impl ExactDaoSnapshotCompensation {
    fn apply_to(self, snapshot: &mut DaoDailySnapshot) {
        snapshot.compensation = self.breakdown.claimed;
        snapshot.cum_dao_compensation = self.total;
        snapshot.unclaimed_compensation = self.breakdown.unclaimed;
        snapshot.unmade_dao_interests = self.breakdown.active_unmade;
        snapshot.secondary_pool = self.secondary_pool;
        snapshot.cum_treasury = self.treasury;
    }

    fn apply_to_input(self, input: &mut DaoSnapshotInput) {
        input.total_compensation = self.breakdown.claimed;
        input.cum_dao_compensation = self.total;
        input.unclaimed_compensation = self.breakdown.unclaimed;
        input.unmade_dao_interests = self.breakdown.active_unmade;
        input.secondary_pool = self.secondary_pool;
        input.cum_treasury = self.treasury;
    }
}

impl BatchWriter {
    /// Upsert one chain-level hourly stats bucket.
    ///
    /// `hour` is the UTC-truncated hour start; the row key is its **UTC**
    /// `%Y%m%d%H` string (`stats_prefix::HOURLY` convention — activity hourly
    /// buckets use UTC+8 keys instead) and `HourlyStats.hour` is its epoch
    /// seconds. `transactions_count` includes the cellbase. Reorg rollback
    /// cutoffs and the bulk-build `ChainStatsAccumulator` mirror exactly
    /// these semantics.
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

    /// Fetch the genesis-derived virtual occupied capacity (the knowledge-size
    /// burn adjustment) from the persisted `GenesisBaseline`.
    ///
    /// Fail-fast if the baseline has not been derived yet: knowledge_size has a
    /// single calculation path and must never silently fall back to a hardcoded
    /// mainnet constant. The indexer derives the baseline at startup (block 0)
    /// before any block — and therefore any daily-stats row — is written.
    fn genesis_virtual_occupied(&self) -> Result<i128> {
        Ok(self
            .store
            .get_genesis_baseline()?
            .ok_or_else(|| anyhow!("genesis baseline not derived; cannot compute knowledge_size"))?
            .virtual_occupied)
    }

    /// Exact genesis total issuance (DAO `C` of block 0) from the persisted
    /// per-network baseline. Seeds the APC model's theoretical cumulative
    /// issuance instead of a hardcoded 33.6B approximation.
    fn genesis_total_issuance(&self) -> Result<i128> {
        Ok(self
            .store
            .get_genesis_baseline()?
            .ok_or_else(|| anyhow!("genesis baseline not derived; cannot compute estimated APC"))?
            .total_issuance)
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

        let (new_block_time_sum_ms, new_block_time_count) = block_time.unwrap_or((0, 0));

        let stats = match existing {
            Some(val) => {
                let mut s: DailyStats = deserialize_stats(&val, "daily stats")?;
                s.block_time_sum_ms += new_block_time_sum_ms;
                s.block_time_count += new_block_time_count;
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
                    let virtual_occupied = self.genesis_virtual_occupied()?;
                    if let Some(ks) = calculate_knowledge_size(dao, virtual_occupied) {
                        s.knowledge_size = Some(ks);
                    }
                }
                s
            }
            None => {
                let knowledge_size = match dao_field {
                    Some(dao) => {
                        let virtual_occupied = self.genesis_virtual_occupied()?;
                        calculate_knowledge_size(dao, virtual_occupied)
                    }
                    None => None,
                };

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
                    block_time_sum_ms: new_block_time_sum_ms,
                    block_time_count: new_block_time_count,
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
        avg_difficulty: f64,
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
                let old_total = s.avg_difficulty * s.block_count as f64;
                s.block_count += block_count;
                s.avg_difficulty =
                    (old_total + avg_difficulty * block_count as f64) / s.block_count as f64;
                s.total_uncles += total_uncles;
                s
            }
            None => DailyBlockStats {
                avg_difficulty,
                block_count,
                total_uncles,
                block_time_sum_ms: 0,
                block_time_count: 0,
            },
        };

        let value = bincode::serialize(&stats)?;
        batch.put_stats(&key, &value);
        Ok(())
    }

    /// Upsert one per-miner daily bucket.
    ///
    /// `date` is the UTC+8 calendar day (`block_date` convention shared by
    /// all date-scoped stats keys); `lock_script_hash` is the cellbase
    /// WITNESS miner (RFC-0022). The bulk-build `ChainStatsAccumulator`
    /// mirrors exactly these semantics.
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
            let blocks_count = (end_block - start_block + 1) as i32;
            EpochStats {
                epoch_number,
                start_block,
                end_block: Some(end_block),
                blocks_count,
                length: epoch_length,
                start_timestamp,
                end_timestamp: if blocks_count >= epoch_length {
                    Some(end_timestamp)
                } else {
                    None
                },
                transactions_count,
            }
        } else if let Some(val) = existing {
            let mut s: EpochStats = deserialize_stats(&val, "epoch stats")?;
            s.end_block = Some(s.end_block.unwrap_or(end_block).max(end_block));
            s.blocks_count = (s.end_block.unwrap_or(end_block) - s.start_block + 1) as i32;
            s.end_timestamp = if s.blocks_count >= s.length {
                Some(end_timestamp)
            } else {
                None
            };
            s.transactions_count += transactions_count;
            s
        } else {
            // A mid-epoch batch (is_new=false) requires an existing row: epoch
            // rows are written atomically with their blocks, and reorg
            // rollback truncates (never deletes) the boundary epoch row.
            // Fabricating a row here would stamp the batch start as the epoch
            // start, corrupting epoch timing (see F1 postmortem: reorg replay
            // used to do exactly that).
            anyhow::bail!(
                "epoch stats row missing for mid-epoch batch: epoch={}, blocks={}..={} (epoch row must exist before non-initial blocks; re-sync required if store predates epoch rows)",
                epoch_number,
                start_block,
                end_block
            );
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
            unmade_dao_interests: dao_snapshot.unmade_dao_interests,
            unclaimed_compensation: dao_snapshot.unclaimed_compensation,
            cumulative_depositors: dao_snapshot.cumulative_depositors,
            daily_depositor_addresses: dao_snapshot.daily_depositor_addresses,
            protocol_deposited: dao_snapshot.protocol_deposited,
        };

        let value = bincode::serialize(&snapshot)?;
        batch.put_stats(&key, &value);
        Ok(())
    }

    /// Accumulate activity stats from a TxActions into DailyActivityStats.
    /// Call once per TxActions from build_tx_actions_for_block().
    pub fn accumulate_tx_activity_stats(tx_actions: &TxActions, stats: &mut DailyActivityStats) {
        accumulate_tx_actions_stats(tx_actions, stats);
    }

    /// Write accumulated daily activity stats for a date.
    /// Reads existing stats for the date, merges with accumulated, writes back.
    /// Uses a persistent address set to correctly dedup unique addresses across batches.
    pub fn update_daily_activity_stats(
        &self,
        date: &str,
        accumulated: &DailyActivityStats,
        batch_addrs: &HashSet<[u8; 32]>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let unique_address_count = self.merge_persistent_addr_set(
            keys::stats_prefix::ACTIVITY_DAILY_ADDR_SET,
            date.as_bytes(),
            batch_addrs,
            batch,
        )?;
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
                e.unique_address_count = unique_address_count;
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
                // Merge protocol action counts
                for (key, count) in &accumulated.protocol_action_counts {
                    *e.protocol_action_counts.entry(key.clone()).or_insert(0) += count;
                }
                e
            }
            None => {
                let mut s = accumulated.clone();
                s.unique_address_count = unique_address_count;
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
    /// Uses a persistent address set to correctly dedup unique addresses across batches.
    pub fn update_hourly_activity_stats(
        &self,
        hour_key: &str,
        accumulated: &DailyActivityStats,
        batch_addrs: &HashSet<[u8; 32]>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let unique_address_count = self.merge_persistent_addr_set(
            keys::stats_prefix::ACTIVITY_HOURLY_ADDR_SET,
            hour_key.as_bytes(),
            batch_addrs,
            batch,
        )?;
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
                e.unique_address_count = unique_address_count;
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
                // Merge protocol action counts
                for (key, count) in &accumulated.protocol_action_counts {
                    *e.protocol_action_counts.entry(key.clone()).or_insert(0) += count;
                }
                e
            }
            None => {
                let mut s = accumulated.clone();
                s.unique_address_count = unique_address_count;
                s
            }
        };
        let key = keys::encode_stats_key(keys::stats_prefix::ACTIVITY_HOURLY, hour_key.as_bytes());
        let value = bincode::serialize(&merged)?;
        batch.put_stats(&key, &value);
        Ok(())
    }

    /// Load an existing persistent address set from CF_STATS_CHAIN, merge in
    /// new addresses from the current batch, store back, and return the total
    /// unique address count as u32.
    ///
    /// Row encoding and count derivation come from the shared store-level
    /// helpers, so reorg rollback repair rebuilds byte-identical rows.
    fn merge_persistent_addr_set(
        &self,
        prefix: u8,
        bucket: &[u8],
        batch_addrs: &HashSet<[u8; 32]>,
        batch: &mut StoreBatch,
    ) -> Result<u32> {
        let set_key = keys::encode_stats_key(prefix, bucket);
        let bucket_label = String::from_utf8_lossy(bucket).into_owned();
        let mut addrs: HashSet<[u8; 32]> = match self.store.get_stats_key(&set_key)? {
            Some(raw) => ckbadger_store::decode_activity_addr_set(&raw, &bucket_label)?,
            None => HashSet::new(),
        };
        addrs.extend(batch_addrs);
        let flat = ckbadger_store::encode_activity_addr_set(addrs.iter().copied());
        batch.put_stats(&set_key, &flat);
        ckbadger_store::activity_addr_set_count(addrs.len(), &bucket_label)
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

        let collections = self.store.list_mnft_collection_aggregates()?;
        let mut total_deleted = 0u64;
        for (collection_id, agg) in collections {
            if agg.standard == ObjectStandard::MnftClass {
                total_deleted += self
                    .store
                    .cleanup_old_object_hourly_buckets(&collection_id, cutoff_hour)?;
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

    /// Materialize a completed day's exact DAO lifecycle values into the
    /// snapshot input, before the batch that writes that day commits.
    ///
    /// `staged_entries` / `staged_completions` are the batch's own uncommitted
    /// DAO mutations, so the day is evaluated against exactly the lifecycle
    /// state this commit is about to persist. That removes the window in which
    /// a completed day's row existed with carried-forward placeholder
    /// cumulative values (DAO-026) and makes the completion marker — the sync
    /// tip — land in the same write as the values it certifies (IDX-001).
    pub(crate) fn apply_exact_completed_dao_snapshot(
        &self,
        boundary: DaoSnapshotBoundary,
        end_ar: u64,
        secondary_pool: i128,
        staged_entries: &HashMap<Vec<u8>, DaoDepositCacheEntry>,
        staged_completions: &HashMap<Vec<u8>, (i64, Vec<u8>)>,
        input: &mut DaoSnapshotInput,
    ) -> Result<()> {
        let exact = self
            .exact_dao_snapshot_compensation_with_staged(
                boundary.end_block,
                end_ar,
                secondary_pool,
                staged_entries,
                staged_completions,
            )
            .with_context(|| {
                format!(
                    "failed to materialize exact DAO snapshot for completed date {} at block {}",
                    boundary.date, boundary.end_block
                )
            })?;
        exact.apply_to_input(input);
        Ok(())
    }

    /// Exact DAO lifecycle values at `observation_block`, observing deposit
    /// mutations staged in an uncommitted batch. Post-commit callers pass empty
    /// overlays.
    fn exact_dao_snapshot_compensation_with_staged(
        &self,
        observation_block: i64,
        observation_ar: u64,
        secondary_pool: i128,
        staged_entries: &HashMap<Vec<u8>, DaoDepositCacheEntry>,
        staged_completions: &HashMap<Vec<u8>, (i64, Vec<u8>)>,
    ) -> Result<ExactDaoSnapshotCompensation> {
        let breakdown = self
            .store
            .compute_dao_compensation_breakdown_at_with_staged(
                observation_block,
                observation_ar,
                staged_entries,
                staged_completions,
            )?;
        let total = breakdown.total().ok_or_else(|| {
            anyhow!(
                "DAO total compensation overflow at observation block {}: claimed={}, unclaimed={}",
                observation_block,
                breakdown.claimed,
                breakdown.unclaimed
            )
        })?;
        let treasury = secondary_pool
            .checked_sub(breakdown.active_unmade)
            .ok_or_else(|| {
                anyhow!(
                    "DAO treasury subtraction overflow at observation block {}: secondary_pool={}, active_unmade={}",
                    observation_block,
                    secondary_pool,
                    breakdown.active_unmade
                )
            })?;
        if treasury < 0 {
            bail!(
                "active DAO interests exceed secondary pool at observation block {}: secondary_pool={}, active_unmade={}",
                observation_block,
                secondary_pool,
                breakdown.active_unmade
            );
        }

        Ok(ExactDaoSnapshotCompensation {
            breakdown,
            total,
            treasury,
            secondary_pool,
        })
    }

    /// Re-derive the DAO singleton statistics and patch the live tip's daily
    /// snapshot from the committed lifecycle state.
    ///
    /// Completed days are NOT touched here: they are materialized exact inside
    /// the atomic batch that writes them (see
    /// `exact_dao_snapshot_compensation_with_staged`). This pass only owns the
    /// incomplete tip day, which every subsequent batch rewrites anyway, plus
    /// the tip-scoped `DaoLatestStatistics` / `DaoTopDepositors` singletons.
    pub fn refresh_latest_dao_statistics(&self) -> Result<()> {
        self.refresh_dao_statistics()
    }

    fn refresh_dao_statistics(&self) -> Result<()> {
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
        let tip_timestamp = header.timestamp;
        let tip_s = extract_s_from_dao(&header.dao).ok_or_else(|| {
            anyhow!(
                "invalid DAO S field in sync tip block while refreshing latest dao statistics: block_number={}, dao_len={}",
                tip_block_number,
                header.dao.len()
            )
        })?;

        let mut total_deposited: i128 = 0;
        let mut pending_withdrawal_capacity: i128 = 0;
        let mut unique_depositors: HashSet<Vec<u8>> = HashSet::new();
        let mut active_deposits = 0i32;
        let mut depositor_map: HashMap<Vec<u8>, (i128, i32, f64)> = HashMap::new();
        let mut weighted_deposit_days: f64 = 0.0;
        let mut avg_total_capacity: i128 = 0;
        let mut status1_for_avg: Vec<(i64, i64, i64)> = Vec::new();

        for scan_status in [0i16, 1] {
            self.store
                .scan_dao_deposits_by_status(scan_status, |_, entry| {
                    active_deposits += 1;

                    // Status-specific accounting
                    if entry.status == 1 {
                        pending_withdrawal_capacity += entry.capacity as i128;
                        if let Some(request_block) = entry.withdraw_request_block {
                            status1_for_avg.push((
                                entry.capacity,
                                entry.deposit_timestamp,
                                request_block,
                            ));
                        }
                    } else {
                        // Only status=0 deposits count toward total_deposited,
                        // unique_depositors, depositor_map, and avg deposit time —
                        // matching CKB explorer convention which subtracts from
                        // total_deposit at phase-1 withdrawal.
                        total_deposited += entry.capacity as i128;
                        unique_depositors.insert(entry.lock_script_hash.clone());

                        let dm = depositor_map
                            .entry(entry.lock_script_hash.clone())
                            .or_insert((0, 0, 0.0));
                        dm.0 += entry.capacity as i128;
                        dm.1 += 1;
                        dm.2 += (tip_timestamp - entry.deposit_timestamp) as f64;

                        let held_ms = tip_timestamp - entry.deposit_timestamp;
                        let days_held = held_ms as f64 / 86_400_000.0;
                        weighted_deposit_days += entry.capacity as f64 * days_held;
                        avg_total_capacity += entry.capacity as i128;
                    }

                    Ok(())
                })?;
        }

        let exact_tip = self.exact_dao_snapshot_compensation_with_staged(
            tip_block_number,
            tip_ar,
            i128::from(tip_s),
            &HashMap::new(),
            &HashMap::new(),
        )?;
        let total_compensation_paid = exact_tip.breakdown.claimed;
        let unclaimed_compensation = exact_tip.breakdown.unclaimed;
        let deposit_compensation_total = exact_tip.total;

        // Resolve status-1 deposit timestamps from block headers for capacity-weighted average.
        for &(capacity, deposit_ts, request_block) in &status1_for_avg {
            if let Some(hdr) = self.store.get_block_header(request_block)? {
                let frozen_days = (hdr.timestamp - deposit_ts) as f64 / 86_400_000.0;
                if frozen_days >= 0.0 {
                    weighted_deposit_days += capacity as f64 * frozen_days;
                    avg_total_capacity += capacity as i128;
                }
            }
        }

        let latest_snapshot = match self.store.get_latest_dao_daily_snapshot()? {
            Some(snapshot) => snapshot,
            None if tip_block_number == 0 => {
                // Startup cleanup can leave only the canonical genesis header.
                // No post-genesis DAO snapshot exists yet; the first forward
                // batch creates it before this refresh is required.
                return Ok(());
            }
            None => {
                bail!(
                    "missing DAO daily snapshot while refreshing latest statistics: tip_block={}",
                    tip_block_number
                );
            }
        };
        let estimated_apc =
            estimated_apc_from_header(&header, self.genesis_total_issuance()?).unwrap_or_default();
        if latest_snapshot.cum_miner_secondary < 0 {
            bail!(
                "negative cum_miner_secondary in dao_daily_snapshots while refreshing latest dao statistics for {}: {}",
                latest_snapshot.date,
                latest_snapshot.cum_miner_secondary
            );
        }
        let burnt = exact_tip.treasury;
        let mining_reward = latest_snapshot.cum_miner_secondary;
        let deposit_compensation = deposit_compensation_total;

        let avg_days = if avg_total_capacity > 0 {
            weighted_deposit_days / avg_total_capacity as f64
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
            average_deposit_days: format_days(avg_days),
            estimated_apc,
            mining_reward,
            deposit_compensation,
            burnt,
            pending_withdrawal_capacity,
        };

        // Batch all DAO stats writes atomically
        let mut dao_batch = StoreBatch::new(&self.store);

        let key = keys::encode_stats_key(keys::STATS_PREFIX_DAO_LATEST_STATS, b"latest");
        let value = bincode::serialize(&latest)?;
        dao_batch.put_stats(&key, &value);

        // Build and store top depositors
        {
            let mut sorted: Vec<_> = depositor_map.into_iter().collect();
            sorted.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
            sorted.truncate(100);

            let depositors = sorted
                .into_iter()
                .map(|(lock_hash, (total_capacity, deposit_count, total_ms))| {
                    let avg_ms = if deposit_count > 0 {
                        total_ms / deposit_count as f64
                    } else {
                        0.0
                    };
                    DaoTopDepositorEntry {
                        lock_script_hash: lock_hash,
                        total_capacity,
                        deposit_count,
                        average_deposit_ms: avg_ms,
                    }
                })
                .collect();

            let top = DaoTopDepositors {
                tip_block_number,
                depositors,
            };
            let top_key = keys::encode_stats_key(keys::STATS_PREFIX_DAO_TOP_DEPOSITORS, b"latest");
            let top_value = bincode::serialize(&top)?;
            dao_batch.put_stats(&top_key, &top_value);
        }

        // Re-evaluate the incomplete tip day at the committed sync tip. The tip
        // day is still accumulating, so every subsequent batch rewrites it; when
        // it finally completes, the batch that crosses the day boundary writes
        // its exact end-of-day values atomically and this pass never revisits it.
        let mut today_snapshot = latest_snapshot;
        exact_tip.apply_to(&mut today_snapshot);
        let date_key = today_snapshot.date.replace('-', "");
        let snap_key =
            keys::encode_stats_key(keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT, date_key.as_bytes());
        let snap_value = bincode::serialize(&today_snapshot)?;
        dao_batch.put_stats(&snap_key, &snap_value);

        dao_batch.commit()?;

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

fn estimated_apc_from_header(header: &CachedBlockHeader, genesis_issuance: i128) -> Option<String> {
    if header.epoch_length == 0 {
        return None;
    }
    let apc = calculate_estimated_apc(
        header.epoch_number,
        header.epoch_index,
        header.epoch_length,
        genesis_issuance,
    );
    (apc > 0.0).then(|| format!("{:.2}", apc))
}

fn format_days(days: f64) -> String {
    if days >= 1000.0 {
        format!("{:.1}K days+", days / 1000.0)
    } else if days < 1.0 && days > 0.0 {
        format!("{:.1} days", days)
    } else {
        format!("{:.0} days", days)
    }
}

/// Calculates knowledge_size from DAO field bytes.
///
/// `virtual_occupied` is the genesis-derived burn adjustment sourced from the
/// persisted `GenesisBaseline` (`GenesisBaseline::virtual_occupied`). It is
/// subtracted from the DAO `U` field so mainnet and testnet share one single
/// calculation path with a network-correct constant instead of a hardcoded
/// mainnet-only literal.
pub fn calculate_knowledge_size(dao_field: &[u8], virtual_occupied: i128) -> Option<i128> {
    if dao_field.len() >= 32 {
        let bytes: [u8; 8] = dao_field[24..32].try_into().ok()?;
        let u_field = u64::from_le_bytes(bytes) as i128;
        Some(u_field - virtual_occupied)
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

    /// Seed the genesis baseline the indexer always derives at block 0. Refresh
    /// paths that compute estimated APC read `GenesisBaseline::total_issuance`,
    /// so tests exercising them must seed it first. Values mirror mainnet genesis.
    fn seed_test_genesis_baseline(store: &Arc<CkbadgerStore>) {
        store
            .set_genesis_baseline(&ckbadger_store::GenesisBaseline {
                // Exact mainnet genesis DAO `C` (not the rounded 33.6B).
                total_issuance: 3_360_000_145_238_488_200,
                burnt: 840_000_000_000_000_000,
                virtual_occupied: 504_000_000_000_000_000,
            })
            .unwrap();
    }

    #[test]
    fn test_calculate_knowledge_size_extracts_u_field() {
        let mut dao = vec![0u8; 32];
        let u_value: u64 = 600_000_000_000_000_000;
        dao[24..32].copy_from_slice(&u_value.to_le_bytes());

        let result = calculate_knowledge_size(&dao, BURN_ADJUSTMENT);
        assert!(result.is_some());
        let expected = u_value as i128 - BURN_ADJUSTMENT;
        assert_eq!(result.unwrap(), expected);
        assert_eq!(result.unwrap(), 96_000_000_000_000_000);
    }

    #[test]
    fn test_calculate_knowledge_size_returns_none_for_short_dao() {
        let short_dao = vec![0u8; 24];
        assert!(calculate_knowledge_size(&short_dao, BURN_ADJUSTMENT).is_none());

        let empty_dao: Vec<u8> = vec![];
        assert!(calculate_knowledge_size(&empty_dao, BURN_ADJUSTMENT).is_none());
    }

    #[test]
    fn test_calculate_knowledge_size_handles_minimum_u_value() {
        let mut dao = vec![0u8; 32];
        let u_value: u64 = BURN_ADJUSTMENT as u64;
        dao[24..32].copy_from_slice(&u_value.to_le_bytes());

        let result = calculate_knowledge_size(&dao, BURN_ADJUSTMENT);
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
                    occupied_capacity: 0,
                    deposit_block_number: 10,
                    deposit_timestamp: 0,
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
                    occupied_capacity: 0,
                    deposit_block_number: 15,
                    deposit_timestamp: 0,
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
        seed_test_genesis_baseline(&store);
        let writer = BatchWriter::new(store.clone(), store.clone());

        // AR = 2, S = 130 CKB.
        // unmade_comp for status-0 deposit: (200-102)*2/1 - (200-102) = 98 CKB
        // treasury = S - unmade_comp = 130 - 98 = 32 CKB
        let mut dao = vec![0u8; 32];
        dao[8..16].copy_from_slice(&2u64.to_le_bytes());
        dao[16..24].copy_from_slice(&130_00000000u64.to_le_bytes());
        let mut seed = StoreBatch::new(&store);
        seed.put_block_header(
            10,
            &CachedBlockHeader {
                hash: vec![0x11; 32],
                parent_hash: vec![0u8; 32],
                timestamp: 1_700_000_000_000,
                epoch_number: 0,
                epoch_index: 0,
                epoch_length: 1,
                dao,
                transactions_count: 1,
                uncles_count: 0,
                proposals_count: 0,
                compact_target: 0,
                miner_lock_hash: None,
                cycles: None,
            },
        );
        seed.put_dao_deposit(
            &keys::encode_outpoint(&[0xAA; 32], 0),
            &DaoDepositCacheEntry {
                capacity: 200_00000000,
                occupied_capacity: 102_00000000,
                deposit_block_number: 10,
                deposit_timestamp: 1_700_000_000_000, // same as tip => 0 days held
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
                occupied_capacity: 102_00000000,
                deposit_block_number: 5,
                deposit_timestamp: 1_699_999_000_000,
                lock_script_hash: vec![0x02; 32],
                deposit_ar: 1,
                status: 2,
                withdraw_request_tx: Some(vec![0x03; 32]),
                withdraw_request_output_index: Some(0),
                withdraw_request_block: Some(8),
                withdraw_request_ar: Some(2),
                withdraw_block: Some(9),
                withdraw_tx: Some(vec![0x04; 32]),
                withdraw_to_output_index: Some(0),
                compensation: Some(198_00000000),
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
            unmade_dao_interests: 0,
            cumulative_depositors: 0,
            daily_depositor_addresses: 0,
            protocol_deposited: None,
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
        assert_eq!(latest.total_compensation_paid, 198_00000000);
        assert_eq!(latest.unclaimed_compensation, 98_00000000);
        assert_eq!(latest.average_deposit_days, "0 days");
        assert!(!latest.estimated_apc.is_empty());
        assert_eq!(latest.mining_reward, 10_00000000);
        assert_eq!(latest.deposit_compensation, 296_00000000);
        assert_eq!(latest.burnt, 32_00000000);

        // Verify snapshot's unmade_dao_interests and secondary_pool were patched
        // to the live tip values, ensuring chart and stats agree.
        let patched_snapshot = store.get_latest_dao_daily_snapshot().unwrap().unwrap();
        assert_eq!(patched_snapshot.compensation, 198_00000000);
        assert_eq!(patched_snapshot.cum_dao_compensation, 296_00000000);
        assert_eq!(patched_snapshot.unmade_dao_interests, 98_00000000);
        assert_eq!(patched_snapshot.secondary_pool, 130_00000000); // tip_s
    }

    #[test]
    fn test_cross_day_refresh_finalizes_previous_snapshot_exactly() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        seed_test_genesis_baseline(&store);
        let writer = BatchWriter::new(store.clone(), store.clone());

        let previous_date = NaiveDate::from_ymd_opt(2026, 7, 24).unwrap();
        let tip_date = previous_date + Duration::days(1);
        let previous_timestamp = DateTime::parse_from_rfc3339("2026-07-24T15:59:59Z")
            .unwrap()
            .timestamp_millis();
        let tip_timestamp = DateTime::parse_from_rfc3339("2026-07-24T16:00:08Z")
            .unwrap()
            .timestamp_millis();

        let dao_field = |ar: u64, secondary_pool: u64| {
            let mut dao = vec![0u8; 32];
            dao[8..16].copy_from_slice(&ar.to_le_bytes());
            dao[16..24].copy_from_slice(&secondary_pool.to_le_bytes());
            dao
        };
        let header = |number: i64, timestamp: i64, dao: Vec<u8>| CachedBlockHeader {
            hash: vec![u8::try_from(number).unwrap(); 32],
            parent_hash: vec![0u8; 32],
            timestamp,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 1,
            dao,
            transactions_count: 1,
            uncles_count: 0,
            proposals_count: 0,
            compact_target: 0,
            miner_lock_hash: None,
            cycles: None,
        };
        let snapshot = |date: NaiveDate| DaoDailySnapshot {
            date: date.format("%Y-%m-%d").to_string(),
            total_deposited: 202_00000000,
            depositors_count: 1,
            new_deposits: 1,
            withdrawals: 0,
            compensation: 0,
            cumulative_deposit_amount: 202_00000000,
            total_issuance: 9_000_000_000i128 * 100_000_000i128,
            secondary_pool: 0,
            occupied_capacity: 0,
            cum_miner_secondary: 0,
            cum_dao_compensation: 0,
            cum_treasury: 0,
            unclaimed_compensation: 0,
            unmade_dao_interests: 0,
            cumulative_depositors: 1,
            daily_depositor_addresses: 0,
            protocol_deposited: Some(202_00000000),
        };

        let mut seed = StoreBatch::new(&store);
        seed.put_block_header(
            10,
            &header(10, previous_timestamp, dao_field(200, 300_00000000)),
        );
        seed.put_block_header(11, &header(11, tip_timestamp, dao_field(300, 500_00000000)));
        seed.put_dao_deposit(
            &keys::encode_outpoint(&[0xAA; 32], 0),
            &DaoDepositCacheEntry {
                capacity: 202_00000000,
                occupied_capacity: 102_00000000,
                deposit_block_number: 1,
                deposit_timestamp: previous_timestamp - 86_400_000,
                lock_script_hash: vec![0x01; 32],
                deposit_ar: 100,
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
        for date in [previous_date, tip_date] {
            let key = keys::encode_stats_key(
                keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT,
                date.format("%Y%m%d").to_string().as_bytes(),
            );
            seed.put_stats(&key, &bincode::serialize(&snapshot(date)).unwrap());
        }
        seed.commit().unwrap();

        // One atomic batch, exactly as the live cross-day path builds it: the
        // completed day is materialized from the exact lifecycle before the
        // commit, the tip day keeps its carried-forward placeholders.
        let mut batch = StoreBatch::new(&store);
        let mut completed_input = placeholder_snapshot_input();
        writer
            .apply_exact_completed_dao_snapshot(
                DaoSnapshotBoundary {
                    date: previous_date,
                    end_block: 10,
                },
                200,
                300_00000000,
                &HashMap::new(),
                &HashMap::new(),
                &mut completed_input,
            )
            .unwrap();
        writer
            .update_dao_daily_snapshot(previous_date, &completed_input, &mut batch)
            .unwrap();
        writer
            .update_dao_daily_snapshot(tip_date, &placeholder_snapshot_input(), &mut batch)
            .unwrap();
        batch.commit().unwrap();

        // No separate refresh step has run: the completed day must already be
        // exact the instant its row exists.
        let previous = store.get_dao_daily_snapshot("20260724").unwrap().unwrap();
        assert_eq!(
            previous.cum_dao_compensation, 100_00000000,
            "the completed day must be exact in the same commit that writes it"
        );
        assert_eq!(previous.unclaimed_compensation, 100_00000000);
        assert_eq!(previous.unmade_dao_interests, 100_00000000);
        assert_eq!(previous.cum_treasury, 200_00000000);
        assert_eq!(previous.secondary_pool, 300_00000000);

        writer.refresh_latest_dao_statistics().unwrap();

        let latest = store.get_dao_daily_snapshot("20260725").unwrap().unwrap();
        assert_eq!(
            latest.cum_dao_compensation, 200_00000000,
            "the current day must still be refreshed at the live tip"
        );

        let previous_after_refresh = store.get_dao_daily_snapshot("20260724").unwrap().unwrap();
        assert_eq!(
            bincode::serialize(&previous_after_refresh).unwrap(),
            bincode::serialize(&previous).unwrap(),
            "the tip refresh must not revisit a completed day"
        );
    }

    #[test]
    fn test_completed_dao_snapshot_observes_deposits_staged_in_the_same_batch() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        seed_test_genesis_baseline(&store);
        let writer = BatchWriter::new(store.clone(), store.clone());

        let completed_date = NaiveDate::from_ymd_opt(2026, 7, 24).unwrap();

        // A deposit created by the very batch that closes this day is not in the
        // committed store yet, so a pre-commit store scan alone would miss it.
        let staged_deposit = DaoDepositCacheEntry {
            capacity: 202_00000000,
            occupied_capacity: 102_00000000,
            deposit_block_number: 7,
            deposit_timestamp: 0,
            lock_script_hash: vec![0x01; 32],
            deposit_ar: 100,
            status: 0,
            withdraw_request_tx: None,
            withdraw_request_output_index: None,
            withdraw_request_block: None,
            withdraw_request_ar: None,
            withdraw_block: None,
            withdraw_tx: None,
            withdraw_to_output_index: None,
            compensation: None,
        };
        let mut staged_entries = HashMap::new();
        staged_entries.insert(
            keys::encode_outpoint(&[0xAA; 32], 0).to_vec(),
            staged_deposit,
        );

        let mut without_overlay = placeholder_snapshot_input();
        writer
            .apply_exact_completed_dao_snapshot(
                DaoSnapshotBoundary {
                    date: completed_date,
                    end_block: 10,
                },
                200,
                300_00000000,
                &HashMap::new(),
                &HashMap::new(),
                &mut without_overlay,
            )
            .unwrap();
        assert_eq!(
            without_overlay.cum_dao_compensation, 0,
            "sanity: the committed store holds no DAO deposits in this fixture"
        );

        let mut with_overlay = placeholder_snapshot_input();
        writer
            .apply_exact_completed_dao_snapshot(
                DaoSnapshotBoundary {
                    date: completed_date,
                    end_block: 10,
                },
                200,
                300_00000000,
                &staged_entries,
                &HashMap::new(),
                &mut with_overlay,
            )
            .unwrap();

        // free = 202 - 102 = 100 CKB; compensation = 100 * 200/100 - 100 = 100 CKB.
        assert_eq!(with_overlay.cum_dao_compensation, 100_00000000);
        assert_eq!(with_overlay.unclaimed_compensation, 100_00000000);
        assert_eq!(with_overlay.unmade_dao_interests, 100_00000000);
        assert_eq!(with_overlay.cum_treasury, 200_00000000);
        assert_eq!(with_overlay.total_compensation, 0);
    }

    #[test]
    fn test_completed_dao_snapshot_claims_completions_staged_in_the_same_batch() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        seed_test_genesis_baseline(&store);
        let writer = BatchWriter::new(store.clone(), store.clone());

        let completed_date = NaiveDate::from_ymd_opt(2026, 7, 24).unwrap();
        let deposit_outpoint = keys::encode_outpoint(&[0xBB; 32], 0);

        // Committed phase-1 deposit: frozen at request AR 200.
        // free = 202 - 102 = 100 CKB; frozen = 100 * 200/100 - 100 = 100 CKB.
        let mut seed = StoreBatch::new(&store);
        seed.put_dao_deposit(
            &deposit_outpoint,
            &DaoDepositCacheEntry {
                capacity: 202_00000000,
                occupied_capacity: 102_00000000,
                deposit_block_number: 1,
                deposit_timestamp: 0,
                lock_script_hash: vec![0x02; 32],
                deposit_ar: 100,
                status: 1,
                withdraw_request_tx: Some(vec![0x03; 32]),
                withdraw_request_output_index: Some(0),
                withdraw_request_block: Some(5),
                withdraw_request_ar: Some(200),
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );
        seed.commit().unwrap();

        // Without the staged completion the frozen value is still unclaimed.
        let mut pending = placeholder_snapshot_input();
        writer
            .apply_exact_completed_dao_snapshot(
                DaoSnapshotBoundary {
                    date: completed_date,
                    end_block: 10,
                },
                400,
                300_00000000,
                &HashMap::new(),
                &HashMap::new(),
                &mut pending,
            )
            .unwrap();
        assert_eq!(pending.unclaimed_compensation, 100_00000000);
        assert_eq!(pending.total_compensation, 0);
        assert_eq!(pending.cum_dao_compensation, 100_00000000);

        // The same batch stages the phase-2 completion at block 8, so by the end
        // of this day the frozen value is claimed, not unclaimed.
        let mut staged_completions = HashMap::new();
        staged_completions.insert(deposit_outpoint.to_vec(), (8i64, vec![0x04; 32]));

        let mut claimed = placeholder_snapshot_input();
        writer
            .apply_exact_completed_dao_snapshot(
                DaoSnapshotBoundary {
                    date: completed_date,
                    end_block: 10,
                },
                400,
                300_00000000,
                &HashMap::new(),
                &staged_completions,
                &mut claimed,
            )
            .unwrap();
        assert_eq!(
            claimed.total_compensation, 100_00000000,
            "a completion staged in this batch must be claimed by the day it closes"
        );
        assert_eq!(claimed.unclaimed_compensation, 0);
        assert_eq!(claimed.unmade_dao_interests, 0);
        assert_eq!(
            claimed.cum_dao_compensation, 100_00000000,
            "the cumulative total is unchanged by the claimed/unclaimed split"
        );
        assert_eq!(claimed.cum_treasury, 300_00000000);
    }

    #[test]
    fn test_completed_dao_snapshot_fails_fast_on_a_completion_without_a_committed_deposit() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        seed_test_genesis_baseline(&store);
        let writer = BatchWriter::new(store.clone(), store.clone());

        let mut staged_completions = HashMap::new();
        staged_completions.insert(
            keys::encode_outpoint(&[0xCC; 32], 0).to_vec(),
            (8i64, vec![0x04; 32]),
        );

        let mut input = placeholder_snapshot_input();
        let error = writer
            .apply_exact_completed_dao_snapshot(
                DaoSnapshotBoundary {
                    date: NaiveDate::from_ymd_opt(2026, 7, 24).unwrap(),
                    end_block: 10,
                },
                200,
                300_00000000,
                &HashMap::new(),
                &staged_completions,
                &mut input,
            )
            .unwrap_err();
        assert!(
            format!("{error:#}").contains("staged DAO completion refers to an uncommitted deposit"),
            "unexpected error: {error:#}"
        );
    }

    /// The carried-forward values a batch stages before any exact evaluation.
    fn placeholder_snapshot_input() -> DaoSnapshotInput {
        DaoSnapshotInput {
            total_deposited: 202_00000000,
            depositors_count: 1,
            total_deposit_count: 1,
            total_withdrawal_count: 0,
            total_compensation: 0,
            cumulative_deposit_amount: 202_00000000,
            total_issuance: 9_000_000_000i128 * 100_000_000i128,
            secondary_pool: 0,
            occupied_capacity: 0,
            cum_miner_secondary: 0,
            cum_dao_compensation: 0,
            cum_treasury: 0,
            unmade_dao_interests: 0,
            unclaimed_compensation: 0,
            cumulative_depositors: 1,
            daily_depositor_addresses: 0,
            protocol_deposited: Some(202_00000000),
        }
    }

    #[test]
    fn test_refresh_dao_statistics_excludes_status1_from_explorer_totals() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        seed_test_genesis_baseline(&store);
        let writer = BatchWriter::new(store.clone(), store.clone());

        // Tip block: AR=4, S=200 CKB, timestamp = day 10
        let tip_timestamp: i64 = 1_700_000_000_000;
        let mut dao = vec![0u8; 32];
        dao[8..16].copy_from_slice(&4u64.to_le_bytes());
        dao[16..24].copy_from_slice(&200_00000000u64.to_le_bytes());
        let mut seed = StoreBatch::new(&store);
        seed.put_block_header(
            100,
            &CachedBlockHeader {
                hash: vec![0x11; 32],
                parent_hash: vec![0u8; 32],
                timestamp: tip_timestamp,
                epoch_number: 1,
                epoch_index: 0,
                epoch_length: 100,
                dao,
                transactions_count: 1,
                uncles_count: 0,
                proposals_count: 0,
                compact_target: 0,
                miner_lock_hash: None,
                cycles: None,
            },
        );

        // Block 80 header (for status-1 deposit frozen time lookup)
        seed.put_block_header(
            80,
            &CachedBlockHeader {
                hash: vec![0x22; 32],
                parent_hash: vec![0u8; 32],
                timestamp: tip_timestamp - 86_400_000, // 1 day before tip
                epoch_number: 0,
                epoch_index: 0,
                epoch_length: 100,
                dao: vec![0u8; 32],
                transactions_count: 1,
                uncles_count: 0,
                proposals_count: 0,
                compact_target: 0,
                miner_lock_hash: None,
                cycles: None,
            },
        );

        // Status=0 deposit: active, deposited 2 days ago, AR at deposit = 2
        // free_capacity = 200_00000000 - 102_00000000 = 98_00000000
        // compensation(tip_ar=4): 98_00000000 * 4/2 - 98_00000000 = 98_00000000
        let two_days_ms: i64 = 2 * 86_400_000;
        seed.put_dao_deposit(
            &keys::encode_outpoint(&[0xAA; 32], 0),
            &DaoDepositCacheEntry {
                capacity: 200_00000000,
                occupied_capacity: 102_00000000,
                deposit_block_number: 50,
                deposit_timestamp: tip_timestamp - two_days_ms,
                lock_script_hash: vec![0x01; 32],
                deposit_ar: 2,
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

        // Status=1 deposit: withdraw requested, deposited 5 days ago, AR at deposit = 2, AR at request = 3
        // free_capacity = 300_00000000 - 102_00000000 = 198_00000000
        // compensation(withdraw_request_ar=3): 198_00000000 * 3/2 - 198_00000000 = 99_00000000
        let five_days_ms: i64 = 5 * 86_400_000;
        seed.put_dao_deposit(
            &keys::encode_outpoint(&[0xBB; 32], 0),
            &DaoDepositCacheEntry {
                capacity: 300_00000000,
                occupied_capacity: 102_00000000,
                deposit_block_number: 20,
                deposit_timestamp: tip_timestamp - five_days_ms,
                lock_script_hash: vec![0x02; 32],
                deposit_ar: 2,
                status: 1,
                withdraw_request_tx: Some(vec![0x03; 32]),
                withdraw_request_output_index: Some(0),
                withdraw_request_block: Some(80),
                withdraw_request_ar: Some(3),
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_to_output_index: None,
                compensation: None,
            },
        );

        // A completed deposit (status=2) for total_compensation_paid:
        // free=48 CKB, request/deposit AR=2, so exact compensation=48 CKB.
        seed.put_dao_deposit(
            &keys::encode_outpoint(&[0xCC; 32], 0),
            &DaoDepositCacheEntry {
                capacity: 150_00000000,
                occupied_capacity: 102_00000000,
                deposit_block_number: 5,
                deposit_timestamp: tip_timestamp - 10 * 86_400_000,
                lock_script_hash: vec![0x03; 32],
                deposit_ar: 1,
                status: 2,
                withdraw_request_tx: Some(vec![0x04; 32]),
                withdraw_request_output_index: Some(0),
                withdraw_request_block: Some(30),
                withdraw_request_ar: Some(2),
                withdraw_block: Some(40),
                withdraw_tx: Some(vec![0x05; 32]),
                withdraw_to_output_index: Some(0),
                compensation: Some(48_00000000),
            },
        );

        // Required: a dao daily snapshot
        let snapshot_key =
            keys::encode_stats_key(keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT, b"20260101");
        let snapshot = DaoDailySnapshot {
            date: "2026-01-01".to_string(),
            total_deposited: 500_00000000,
            depositors_count: 2,
            new_deposits: 3,
            withdrawals: 1,
            compensation: 0,
            cumulative_deposit_amount: 650_00000000,
            total_issuance: 9_000_000_000i128 * 100_000_000i128,
            secondary_pool: 0,
            occupied_capacity: 0,
            cum_miner_secondary: 10_00000000,
            cum_dao_compensation: 20_00000000,
            cum_treasury: 30_00000000,
            unclaimed_compensation: 0,
            unmade_dao_interests: 0,
            cumulative_depositors: 0,
            daily_depositor_addresses: 0,
            protocol_deposited: None,
        };
        let snapshot_val = bincode::serialize(&snapshot).unwrap();
        seed.put_stats(&snapshot_key, &snapshot_val);
        seed.commit().unwrap();

        writer.refresh_latest_dao_statistics().unwrap();
        let latest = store.get_latest_dao_statistics().unwrap().unwrap();

        // active_deposits still counts both status=0 and status=1
        assert_eq!(latest.active_deposits, 2);
        // total_deposited = status=0 only: 200_00000000
        assert_eq!(latest.total_deposited, 200_00000000);
        // 1 unique depositor (only status=0 lock hash 0x01)
        assert_eq!(latest.total_depositors, 1);
        // compensation_paid from the status=2 deposit
        assert_eq!(latest.total_compensation_paid, 48_00000000);
        // unclaimed_compensation: status=0 (tip_ar=4) + status=1 (withdraw_request_ar=3)
        // = 98_00000000 + 99_00000000 = 197_00000000
        assert_eq!(latest.unclaimed_compensation, 197_00000000);
        assert_eq!(latest.deposit_compensation, 245_00000000);
        // pending_withdrawal_capacity = status=1: 300_00000000
        assert_eq!(latest.pending_withdrawal_capacity, 300_00000000);
        // Capacity-weighted average: status-0 (200 CKB * 2 days) + status-1 (300 CKB * 4 days frozen)
        // = (400_00000000 + 1200_00000000) / 500_00000000 = 3.2 → "3 days"
        assert_eq!(latest.average_deposit_days, "3 days");
        assert_eq!(latest.tip_block_number, 100);
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
        seed.put_mnft_collection_aggregate(
            &mnft_collection,
            &MnftCollectionAggregate {
                standard: ObjectStandard::MnftClass,
                ..Default::default()
            },
        );
        seed.put_mnft_collection_aggregate(
            &spore_collection,
            &MnftCollectionAggregate {
                standard: ObjectStandard::SporeCluster,
                ..Default::default()
            },
        );
        seed.put_mnft_hourly_transfer(&mnft_collection, old_hour, 9);
        seed.put_mnft_hourly_transfer(&mnft_collection, current_hour, 3);
        seed.put_mnft_hourly_transfer(&spore_collection, old_hour, 7);
        seed.commit().unwrap();

        let deleted = writer.refresh_mnft_24h_transfers().unwrap();
        assert_eq!(deleted, 1);

        let mnft_old_key = keys::encode_object_hourly_key(&mnft_collection, old_hour);
        let mnft_new_key = keys::encode_object_hourly_key(&mnft_collection, current_hour);
        let spore_old_key = keys::encode_object_hourly_key(&spore_collection, old_hour);

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
                parent_hash: vec![0u8; 32],
                timestamp: i64::MAX,
                epoch_number: 0,
                epoch_index: 0,
                epoch_length: 1,
                dao: vec![0; 32],
                transactions_count: 1,
                uncles_count: 0,
                proposals_count: 0,
                compact_target: 0,
                miner_lock_hash: None,
                cycles: None,
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

    #[test]
    fn test_daily_stats_incremental_avg_matches_true_avg_across_batches() {
        // Regression: previously the incremental update folded the new batch
        // into the running average using `prev_count = blocks_count - 1`,
        // which assumed deltas-per-day == blocks-per-day - 1. For any
        // non-genesis day the first delta crosses the midnight boundary into
        // that day, so deltas-per-day == blocks-per-day and the formula
        // systematically under-weighted history. The fix accumulates raw sum +
        // count and derives the average on read.
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());
        let day = NaiveDate::from_ymd_opt(2024, 6, 1).unwrap();
        let key = keys::encode_stats_key(
            keys::STATS_PREFIX_DAILY,
            day.format("%Y%m%d").to_string().as_bytes(),
        );

        // Batch 1: 3 blocks with deltas 8000+9000+10000ms (avg 9000)
        let mut batch = StoreBatch::new(&store);
        writer
            .update_daily_statistics(
                day,
                3,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                None,
                Some((27_000, 3)),
                None,
                &mut batch,
            )
            .unwrap();
        batch.commit().unwrap();

        // Batch 2: 2 blocks with deltas 6000+7000ms (avg 6500)
        let mut batch = StoreBatch::new(&store);
        writer
            .update_daily_statistics(
                day,
                2,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                None,
                Some((13_000, 2)),
                None,
                &mut batch,
            )
            .unwrap();
        batch.commit().unwrap();

        // True average across all 5 deltas: (27000 + 13000) / 5 = 8000ms
        let stats: DailyStats =
            bincode::deserialize(&store.get_stats_key(&key).unwrap().unwrap()).unwrap();
        assert_eq!(
            stats.avg_block_time_ms(),
            Some(8000),
            "incremental updates must yield the true sum/count average"
        );
    }
}

#[cfg(test)]
mod epoch_stats_tests {
    use super::*;
    use std::sync::Arc;

    use ckbadger_store::CkbadgerStore;

    fn ts(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).unwrap()
    }

    #[test]
    fn incomplete_epoch_has_no_end_timestamp() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let mut batch = StoreBatch::new(&store);
        // epoch 10: 3 blocks out of 1800 length → incomplete
        writer
            .upsert_epoch_statistics_batch(
                10,
                1000,
                1002,
                1800,
                ts(1_700_000_000),
                ts(1_700_000_020),
                100,
                true,
                &mut batch,
            )
            .unwrap();
        batch.commit().unwrap();

        let stats = store.get_epoch_stats(10).unwrap().unwrap();
        assert_eq!(stats.blocks_count, 3);
        assert!(
            stats.end_timestamp.is_none(),
            "incomplete epoch must not have end_timestamp"
        );
    }

    #[test]
    fn complete_epoch_has_end_timestamp() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let mut batch = StoreBatch::new(&store);
        // epoch 10: 1800 blocks out of 1800 length → complete
        writer
            .upsert_epoch_statistics_batch(
                10,
                1000,
                2799,
                1800,
                ts(1_700_000_000),
                ts(1_700_014_400),
                5000,
                true,
                &mut batch,
            )
            .unwrap();
        batch.commit().unwrap();

        let stats = store.get_epoch_stats(10).unwrap().unwrap();
        assert_eq!(stats.blocks_count, 1800);
        assert!(
            stats.end_timestamp.is_some(),
            "complete epoch must have end_timestamp"
        );
    }

    #[test]
    fn upsert_sets_end_timestamp_on_completion() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        // First write: epoch 10, 3 blocks out of 5 → incomplete
        let mut batch = StoreBatch::new(&store);
        writer
            .upsert_epoch_statistics_batch(
                10,
                100,
                102,
                5,
                ts(1_700_000_000),
                ts(1_700_000_020),
                10,
                true,
                &mut batch,
            )
            .unwrap();
        batch.commit().unwrap();

        let stats = store.get_epoch_stats(10).unwrap().unwrap();
        assert!(
            stats.end_timestamp.is_none(),
            "incomplete epoch must not have end_timestamp"
        );

        // Second write: update with end_block=104, now 5/5 blocks → complete
        let mut batch = StoreBatch::new(&store);
        writer
            .upsert_epoch_statistics_batch(
                10,
                103,
                104,
                5,
                ts(1_700_000_030),
                ts(1_700_000_040),
                5,
                false,
                &mut batch,
            )
            .unwrap();
        batch.commit().unwrap();

        let stats = store.get_epoch_stats(10).unwrap().unwrap();
        assert_eq!(stats.blocks_count, 5);
        assert!(
            stats.end_timestamp.is_some(),
            "complete epoch must have end_timestamp after update"
        );
    }

    /// Regression (F1): a mid-epoch batch whose epoch row is missing is an
    /// invariant violation. The writer used to silently fabricate a row whose
    /// start_block/start_timestamp pointed at the batch start (e.g. a reorg
    /// replay start), corrupting epoch timing data. It must fail fast instead.
    #[test]
    fn mid_epoch_upsert_without_existing_row_fails_fast() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let mut batch = StoreBatch::new(&store);
        let err = writer
            .upsert_epoch_statistics_batch(
                10,
                150,
                160,
                1800,
                ts(1_700_000_000),
                ts(1_700_000_100),
                10,
                false,
                &mut batch,
            )
            .expect_err("mid-epoch upsert with no existing row must error");
        let msg = err.to_string();
        assert!(
            msg.contains("epoch") && msg.contains("10"),
            "error must identify the epoch: {msg}"
        );
    }
}

#[cfg(test)]
mod activity_stats_tests {
    use super::*;
    use std::sync::Arc;

    use ckbadger_store::types::{
        DailyActivityStats, ItemDelta, ParticipantDelta, ProtocolAction, TxActions, TypeCallEntry,
        ITEM_KIND_IDENTITY, ITEM_KIND_OBJECT, ITEM_KIND_TOKEN, TAG_DAO, TAG_IDENTITY, TAG_OBJECT,
        TAG_TOKEN, TAG_TYPE_CALL,
    };
    use ckbadger_store::CkbadgerStore;

    /// Build a TxActions for testing accumulate_tx_actions_stats.
    fn make_tx_actions(
        is_cellbase: bool,
        participants: Vec<ParticipantDelta>,
        protocol_actions: Vec<ProtocolAction>,
        type_calls: Vec<TypeCallEntry>,
    ) -> TxActions {
        TxActions {
            tx_hash: vec![0; 32],
            block_hash: vec![0; 32],
            block_number: 100,
            tx_index: 0,
            timestamp: 1700000000000,
            is_cellbase,
            protocol_actions,
            type_calls,
            lock_calls: vec![],
            participants,
        }
    }

    fn make_participant(ckb_delta: i128, tags: u16) -> ParticipantDelta {
        ParticipantDelta {
            lock_hash: vec![0xAA; 32],
            ckb_delta,
            used_delta: 0,
            item_deltas: vec![],
            tags,
        }
    }

    #[test]
    fn test_coinbase_classified_correctly() {
        let mut stats = DailyActivityStats::default();
        let actions = make_tx_actions(
            true,
            vec![make_participant(500_00000000, 0)],
            vec![],
            vec![],
        );
        accumulate_tx_actions_stats(&actions, &mut stats);
        assert_eq!(stats.coinbase_count, 1);
        assert_eq!(stats.transfer_count, 0);
        assert_eq!(stats.total_ckb_moved, 0);
        assert!(stats.script_counts.is_empty());
    }

    #[test]
    fn test_plain_transfer_classified_correctly() {
        let mut stats = DailyActivityStats::default();
        let actions = make_tx_actions(
            false,
            vec![make_participant(-100_00000000, 0)],
            vec![],
            vec![],
        );
        accumulate_tx_actions_stats(&actions, &mut stats);
        assert_eq!(stats.transfer_count, 1);
        assert_eq!(stats.coinbase_count, 0);
        assert_eq!(stats.total_ckb_moved, 100_00000000);
    }

    #[test]
    fn test_dao_deposit_classified_correctly() {
        let mut stats = DailyActivityStats::default();
        let actions = make_tx_actions(
            false,
            vec![make_participant(-200_00000000, TAG_DAO)],
            vec![ProtocolAction::new(
                "dao",
                "deposit",
                serde_json::json!({"capacity": 200_00000000i64}),
            )],
            vec![],
        );
        accumulate_tx_actions_stats(&actions, &mut stats);
        assert_eq!(stats.dao_deposit_count, 1);
        assert_eq!(stats.transfer_count, 0);
    }

    #[test]
    fn test_dao_withdraw_request_classified_correctly() {
        let mut stats = DailyActivityStats::default();
        let actions = make_tx_actions(
            false,
            vec![make_participant(0, TAG_DAO)],
            vec![ProtocolAction::new(
                "dao",
                "withdraw_request",
                serde_json::json!({}),
            )],
            vec![],
        );
        accumulate_tx_actions_stats(&actions, &mut stats);
        assert_eq!(stats.dao_withdraw_request_count, 1);
        assert_eq!(stats.transfer_count, 0);
    }

    #[test]
    fn test_dao_withdraw_complete_classified_correctly() {
        let mut stats = DailyActivityStats::default();
        let actions = make_tx_actions(
            false,
            vec![make_participant(200_00000000, TAG_DAO)],
            vec![ProtocolAction::new(
                "dao",
                "withdraw_complete",
                serde_json::json!({}),
            )],
            vec![],
        );
        accumulate_tx_actions_stats(&actions, &mut stats);
        assert_eq!(stats.dao_withdraw_complete_count, 1);
        assert_eq!(stats.transfer_count, 0);
    }

    #[test]
    fn test_token_transfer_classified_correctly() {
        let mut stats = DailyActivityStats::default();
        let actions = make_tx_actions(
            false,
            vec![{
                let mut p = make_participant(0, TAG_TOKEN);
                p.item_deltas = vec![ItemDelta {
                    item_id: vec![0xAA; 32],
                    kind: ITEM_KIND_TOKEN,
                    magnitude: 1000,
                    negative: false,
                }];
                p
            }],
            vec![],
            vec![],
        );
        accumulate_tx_actions_stats(&actions, &mut stats);
        assert_eq!(stats.token_count, 1);
        assert_eq!(stats.transfer_count, 0);
    }

    #[test]
    fn test_object_classified_correctly() {
        let mut stats = DailyActivityStats::default();
        let actions = make_tx_actions(
            false,
            vec![{
                let mut p = make_participant(0, TAG_OBJECT);
                p.item_deltas = vec![ItemDelta {
                    item_id: vec![0xBB; 32],
                    kind: ITEM_KIND_OBJECT,
                    magnitude: 1,
                    negative: false,
                }];
                p
            }],
            vec![],
            vec![],
        );
        accumulate_tx_actions_stats(&actions, &mut stats);
        assert_eq!(stats.object_count, 1);
        assert_eq!(stats.transfer_count, 0);
    }

    #[test]
    fn test_identity_classified_correctly() {
        let mut stats = DailyActivityStats::default();
        let actions = make_tx_actions(
            false,
            vec![{
                let mut p = make_participant(0, TAG_IDENTITY);
                p.item_deltas = vec![ItemDelta {
                    item_id: vec![0xCC; 32],
                    kind: ITEM_KIND_IDENTITY,
                    magnitude: 1,
                    negative: false,
                }];
                p
            }],
            vec![],
            vec![],
        );
        accumulate_tx_actions_stats(&actions, &mut stats);
        assert_eq!(stats.identity_count, 1);
        assert_eq!(stats.transfer_count, 0);
    }

    #[test]
    fn test_mixed_token_and_dao_counts_both() {
        let mut stats = DailyActivityStats::default();
        let actions = make_tx_actions(
            false,
            vec![{
                let mut p = make_participant(-500_00000000, TAG_TOKEN | TAG_DAO);
                p.item_deltas = vec![ItemDelta {
                    item_id: vec![0xAA; 32],
                    kind: ITEM_KIND_TOKEN,
                    magnitude: 1000,
                    negative: false,
                }];
                p
            }],
            vec![ProtocolAction::new("dao", "deposit", serde_json::json!({}))],
            vec![],
        );
        accumulate_tx_actions_stats(&actions, &mut stats);
        assert_eq!(stats.dao_deposit_count, 1);
        assert_eq!(stats.token_count, 1);
        assert_eq!(stats.transfer_count, 0);
    }

    #[test]
    fn test_multiple_txactions_accumulate() {
        let mut stats = DailyActivityStats::default();
        // 2 transfers + 1 coinbase
        accumulate_tx_actions_stats(
            &make_tx_actions(
                false,
                vec![make_participant(-50_00000000, 0)],
                vec![],
                vec![],
            ),
            &mut stats,
        );
        accumulate_tx_actions_stats(
            &make_tx_actions(
                false,
                vec![make_participant(30_00000000, 0)],
                vec![],
                vec![],
            ),
            &mut stats,
        );
        accumulate_tx_actions_stats(
            &make_tx_actions(
                true,
                vec![make_participant(100_00000000, 0)],
                vec![],
                vec![],
            ),
            &mut stats,
        );
        assert_eq!(stats.transfer_count, 2);
        assert_eq!(stats.coinbase_count, 1);
        assert_eq!(stats.total_ckb_moved, 80_00000000);
    }

    #[test]
    fn test_negative_delta_uses_absolute_value() {
        let mut stats = DailyActivityStats::default();
        let actions = make_tx_actions(
            false,
            vec![make_participant(-999_00000000, 0)],
            vec![],
            vec![],
        );
        accumulate_tx_actions_stats(&actions, &mut stats);
        assert_eq!(stats.total_ckb_moved, 999_00000000);
    }

    #[test]
    fn test_script_call_classified_correctly() {
        let mut stats = DailyActivityStats::default();
        let actions = make_tx_actions(
            false,
            vec![make_participant(-50_00000000, TAG_TYPE_CALL)],
            vec![],
            vec![TypeCallEntry {
                type_code_hash: vec![0xFF; 32],
                type_hash_type: 1,
                type_args: vec![0xEE; 20],
            }],
        );
        accumulate_tx_actions_stats(&actions, &mut stats);
        assert_eq!(stats.script_call_count, 1);
        assert_eq!(stats.transfer_count, 0);
        assert_eq!(stats.unknown_count, 0);
    }

    #[test]
    fn test_accumulate_protocol_action_counts() {
        let mut stats = DailyActivityStats::default();
        let actions = make_tx_actions(
            false,
            vec![make_participant(100, 0)],
            vec![ProtocolAction::new(
                "rgbpp",
                "leap_to_ckb",
                serde_json::json!({}),
            )],
            vec![],
        );
        accumulate_tx_actions_stats(&actions, &mut stats);
        assert_eq!(
            *stats
                .protocol_action_counts
                .get("rgbpp:leap_to_ckb")
                .unwrap(),
            1
        );
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
        let addrs: HashSet<[u8; 32]> = (0..3u8)
            .map(|i| {
                let mut h = [0u8; 32];
                h[0] = i;
                h
            })
            .collect();
        writer
            .update_hourly_activity_stats("2026030912", &stats, &addrs, &mut batch)
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

        // First write: 3 addresses (bytes 0, 1, 2)
        let mut batch = StoreBatch::new(&store);
        let s1 = DailyActivityStats {
            transfer_count: 5,
            total_ckb_moved: 50_00000000,
            ..Default::default()
        };
        let addrs1: HashSet<[u8; 32]> = (0..3u8)
            .map(|i| {
                let mut h = [0u8; 32];
                h[0] = i;
                h
            })
            .collect();
        writer
            .update_hourly_activity_stats("2026030912", &s1, &addrs1, &mut batch)
            .unwrap();
        batch.commit().unwrap();

        // Merge write: 7 addresses (bytes 1..8), 2 overlap with first batch
        let mut batch2 = StoreBatch::new(&store);
        let s2 = DailyActivityStats {
            transfer_count: 10,
            dao_deposit_count: 2,
            total_ckb_moved: 100_00000000,
            ..Default::default()
        };
        let addrs2: HashSet<[u8; 32]> = (1..8u8)
            .map(|i| {
                let mut h = [0u8; 32];
                h[0] = i;
                h
            })
            .collect();
        writer
            .update_hourly_activity_stats("2026030912", &s2, &addrs2, &mut batch2)
            .unwrap();
        batch2.commit().unwrap();

        let got = store
            .get_hourly_activity_stats("2026030912")
            .unwrap()
            .unwrap();
        assert_eq!(got.transfer_count, 15);
        assert_eq!(got.dao_deposit_count, 2);
        assert_eq!(got.total_ckb_moved, 150_00000000);
        // Cross-batch dedup: {0,1,2} ∪ {1,2,3,4,5,6,7} = 8 unique addresses
        assert_eq!(got.unique_address_count, 8);
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
