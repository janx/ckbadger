use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use rocksdb::{Direction, IteratorMode};
use std::collections::{BTreeSet, HashMap, HashSet};

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
#[cfg(test)]
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
    /// Unclaimed DAO compensation at this point (shannons): accruing on live
    /// deposits plus already frozen on phase-1 withdraw-request cells.
    pub unclaimed_compensation: i128,
    /// Exact compensation frozen on phase-1 withdraw-request cells.
    pub frozen_phase1_compensation: i128,
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
    frozen_phase1: i128,
    secondary_pool: i128,
}

impl ExactDaoSnapshotCompensation {
    fn from_breakdown(
        breakdown: DaoCompensationBreakdown,
        secondary_pool: i128,
        observation_block: i64,
    ) -> Result<Self> {
        let total = breakdown.total().ok_or_else(|| {
            anyhow!(
                "DAO total compensation overflow at observation block {}: claimed={}, unclaimed={}",
                observation_block,
                breakdown.claimed,
                breakdown.unclaimed
            )
        })?;
        let treasury = breakdown
            .treasury(secondary_pool)
            .with_context(|| format!("at observation block {observation_block}"))?;
        let frozen_phase1 = breakdown
            .frozen_phase1()
            .with_context(|| format!("at observation block {observation_block}"))?;
        Ok(Self {
            breakdown,
            total,
            treasury,
            frozen_phase1,
            secondary_pool,
        })
    }

    #[cfg(test)]
    fn apply_to_input(self, input: &mut DaoSnapshotInput) {
        input.total_compensation = self.breakdown.claimed;
        input.cum_dao_compensation = self.total;
        input.unclaimed_compensation = self.breakdown.unclaimed;
        input.frozen_phase1_compensation = self.frozen_phase1;
        input.secondary_pool = self.secondary_pool;
        input.cum_treasury = self.treasury;
    }
}

