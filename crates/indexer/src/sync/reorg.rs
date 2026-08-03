#![allow(clippy::type_complexity)]

use std::collections::HashMap;

use anyhow::{anyhow, Result};
use tracing::{error, info, warn};

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::AddressBalance;
use ckbadger_store::types::PositionedCellInfo;

use crate::cache::CacheInvalidator;
use crate::config::DEEP_FORK_DEPTH;
use crate::db::writer::cell_distribution::CellDistributionTracker;
use crate::db::writer::hodl_wave::HodlWaveTracker;
use crate::rpc::CkbRpcClient;

use super::checked_tx_count;
use super::dao_helpers::occupied_capacity_shannons_i64;
use super::helpers::*;
use super::indexer::{
    mempool_short_tx_id, rebuild_cell_dist_tracker_from_state, rebuild_hodl_tracker_from_state,
    Indexer,
};
use super::types::{ReorgAction, TxData};

/// Open a block on the cell-distribution tracker: seal the previous day if this
/// block starts a new one, then record the block→date transition.
///
/// MUST run **before** the block's transactions are applied. A snapshot labelled
/// day D is the tracker state at the *end* of day D, and the block that triggers
/// it is the first block of D+1 — its cells belong to D+1's snapshot, not D's.
/// Sealing after application dated every snapshot one block too late (mainnet
/// 2026-07-31 gained the 00:00:21 cellbase of 2026-08-01).
///
/// The address cohort is sealed here too: it is the same instant of the same
/// tracker, so the two rows can never drift apart.
///
/// Live sync and bulk build share this function; a divergence between them would
/// make a rebuilt database disagree with an incrementally synced one.
pub(crate) fn begin_cell_distribution_block(
    tracker: &mut CellDistributionTracker,
    block_number: i64,
    block_date: chrono::NaiveDate,
) -> Option<(
    chrono::NaiveDate,
    ckbadger_store::DailyCellDistribution,
    ckbadger_store::DailyAddressCohort,
)> {
    let sealed = tracker
        .maybe_snapshot(block_date)
        .map(|(date, distribution)| (date, distribution, tracker.cohort_snapshot()));
    tracker.record_block_date(block_number, block_date);
    sealed
}

/// Open a block on the HODL wave tracker, with the same end-of-day contract as
/// [`begin_cell_distribution_block`]: seal first, then record, then apply the
/// block's transactions.
pub(crate) fn begin_hodl_wave_block(
    tracker: &mut HodlWaveTracker,
    block_number: i64,
    block_date: chrono::NaiveDate,
) -> Option<(chrono::NaiveDate, ckbadger_store::DailyHodlWave)> {
    let sealed = tracker.maybe_snapshot(block_date);
    tracker.record_block_date(block_number, block_date);
    sealed
}

impl Indexer {
    pub(crate) fn reconcile_hodl_tracker_with_tip(&self, tip_block: i64) -> Result<()> {
        let state = self.writer.store().get_hodl_tracker_state()?;
        let rebuilt = rebuild_hodl_tracker_from_state(state, tip_block)?;

        let mut tracker = self.hodl_tracker.lock().unwrap();
        *tracker = rebuilt;
        Ok(())
    }