/// Outcome of one bounded hourly retention step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HourlyRetentionStepResult {
    pub deleted: u64,
    pub scanned: usize,
    /// `true` when the round reached the end of the family.
    pub completed: bool,
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
        let existing = batch.get_stats(&key)?;

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
        let existing = batch.get_stats(&key)?;

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
                    batch
                            .get_stats(&prev_key)?
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
        difficulty_sum: u128,
        block_count: i32,
        total_uncles: i32,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        let key = keys::encode_stats_key(
            keys::STATS_PREFIX_DAILY_BLOCK,
            date.format("%Y%m%d").to_string().as_bytes(),
        );
        let existing = batch.get_stats(&key)?;

        let stats =
            match existing {
                Some(val) => {
                    let mut s: DailyBlockStats = deserialize_stats(&val, "daily block stats")?;
                    s.block_count = s.block_count.checked_add(block_count).ok_or_else(|| {
                        anyhow::anyhow!("daily block count overflow: date={date}")
                    })?;
                    s.difficulty_sum = s
                        .difficulty_sum
                        .checked_add(difficulty_sum)
                        .ok_or_else(|| anyhow::anyhow!("daily difficulty overflow: date={date}"))?;
                    s.total_uncles = s.total_uncles.checked_add(total_uncles).ok_or_else(|| {
                        anyhow::anyhow!("daily uncle count overflow: date={date}")
                    })?;
                    s
                }
                None => DailyBlockStats {
                    difficulty_sum,
                    block_count,
                    total_uncles,
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
        let existing = batch.get_stats(&key)?;

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
        let existing = batch.get_stats(&key)?;

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

    fn update_chain_time_histogram_batch(
        &self,
        prefix: u8,
        kind: &str,
        bucket: i32,
        count: i32,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        if bucket < 0 || count <= 0 {
            anyhow::bail!("invalid {kind} histogram delta: bucket={bucket} count={count}");
        }
        let key = keys::encode_stats_key(prefix, &bucket.to_be_bytes());
        let previous = match batch.get_stats(&key)? {
            Some(bytes) => {
                let bytes: [u8; 4] = bytes.as_slice().try_into().map_err(|_| {
                    anyhow!(
                        "invalid {kind} histogram count length: bucket={bucket} expected=4 got={}",
                        bytes.len()
                    )
                })?;
                i32::from_le_bytes(bytes)
            }
            None => 0,
        };
        if previous < 0 {
            anyhow::bail!("negative {kind} histogram count: bucket={bucket} previous={previous}");
        }
        let next = previous.checked_add(count).ok_or_else(|| {
            anyhow!(
            "{kind} histogram count overflow: bucket={bucket} previous={previous} added={count}"
        )
        })?;
        batch.put_stats(&key, &next.to_le_bytes());
        Ok(())
    }

    pub fn update_block_time_distribution_batch(
        &self,
        bucket_seconds: i32,
        count: i32,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        self.update_chain_time_histogram_batch(
            keys::STATS_PREFIX_BLOCK_TIME_DIST,
            "block time",
            bucket_seconds,
            count,
            batch,
        )
    }

    pub fn update_epoch_time_distribution_batch(
        &self,
        bucket_minutes: i32,
        count: i32,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        self.update_chain_time_histogram_batch(
            keys::STATS_PREFIX_EPOCH_TIME_DIST,
            "epoch time",
            bucket_minutes,
            count,
            batch,
        )
    }

    /// Read a predecessor's immutable raw header timestamp for diagnostics.
    /// Statistics writers use `CachedBlockHeader.chain_time_ms` instead.
    pub fn get_previous_block_raw_timestamp(
        &self,
        block_number: i64,
    ) -> Result<Option<DateTime<Utc>>> {
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

    #[cfg(test)]
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
            frozen_phase1_compensation: dao_snapshot.frozen_phase1_compensation,
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

    /// Apply one block's activity counts to its bucket of `kind` (the
    /// block's UTC+8 day or hour) on top of the row the previous block left,
    /// and record the block's addresses the bucket has not seen yet.
    ///
    /// A bucket's unique addresses are its `ACTIVITY_*_ADDR_ROW` rows (bulk
    /// build writes them for the buckets open at the handoff tip). An address
    /// is new iff it has no row; each new one gets its row in this block's
    /// scope and raises `unique_address_count` by one. So a block journals one
    /// small entry per new address and its bucket row, never the bucket-wide
    /// set. A bucket's rows outlive its blocks only until the rollback
    /// coverage floor leaves it ([`Self::stage_activity_addr_retention`]).
    ///
    /// `addr_row_floor` is `sync_meta` → `activity_addr_row_floor`: the first
    /// UTC+8 hour whose address rows are kept (every earlier hour, and every
    /// day before its day, has none: pruned, or sealed by the bulk build,
    /// which records the handoff hour). A block landing in such a bucket
    /// would count every address again, so it is rebuild-required
    /// (finding 11).
    pub fn update_activity_bucket_stats(
        &self,
        kind: keys::ActivityBucketKind,
        bucket: &str,
        block_stats: &DailyActivityStats,
        block_addrs: &BTreeSet<[u8; 32]>,
        addr_row_floor: Option<&str>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
        if let Some(floor) = addr_row_floor {
            let first_kept = floor.get(..kind.bucket_len()).ok_or_else(|| {
                anyhow!("activity address row floor {floor:?} is not a UTC+8 hour label")
            })?;
            if bucket < first_kept {
                return Err(anyhow::Error::new(
                    crate::lifecycle::RebuildRequiredError::new(format!(
                        "{kind:?} activity bucket {bucket} is behind the activity address row \
                         floor {floor}: its address rows were pruned or sealed, so every \
                         address of the block would count again"
                    )),
                ));
            }
        }
        let key = kind.stats_key(bucket)?;
        let existing: Option<DailyActivityStats> = batch
            .get_stats(&key)?
            .map(|raw| deserialize_stats(&raw, "activity bucket stats"))
            .transpose()
            .with_context(|| format!("{kind:?} activity bucket {bucket}"))?;
        let mut new_addrs = 0u32;
        for lock_hash in block_addrs {
            let row_key = kind.addr_row_key(bucket, lock_hash)?;
            if batch.get_stats(&row_key)?.is_some() {
                if existing.is_none() {
                    bail!(
                        "{kind:?} activity bucket {bucket} has an address row for 0x{} but no stats row",
                        hex::encode(lock_hash)
                    );
                }
                continue;
            }
            batch.put_stats(&row_key, &[]);
            new_addrs = new_addrs.checked_add(1).ok_or_else(|| {
                anyhow!("{kind:?} activity bucket {bucket}: new address count overflow")
            })?;
        }
        let merged = match existing {
            Some(mut row) => {
                let previous_unique = row.unique_address_count;
                row.add_counts(block_stats).with_context(|| {
                    format!("merging {kind:?} activity stats for bucket {bucket}")
                })?;
                row.unique_address_count =
                    previous_unique.checked_add(new_addrs).ok_or_else(|| {
                        anyhow!(
                            "{kind:?} activity bucket {bucket}: unique_address_count overflow: {previous_unique} + {new_addrs}"
                        )
                    })?;
                row
            }
            None => {
                let mut row = block_stats.clone();
                row.unique_address_count = new_addrs;
                row
            }
        };
        batch.put_stats(&key, &bincode::serialize(&merged)?);
        Ok(())
    }

    /// Delete the activity address rows of every bucket the rollback
    /// coverage floor has left (C-3), in the live batch, journaled.
    ///
    /// `coverage_floor` is the floor this batch commits
    /// (`entity_stats_undo_contract`). A block reaches a bucket only while its
    /// chain time can still fall in it: every block above the floor has a
    /// chain time at or after the floor block's (chain time never decreases),
    /// and no rollback goes below the floor. So every bucket before the floor
    /// block's UTC+8 hour (and day) is final, and its rows — the dedup memory
    /// of a bucket no block reaches again — are deleted; the bucket stats rows
    /// are kept.
    ///
    /// `sync_meta` → `activity_addr_row_floor` records the hour pruned to, so
    /// a batch scans only the buckets between it and the floor's hour and
    /// never walks deleted rows again. Deletes and the record go through
    /// `batch`'s journal (the caller scopes it at the batch's first block): a
    /// rollback of that block restores the rows and the older record
    /// together, and the next batch, whose floor never moves down, prunes them
    /// again. The batch's own blocks lie above the floor, so they write no row
    /// this deletes. Returns the rows deleted.
    pub fn stage_activity_addr_retention(
        &self,
        batch: &mut StoreBatch,
        coverage_floor: i64,
    ) -> Result<u64> {
        if coverage_floor < 0 {
            return Ok(0);
        }
        let floor_time = self
            .store
            .get_block_header(coverage_floor)?
            .map(|header| header.chain_time_ms)
            .ok_or_else(|| {
                anyhow!(
                    "missing header of the coverage floor block while pruning activity \
                     address rows: block={coverage_floor}"
                )
            })?;
        let floor_hour = keys::ActivityBucketKind::Hourly.bucket_label(floor_time)?;
        let pruned_to = self.store.get_activity_addr_row_floor()?;
        // A record at or past the floor's hour leaves nothing behind the
        // floor: the bulk build records its handoff hour (finding 11), which
        // the coverage floor reaches only some 1000 blocks later.
        if pruned_to
            .as_deref()
            .is_some_and(|previous| previous >= floor_hour.as_str())
        {
            return Ok(0);
        }
        let mut deleted = 0u64;
        for kind in [
            keys::ActivityBucketKind::Daily,
            keys::ActivityBucketKind::Hourly,
        ] {
            // A day label is its UTC+8 hour label's first eight digits.
            let label = |hour: &str| hour[..kind.bucket_len()].to_string();
            let start = match pruned_to.as_deref() {
                Some(previous) => kind.addr_row_bucket_prefix(&label(previous))?,
                None => vec![kind.addr_row_prefix()],
            };
            let end = kind.addr_row_bucket_prefix(&label(&floor_hour))?;
            let rows = self.store.iterator_cf(
                self.store.cf_stats_chain(),
                rocksdb::IteratorMode::From(&start, rocksdb::Direction::Forward),
            );
            for item in rows {
                let (key, _) = item.map_err(|e| {
                    anyhow!("failed to iterate {kind:?} activity address rows for pruning: {e}")
                })?;
                if key.as_ref() >= end.as_slice() {
                    break;
                }
                batch.delete_stats(&key);
                deleted += 1;
            }
        }
        batch.put_activity_addr_row_floor(&floor_hour);
        Ok(deleted)
    }

    /// Hours of per-entity hourly transfer counters the read path relies on.
    pub const HOURLY_RETENTION_WINDOW_HOURS: i64 = 48;

    /// Largest number of keys one retention step may delete.
    pub const HOURLY_RETENTION_STEP_BUDGET: usize = 5_000;

    /// The cutoff hour a retention round may delete below.
    ///
    /// Two bounds, and the lower one wins:
    /// - the read contract: keep the last 48 hours;
    /// - the rollback contract: every hourly key a retained `EntityStats` undo
    ///   entry could restore must still exist. That is a BLOCK-derived bound,
    ///   read from the header at `tip - ENTITY_STATS_UNDO_RETAIN_BLOCKS`. The
    ///   old code assumed 36 blocks is always less than 48 hours; a stalled
    ///   chain breaks that assumption and would let retention delete a bucket
    ///   rollback still has to restore.
    ///
    /// `executed_cutoff_hour` is a floor: a clock that goes backwards must not
    /// lower a boundary whose deletions already happened.
    pub fn hourly_retention_cutoff_hour(
        &self,
        now_ms: i64,
        committed_tip: i64,
        already_executed: i64,
    ) -> Result<i64> {
        let by_clock = now_ms / 3_600_000 - Self::HOURLY_RETENTION_WINDOW_HOURS;
        // While the whole chain is inside the undo window there is no block
        // old enough to bound anything, and so no cutoff at all; the caller
        // skips the step (`hourly_retention_undo_window_block`). Answering
        // "the clock alone" here is what persisted `i64::MIN` as a cutoff.
        let undo_window_block = Self::hourly_retention_undo_window_block(committed_tip)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "hourly retention cutoff requested while the chain is inside the undo \
                     window: committed_tip={}, retain_blocks={}",
                    committed_tip,
                    crate::sync::ENTITY_STATS_UNDO_RETAIN_BLOCKS
                )
            })?;
        let by_undo_window = match self.store.get_block_header(undo_window_block)? {
            Some(header) => ckbadger_common::stats_bucket::stats_hour_index(header.chain_time_ms)?,
            None => {
                // A canonical block at or below the tip with no header is
                // store corruption. Turning that into "delete nothing"
                // would hide it behind a retention sweep that quietly stops
                // working.
                bail!(
                    "missing header for the entity-stats undo window block while computing \
                     the hourly retention cutoff: block={}, committed_tip={}, retain_blocks={}",
                    undo_window_block,
                    committed_tip,
                    crate::sync::ENTITY_STATS_UNDO_RETAIN_BLOCKS
                );
            }
        };
        Ok(by_clock.min(by_undo_window).max(already_executed))
    }

    /// The block whose header bounds hourly retention —
    /// `committed_tip - ENTITY_STATS_UNDO_RETAIN_BLOCKS` — or `None` while the
    /// whole chain is still inside the undo window, when there is none.
    pub fn hourly_retention_undo_window_block(committed_tip: i64) -> Option<i64> {
        let block = committed_tip - crate::sync::ENTITY_STATS_UNDO_RETAIN_BLOCKS;
        (block >= 0).then_some(block)
    }

    /// Delete up to `HOURLY_RETENTION_STEP_BUDGET` expired keys of one hourly
    /// family, staging both the deletions and the advanced state into `batch`.
    ///
    /// Runs inside the writer that owns every other chain-store write, so a
    /// deletion can never race an undo replay that is restoring the same key.
    pub fn stage_hourly_retention_step(
        &self,
        batch: &mut StoreBatch,
        family: ckbadger_store::types::HourlyRetentionFamily,
        cutoff_hour: i64,
        now_ms: i64,
    ) -> Result<HourlyRetentionStepResult> {
        use ckbadger_store::types::{
            HourlyRetentionFamily, HourlyRetentionState, HOURLY_RETENTION_POLICY_VERSION,
        };

        let (cf, prefix) = match family {
            HourlyRetentionFamily::Token => (
                self.store.cf_stats_token(),
                ckbadger_store::keys::STATS_PREFIX_TOKEN_HOURLY,
            ),
            HourlyRetentionFamily::Mnft => (
                self.store.cf_stats_mnft(),
                ckbadger_store::keys::STATS_PREFIX_OBJECT_HOURLY,
            ),
        };

        let existing = self.store.get_hourly_retention_state(family)?;
        let round_started_at = existing
            .as_ref()
            .filter(|state| state.cursor.is_some())
            .map(|state| state.round_started_at)
            .unwrap_or(now_ms);
        let start_key = existing
            .as_ref()
            .and_then(|state| state.cursor.clone())
            .unwrap_or_else(|| vec![prefix]);

        let mut deleted = 0u64;
        let mut scanned = 0usize;
        let mut cursor: Option<Vec<u8>> = None;
        // `OBJECT_HOURLY` also carries rows for Spore-cluster collections, which
        // have NO retention policy. Only mNFT classes expire; a whole-prefix
        // sweep would invent an expiry contract for Spore that nothing promises.
        //
        // The join has to go through `pad_id_32`: the hourly key stores the
        // collection id zero-padded to 32 bytes, while `cf_mnft_collection_agg`
        // is keyed by the RAW id — 24 bytes for every real mNFT class. Looking
        // the padded key up directly always missed, so no mNFT hourly row was
        // ever deleted while the retention state claimed the sweep had run.
        // Two writers put rows under `OBJECT_HOURLY`: `mnft.rs` keyed by an mNFT
        // class id (has an entry in `cf_mnft_collection_agg`) and `dotbit.rs`
        // keyed by `DOTBIT_SENTINEL_COLLECTION` (has one in `cf_identity_agg`
        // instead). Only mNFT classes expire; identity collections have no
        // retention policy, exactly like Spore clusters.
        let mut expiry_by_collection: HashMap<[u8; 32], bool> = HashMap::new();
        if family == HourlyRetentionFamily::Mnft {
            for (collection_id, agg) in self.store.list_mnft_collection_aggregates()? {
                expiry_by_collection.insert(
                    ckbadger_store::keys::pad_id_32(&collection_id),
                    agg.standard == ObjectStandard::MnftClass,
                );
            }
            for (collection_id, _) in self.store.list_identity_collection_aggregates()? {
                expiry_by_collection
                    .entry(ckbadger_store::keys::pad_id_32(&collection_id))
                    .or_insert(false);
            }
        }
        {
            let iter = self.store.iterator_cf(
                cf,
                rocksdb::IteratorMode::From(&start_key, rocksdb::Direction::Forward),
            );
            for item in iter {
                let (key, _) = item?;
                if key.first() != Some(&prefix) {
                    break;
                }
                if key.len() != 41 {
                    bail!(
                        "invalid {} hourly key length during retention: len={}, key=0x{}",
                        family.as_str(),
                        key.len(),
                        hex::encode(&key)
                    );
                }
                scanned += 1;
                let hour = i64::from_be_bytes(key[33..41].try_into().map_err(|_| {
                    anyhow::anyhow!(
                        "failed to decode {} hourly bucket during retention: key=0x{}",
                        family.as_str(),
                        hex::encode(&key)
                    )
                })?);
                let expires = match family {
                    HourlyRetentionFamily::Token => true,
                    HourlyRetentionFamily::Mnft => {
                        let padded: [u8; 32] = key[1..33].try_into().map_err(|_| {
                            anyhow::anyhow!(
                                "object hourly key has no 32-byte collection id: key=0x{}",
                                hex::encode(&key)
                            )
                        })?;
                        // An hourly row whose collection has NO aggregate of
                        // either kind is a broken invariant, not "does not
                        // expire". Silently answering `false` is exactly how the
                        // padding bug turned this sweep into a no-op.
                        match expiry_by_collection.get(&padded) {
                            Some(expires) => *expires,
                            None => bail!(
                                "object hourly bucket has no collection aggregate: \
                                 collection_id=0x{}, hour_bucket={}, key=0x{}",
                                hex::encode(padded),
                                hour,
                                hex::encode(&key)
                            ),
                        }
                    }
                };
                if expires && hour < cutoff_hour {
                    batch.delete_stats(&key);
                    deleted += 1;
                }
                if deleted as usize >= Self::HOURLY_RETENTION_STEP_BUDGET
                    || scanned >= Self::HOURLY_RETENTION_STEP_BUDGET * 4
                {
                    // Resume strictly after this key next round.
                    let mut next = key.to_vec();
                    next.push(0);
                    cursor = Some(next);
                    break;
                }
            }
        }

        let completed = cursor.is_none();
        let previous_executed = existing
            .as_ref()
            .map(|state| state.executed_cutoff_hour)
            .unwrap_or(i64::MIN);
        // `executed_cutoff_hour` is the store's claim about what it has ALREADY
        // deleted, and a reader uses it to tell legitimate retention from
        // corruption. A partial round has only swept up to `cursor`, so
        // advancing it there would claim deletions that have not happened for
        // every key past the cursor. It moves only when the round reaches the
        // end of the family; `round_in_progress_cutoff_hour` records what the
        // current round is working towards.
        let executed_cutoff_hour = if completed {
            cutoff_hour.max(previous_executed)
        } else {
            previous_executed
        };
        let state = HourlyRetentionState {
            policy_version: HOURLY_RETENTION_POLICY_VERSION,
            family,
            executed_cutoff_hour,
            round_in_progress_cutoff_hour: (!completed).then_some(cutoff_hour),
            cursor,
            round_started_at,
            round_completed_at: completed.then_some(now_ms),
        };
        batch.put_hourly_retention_state(&state);

        Ok(HourlyRetentionStepResult {
            deleted,
            scanned,
            completed,
        })
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
    #[cfg(test)]
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
    #[cfg(test)]
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
        ExactDaoSnapshotCompensation::from_breakdown(breakdown, secondary_pool, observation_block)
    }

    /// Refresh the DAO tip summary: the singleton statistics (latest stats,
    /// top depositors) and the daily snapshot of the tip's still-open UTC+8
    /// day, every one a function of the sync tip, from one `dao_deposits`
    /// scan. It runs after every tip change — each forward live batch, every
    /// rollback (reorg, startup cleanup, batch-range cleanup), startup and
    /// bulk finalize — and writes outside the block journal: a rollback
    /// replays the journal, then recomputes these rows at its new tip.
    ///
    /// The daily rows are split by ownership: a sealed day (a later day's
    /// block exists) is the forward writer's, written in its sealer block's
    /// scope and never revisited here; the open day's row is this summary's,
    /// and no row may exist for a day after the tip's (such a row can only be
    /// the open-day row of a former tip, which a journal replay restores as
    /// the previous value of a sealed write), so they are removed.
    ///
    /// Each call logs one debug line, [`DAO_TIP_SUMMARY_LOG`], with its
    /// elapsed time, the open day's headers walked and the live DAO cells
    /// scanned (the 24 h run's sampler reads it; keep the text and fields).
    pub fn refresh_latest_dao_statistics(&self) -> Result<()> {
        let started = std::time::Instant::now();
        let work = self.refresh_dao_statistics()?;
        tracing::debug!(
            elapsed_ms = started.elapsed().as_secs_f64() * 1_000.0,
            tip_block = work.tip_block,
            open_day_headers = work.open_day_headers,
            live_dao_cells = work.live_dao_cells,
            "{}",
            DAO_TIP_SUMMARY_LOG
        );
        Ok(())
    }

    fn refresh_dao_statistics(&self) -> Result<DaoTipSummaryWork> {
        let Some((tip_block_number, header)) = self.store.get_sync_tip_block()? else {
            return Ok(DaoTipSummaryWork {
                tip_block: -1,
                open_day_headers: 0,
                live_dao_cells: 0,
            });
        };

        let tip_ar = extract_ar_from_dao_field(&header.dao).ok_or_else(|| {
            anyhow!(
                "invalid DAO field in sync tip block while refreshing latest dao statistics: block_number={}, dao_len={}",
                tip_block_number,
                header.dao.len()
            )
        })?;
        let tip_timestamp = header.chain_time_ms;
        let (tip_total_issuance, tip_secondary_pool, tip_occupied_capacity) =
            ckbadger_common::dao::extract_dao_csu(&header.dao).ok_or_else(|| {
                anyhow!(
                    "invalid DAO field in sync tip block while refreshing latest dao statistics: block_number={}, dao_len={}",
                    tip_block_number,
                    header.dao.len()
                )
            })?;
        let tip_s = extract_s_from_dao(&header.dao).ok_or_else(|| {
            anyhow!(
                "invalid DAO S field in sync tip block while refreshing latest dao statistics: block_number={}, dao_len={}",
                tip_block_number,
                header.dao.len()
            )
        })?;
        let estimated_apc = estimated_apc_from_header(&header, self.genesis_total_issuance()?)
            .ok_or_else(|| anyhow!(
                "invalid estimated APC inputs at tip block {tip_block_number}: epoch={}:{}/{}, genesis issuance unavailable or nonpositive APC",
                header.epoch_number, header.epoch_index, header.epoch_length
            ))?;

        let mut total_deposited: i128 = 0;
        let mut pending_withdrawal_capacity: i128 = 0;
        let mut dao_capacity: i128 = 0;
        let mut dao_free_capacity: i128 = 0;
        let mut phase0_dao_free_capacity: i128 = 0;
        let mut unique_depositors: HashSet<Vec<u8>> = HashSet::new();
        let mut active_deposits = 0i32;
        let mut deposit_cells_count = 0i32;
        let mut withdrawal_request_cells_count = 0i32;
        let mut depositor_map: HashMap<Vec<u8>, (i128, i32, i128)> = HashMap::new();
        let mut weighted_deposit_ms: i128 = 0;
        let mut avg_total_capacity: i128 = 0;
        let mut chain_time_cache: HashMap<i64, i64> = HashMap::new();
        let mut chain_time_for = |block_number: i64| -> Result<i64> {
            if let Some(cached) = chain_time_cache.get(&block_number) {
                return Ok(*cached);
            }
            let block = self.store.get_block_header(block_number)?.ok_or_else(|| anyhow!(
                "missing DAO lifecycle block header while refreshing holding time: tip={tip_block_number}, block={block_number}"
            ))?;
            let value = block.chain_time_ms;
            chain_time_cache.insert(block_number, value);
            Ok(value)
        };

        for scan_status in [0i16, 1] {
            self.store
                .scan_dao_deposits_by_status(scan_status, |outpoint, entry| {
                    active_deposits = active_deposits.checked_add(1).context("live DAO cell count overflow")?;
                    let count = if scan_status == 0 { &mut deposit_cells_count } else { &mut withdrawal_request_cells_count };
                    *count = count.checked_add(1).with_context(|| format!("DAO phase {scan_status} cell count overflow at tip {tip_block_number}"))?;

                    // The live DAO cell is the deposit (phase 0) or the
                    // withdraw-request cell (phase 1), which keeps the
                    // deposit's capacity but may carry another lock.
                    let live_cell_occupied = match entry.status {
                        0 => entry.occupied_capacity,
                        _ => entry.withdraw_request_occupied_capacity.ok_or_else(|| {
                            anyhow!(
                                "phase-1 DAO entry has no withdraw_request_occupied_capacity: deposit_outpoint=0x{}, status={}",
                                hex::encode(outpoint),
                                entry.status
                            )
                        })?,
                    };
                    let free = entry.capacity.checked_sub(live_cell_occupied).filter(|free| *free >= 0)
                        .ok_or_else(|| {
                            anyhow!(
                                "live DAO cell occupies more than its capacity: deposit_outpoint=0x{}, status={}, capacity={}, occupied={}",
                                hex::encode(outpoint),
                                entry.status,
                                entry.capacity,
                                live_cell_occupied
                            )
                        })?;
                    dao_capacity += i128::from(entry.capacity);
                    dao_free_capacity += i128::from(free);
                    if entry.status == 0 {
                        phase0_dao_free_capacity += i128::from(free);
                    }

                    // Status-specific accounting
                    if entry.status == 1 {
                        pending_withdrawal_capacity += entry.capacity as i128;
                        let request_block = entry.withdraw_request_block.ok_or_else(|| anyhow!(
                            "phase-1 DAO entry missing request block while refreshing averages: deposit_outpoint=0x{}",
                            hex::encode(outpoint)
                        ))?;
                        let deposit_time = entry.deposit_chain_time_ms;
                        let request_time = chain_time_for(request_block)?;
                        let elapsed = request_time.checked_sub(deposit_time).filter(|v| *v >= 0)
                            .ok_or_else(|| anyhow!("negative/overflow DAO deposit duration while refreshing averages: deposit_outpoint=0x{}, deposit_block={}, request_block={request_block}, deposit_chain_time={deposit_time}, request_chain_time={request_time}", hex::encode(outpoint), entry.deposit_block_number))?;
                        let weighted = i128::from(entry.capacity).checked_mul(i128::from(elapsed))
                            .ok_or_else(|| anyhow!("DAO weighted holding duration overflow: deposit_outpoint=0x{}", hex::encode(outpoint)))?;
                        weighted_deposit_ms = weighted_deposit_ms.checked_add(weighted)
                            .ok_or_else(|| anyhow!("DAO weighted holding duration sum overflow at tip {tip_block_number}"))?;
                        avg_total_capacity = avg_total_capacity.checked_add(i128::from(entry.capacity))
                            .ok_or_else(|| anyhow!("DAO average capacity sum overflow at tip {tip_block_number}"))?;
                    } else {
                        // Only status=0 deposits count toward total_deposited,
                        // unique_depositors, depositor_map, and avg deposit time —
                        // matching CKB explorer convention which subtracts from
                        // total_deposit at phase-1 withdrawal.
                        total_deposited += entry.capacity as i128;
                        unique_depositors.insert(entry.lock_script_hash.clone());

                        let deposit_time = entry.deposit_chain_time_ms;
                        let held_ms = tip_timestamp.checked_sub(deposit_time).filter(|v| *v >= 0)
                            .ok_or_else(|| anyhow!("negative/overflow DAO live deposit duration: deposit_outpoint=0x{}, deposit_block={}, tip={tip_block_number}, deposit_chain_time={deposit_time}, tip_chain_time={tip_timestamp}", hex::encode(outpoint), entry.deposit_block_number))?;
                        let dm = depositor_map
                            .entry(entry.lock_script_hash.clone())
                            .or_insert((0, 0, 0));
                        dm.0 = dm.0.checked_add(i128::from(entry.capacity))
                            .ok_or_else(|| anyhow!("DAO top depositor capacity overflow: lock=0x{}", hex::encode(&entry.lock_script_hash)))?;
                        dm.1 = dm.1.checked_add(1)
                            .ok_or_else(|| anyhow!("DAO top depositor count overflow: lock=0x{}", hex::encode(&entry.lock_script_hash)))?;
                        dm.2 = dm.2.checked_add(i128::from(held_ms))
                            .ok_or_else(|| anyhow!("DAO top depositor duration overflow: lock=0x{}", hex::encode(&entry.lock_script_hash)))?;

                        let weighted = i128::from(entry.capacity).checked_mul(i128::from(held_ms))
                            .ok_or_else(|| anyhow!("DAO weighted holding duration overflow: deposit_outpoint=0x{}", hex::encode(outpoint)))?;
                        weighted_deposit_ms = weighted_deposit_ms.checked_add(weighted)
                            .ok_or_else(|| anyhow!("DAO weighted holding duration sum overflow at tip {tip_block_number}"))?;
                        avg_total_capacity = avg_total_capacity.checked_add(i128::from(entry.capacity))
                            .ok_or_else(|| anyhow!("DAO average capacity sum overflow at tip {tip_block_number}"))?;
                    }

                    Ok(())
                })?;
        }

        let cutoff_chain_time_ms =
            header
                .chain_time_ms
                .checked_sub(86_400_000)
                .ok_or_else(|| {
                    anyhow!(
                "DAO 24h cutoff underflow at tip block {tip_block_number}: chain_time_ms={}",
                header.chain_time_ms
            )
                })?;
        let baseline_observation = match self
            .store
            .find_last_block_at_or_before_chain_time_ms(cutoff_chain_time_ms)?
        {
            None => None,
            Some(block_number) => {
                let baseline_header = self.store.get_block_header(block_number)?.ok_or_else(|| {
                        anyhow!("missing DAO 24h baseline header at block {block_number}, tip={tip_block_number}")
                    })?;
                let ar = extract_ar_from_dao_field(&baseline_header.dao).ok_or_else(|| {
                    anyhow!(
                        "invalid DAO field at 24h baseline block {block_number}: dao_len={}",
                        baseline_header.dao.len()
                    )
                })?;
                Some((block_number, ar))
            }
        };
        // The open day: the tip's UTC+8 day, from its first block to the tip.
        let tip_day = ckbadger_common::stats_bucket::stats_day(header.chain_time_ms)?;
        let (open_day_start, open_day_end) = self
            .store
            .dao_day_block_range(tip_day, tip_block_number)?
            .ok_or_else(|| {
                anyhow!("the sync tip block {tip_block_number} is not on its own day {tip_day}")
            })?;
        if open_day_end != tip_block_number {
            bail!(
                "the open DAO day {tip_day} ends at block {open_day_end}, not at the sync tip {tip_block_number}"
            );
        }
        let mut open_day =
            ckbadger_store::DaoDayFacts::new(tip_day, open_day_start, tip_block_number)?;
        // One lifecycle scan: the exact partition at the tip, the 24 h
        // baseline's state, and the open day's lifecycle facts.
        let (breakdown, baseline_state) = self.store.scan_dao_tip_observations(
            tip_block_number,
            tip_ar,
            baseline_observation,
            &mut |entry| {
                open_day.observe(entry);
                Ok(())
            },
        )?;
        let exact_tip = ExactDaoSnapshotCompensation::from_breakdown(
            breakdown,
            i128::from(tip_s),
            tip_block_number,
        )?;
        let total_compensation_paid = exact_tip.breakdown.claimed;
        let unclaimed_compensation = exact_tip.breakdown.unclaimed;
        let deposit_compensation_total = exact_tip.total;
        // Secondary-issuance depositor weight: Σ_phase0 free_i × AR_tip / AR_dep_i
        // = phase-0 nominal free + phase-0 accrued compensation
        // = phase-0 free + (unclaimed − frozen_phase1).
        let depositor_allocation_weight = phase0_dao_free_capacity
            .checked_add(unclaimed_compensation)
            .and_then(|v| v.checked_sub(exact_tip.frozen_phase1))
            .ok_or_else(|| {
                anyhow!(
                    "DAO depositor allocation weight overflow/underflow at tip {tip_block_number}: phase0_free={phase0_dao_free_capacity}, unclaimed={unclaimed_compensation}, frozen_phase1={}",
                    exact_tip.frozen_phase1
                )
            })?;
        if depositor_allocation_weight < 0 {
            bail!(
                "negative DAO depositor allocation weight at tip {tip_block_number}: phase0_free={phase0_dao_free_capacity}, unclaimed={unclaimed_compensation}, frozen_phase1={}, weight={depositor_allocation_weight}",
                exact_tip.frozen_phase1
            );
        }

        // The open day's snapshot: the last sealed day's row plus the open
        // day's own facts, observed at the tip with the exact partition the
        // scan above computed.
        let open_day_snapshot = self.store.reduce_dao_day_snapshot(open_day, breakdown)?;
        let burnt = exact_tip.treasury;
        let mining_reward = open_day_snapshot.cum_miner_secondary;
        let deposit_compensation = deposit_compensation_total;

        let average_deposit_days =
            format_weighted_deposit_days(weighted_deposit_ms, avg_total_capacity)?;

        let total_depositors = i32::try_from(unique_depositors.len())
            .context("DAO depositor count exceeds i32 at latest tip")?;
        let rolling_24h = match (baseline_observation, baseline_state) {
            (None, None) => None,
            (Some((baseline_block_number, _)), Some(baseline)) => {
                let baseline_depositors = i32::try_from(baseline.depositors_count)
                    .context("DAO baseline depositor count exceeds i32")?;
                Some(DaoRolling24h {
                    baseline_block_number,
                    cutoff_chain_time_ms,
                    deposited_change: total_deposited.checked_sub(baseline.total_deposited)
                        .ok_or_else(|| anyhow!("DAO 24h deposited delta overflow at tip {tip_block_number}"))?,
                    depositors_change: total_depositors.checked_sub(baseline_depositors)
                        .ok_or_else(|| anyhow!("DAO 24h depositor delta overflow at tip {tip_block_number}"))?,
                    claimed_compensation_change: total_compensation_paid.checked_sub(baseline.compensation.claimed)
                        .ok_or_else(|| anyhow!("DAO 24h claimed compensation delta overflow at tip {tip_block_number}"))?,
                    unclaimed_compensation_change: unclaimed_compensation.checked_sub(baseline.compensation.unclaimed)
                        .ok_or_else(|| anyhow!("DAO 24h unclaimed compensation delta overflow at tip {tip_block_number}"))?,
                })
            }
            _ => unreachable!("baseline observation and result are paired"),
        };

        let tip_block_hash: [u8; 32] = header.hash.as_slice().try_into().map_err(|_| {
            anyhow!(
                "sync tip block hash has {} bytes while refreshing latest dao statistics: block_number={}",
                header.hash.len(),
                tip_block_number
            )
        })?;
        let latest = DaoLatestStatistics {
            tip_block_number,
            tip_block_hash,
            total_deposited,
            total_depositors,
            active_deposits,
            deposit_cells_count,
            withdrawal_request_cells_count,
            total_compensation_paid,
            unclaimed_compensation,
            average_deposit_days,
            estimated_apc,
            mining_reward,
            deposit_compensation,
            burnt,
            pending_withdrawal_capacity,
            dao_capacity,
            dao_free_capacity,
            depositor_allocation_weight,
            tip_total_issuance,
            tip_secondary_pool,
            tip_occupied_capacity,
            rolling_24h,
        };

        // Batch all DAO stats writes atomically, outside the journal as the
        // DAO tip summary (the one unjournaled writer of the open day's row).
        let mut dao_batch =
            StoreBatch::outside_journal(&self.store, ckbadger_store::OutsideWriter::DaoTipSummary);

        let key = keys::encode_stats_key(keys::STATS_PREFIX_DAO_LATEST_STATS, b"latest");
        let value = bincode::serialize(&latest)?;
        dao_batch.put_stats(&key, &value);

        // Build and store top depositors
        {
            let mut sorted: Vec<_> = depositor_map.into_iter().collect();
            sorted.sort_by_key(|item| std::cmp::Reverse(item.1 .0));
            sorted.truncate(100);

            let depositors = sorted
                .into_iter()
                .map(|(lock_hash, (total_capacity, deposit_count, total_ms))| {
                    DaoTopDepositorEntry {
                        lock_script_hash: lock_hash,
                        total_capacity,
                        deposit_count,
                        total_deposit_ms: total_ms,
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

        // The open day's row, and none for a later day.
        let open_day_key = keys::encode_stats_key(
            keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT,
            tip_day.format("%Y%m%d").to_string().as_bytes(),
        );
        dao_batch.put_stats(&open_day_key, &bincode::serialize(&open_day_snapshot)?);
        let mut later_days = 0usize;
        for item in self.store.iterator_cf(
            self.store.cf_stats_dao(),
            IteratorMode::From(&open_day_key, Direction::Forward),
        ) {
            let (key, _) = item.map_err(|e| {
                anyhow!("failed to iterate DAO daily snapshots after the open day {tip_day}: {e}")
            })?;
            if key.first() != Some(&keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT) {
                break;
            }
            if key.as_ref() != open_day_key.as_slice() {
                dao_batch.delete_stats(&key);
                later_days += 1;
            }
        }
        if later_days > 0 {
            tracing::info!(
                tip_block_number,
                %tip_day,
                later_days,
                "DAO tip summary removed the open-day rows of former tips after the tip's day"
            );
        }

        dao_batch.commit()?;

        Ok(DaoTipSummaryWork {
            tip_block: tip_block_number,
            open_day_headers: tip_block_number - open_day_start + 1,
            live_dao_cells: i64::from(active_deposits),
        })
    }
}

/// The debug message of one DAO tip summary refresh
/// (`refresh_latest_dao_statistics`); a sampler matches it verbatim.
pub const DAO_TIP_SUMMARY_LOG: &str = "DAO tip summary refreshed";

/// What one DAO tip summary refresh walked, for its debug line.
struct DaoTipSummaryWork {
    tip_block: i64,
    /// The open day's blocks, `open_day_start ..= tip`, whose headers the
    /// open-day snapshot walks.
    open_day_headers: i64,
    /// Live DAO cells (deposits and withdraw requests) the summary scanned.
    live_dao_cells: i64,
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

fn format_weighted_deposit_days(weighted_ms: i128, total_capacity: i128) -> Result<Option<String>> {
    if weighted_ms < 0 || total_capacity < 0 {
        bail!("negative DAO weighted holding inputs: weighted_ms={weighted_ms}, total_capacity={total_capacity}");
    }
    if total_capacity == 0 {
        if weighted_ms != 0 {
            bail!("DAO weighted holding time is nonzero without deposit capacity: weighted_ms={weighted_ms}");
        }
        return Ok(None);
    }
    let denominator = total_capacity
        .checked_mul(86_400_000)
        .context("DAO weighted holding denominator overflow")?;
    let round_scaled = |denominator: i128, scale: i128| -> Result<i128> {
        let scaled = weighted_ms
            .checked_mul(scale)
            .and_then(|v| v.checked_add(denominator / 2))
            .context("DAO weighted holding display overflow")?;
        Ok(scaled / denominator)
    };
    if weighted_ms
        >= denominator
            .checked_mul(1000)
            .context("DAO thousand-day threshold overflow")?
    {
        let thousand_denominator = denominator
            .checked_mul(1000)
            .context("DAO thousand-day denominator overflow")?;
        let tenths = round_scaled(thousand_denominator, 10)?;
        Ok(Some(format!("{}.{}K days+", tenths / 10, tenths % 10)))
    } else if weighted_ms > 0 && weighted_ms < denominator {
        let tenths = round_scaled(denominator, 10)?;
        Ok(Some(format!("{}.{} days", tenths / 10, tenths % 10)))
    } else {
        let days = round_scaled(denominator, 1)?;
        Ok(Some(format!("{days} days")))
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

    /// The real consensus `secondary_epoch_reward`, which the indexer
    /// persists at startup; the open day's miner series needs it.
    const TEST_SECONDARY_EPOCH_REWARD: u64 = 61_369_863_013_698;

    /// Singleton refresh tests seed synthetic DAO lifecycles directly. The
    /// DAO tip summary computes the open day's row on top of the last sealed
    /// day's, so seal every day before the tip's with the store's one day
    /// calculation, as the forward writer would have.
    fn seal_dao_days_before_tip(store: &Arc<CkbadgerStore>) {
        store
            .set_secondary_epoch_reward(TEST_SECONDARY_EPOCH_REWARD)
            .unwrap();
        let (tip_block, header) = store.get_sync_tip_block().unwrap().unwrap();
        let tip_day = ckbadger_common::stats_bucket::stats_day(header.chain_time_ms).unwrap();
        let genesis = store.get_block_header(0).unwrap().unwrap();
        let mut day = ckbadger_common::stats_bucket::stats_day(genesis.chain_time_ms).unwrap();
        while day < tip_day {
            let mut batch = StoreBatch::new(store);
            store
                .recompute_dao_daily_snapshot_for_date(day, tip_block, &mut batch)
                .unwrap();
            batch.commit().unwrap();
            day += Duration::days(1);
        }
    }

    /// `floor(s * U / C)` for one block of a one-block epoch (RFC-0023).
    fn one_block_epoch_miner_secondary(c: u64, u: u64) -> i128 {
        let s = ckbadger_common::dao::secondary_block_issuance(0, 1, TEST_SECONDARY_EPOCH_REWARD)
            .unwrap();
        ckbadger_common::dao::calculate_miner_secondary_issuance(s, i128::from(c), i128::from(u))
            .unwrap()
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
                    deposit_chain_time_ms: 0,
                    lock_script_hash: vec![0xAA; 32],
                    deposit_ar: 1,
                    status: 2,
                    withdraw_request_tx: Some(vec![0x01; 32]),
                    withdraw_request_output_index: Some(0),
                    withdraw_request_block: Some(20),
                    withdraw_request_ar: Some(2),
                    withdraw_block: Some(25),
                    withdraw_tx: Some(vec![0x02; 32]),
                    withdraw_request_occupied_capacity: Some(0),
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
                    deposit_chain_time_ms: 0,
                    lock_script_hash: vec![0xBB; 32],
                    deposit_ar: 1,
                    status: 0,
                    withdraw_request_tx: None,
                    withdraw_request_output_index: None,
                    withdraw_request_block: None,
                    withdraw_request_ar: None,
                    withdraw_block: None,
                    withdraw_tx: None,
                    withdraw_request_occupied_capacity: None,
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

        // AR = 2, S = 130 CKB, C = 9e9 CKB, U = C / 10.
        // unmade_comp for status-0 deposit: (200-102)*2/1 - (200-102) = 98 CKB
        // treasury = S - unmade_comp = 130 - 98 = 32 CKB
        const C: u64 = 9_000_000_000 * 100_000_000;
        const U: u64 = C / 10;
        let mut dao = vec![0u8; 32];
        dao[0..8].copy_from_slice(&C.to_le_bytes());
        dao[8..16].copy_from_slice(&2u64.to_le_bytes());
        dao[16..24].copy_from_slice(&130_00000000u64.to_le_bytes());
        dao[24..32].copy_from_slice(&U.to_le_bytes());
        let mut seed = StoreBatch::new(&store);
        let tip_header = CachedBlockHeader {
            hash: vec![0x11; 32],
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
            chain_time_ms: 1_700_000_000_000,
        };
        for block in 0..10 {
            let mut earlier = tip_header.clone();
            earlier.timestamp = 1_699_998_500_000 + block * 100_000;
            earlier.chain_time_ms = earlier.timestamp;
            seed.put_block_header(block, &earlier);
        }
        seed.put_block_header(10, &tip_header);
        seed.put_dao_deposit(
            &keys::encode_outpoint(&[0xAA; 32], 0),
            &DaoDepositCacheEntry {
                capacity: 200_00000000,
                occupied_capacity: 102_00000000,
                deposit_block_number: 10,
                deposit_timestamp: 1_700_000_000_000, // same as tip => 0 days held
                deposit_chain_time_ms: 1_700_000_000_000,
                lock_script_hash: vec![0x01; 32],
                deposit_ar: 1,
                status: 0,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_request_occupied_capacity: None,
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
                deposit_chain_time_ms: 1_699_999_000_000,
                lock_script_hash: vec![0x02; 32],
                deposit_ar: 1,
                status: 2,
                withdraw_request_tx: Some(vec![0x03; 32]),
                withdraw_request_output_index: Some(0),
                withdraw_request_block: Some(8),
                withdraw_request_ar: Some(2),
                withdraw_block: Some(9),
                withdraw_tx: Some(vec![0x04; 32]),
                withdraw_request_occupied_capacity: Some(102_00000000),
                withdraw_to_output_index: Some(0),
                compensation: Some(198_00000000),
            },
        );

        // A stale row for the open day (every block is on 2023-11-15 UTC+8):
        // the open day's row is the tip summary's, so the refresh replaces it
        // with the exact one rather than trusting or rejecting it.
        let snapshot_key =
            keys::encode_stats_key(keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT, b"20231115");
        let stale = DaoDailySnapshot {
            date: "2023-11-15".to_string(),
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
            frozen_phase1_compensation: 0,
            cumulative_depositors: 0,
            daily_depositor_addresses: 0,
            protocol_deposited: None,
        };
        seed.put_stats(&snapshot_key, &bincode::serialize(&stale).unwrap());
        // And the open-day row of a former tip on a later day, as a journal
        // replay to this tip restores it: no row may outlive the tip's day.
        let later_key = keys::encode_stats_key(keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT, b"20231116");
        seed.put_stats(
            &later_key,
            &bincode::serialize(&DaoDailySnapshot {
                date: "2023-11-16".to_string(),
                ..stale.clone()
            })
            .unwrap(),
        );
        seed.commit().unwrap();

        seal_dao_days_before_tip(&store);
        let (refreshed, logs) =
            crate::label_import::test_log_capture::capture_logs(tracing::Level::DEBUG, || {
                writer.refresh_latest_dao_statistics()
            });
        refreshed.unwrap();
        // One debug line per call, the 24 h run's sampler reads it: the open
        // day is blocks 0..=10 (one UTC+8 day), one live DAO cell (the other
        // deposit is withdrawn).
        let line = logs
            .lines()
            .find(|line| line.contains(super::DAO_TIP_SUMMARY_LOG))
            .unwrap_or_else(|| panic!("no DAO tip summary line in: {logs}"));
        assert!(line.contains("DEBUG"), "{line}");
        assert!(line.contains("elapsed_ms="), "{line}");
        assert!(line.contains("tip_block=10"), "{line}");
        assert!(line.contains("open_day_headers=11"), "{line}");
        assert!(line.contains("live_dao_cells=1"), "{line}");
        assert!(
            store.get_dao_daily_snapshot("20231116").unwrap().is_none(),
            "the refresh removes the rows of days after the tip's"
        );
        let latest = store.get_latest_dao_statistics().unwrap().unwrap();

        assert_eq!(latest.tip_block_number, 10);
        assert_eq!(
            latest.tip_block_hash, [0x11; 32],
            "the row names the sync-tip block it was computed at"
        );
        assert_eq!(latest.total_deposited, 200_00000000);
        assert_eq!(latest.total_depositors, 1);
        assert_eq!(latest.active_deposits, 1);
        assert_eq!(latest.total_compensation_paid, 198_00000000);
        assert_eq!(latest.unclaimed_compensation, 98_00000000);
        assert_eq!(latest.average_deposit_days.as_deref(), Some("0 days"));
        assert!(!latest.estimated_apc.is_empty());
        // The day starts at genesis: blocks 1-10 each split one one-block
        // epoch's secondary issuance against their parent's C/U.
        let per_block_miner = one_block_epoch_miner_secondary(C, U);
        assert!(per_block_miner > 0);
        assert_eq!(latest.mining_reward, 10 * per_block_miner);
        assert_eq!(latest.deposit_compensation, 296_00000000);
        assert_eq!(latest.burnt, 32_00000000);

        // The open day's row is the tip summary's exact row, the stale one
        // replaced: deposits at 5 and 10, the request at 8, the completion
        // at 9, the partition at the tip.
        let open_day = store.get_latest_dao_daily_snapshot().unwrap().unwrap();
        assert_eq!(open_day.date, "2023-11-15");
        assert_ne!(
            bincode::serialize(&open_day).unwrap(),
            bincode::serialize(&stale).unwrap()
        );
        assert_eq!(open_day.total_deposited, 200_00000000);
        assert_eq!(open_day.protocol_deposited, Some(200_00000000));
        assert_eq!(open_day.cumulative_deposit_amount, 500_00000000);
        assert_eq!(open_day.new_deposits, 2);
        assert_eq!(open_day.withdrawals, 1);
        assert_eq!(open_day.depositors_count, 1);
        assert_eq!(open_day.cumulative_depositors, 2);
        assert_eq!(open_day.daily_depositor_addresses, 2);
        assert_eq!(open_day.cum_miner_secondary, latest.mining_reward);
        assert_eq!(open_day.compensation, 198_00000000);
        assert_eq!(open_day.cum_dao_compensation, 296_00000000);
        assert_eq!(open_day.unclaimed_compensation, 98_00000000);
        assert_eq!(open_day.secondary_pool, 130_00000000); // tip_s

        // A second refresh at the same tip writes the same bytes.
        writer.refresh_latest_dao_statistics().unwrap();
        assert_eq!(
            bincode::serialize(&store.get_latest_dao_daily_snapshot().unwrap().unwrap()).unwrap(),
            bincode::serialize(&open_day).unwrap()
        );
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
            dao[0..8].copy_from_slice(&(9_000_000_000u64 * 100_000_000).to_le_bytes());
            dao[8..16].copy_from_slice(&ar.to_le_bytes());
            dao[16..24].copy_from_slice(&secondary_pool.to_le_bytes());
            dao
        };
        let header = |number: i64, timestamp: i64, dao: Vec<u8>| CachedBlockHeader {
            hash: vec![u8::try_from(number).unwrap(); 32],
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
            chain_time_ms: timestamp,
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
            frozen_phase1_compensation: 0,
            cumulative_depositors: 1,
            daily_depositor_addresses: 0,
            protocol_deposited: Some(202_00000000),
        };

        let mut seed = StoreBatch::new(&store);
        for block in 0..10 {
            let timestamp = previous_timestamp - 86_400_000 + (block - 1) * 1_000;
            seed.put_block_header(block, &header(block, timestamp, dao_field(100, 0)));
        }
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
                deposit_chain_time_ms: previous_timestamp - 86_400_000,
                lock_script_hash: vec![0x01; 32],
                deposit_ar: 100,
                status: 0,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_request_occupied_capacity: None,
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

        // Exercise a staged completed-day observation and an intentionally
        // stale tip row: the open day's row is the tip summary's, so the
        // refresh replaces it; the completed day is the forward writer's and
        // stays as it is.
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
        assert_eq!(previous.cum_treasury, 200_00000000);
        assert_eq!(previous.secondary_pool, 300_00000000);

        let stale = store.get_dao_daily_snapshot("20260725").unwrap().unwrap();
        assert_eq!(
            stale.cum_dao_compensation, 0,
            "sanity: the tip row is stale"
        );
        store
            .set_secondary_epoch_reward(TEST_SECONDARY_EPOCH_REWARD)
            .unwrap();
        writer.refresh_latest_dao_statistics().unwrap();

        let latest = store.get_dao_daily_snapshot("20260725").unwrap().unwrap();
        assert_eq!(
            latest.cum_dao_compensation, 200_00000000,
            "the tip summary computes the open day exactly"
        );
        assert_eq!(latest.unclaimed_compensation, 200_00000000);
        assert_eq!(latest.secondary_pool, 500_00000000);

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
            deposit_chain_time_ms: 0,
            lock_script_hash: vec![0x01; 32],
            deposit_ar: 100,
            status: 0,
            withdraw_request_tx: None,
            withdraw_request_output_index: None,
            withdraw_request_block: None,
            withdraw_request_ar: None,
            withdraw_block: None,
            withdraw_tx: None,
            withdraw_request_occupied_capacity: None,
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
                deposit_chain_time_ms: 0,
                lock_script_hash: vec![0x02; 32],
                deposit_ar: 100,
                status: 1,
                withdraw_request_tx: Some(vec![0x03; 32]),
                withdraw_request_output_index: Some(0),
                withdraw_request_block: Some(5),
                withdraw_request_ar: Some(200),
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_request_occupied_capacity: Some(102_00000000),
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
        // Regression: the protocol removes this deposit's interest from `S` only
        // at phase-2 completion, so while the request is pending the 100 CKB is
        // still inside `S` AND already counted in `cum_dao_compensation`.
        // Deriving treasury from the status-0 share alone left it in treasury
        // too, double-counting it across two buckets of one partition.
        assert_eq!(
            pending.cum_treasury, 200_00000000,
            "phase-1 frozen interest must be excluded from treasury"
        );
        assert_eq!(
            pending.cum_treasury + pending.unclaimed_compensation,
            300_00000000,
            "treasury + unclaimed must reconstruct the protocol's S exactly"
        );

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
            unclaimed_compensation: 0,
            frozen_phase1_compensation: 0,
            cumulative_depositors: 1,
            daily_depositor_addresses: 0,
            protocol_deposited: Some(202_00000000),
        }
    }

    /// A `DaoDepositCacheEntry` in `status` with the given capacities; phase-1
    /// and phase-2 entries carry the request cell's exact occupied capacity.
    fn dao_free_test_entry(
        capacity: i64,
        occupied_capacity: i64,
        status: i16,
        withdraw_request_occupied_capacity: Option<i64>,
    ) -> DaoDepositCacheEntry {
        DaoDepositCacheEntry {
            capacity,
            occupied_capacity,
            deposit_block_number: 50,
            deposit_timestamp: 1_700_000_000_000 - 86_400_000,
            deposit_chain_time_ms: 1_700_000_000_000 - 86_400_000,
            lock_script_hash: vec![status as u8 + 1; 32],
            deposit_ar: 2,
            status,
            withdraw_request_tx: (status >= 1).then(|| vec![0x03; 32]),
            withdraw_request_output_index: (status >= 1).then_some(0),
            withdraw_request_block: (status >= 1).then_some(80),
            withdraw_request_ar: (status >= 1).then_some(2),
            withdraw_block: (status == 2).then_some(90),
            withdraw_tx: (status == 2).then(|| vec![0x05; 32]),
            withdraw_request_occupied_capacity,
            withdraw_to_output_index: (status == 2).then_some(0),
            compensation: (status == 2).then_some(0),
        }
    }

    const DAO_FREE_FIXTURE_C: u64 = 4_790_000_000_000_000_000;
    const DAO_FREE_FIXTURE_S: u64 = 30_000_000_000_000_000;
    const DAO_FREE_FIXTURE_U: u64 = 519_967_746_700_000_000;

    /// Seed a tip (block 100, AR 2) plus the block-80 request header, then the
    /// given entries. A test whose refresh succeeds seals the days before
    /// the tip's (`seal_dao_days_before_tip`).
    fn seed_dao_free_fixture(store: &Arc<CkbadgerStore>, entries: &[DaoDepositCacheEntry]) {
        let header = |hash: u8, timestamp: i64| {
            let mut dao = vec![0u8; 32];
            dao[0..8].copy_from_slice(&DAO_FREE_FIXTURE_C.to_le_bytes());
            dao[8..16].copy_from_slice(&2u64.to_le_bytes());
            dao[16..24].copy_from_slice(&DAO_FREE_FIXTURE_S.to_le_bytes());
            dao[24..32].copy_from_slice(&DAO_FREE_FIXTURE_U.to_le_bytes());
            CachedBlockHeader {
                hash: vec![hash; 32],
                timestamp,
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
                chain_time_ms: timestamp,
            }
        };
        let mut seed = StoreBatch::new(store);
        let tip = 1_700_000_000_000i64;
        for n in 0..=100i64 {
            let time = if n <= 50 {
                tip - 2 * 86_400_000 + n * 86_400_000 / 50
            } else if n <= 80 {
                tip - 86_400_000 + (n - 50) * (86_400_000 - 3_600_000) / 30
            } else {
                tip - 3_600_000 + (n - 80) * 3_600_000 / 20
            };
            seed.put_block_header(n, &header(n as u8, time));
        }
        for (i, entry) in entries.iter().enumerate() {
            seed.put_dao_deposit(&keys::encode_outpoint(&[0xA0 + i as u8; 32], 0), entry);
        }
        seed.commit().unwrap();
    }

    /// The homepage partitions circulation into Knowledge (which already holds
    /// every live cell's occupied capacity, DAO cells included), the DAO cells'
    /// FREE capacity, and the rest. The DAO term is exact per live cell: a
    /// phase-0 deposit counts against its own occupied capacity, a phase-1
    /// withdraw request against the REQUEST cell's (it may carry another lock:
    /// here 115 CKB at deposit, 102 CKB once requested). Claimed deposits have
    /// no live DAO cell left.
    #[test]
    fn test_refresh_dao_statistics_sums_free_capacity_of_live_dao_cells() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        seed_test_genesis_baseline(&store);
        seed_dao_free_fixture(
            &store,
            &[
                dao_free_test_entry(200_00000000, 102_00000000, 0, None),
                dao_free_test_entry(300_00000000, 115_00000000, 1, Some(102_00000000)),
                dao_free_test_entry(150_00000000, 102_00000000, 2, Some(102_00000000)),
            ],
        );
        let writer = BatchWriter::new(store.clone(), store.clone());

        seal_dao_days_before_tip(&store);
        writer.refresh_latest_dao_statistics().unwrap();
        let latest = store.get_latest_dao_statistics().unwrap().unwrap();

        // (200 − 102) + (300 − 102) CKB.
        assert_eq!(latest.dao_free_capacity, 296_00000000);
        // Every live DAO cell's capacity, both phases.
        assert_eq!(latest.dao_capacity, 500_00000000);
        // Every deposit here was made at the tip AR, so nothing has accrued:
        // the depositor weight is the phase-0 free capacity alone.
        assert_eq!(latest.depositor_allocation_weight, 98_00000000);
        // The tip header's C, S and U travel with the row, so a reader can
        // combine them with the DAO figures without the (possibly rolled
        // back) header.
        assert_eq!(
            (
                latest.tip_total_issuance,
                latest.tip_secondary_pool,
                latest.tip_occupied_capacity
            ),
            (
                i128::from(DAO_FREE_FIXTURE_C),
                i128::from(DAO_FREE_FIXTURE_S),
                i128::from(DAO_FREE_FIXTURE_U)
            )
        );
    }

    /// The depositor weight is `Σ_phase0 free_i × AR_tip / AR_dep_i`, computed
    /// as phase-0 free + (unclaimed − frozen phase-1). Both DAO phases here
    /// were deposited at AR 1 and the tip is at AR 2, so both have accrued
    /// compensation and `unclaimed` (phase-0 accrued + phase-1 frozen) is
    /// strictly larger than `frozen_phase1`:
    ///
    /// - phase 0: free 98 CKB, accrued 98 × 2 / 1 − 98 = 98 CKB
    /// - phase 1: free 198 CKB against the request cell's 102 CKB occupied,
    ///   frozen at the request AR 2: 198 × 2 / 1 − 198 = 198 CKB
    /// - unclaimed = 98 + 198 = 296 CKB, frozen_phase1 = 198 CKB
    /// - weight = 98 + (296 − 198) = 196 CKB = 98 × 2 / 1
    ///
    /// Dropping the accrued term (98), flipping its sign (0) or counting the
    /// frozen phase-1 compensation (394) all miss 196.
    #[test]
    fn test_refresh_dao_statistics_depositor_weight_includes_phase0_accrued_compensation() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        seed_test_genesis_baseline(&store);
        seed_dao_free_fixture(
            &store,
            &[
                DaoDepositCacheEntry {
                    deposit_ar: 1,
                    ..dao_free_test_entry(200_00000000, 102_00000000, 0, None)
                },
                DaoDepositCacheEntry {
                    deposit_ar: 1,
                    ..dao_free_test_entry(300_00000000, 115_00000000, 1, Some(102_00000000))
                },
            ],
        );
        let writer = BatchWriter::new(store.clone(), store.clone());

        seal_dao_days_before_tip(&store);
        writer.refresh_latest_dao_statistics().unwrap();
        let latest = store.get_latest_dao_statistics().unwrap().unwrap();

        assert_eq!(latest.unclaimed_compensation, 296_00000000);
        assert_eq!(latest.depositor_allocation_weight, 196_00000000);
    }

    /// A phase-1 entry without its request cell's occupied capacity cannot be
    /// split into occupied and free: the refresh fails naming the cell.
    #[test]
    fn test_refresh_dao_statistics_fails_without_request_occupied_capacity() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        seed_test_genesis_baseline(&store);
        seed_dao_free_fixture(
            &store,
            &[dao_free_test_entry(300_00000000, 102_00000000, 1, None)],
        );
        let writer = BatchWriter::new(store.clone(), store.clone());

        let err = writer
            .refresh_latest_dao_statistics()
            .unwrap_err()
            .to_string();
        assert!(err.contains("withdraw_request_occupied_capacity"), "{err}");
    }

    #[test]
    fn test_refresh_dao_statistics_fails_without_phase1_request_block() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        seed_test_genesis_baseline(&store);
        let mut entry = dao_free_test_entry(300_00000000, 102_00000000, 1, Some(102_00000000));
        entry.withdraw_request_block = None;
        seed_dao_free_fixture(&store, &[entry]);
        let writer = BatchWriter::new(store.clone(), store.clone());
        let err = writer
            .refresh_latest_dao_statistics()
            .unwrap_err()
            .to_string();
        assert!(err.contains("missing request block"), "{err}");
    }

    #[test]
    fn test_refresh_dao_statistics_fails_on_zero_epoch_length() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        seed_test_genesis_baseline(&store);
        seed_dao_free_fixture(&store, &[]);
        let mut header = store.get_block_header(100).unwrap().unwrap();
        header.epoch_length = 0;
        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(100, &header);
        batch.commit().unwrap();
        let writer = BatchWriter::new(store.clone(), store.clone());
        let err = writer
            .refresh_latest_dao_statistics()
            .unwrap_err()
            .to_string();
        assert!(err.contains("invalid estimated APC inputs"), "{err}");
    }

    #[test]
    fn test_refresh_dao_statistics_rejects_negative_phase1_duration() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        seed_test_genesis_baseline(&store);
        seed_dao_free_fixture(
            &store,
            &[dao_free_test_entry(
                300_00000000,
                102_00000000,
                1,
                Some(102_00000000),
            )],
        );
        let mut request = store.get_block_header(80).unwrap().unwrap();
        request.timestamp = 1_700_000_000_000 - 2 * 86_400_000;
        request.chain_time_ms = request.timestamp;
        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(80, &request);
        batch.commit().unwrap();
        let writer = BatchWriter::new(store.clone(), store.clone());
        let err = writer
            .refresh_latest_dao_statistics()
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("negative/overflow DAO deposit duration"),
            "{err}"
        );
    }

    #[test]
    fn test_refresh_dao_statistics_uses_effective_chain_time_for_raw_timestamp_regression() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        seed_test_genesis_baseline(&store);
        seed_dao_free_fixture(
            &store,
            &[dao_free_test_entry(
                300_00000000,
                102_00000000,
                1,
                Some(102_00000000),
            )],
        );
        let mut request = store.get_block_header(80).unwrap().unwrap();
        request.timestamp = 1_700_000_000_000 - 2 * 86_400_000;
        // The canonical effective time remains monotone despite the raw
        // timestamp regression in the request header.
        let mut batch = StoreBatch::new(&store);
        batch.put_block_header(80, &request);
        batch.commit().unwrap();
        seal_dao_days_before_tip(&store);
        BatchWriter::new(store.clone(), store.clone())
            .refresh_latest_dao_statistics()
            .unwrap();
        let latest = store.get_latest_dao_statistics().unwrap().unwrap();
        assert_eq!(latest.average_deposit_days.as_deref(), Some("1.0 days"));
    }

    #[test]
    fn test_weighted_holding_display_uses_exact_integer_ratio_and_null_without_capacity() {
        assert_eq!(format_weighted_deposit_days(0, 0).unwrap(), None);
        assert!(format_weighted_deposit_days(1, 0).is_err());
        let capacity = 9_007_199_254_740_993i128;
        let weighted = capacity * 86_400_000 + capacity * 43_200_000;
        assert_eq!(
            format_weighted_deposit_days(weighted, capacity)
                .unwrap()
                .as_deref(),
            Some("2 days")
        );
    }

    #[test]
    fn test_refresh_dao_statistics_excludes_status1_from_explorer_totals() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        seed_test_genesis_baseline(&store);
        let writer = BatchWriter::new(store.clone(), store.clone());

        // Tip block: AR=4, S=200 CKB, C=9e9 CKB, timestamp = day 10
        let tip_timestamp: i64 = 1_700_000_000_000;
        let mut dao = vec![0u8; 32];
        dao[0..8].copy_from_slice(&(9_000_000_000u64 * 100_000_000).to_le_bytes());
        dao[8..16].copy_from_slice(&4u64.to_le_bytes());
        dao[16..24].copy_from_slice(&200_00000000u64.to_le_bytes());
        let mut seed = StoreBatch::new(&store);
        // A rolling baseline searches a canonical contiguous header history.
        for n in 0..100i64 {
            let time = if n <= 20 {
                tip_timestamp - 10 * 86_400_000 + n * 5 * 86_400_000 / 20
            } else if n <= 50 {
                tip_timestamp - 5 * 86_400_000 + (n - 20) * 3 * 86_400_000 / 30
            } else if n <= 80 {
                tip_timestamp - 2 * 86_400_000 + (n - 50) * 86_400_000 / 30
            } else {
                tip_timestamp - 86_400_000 + (n - 80) * 86_400_000 / 20
            };
            let mut historical_dao = dao.clone();
            let ar = if n < 20 {
                1u64
            } else if n < 80 {
                2
            } else {
                3
            };
            historical_dao[8..16].copy_from_slice(&ar.to_le_bytes());
            seed.put_block_header(
                n,
                &CachedBlockHeader {
                    hash: vec![n as u8; 32],
                    timestamp: time,
                    chain_time_ms: time,
                    epoch_number: 1,
                    epoch_index: 0,
                    epoch_length: 100,
                    dao: historical_dao,
                    transactions_count: 1,
                    uncles_count: 0,
                    proposals_count: 0,
                    compact_target: 0,
                    miner_lock_hash: None,
                    cycles: None,
                },
            );
        }
        seed.put_block_header(
            100,
            &CachedBlockHeader {
                hash: vec![0x11; 32],
                timestamp: tip_timestamp,
                epoch_number: 1,
                epoch_index: 0,
                epoch_length: 100,
                dao: dao.clone(),
                transactions_count: 1,
                uncles_count: 0,
                proposals_count: 0,
                compact_target: 0,
                miner_lock_hash: None,
                cycles: None,
                chain_time_ms: tip_timestamp,
            },
        );

        // Block 80 header (for status-1 deposit frozen time lookup)
        seed.put_block_header(
            80,
            &CachedBlockHeader {
                hash: vec![0x22; 32],
                timestamp: tip_timestamp - 86_400_000, // 1 day before tip
                epoch_number: 0,
                epoch_index: 0,
                epoch_length: 100,
                dao: {
                    let mut request_dao = dao.clone();
                    request_dao[8..16].copy_from_slice(&3u64.to_le_bytes());
                    request_dao
                },
                transactions_count: 1,
                uncles_count: 0,
                proposals_count: 0,
                compact_target: 0,
                miner_lock_hash: None,
                cycles: None,
                chain_time_ms: tip_timestamp - 86_400_000,
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
                deposit_chain_time_ms: tip_timestamp - two_days_ms,
                lock_script_hash: vec![0x01; 32],
                deposit_ar: 2,
                status: 0,
                withdraw_request_tx: None,
                withdraw_request_output_index: None,
                withdraw_request_block: None,
                withdraw_request_ar: None,
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_request_occupied_capacity: None,
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
                deposit_chain_time_ms: tip_timestamp - five_days_ms,
                lock_script_hash: vec![0x02; 32],
                deposit_ar: 2,
                status: 1,
                withdraw_request_tx: Some(vec![0x03; 32]),
                withdraw_request_output_index: Some(0),
                withdraw_request_block: Some(80),
                withdraw_request_ar: Some(3),
                withdraw_block: None,
                withdraw_tx: None,
                withdraw_request_occupied_capacity: Some(102_00000000),
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
                deposit_chain_time_ms: tip_timestamp - 10 * 86_400_000,
                lock_script_hash: vec![0x03; 32],
                deposit_ar: 1,
                status: 2,
                withdraw_request_tx: Some(vec![0x04; 32]),
                withdraw_request_output_index: Some(0),
                withdraw_request_block: Some(30),
                withdraw_request_ar: Some(2),
                withdraw_block: Some(40),
                withdraw_tx: Some(vec![0x05; 32]),
                withdraw_request_occupied_capacity: Some(102_00000000),
                withdraw_to_output_index: Some(0),
                compensation: Some(48_00000000),
            },
        );

        seed.commit().unwrap();

        seal_dao_days_before_tip(&store);
        writer.refresh_latest_dao_statistics().unwrap();
        let latest = store.get_latest_dao_statistics().unwrap().unwrap();

        // active_deposits still counts both status=0 and status=1
        assert_eq!(latest.active_deposits, 2);
        let json = serde_json::to_value(&latest).unwrap();
        assert_eq!(json["deposit_cells_count"], 1);
        assert_eq!(json["withdrawal_request_cells_count"], 1);
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
        assert_eq!(latest.average_deposit_days.as_deref(), Some("3 days"));
        let rolling = latest
            .rolling_24h
            .as_ref()
            .expect("complete 24h chain history");
        assert_eq!(rolling.baseline_block_number, 80);
        assert_eq!(rolling.deposited_change, 0);
        assert_eq!(rolling.depositors_change, 0);
        assert_eq!(rolling.claimed_compensation_change, 0);
        assert_eq!(rolling.unclaimed_compensation_change, 49_00000000);
        assert_eq!(latest.tip_block_number, 100);

        let snapshot = store.get_latest_dao_daily_snapshot().unwrap().unwrap();
        assert_eq!(snapshot.unclaimed_compensation, 197_00000000);
        assert_eq!(snapshot.frozen_phase1_compensation, 99_00000000);
    }

    /// Restores the invariant that `token_ops.rs`'s deleted
    /// `test_cleanup_old_object_hourly_buckets_accepts_24_byte_collection_id`
    /// used to hold: a collection id at its natural 24-byte width must still be
    /// resolvable from its zero-padded hourly key.
    ///
    /// Retention joins `OBJECT_HOURLY` (id padded to 32 by
    /// `encode_object_hourly_key`) to `cf_mnft_collection_agg` (keyed by the RAW
    /// id). Padding the aggregate's id is the only correct direction; looking up
    /// the 32-byte key verbatim missed every real mNFT class and turned the whole
    /// sweep into a no-op that still advanced `executed_cutoff_hour`.
    #[test]
    fn test_hourly_retention_resolves_natural_width_collection_ids() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let now_ms = chrono::Utc::now().timestamp_millis();
        let current_hour = now_ms / 3_600_000;
        let old_hour = current_hour - 100;

        // Every width a real collection id can have.
        let widths = [20usize, 24, 32];
        let mut seed = StoreBatch::new(&store);
        for (i, width) in widths.iter().enumerate() {
            let collection_id = vec![0x30 + i as u8; *width];
            seed.put_mnft_collection_aggregate(
                &collection_id,
                &MnftCollectionAggregate {
                    standard: ObjectStandard::MnftClass,
                    ..Default::default()
                },
            );
            seed.put_mnft_hourly_transfer(&collection_id, old_hour, 9);
            seed.put_mnft_hourly_transfer(&collection_id, current_hour, 3);
        }
        seed.commit().unwrap();

        let mut batch = StoreBatch::new(&store);
        let result = writer
            .stage_hourly_retention_step(
                &mut batch,
                ckbadger_store::types::HourlyRetentionFamily::Mnft,
                current_hour - 48,
                now_ms,
            )
            .unwrap();
        batch.commit().unwrap();
        assert_eq!(
            result.deleted,
            widths.len() as u64,
            "one expired bucket per collection, whatever its id width"
        );

        for (i, width) in widths.iter().enumerate() {
            let collection_id = vec![0x30 + i as u8; *width];
            let old_key = keys::encode_object_hourly_key(&collection_id, old_hour);
            let new_key = keys::encode_object_hourly_key(&collection_id, current_hour);
            assert!(
                store.get_stats_key(&old_key).unwrap().is_none(),
                "{width}-byte collection id: expired bucket must be deleted"
            );
            assert!(
                store.get_stats_key(&new_key).unwrap().is_some(),
                "{width}-byte collection id: in-window bucket must survive"
            );
        }
    }

    /// `executed_cutoff_hour` is the store's claim about what it has already
    /// deleted. A partial round has only swept as far as its cursor, so
    /// advancing the boundary there would claim deletions that never happened
    /// for every key past it — and a verifier reading the boundary would score
    /// those surviving buckets as corruption.
    #[test]
    fn test_partial_retention_round_does_not_advance_the_executed_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let now_ms = chrono::Utc::now().timestamp_millis();
        let current_hour = now_ms / 3_600_000;

        // More expired buckets than one step's delete budget.
        let overflow = BatchWriter::HOURLY_RETENTION_STEP_BUDGET + 10;
        let type_hash = vec![0x5C; 32];
        let mut seed = StoreBatch::new(&store);
        for i in 0..overflow {
            // All well past the 48h window, so every one of them expires.
            seed.put_token_hourly_transfer(&type_hash, current_hour - 10_000 + i as i64, 1);
        }
        seed.commit().unwrap();

        let mut batch = StoreBatch::new(&store);
        let result = writer
            .stage_hourly_retention_step(
                &mut batch,
                ckbadger_store::types::HourlyRetentionFamily::Token,
                current_hour - 48,
                now_ms,
            )
            .unwrap();
        batch.commit().unwrap();
        assert!(!result.completed, "fixture must overflow one step's budget");

        let state = store
            .get_hourly_retention_state(ckbadger_store::types::HourlyRetentionFamily::Token)
            .unwrap()
            .unwrap();
        assert_eq!(
            state.executed_cutoff_hour,
            i64::MIN,
            "a partial round must not claim a boundary it has not reached"
        );
        assert_eq!(
            state.round_in_progress_cutoff_hour,
            Some(current_hour - 48),
            "the in-flight target is recorded separately, as diagnostics only"
        );
        assert!(state.cursor.is_some());

        // Finishing the round is what earns the boundary.
        let mut rounds = 0;
        loop {
            let mut batch = StoreBatch::new(&store);
            let result = writer
                .stage_hourly_retention_step(
                    &mut batch,
                    ckbadger_store::types::HourlyRetentionFamily::Token,
                    current_hour - 48,
                    now_ms,
                )
                .unwrap();
            batch.commit().unwrap();
            rounds += 1;
            assert!(rounds < 10, "the cursor must make progress");
            if result.completed {
                break;
            }
        }
        let state = store
            .get_hourly_retention_state(ckbadger_store::types::HourlyRetentionFamily::Token)
            .unwrap()
            .unwrap();
        assert_eq!(state.executed_cutoff_hour, current_hour - 48);
        assert_eq!(state.round_in_progress_cutoff_hour, None);
        assert!(state.cursor.is_none());
        assert!(state.round_completed_at.is_some());
    }

    /// `dotbit.rs` writes `OBJECT_HOURLY` rows under
    /// `DOTBIT_SENTINEL_COLLECTION`, whose aggregate lives in `cf_identity_agg`,
    /// not `cf_mnft_collection_agg`. On both production stores that sentinel is
    /// the ONLY `OBJECT_HOURLY` entity (Task 0.3: `object_hourly entities=1`),
    /// so treating "no mNFT aggregate" as a hard error would have made every
    /// retention step fail on a real chain. Identity collections simply have no
    /// retention policy, like Spore clusters.
    #[test]
    fn test_hourly_retention_keeps_identity_collection_buckets() {
        use ckbadger_store::types::{IdentityCollectionAggregate, DOTBIT_SENTINEL_COLLECTION};

        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let now_ms = chrono::Utc::now().timestamp_millis();
        let current_hour = now_ms / 3_600_000;
        let old_hour = current_hour - 100;

        let mut seed = StoreBatch::new(&store);
        seed.put_identity_collection_aggregate(
            &DOTBIT_SENTINEL_COLLECTION,
            &IdentityCollectionAggregate::default(),
        );
        seed.put_mnft_hourly_transfer(&DOTBIT_SENTINEL_COLLECTION, old_hour, 4);
        seed.commit().unwrap();

        let mut batch = StoreBatch::new(&store);
        let result = writer
            .stage_hourly_retention_step(
                &mut batch,
                ckbadger_store::types::HourlyRetentionFamily::Mnft,
                current_hour - 48,
                now_ms,
            )
            .expect("an identity collection must not fail the sweep");
        batch.commit().unwrap();

        assert_eq!(result.deleted, 0);
        let key = keys::encode_object_hourly_key(&DOTBIT_SENTINEL_COLLECTION, old_hour);
        assert!(
            store.get_stats_key(&key).unwrap().is_some(),
            "no retention policy covers identity collections, so nothing may be deleted"
        );
    }

    /// An `OBJECT_HOURLY` row whose collection has no aggregate at all is a
    /// broken invariant. Answering "does not expire" is what let the padding bug
    /// hide for a whole sweep; it must fail with the key instead.
    #[test]
    fn test_hourly_retention_fails_on_hourly_bucket_without_collection_aggregate() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let now_ms = chrono::Utc::now().timestamp_millis();
        let current_hour = now_ms / 3_600_000;
        let orphan = vec![0x4A; 24];
        let mut seed = StoreBatch::new(&store);
        seed.put_mnft_hourly_transfer(&orphan, current_hour - 100, 5);
        seed.commit().unwrap();

        let mut batch = StoreBatch::new(&store);
        let err = writer
            .stage_hourly_retention_step(
                &mut batch,
                ckbadger_store::types::HourlyRetentionFamily::Mnft,
                current_hour - 48,
                now_ms,
            )
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("object hourly bucket has no collection aggregate")
                && err.to_string().contains("4a4a4a"),
            "got: {err}"
        );
    }

    #[test]
    fn test_refresh_mnft_24h_transfers_cleans_only_mnft_collections() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());

        let now_ms = chrono::Utc::now().timestamp_millis();
        let current_hour = now_ms / 3_600_000;
        let old_hour = current_hour - 100;

        // Real mNFT class ids are 24 bytes (`parser/mnft.rs:201`,
        // `type_args[..24]`), while `encode_object_hourly_key` zero-pads them to
        // 32 and `cf_mnft_collection_agg` is keyed by the RAW id. A 32-byte-only
        // fixture cannot see that mismatch; this is the case the deleted
        // `test_cleanup_old_object_hourly_buckets_accepts_24_byte_collection_id`
        // used to pin.
        let mnft_collection = vec![0x10; 24];
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

        // Same assertions, now against the single retention path: an mNFT class
        // expires, a Spore cluster sharing the OBJECT_HOURLY prefix does not.
        let mut batch = StoreBatch::new(&store);
        let result = writer
            .stage_hourly_retention_step(
                &mut batch,
                ckbadger_store::types::HourlyRetentionFamily::Mnft,
                current_hour - 48,
                now_ms,
            )
            .unwrap();
        batch.commit().unwrap();
        assert_eq!(result.deleted, 1);
        assert!(result.completed);

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
    fn test_get_previous_block_raw_timestamp_errors_on_invalid_millis() {
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
                uncles_count: 0,
                proposals_count: 0,
                compact_target: 0,
                miner_lock_hash: None,
                cycles: None,
                chain_time_ms: i64::MAX,
            },
        );
        batch.commit().unwrap();

        let err = writer.get_previous_block_raw_timestamp(1).unwrap_err();
        assert!(err
            .to_string()
            .contains("invalid previous block timestamp millis"));
    }

    #[test]
    fn histogram_writers_reject_malformed_and_overflowed_existing_counts() {
        for (prefix, bucket) in [
            (keys::STATS_PREFIX_BLOCK_TIME_DIST, 7i32),
            (keys::STATS_PREFIX_EPOCH_TIME_DIST, 42i32),
        ] {
            for (existing, expected) in [
                (vec![0xaa], "length"),
                (i32::MAX.to_le_bytes().to_vec(), "overflow"),
            ] {
                let dir = tempfile::tempdir().unwrap();
                let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
                let writer = BatchWriter::new(store.clone(), store.clone());
                let key = keys::encode_stats_key(prefix, &bucket.to_be_bytes());
                let mut seed = StoreBatch::new(&store);
                seed.put_stats(&key, &existing);
                seed.commit().unwrap();

                let mut batch = StoreBatch::new(&store);
                let error = if prefix == keys::STATS_PREFIX_BLOCK_TIME_DIST {
                    writer.update_block_time_distribution_batch(bucket, 1, &mut batch)
                } else {
                    writer.update_epoch_time_distribution_batch(bucket, 1, &mut batch)
                }
                .unwrap_err()
                .to_string();
                assert!(error.contains(expected), "{error}");
                assert!(error.contains(&bucket.to_string()), "{error}");
            }
        }
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
    fn test_daily_difficulty_preserves_exact_work_across_batches() {
        let dir = tempfile::tempdir().unwrap();
        let store = std::sync::Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());
        let day = NaiveDate::from_ymd_opt(2026, 10, 24).unwrap();
        let first = 9_007_199_254_740_993u128;
        let second = first + 4;
        for work in [first, second] {
            let mut batch = StoreBatch::new(&store);
            writer
                .update_daily_block_stats_batch(day, work, 1, 0, &mut batch)
                .unwrap();
            batch.commit().unwrap();
        }
        let row = store.get_daily_block_stats("20261024").unwrap().unwrap();
        assert_eq!(row.difficulty_sum, first + second);
        assert_eq!(row.block_count, 2);
        assert_eq!(
            ckbadger_common::work::rounded_ratio(row.difficulty_sum, 2).unwrap(),
            first + 2
        );
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
        DailyActivityStats, IdentityStandard, ItemDelta, ItemKind, ParticipantDelta,
        ProtocolAction, TxActions, TypeCallEntry, TAG_DAO, TAG_IDENTITY, TAG_OBJECT, TAG_TOKEN,
        TAG_TYPE_CALL,
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
            id: ckbadger_store::types::ParticipantId::Lock([0xAA; 32]),
            ckb_delta,
            used_delta: 0,
            item_deltas: vec![],
            tags,
            roles: 0,
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
        assert_eq!(stats.total_ckb_moved, 0);
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
                    kind: ItemKind::Token,
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
                    kind: ItemKind::Object,
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
                    kind: ItemKind::Identity(IdentityStandard::DotBit),
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
                    kind: ItemKind::Token,
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
        assert_eq!(stats.total_ckb_moved, 30_00000000);
    }

    #[test]
    fn test_negative_delta_is_not_received_capacity() {
        let mut stats = DailyActivityStats::default();
        let actions = make_tx_actions(
            false,
            vec![make_participant(-999_00000000, 0)],
            vec![],
            vec![],
        );
        accumulate_tx_actions_stats(&actions, &mut stats);
        assert_eq!(stats.total_ckb_moved, 0);
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
        let addrs: BTreeSet<[u8; 32]> = (0..3u8)
            .map(|i| {
                let mut h = [0u8; 32];
                h[0] = i;
                h
            })
            .collect();
        writer
            .update_activity_bucket_stats(
                keys::ActivityBucketKind::Hourly,
                "2026030912",
                &stats,
                &addrs,
                None,
                &mut batch,
            )
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
        let addrs1: BTreeSet<[u8; 32]> = (0..3u8)
            .map(|i| {
                let mut h = [0u8; 32];
                h[0] = i;
                h
            })
            .collect();
        writer
            .update_activity_bucket_stats(
                keys::ActivityBucketKind::Hourly,
                "2026030912",
                &s1,
                &addrs1,
                None,
                &mut batch,
            )
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
        let addrs2: BTreeSet<[u8; 32]> = (1..8u8)
            .map(|i| {
                let mut h = [0u8; 32];
                h[0] = i;
                h
            })
            .collect();
        writer
            .update_activity_bucket_stats(
                keys::ActivityBucketKind::Hourly,
                "2026030912",
                &s2,
                &addrs2,
                None,
                &mut batch2,
            )
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

    fn addr(byte: u8) -> [u8; 32] {
        [byte; 32]
    }

    /// One block's write to one daily bucket through a journaled batch of a
    /// live batch overlay; returns the bucket row after it.
    fn write_block_bucket(
        writer: &BatchWriter,
        batch: &mut StoreBatch,
        block: i64,
        addrs: &[[u8; 32]],
    ) -> DailyActivityStats {
        let block_stats = DailyActivityStats {
            transfer_count: 1,
            ..Default::default()
        };
        let addrs: BTreeSet<[u8; 32]> = addrs.iter().copied().collect();
        writer
            .update_activity_bucket_stats(
                keys::ActivityBucketKind::Daily,
                "20260309",
                &block_stats,
                &addrs,
                None,
                &mut batch.at(block),
            )
            .unwrap();
        let key = keys::ActivityBucketKind::Daily
            .stats_key("20260309")
            .unwrap();
        bincode::deserialize(&batch.get_stats(&key).unwrap().unwrap()).unwrap()
    }

    fn commit_journaled(mut batch: StoreBatch) {
        let mut counters = HashMap::new();
        batch
            .stage_journal(&mut |block| {
                ckbadger_store::block_overlay::journal_seq(&mut counters, block)
            })
            .unwrap();
        batch.commit().unwrap();
    }

    /// A bucket's unique addresses are its rows, whoever wrote them (a live
    /// block, or bulk build for the bucket open at the handoff): an address
    /// is new iff it has no row; only new addresses get a row and raise
    /// `unique_address_count`.
    #[test]
    fn activity_bucket_addresses_are_the_buckets_rows() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());
        let kind = keys::ActivityBucketKind::Daily;
        let bucket_addrs =
            |store: &CkbadgerStore| store.get_activity_bucket_addrs(kind, "20260309").unwrap();

        // 1. A new bucket: blocks 10 and 11 of one batch; 11 repeats an
        //    address of 10 (seen through the overlay, not the store).
        let overlay = ckbadger_store::block_overlay::SharedBlockOverlay::new(&store).unwrap();
        let mut batch = StoreBatch::journaled(&store, &overlay, 0);
        let row = write_block_bucket(&writer, &mut batch, 10, &[addr(1), addr(2)]);
        assert_eq!(row.unique_address_count, 2);
        let row = write_block_bucket(&writer, &mut batch, 11, &[addr(2), addr(3)]);
        assert_eq!(row.unique_address_count, 3);
        assert_eq!(row.transfer_count, 2);
        commit_journaled(batch);
        assert_eq!(
            bucket_addrs(&store),
            BTreeSet::from([addr(1), addr(2), addr(3)])
        );

        // 2. Rows present (a later batch): only addresses without a row count.
        let overlay = ckbadger_store::block_overlay::SharedBlockOverlay::new(&store).unwrap();
        let mut batch = StoreBatch::journaled(&store, &overlay, 0);
        let row = write_block_bucket(&writer, &mut batch, 12, &[addr(3), addr(4)]);
        assert_eq!(row.unique_address_count, 4);
        commit_journaled(batch);
        assert_eq!(bucket_addrs(&store).len(), 4);

        // 3. The bucket open at a bulk handoff: bulk wrote its stats row and
        //    its address rows; live sync continues from them.
        let bulk_dir = tempfile::tempdir().unwrap();
        let bulk_store = Arc::new(CkbadgerStore::open_domain(bulk_dir.path()).unwrap());
        let bulk_writer = BatchWriter::new(bulk_store.clone(), bulk_store.clone());
        for address in [addr(1), addr(2)] {
            bulk_store
                .put_stats_key(&kind.addr_row_key("20260309", &address).unwrap(), &[])
                .unwrap();
        }
        let bulk_row = DailyActivityStats {
            transfer_count: 7,
            unique_address_count: 2,
            ..Default::default()
        };
        bulk_store
            .put_stats_key(
                &kind.stats_key("20260309").unwrap(),
                &bincode::serialize(&bulk_row).unwrap(),
            )
            .unwrap();
        let overlay = ckbadger_store::block_overlay::SharedBlockOverlay::new(&bulk_store).unwrap();
        let mut batch = StoreBatch::journaled(&bulk_store, &overlay, 0);
        let row = write_block_bucket(&bulk_writer, &mut batch, 20, &[addr(2)]);
        assert_eq!(
            row.unique_address_count, 2,
            "a bulk-written address is not new"
        );
        assert_eq!(row.transfer_count, 8);
        let row = write_block_bucket(&bulk_writer, &mut batch, 21, &[addr(1), addr(5)]);
        assert_eq!(row.unique_address_count, 3);
        commit_journaled(batch);
        assert_eq!(
            bulk_store
                .get_activity_bucket_addrs(kind, "20260309")
                .unwrap(),
            BTreeSet::from([addr(1), addr(2), addr(5)])
        );

        // An address row of a bucket without its stats row is a broken
        // invariant.
        bulk_store
            .put_stats_key(&kind.addr_row_key("20260311", &addr(9)).unwrap(), &[])
            .unwrap();
        let err = bulk_writer
            .update_activity_bucket_stats(
                kind,
                "20260311",
                &DailyActivityStats::default(),
                &BTreeSet::from([addr(9)]),
                None,
                &mut StoreBatch::new(&bulk_store),
            )
            .unwrap_err()
            .to_string();
        assert!(err.contains("but no stats row"), "{err}");
    }

    /// C-3: the live batch deletes the address rows of every bucket the
    /// coverage floor's chain time has left — daily and hourly alike — and
    /// keeps the bucket the floor block is in, every later bucket, and every
    /// stats row; a bucket ending exactly at the floor block's chain time is
    /// final. The recorded hour bounds the next scan from below and never
    /// moves down.
    #[test]
    fn activity_addr_rows_behind_the_coverage_floor_are_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());
        const HOUR: i64 = 3_600_000;
        // 2026-03-09 00:00 UTC+8.
        let day_start: i64 = 1_772_985_600_000;
        assert_eq!(
            keys::ActivityBucketKind::Daily
                .bucket_label(day_start)
                .unwrap(),
            "20260309"
        );
        let put_header = |number: i64, chain_time_ms: i64| {
            let mut batch = StoreBatch::new(&store);
            batch.put_block_header(
                number,
                &CachedBlockHeader {
                    hash: vec![number as u8; 32],
                    timestamp: chain_time_ms,
                    chain_time_ms,
                    epoch_number: 1,
                    epoch_index: 0,
                    epoch_length: 100,
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
        };
        // Block 5 opens the 20260309 01 hour; block 9 is exactly at the end
        // of hour 01 (the start of 02); block 12 is in day 20260310.
        put_header(5, day_start + HOUR);
        put_header(9, day_start + 2 * HOUR);
        put_header(12, day_start + 24 * HOUR + 30 * 60_000);
        let rows = [
            (keys::ActivityBucketKind::Hourly, "2026030900"),
            (keys::ActivityBucketKind::Hourly, "2026030901"),
            (keys::ActivityBucketKind::Hourly, "2026030902"),
            (keys::ActivityBucketKind::Hourly, "2026031000"),
            (keys::ActivityBucketKind::Daily, "20260308"),
            (keys::ActivityBucketKind::Daily, "20260309"),
            (keys::ActivityBucketKind::Daily, "20260310"),
        ];
        for (kind, bucket) in rows {
            for address in [addr(1), addr(2)] {
                store
                    .put_stats_key(&kind.addr_row_key(bucket, &address).unwrap(), &[])
                    .unwrap();
            }
            store
                .put_stats_key(
                    &kind.stats_key(bucket).unwrap(),
                    &bincode::serialize(&DailyActivityStats::default()).unwrap(),
                )
                .unwrap();
        }
        let present = |store: &CkbadgerStore| -> Vec<(keys::ActivityBucketKind, &str)> {
            rows.iter()
                .copied()
                .filter(|(kind, bucket)| {
                    !store
                        .get_activity_bucket_addrs(*kind, bucket)
                        .unwrap()
                        .is_empty()
                })
                .collect()
        };
        let prune_result = |floor: i64| -> Result<u64> {
            let overlay = ckbadger_store::block_overlay::SharedBlockOverlay::new(&store).unwrap();
            let mut batch = StoreBatch::journaled(&store, &overlay, 0);
            let deleted = writer.stage_activity_addr_retention(&mut batch.at(100), floor)?;
            commit_journaled(batch);
            Ok(deleted)
        };
        let prune = |floor: i64| prune_result(floor).unwrap();

        // No floor yet: nothing.
        assert_eq!(prune(-1), 0);
        assert_eq!(store.get_activity_addr_row_floor().unwrap(), None);
        // First prune (no recorded hour): the floor block 5 is in hour 01 of
        // day 20260309, so hour 00 and day 20260308 are final.
        assert_eq!(prune(5), 4);
        assert_eq!(
            store.get_activity_addr_row_floor().unwrap().as_deref(),
            Some("2026030901")
        );
        assert_eq!(
            present(&store),
            vec![
                (keys::ActivityBucketKind::Hourly, "2026030901"),
                (keys::ActivityBucketKind::Hourly, "2026030902"),
                (keys::ActivityBucketKind::Hourly, "2026031000"),
                (keys::ActivityBucketKind::Daily, "20260309"),
                (keys::ActivityBucketKind::Daily, "20260310"),
            ]
        );
        // Floor 5 → 9: block 9's chain time is the end of hour 01, so hour 01
        // is final; the day is not.
        assert_eq!(prune(9), 2);
        // Floor 9 → 12 (day 20260310): hour 02 and day 20260309 go.
        assert_eq!(prune(12), 4);
        // The same floor again: nothing to scan.
        assert_eq!(prune(12), 0);
        assert_eq!(
            present(&store),
            vec![
                (keys::ActivityBucketKind::Hourly, "2026031000"),
                (keys::ActivityBucketKind::Daily, "20260310"),
            ]
        );
        // No stats row is deleted.
        for (kind, bucket) in rows {
            assert!(store
                .get_stats_key(&kind.stats_key(bucket).unwrap())
                .unwrap()
                .is_some());
        }
        // A floor block without a header is store corruption.
        let err = prune_result(13).unwrap_err().to_string();
        assert!(err.contains("missing header"), "{err}");
        // A floor whose hour is before the record (as after a bulk handoff,
        // which records the handoff hour: finding 11) prunes nothing.
        assert_eq!(prune(9), 0);
        assert_eq!(
            store.get_activity_addr_row_floor().unwrap().as_deref(),
            Some("2026031000")
        );
    }

    /// Finding 11: a block landing in a bucket behind the activity address
    /// row floor (its rows pruned, or sealed by the bulk build) would count
    /// every address again: rebuild-required, before any write.
    #[test]
    fn a_block_behind_the_activity_addr_row_floor_is_rebuild_required() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());
        let addrs = BTreeSet::from([addr(1)]);
        let update = |kind: keys::ActivityBucketKind, bucket: &str| {
            let mut batch = StoreBatch::new(&store);
            writer.update_activity_bucket_stats(
                kind,
                bucket,
                &DailyActivityStats::default(),
                &addrs,
                Some("2026030912"),
                &mut batch,
            )
        };
        for (kind, bucket) in [
            (keys::ActivityBucketKind::Hourly, "2026030911"),
            (keys::ActivityBucketKind::Hourly, "2026030823"),
            (keys::ActivityBucketKind::Daily, "20260308"),
        ] {
            let err = update(kind, bucket).unwrap_err();
            assert!(crate::lifecycle::is_rebuild_required(&err), "{err:#}");
            assert!(err.to_string().contains(bucket), "{err:#}");
            assert!(err.to_string().contains("2026030912"), "{err:#}");
        }
        // The floor's own hour and day, and later buckets, are kept.
        for (kind, bucket) in [
            (keys::ActivityBucketKind::Hourly, "2026030912"),
            (keys::ActivityBucketKind::Hourly, "2026030913"),
            (keys::ActivityBucketKind::Daily, "20260309"),
        ] {
            update(kind, bucket).unwrap();
        }
    }

    #[test]
    fn test_refresh_dao_statistics_computes_top_depositors() {
        use std::collections::HashMap;

        let mut depositor_map: HashMap<Vec<u8>, (i128, i32, i128)> = HashMap::new();
        let lock_a = vec![0xAA; 32];
        let lock_b = vec![0xBB; 32];

        // Depositor A: two deposits
        let e = depositor_map.entry(lock_a.clone()).or_insert((0, 0, 0));
        e.0 += 500_00000000i128;
        e.1 += 1;
        e.2 += 1000;
        let e = depositor_map.entry(lock_a.clone()).or_insert((0, 0, 0));
        e.0 += 300_00000000i128;
        e.1 += 1;
        e.2 += 500;

        // Depositor B: one deposit, larger
        let e = depositor_map.entry(lock_b.clone()).or_insert((0, 0, 0));
        e.0 += 1000_00000000i128;
        e.1 += 1;
        e.2 += 2000;

        let mut sorted: Vec<_> = depositor_map.into_iter().collect();
        sorted.sort_by_key(|item| std::cmp::Reverse(item.1 .0));
        sorted.truncate(100);

        assert_eq!(sorted[0].0, lock_b);
        assert_eq!(sorted[0].1 .0, 1000_00000000);
        assert_eq!(sorted[1].0, lock_a);
        assert_eq!(sorted[1].1 .0, 800_00000000);
        assert_eq!(sorted[1].1 .1, 2);
    }
}