    /// Prepare HODL wave tracker updates into the provided domain batch.
    ///
    /// Holder count and cell age changes are applied per-tx within the per-block
    /// loop, and each block opens with [`begin_hodl_wave_block`], so a
    /// day-boundary snapshot holds exactly that day's blocks — neither the rest
    /// of the batch nor the first block of the next day (matching bulk-build
    /// behavior).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare_hodl_wave_batch(
        &self,
        all_parsed_blocks: &[crate::parser::block::ParsedBlock],
        all_tx_data: &[TxData],
        input_cell_info: &HashMap<(Vec<u8>, i16), PositionedCellInfo>,
        batch_cell_infos: &HashMap<(Vec<u8>, i16), PositionedCellInfo>,
        prefetched_balances: &HashMap<Vec<u8>, Option<AddressBalance>>,
        batch: &mut StoreBatch<'_>,
    ) -> Result<crate::db::writer::hodl_wave::HodlWaveTracker> {
        let mut tracker = self.hodl_tracker.lock().unwrap().clone();

        // Running map of live cell counts per lock_hash, initialized from DB.
        let mut live_cells_by_lock: HashMap<Vec<u8>, i32> = prefetched_balances
            .iter()
            .filter_map(|(k, v)| v.as_ref().map(|b| (k.clone(), b.live_cells_count)))
            .collect();

        let mut block_tx_idx = 0usize;
        for parsed in all_parsed_blocks {
            let block_date = ckbadger_common::block_date(parsed.timestamp);
            if let Some((snapshot_date, snapshot)) =
                begin_hodl_wave_block(&mut tracker, parsed.number, block_date)
            {
                let date_str = snapshot_date.format("%Y%m%d").to_string();
                batch.put_hodl_wave(&date_str, &snapshot);
            }

            let tx_count = checked_tx_count(parsed.transactions_count, parsed.number)?;
            let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count];
            block_tx_idx += tx_count;

            for tx_data in tx_slice {
                // Cell consumes — update holder count then age tracker
                if !tx_data.is_cellbase {
                    for input in &tx_data.inputs {
                        let key = (
                            input.previous_tx_hash.to_vec(),
                            parsed_input_outpoint_index_i16(
                                input.previous_output_index,
                                "sync_indexer",
                            )?,
                        );
                        let info = input_cell_info
                            .get(&key)
                            .or_else(|| batch_cell_infos.get(&key))
                            .ok_or_else(|| {
                                anyhow!(
                                    "missing input cell info in HODL tracker: block={}, tx_hash=0x{}, prev_outpoint=0x{}:{}",
                                    parsed.number,
                                    hex::encode(tx_data.hash),
                                    hex::encode(input.previous_tx_hash),
                                    input.previous_output_index
                                )
                            })?;
                        let lock_hash = &info.cell.lock_script_hash;
                        let old_live = live_cells_by_lock.get(lock_hash).copied().unwrap_or(0);
                        let new_live = old_live.checked_add(-1).ok_or_else(|| {
                            anyhow!(
                                "holder_count live_cells overflow (consume) in HODL tracker: lock_hash=0x{}, old_live={}",
                                hex::encode(lock_hash),
                                old_live
                            )
                        })?;
                        if new_live < 0 {
                            anyhow::bail!(
                                "holder_count live_cells underflow in HODL tracker: lock_hash=0x{}, old_live={}, new_live={}",
                                hex::encode(lock_hash),
                                old_live,
                                new_live
                            );
                        }
                        tracker.update_holder_count(old_live, new_live)?;
                        live_cells_by_lock.insert(lock_hash.clone(), new_live);
                        tracker.cell_consumed(info.created_at_block, info.capacity)?;
                    }
                }
                // Cell creates — update holder count then age tracker
                for cell in &tx_data.cells {
                    let lock_hash = &cell.lock_script_hash;
                    let old_live = live_cells_by_lock.get(lock_hash).copied().unwrap_or(0);
                    let new_live = old_live.checked_add(1).ok_or_else(|| {
                        anyhow!(
                            "holder_count live_cells overflow (create) in HODL tracker: lock_hash=0x{}, old_live={}",
                            hex::encode(lock_hash),
                            old_live
                        )
                    })?;
                    tracker.update_holder_count(old_live, new_live)?;
                    live_cells_by_lock.insert(lock_hash.clone(), new_live);
                    tracker.cell_created(block_date, cell.capacity);
                }
            }
        }

        batch.put_hodl_tracker_state(&tracker.to_state());
        Ok(tracker)
    }

    pub(crate) fn reconcile_cell_dist_tracker_with_tip(&self, tip_block: i64) -> Result<()> {
        let state = self.writer.store().get_cell_dist_tracker_state()?;
        let rebuilt = rebuild_cell_dist_tracker_from_state(state, tip_block)?;

        let mut tracker = self.cell_dist_tracker.lock().unwrap();
        *tracker = rebuilt;
        Ok(())
    }

    /// Prepare cell distribution tracker updates into the provided domain batch.
    ///
    /// Cohort deltas are applied per-tx within the per-block loop, and each block
    /// opens with [`begin_cell_distribution_block`], so a day-boundary snapshot
    /// holds exactly that day's blocks — neither the rest of the batch nor the
    /// first block of the next day (matching bulk-build behavior).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare_cell_distribution_batch(
        &self,
        all_parsed_blocks: &[crate::parser::block::ParsedBlock],
        all_tx_data: &[TxData],
        input_cell_info: &HashMap<(Vec<u8>, i16), PositionedCellInfo>,
        batch_cell_infos: &HashMap<(Vec<u8>, i16), PositionedCellInfo>,
        prefetched_balances: &HashMap<Vec<u8>, Option<AddressBalance>>,
        batch: &mut StoreBatch<'_>,
    ) -> Result<crate::db::writer::cell_distribution::CellDistributionTracker> {
        let mut tracker = self.cell_dist_tracker.lock().unwrap().clone();

        // Running map of first_seen_block per address.
        // For existing addresses: from prefetched DB state.
        // For new addresses: set to the block where they first appear in this batch.
        let mut first_seen_by_lock: HashMap<Vec<u8>, i64> = prefetched_balances
            .iter()
            .filter_map(|(k, v)| v.as_ref().map(|b| (k.clone(), b.first_seen_block)))
            .collect();

        let mut block_tx_idx = 0usize;
        for parsed in all_parsed_blocks {
            let block_date = ckbadger_common::block_date(parsed.timestamp);
            if let Some((snapshot_date, snapshot, cohort)) =
                begin_cell_distribution_block(&mut tracker, parsed.number, block_date)
            {
                let date_str = snapshot_date.format("%Y%m%d").to_string();
                batch.put_cell_distribution(&date_str, &snapshot);
                batch.put_address_cohort(&date_str, &cohort);
            }

            let tx_count = checked_tx_count(parsed.transactions_count, parsed.number)?;
            let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count];
            block_tx_idx += tx_count;

            for tx_data in tx_slice {
                // Compute per-address balance and used_capacity deltas for this tx
                let mut tx_addr_deltas: HashMap<Vec<u8>, (i128, i128)> = HashMap::new();

                // Cell creates
                for cell in &tx_data.cells {
                    let occ = occupied_capacity_shannons_i64(
                        cell.lock_args.len(),
                        cell.type_args.as_ref().map(|args| args.len()),
                        cell.data_size,
                    );
                    tracker.cell_created(occ);

                    let lock_hash = &cell.lock_script_hash;
                    let entry = tx_addr_deltas.entry(lock_hash.clone()).or_insert((0, 0));
                    entry.0 += occ as i128; // used_capacity_delta
                    entry.1 += cell.capacity as i128; // balance_delta

                    // Track first_seen for new addresses
                    first_seen_by_lock
                        .entry(lock_hash.clone())
                        .or_insert(parsed.number);
                }

                // Cell consumes
                if !tx_data.is_cellbase {
                    for input in &tx_data.inputs {
                        let key = (
                            input.previous_tx_hash.to_vec(),
                            parsed_input_outpoint_index_i16(
                                input.previous_output_index,
                                "cell_dist_tracker",
                            )?,
                        );
                        let info = input_cell_info
                            .get(&key)
                            .or_else(|| batch_cell_infos.get(&key))
                            .ok_or_else(|| {
                                anyhow!(
                                    "missing input cell info in cell distribution tracker: block={}, tx_hash=0x{}, prev_outpoint=0x{}:{}",
                                    parsed.number,
                                    hex::encode(tx_data.hash),
                                    hex::encode(input.previous_tx_hash),
                                    input.previous_output_index
                                )
                            })?;
                        tracker.cell_consumed(info.occupied_capacity)?;

                        let lock_hash = &info.cell.lock_script_hash;
                        let entry = tx_addr_deltas.entry(lock_hash.clone()).or_insert((0, 0));
                        entry.0 -= info.occupied_capacity as i128; // used_capacity_delta
                        entry.1 -= info.capacity as i128; // balance_delta
                    }
                }

                // Apply cohort deltas for this tx
                for (lock_hash, (used_delta, balance_delta)) in &tx_addr_deltas {
                    let first_seen_block = first_seen_by_lock
                        .get(lock_hash)
                        .copied()
                        .unwrap_or(parsed.number);
                    tracker.apply_cohort_delta(first_seen_block, *used_delta, *balance_delta)?;
                }
            }
        }

        batch.put_cell_dist_tracker_state(&tracker.to_state());
        Ok(tracker)
    }
    // === get_chain_block_hash, get_chain_tip ===

    /// Get the block hash for a given block number, using direct RocksDB reads when available.
    pub(crate) async fn get_chain_block_hash(&self, number: u64) -> Result<Vec<u8>> {
        let store = self
            .ckb_store
            .as_ref()
            .expect("ckb_store must exist for chain block hash reads");
        store.refresh()?;
        store
            .get_block_hash(number)
            .map(|h| h.to_vec())
            .ok_or_else(|| anyhow::anyhow!("Block {} not found in CKB RocksDB", number))
    }

    /// Get the chain tip block number, using direct RocksDB reads when available.
    pub(crate) async fn get_chain_tip(&self) -> Result<u64> {
        let store = self
            .ckb_store
            .as_ref()
            .expect("ckb_store must exist for chain tip reads");
        store.refresh()?;
        store
            .tip_number()
            .ok_or_else(|| anyhow::anyhow!("Failed to get chain tip from CKB RocksDB"))
    }

    // === check_and_handle_reorg, find_fork_point ===

    pub(crate) async fn check_and_handle_reorg(
        &self,
        db_tip: u64,
        stored_hash: &[u8],
    ) -> Result<Option<ReorgAction>> {
        let chain_hash_bytes = self.get_chain_block_hash(db_tip).await?;

        if chain_hash_bytes == stored_hash {
            return Ok(None);
        }

        warn!(
            run_id = %self.run_id,
            db_tip,
            stored_hash = %hex::encode(stored_hash),
            chain_hash = %hex::encode(&chain_hash_bytes),
            "Reorg detected at current DB tip"
        );

        let bounded_floor = db_tip.saturating_sub(DEEP_FORK_DEPTH);
        if bounded_floor > 0 {
            let bounded_floor_i64 = i64::try_from(bounded_floor)
                .map_err(|_| anyhow!("fork-search floor exceeds i64 range: {}", bounded_floor))?;
            let db_floor_hash = self
                .repo
                .get_block_hash_at_height(bounded_floor_i64)?
                .ok_or_else(|| anyhow!("Block {} not found in DB", bounded_floor))?;
            let chain_floor_hash = self.get_chain_block_hash(bounded_floor).await?;

            if db_floor_hash != chain_floor_hash {
                let chain_tip = self.get_chain_tip().await?;
                let chain_tip_hash_bytes = self.get_chain_block_hash(chain_tip).await?;
                let depth_lower_bound = DEEP_FORK_DEPTH
                    .checked_add(1)
                    .ok_or_else(|| anyhow!("deep fork depth lower-bound overflow"))?;

                error!(
                    run_id = %self.run_id,
                    db_tip,
                    chain_tip,
                    bounded_floor,
                    depth_lower_bound,
                    deep_fork_limit = DEEP_FORK_DEPTH,
                    "DEEP FORK DETECTED by bounded probe! Manual intervention required."
                );

                let fork_point_i64 = i64::try_from(bounded_floor).map_err(|_| {
                    anyhow!(
                        "fork point exceeds i64 range for deep fork probe: fork_point={}",
                        bounded_floor
                    )
                })?;
                let db_tip_i64 = i64::try_from(db_tip).map_err(|_| {
                    anyhow!("db tip exceeds i64 range for deep fork: db_tip={}", db_tip)
                })?;
                let chain_tip_i64 = i64::try_from(chain_tip).map_err(|_| {
                    anyhow!(
                        "chain tip exceeds i64 range for deep fork: chain_tip={}",
                        chain_tip
                    )
                })?;
                let depth_i64 = i64::try_from(depth_lower_bound).map_err(|_| {
                    anyhow!(
                        "reorg depth lower bound exceeds i64 range: depth_lower_bound={}",
                        depth_lower_bound
                    )
                })?;

                self.writer.record_deep_fork(
                    fork_point_i64,
                    &db_floor_hash,
                    db_tip_i64,
                    stored_hash,
                    chain_tip_i64,
                    &chain_tip_hash_bytes,
                    depth_i64,
                )?;

                return Ok(Some(ReorgAction::DeepForkPaused));
            }
        }

        let (fork_point, fork_hash) = self.find_fork_point(db_tip, bounded_floor).await?;
        let depth = db_tip - fork_point;

        info!(
            run_id = %self.run_id,
            fork_point,
            depth,
            "Reorg fork point discovered"
        );

        let chain_tip = self.get_chain_tip().await?;
        let chain_tip_hash_bytes = self.get_chain_block_hash(chain_tip).await?;

        if depth > DEEP_FORK_DEPTH {
            error!(
                run_id = %self.run_id,
                db_tip,
                chain_tip,
                fork_point,
                depth,
                deep_fork_limit = DEEP_FORK_DEPTH,
                "DEEP FORK DETECTED! Manual intervention required."
            );

            let fork_point_i64 = i64::try_from(fork_point).map_err(|_| {
                anyhow!(
                    "fork point exceeds i64 range for deep fork: fork_point={}",
                    fork_point
                )
            })?;
            let db_tip_i64 = i64::try_from(db_tip).map_err(|_| {
                anyhow!("db tip exceeds i64 range for deep fork: db_tip={}", db_tip)
            })?;
            let chain_tip_i64 = i64::try_from(chain_tip).map_err(|_| {
                anyhow!(
                    "chain tip exceeds i64 range for deep fork: chain_tip={}",
                    chain_tip
                )
            })?;
            let depth_i64 = i64::try_from(depth)
                .map_err(|_| anyhow!("reorg depth exceeds i64 range: depth={}", depth))?;

            self.writer.record_deep_fork(
                fork_point_i64,
                &fork_hash,
                db_tip_i64,
                stored_hash,
                chain_tip_i64,
                &chain_tip_hash_bytes,
                depth_i64,
            )?;

            return Ok(Some(ReorgAction::DeepForkPaused));
        }

        info!(
            run_id = %self.run_id,
            db_tip,
            chain_tip,
            fork_point,
            depth,
            deep_fork_limit = DEEP_FORK_DEPTH,
            "Processing automatic reorg"
        );

        self.writer
            .execute_reorg(
                self.append_only_store.as_ref(),
                i64::try_from(fork_point)
                    .map_err(|_| anyhow!("fork point exceeds i64 range: {}", fork_point))?,
                &fork_hash,
                i64::try_from(db_tip)
                    .map_err(|_| anyhow!("db tip exceeds i64 range: {}", db_tip))?,
                stored_hash,
                i64::try_from(chain_tip)
                    .map_err(|_| anyhow!("chain tip exceeds i64 range: {}", chain_tip))?,
                &chain_tip_hash_bytes,
            )
            .await?;

        Ok(Some(ReorgAction::Handled))
    }

    pub(crate) async fn find_fork_point(
        &self,
        db_tip: u64,
        min_height: u64,
    ) -> Result<(u64, Vec<u8>)> {
        let mut height = db_tip;

        loop {
            let height_i64 = i64::try_from(height)
                .map_err(|_| anyhow!("fork-search height exceeds i64 range: {}", height))?;
            let db_hash = self
                .repo
                .get_block_hash_at_height(height_i64)?
                .ok_or_else(|| anyhow::anyhow!("Block {} not found in DB", height))?;

            let chain_hash_bytes = self.get_chain_block_hash(height).await?;

            if db_hash == chain_hash_bytes {
                return Ok((height, db_hash));
            }

            if height == 0 {
                return Err(anyhow::anyhow!(
                    "No common ancestor found - genesis mismatch!"
                ));
            }

            if height == min_height {
                return Err(anyhow!(
                    "No common ancestor found within bounded reorg search window: db_tip={}, min_height={}",
                    db_tip,
                    min_height
                ));
            }

            height -= 1;
        }
    }

    // === run_proposal_cache_batch ===

    /// Enrich and cache block proposals against a single mempool snapshot.
    /// Called from a spawned task — does not require &self.
    pub(crate) async fn run_proposal_cache_batch(
        rpc: CkbRpcClient,
        cache_invalidator: CacheInvalidator,
        proposals: Vec<(Vec<u8>, i64, i16)>,
        last_block_number: i64,
    ) {
        use ckbadger_common::CachedProposal;

        if proposals.is_empty() || !cache_invalidator.is_enabled() {
            return;
        }

        let mempool = match rpc.get_raw_tx_pool_verbose().await {
            Ok(pool) => pool,
            Err(e) => {
                warn!("Failed to fetch mempool for proposal enrichment: {}", e);
                let cached: Vec<CachedProposal> = proposals
                    .iter()
                    .map(|(bytes, bn, idx)| {
                        CachedProposal::new_minimal(hex::encode(bytes), *bn, *idx)
                    })
                    .collect();
                cache_invalidator.cache_proposals(&cached).await;
                return;
            }
        };

        let mut all_mempool_txs: HashMap<String, &crate::rpc::TxPoolEntry> = HashMap::new();
        for (tx_hash, entry) in mempool.pending.iter().chain(mempool.proposed.iter()) {
            let short_id = mempool_short_tx_id(tx_hash);
            all_mempool_txs.insert(short_id.to_string(), entry);
        }

        let mut cached_proposals = Vec::with_capacity(proposals.len());

        for (proposal_bytes, block_number, idx) in &proposals {
            let proposal_id = hex::encode(proposal_bytes);

            if let Some(entry) = all_mempool_txs.get(&proposal_id) {
                if let (Ok(fee), Ok(size), Ok(cycles)) = (
                    parse_prefixed_hex_u64(&entry.fee),
                    parse_prefixed_hex_u64(&entry.size),
                    parse_prefixed_hex_u64(&entry.cycles),
                ) {
                    cached_proposals.push(CachedProposal::new_with_details(
                        proposal_id,
                        String::new(),
                        *block_number,
                        *idx,
                        fee,
                        size,
                        cycles,
                    ));
                } else {
                    warn!(
                        proposal_id = %proposal_id,
                        fee = %entry.fee,
                        size = %entry.size,
                        cycles = %entry.cycles,
                        "Malformed mempool entry fields, falling back to minimal proposal"
                    );
                    cached_proposals.push(CachedProposal::new_minimal(
                        proposal_id,
                        *block_number,
                        *idx,
                    ));
                }
            } else {
                cached_proposals.push(CachedProposal::new_minimal(
                    proposal_id,
                    *block_number,
                    *idx,
                ));
            }
        }

        cache_invalidator.cache_proposals(&cached_proposals).await;
        cache_invalidator
            .cleanup_expired_proposals(last_block_number)
            .await;
    }
}

#[cfg(test)]
#[allow(clippy::inconsistent_digit_grouping)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn jan(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2024, 1, day).expect("valid date")
    }

    #[test]
    fn cell_distribution_day_boundary_seals_before_the_new_days_block_is_applied() {
        let mut tracker = CellDistributionTracker::new();

        // First block ever: nothing to seal, only the date is recorded.
        assert!(begin_cell_distribution_block(&mut tracker, 100, jan(15)).is_none());
        tracker.cell_created(61_00000000);
        tracker
            .apply_cohort_delta(100, 61_00000000, 140_00000000)
            .unwrap();

        // First block of the next day: the seal happens here, and only then are
        // this block's cells applied.
        let (date, distribution, cohort) =
            begin_cell_distribution_block(&mut tracker, 200, jan(16)).expect("sealed day");
        tracker.cell_created(61_00000000);
        tracker
            .apply_cohort_delta(200, 61_00000000, 140_00000000)
            .unwrap();

        assert_eq!(date, jan(15));
        assert_eq!(distribution.size_bucket_counts, [1, 0, 0, 0, 0, 0]);
        assert_eq!(
            distribution.size_bucket_capacities,
            [61_00000000, 0, 0, 0, 0, 0]
        );
        assert_eq!(cohort.cohorts.len(), 1);
        assert_eq!(cohort.cohorts[0].used_capacity, 61_00000000);
        assert_eq!(cohort.cohorts[0].total_balance, 140_00000000);

        // The next day carries both cells forward.
        let (date, distribution, _) =
            begin_cell_distribution_block(&mut tracker, 300, jan(17)).expect("sealed day");
        assert_eq!(date, jan(16));
        assert_eq!(distribution.size_bucket_counts, [2, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn hodl_wave_day_boundary_seals_before_the_new_days_block_is_applied() {
        let mut tracker = HodlWaveTracker::new();

        assert!(begin_hodl_wave_block(&mut tracker, 100, jan(15)).is_none());
        tracker.update_holder_count(0, 1).unwrap();
        tracker.cell_created(jan(15), 140_00000000);

        let (date, wave) = begin_hodl_wave_block(&mut tracker, 200, jan(16)).expect("sealed day");
        tracker.update_holder_count(0, 1).unwrap();
        tracker.cell_created(jan(16), 61_00000000);

        assert_eq!(date, jan(15));
        assert_eq!(wave.holder_count, 1);
        assert_eq!(wave.band_24h, 140_00000000);
        // A cell minted on 2024-01-16 would be "-1 day old" against a
        // 2024-01-15 snapshot and land in the oldest band.
        assert_eq!(wave.band_gt_3y, 0);

        let (date, wave) = begin_hodl_wave_block(&mut tracker, 300, jan(17)).expect("sealed day");
        assert_eq!(date, jan(16));
        assert_eq!(wave.holder_count, 2);
        assert_eq!(wave.band_24h, 61_00000000);
        assert_eq!(wave.band_1d_1w, 140_00000000);
    }

    #[test]
    fn day_boundary_helpers_record_every_block_date_for_cohort_lookups() {
        let mut tracker = CellDistributionTracker::new();

        begin_cell_distribution_block(&mut tracker, 100, jan(15));
        begin_cell_distribution_block(&mut tracker, 101, jan(15));
        begin_cell_distribution_block(&mut tracker, 200, jan(16));

        // An address first seen in this very block must resolve to its cohort
        // month, which requires the transition to be recorded before the block's
        // transactions are applied.
        assert_eq!(tracker.block_number_to_date(100), Some(jan(15)));
        assert_eq!(tracker.block_number_to_date(150), Some(jan(15)));
        assert_eq!(tracker.block_number_to_date(200), Some(jan(16)));
    }
}
