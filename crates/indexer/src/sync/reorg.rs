#![allow(clippy::type_complexity)]

use std::collections::HashMap;

use anyhow::{anyhow, Result};
use tracing::{error, info, warn};

use ckbadger_store::types::PositionedCellInfo;

use crate::cache::CacheInvalidator;
use crate::config::DEEP_FORK_DEPTH;
use crate::rpc::CkbRpcClient;

use super::dao_helpers::derive_pre_batch_live_cells;
use super::helpers::*;
use super::indexer::{mempool_short_tx_id, rebuild_hodl_tracker_from_state, Indexer};
use super::types::{ReorgAction, TxData};

impl Indexer {
    pub(crate) fn reconcile_hodl_tracker_with_tip(&self, tip_block: i64) -> Result<()> {
        let state = self.writer.store().get_hodl_tracker_state()?;
        let rebuilt = rebuild_hodl_tracker_from_state(state, tip_block)?;

        let mut tracker = self.hodl_tracker.lock().unwrap();
        *tracker = rebuilt;
        Ok(())
    }

    /// Feed parsed block data into the HODL wave tracker and write snapshots at day boundaries.
    pub(crate) fn update_hodl_wave(
        &self,
        all_parsed_blocks: &[crate::parser::block::ParsedBlock],
        all_tx_data: &[TxData],
        input_cell_info: &HashMap<(Vec<u8>, i16), PositionedCellInfo>,
        batch_cell_infos: &HashMap<(Vec<u8>, i16), PositionedCellInfo>,
        address_balance_changes: &HashMap<Vec<u8>, (i128, i32, i32, i64, i64, Vec<u8>, i128)>,
    ) -> Result<()> {
        let mut tracker = self.hodl_tracker.lock().unwrap();
        let store = self.writer.store();

        // Phase 1: Record block dates and cell creates/consumes
        let mut block_tx_idx = 0usize;
        for parsed in all_parsed_blocks {
            let block_date = ckbadger_common::block_date(parsed.timestamp);
            tracker.record_block_date(parsed.number, block_date);

            let tx_count = parsed.transactions_count as usize;
            let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count];
            block_tx_idx += tx_count;

            for tx_data in tx_slice {
                // Cell creates
                for cell in &tx_data.cells {
                    tracker.cell_created(block_date, cell.capacity);
                }
                // Cell consumes
                if !tx_data.is_cellbase {
                    for input in &tx_data.inputs {
                        let key = (
                            input.previous_tx_hash.to_vec(),
                            parsed_input_outpoint_index_i16(
                                input.previous_output_index,
                                "sync_indexer",
                            ),
                        );
                        let info = input_cell_info
                            .get(&key)
                            .or_else(|| batch_cell_infos.get(&key));
                        if let Some(info) = info {
                            tracker.cell_consumed(info.created_at_block, info.capacity)?;
                        }
                    }
                }
            }

            // Check for day boundary and write snapshot
            if let Some((snapshot_date, snapshot)) = tracker.maybe_snapshot(block_date) {
                let date_str = snapshot_date.format("%Y%m%d").to_string();
                store.put_hodl_wave(&date_str, &snapshot)?;
            }
        }

        // Phase 2: Update holder count from address balance changes
        // Each entry: (balance_delta, live_delta, total_delta, tx_delta, block_num, tx_hash, used_delta)
        // Batch-read all address balances in a single multi_get_cf call
        let lock_hash_refs: Vec<&Vec<u8>> = address_balance_changes.keys().collect();
        let balance_map = if lock_hash_refs.is_empty() {
            HashMap::new()
        } else {
            self.writer.read_address_balances(&lock_hash_refs)?
        };
        for (
            lock_hash,
            (
                _balance_delta,
                live_delta,
                _total_delta,
                _tx_delta,
                _block_num,
                _tx_hash,
                _used_delta,
            ),
        ) in address_balance_changes
        {
            let current_balance = balance_map.get(lock_hash).and_then(|o| o.as_ref());
            let post_live = current_balance.map(|b| b.live_cells_count).unwrap_or(0);
            let old_live = derive_pre_batch_live_cells(post_live, *live_delta)?;
            tracker.update_holder_count(old_live, post_live)?;
        }

        // Phase 3: Persist tracker state
        store.put_hodl_tracker_state(&tracker.to_state())?;

        Ok(())
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

        let result = self
            .writer
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

        Ok(Some(ReorgAction::Handled(result)))
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
            match mempool_short_tx_id(tx_hash) {
                Ok(short_id) => {
                    all_mempool_txs.insert(short_id.to_string(), entry);
                }
                Err(e) => {
                    warn!(tx_hash, error = %e, "Skipping malformed mempool tx hash in proposal cache");
                }
            }
        }

        let mut cached_proposals = Vec::with_capacity(proposals.len());

        for (proposal_bytes, block_number, idx) in &proposals {
            let proposal_id = hex::encode(proposal_bytes);

            if let Some(entry) = all_mempool_txs.get(&proposal_id) {
                match (
                    parse_prefixed_hex_u64(&entry.fee, "mempool proposal fee"),
                    parse_prefixed_hex_u64(&entry.size, "mempool proposal size"),
                    parse_prefixed_hex_u64(&entry.cycles, "mempool proposal cycles"),
                ) {
                    (Ok(fee), Ok(size), Ok(cycles)) => {
                        cached_proposals.push(CachedProposal::new_with_details(
                            proposal_id,
                            String::new(),
                            *block_number,
                            *idx,
                            fee,
                            size,
                            cycles,
                        ));
                    }
                    _ => {
                        warn!(
                            "Invalid mempool entry fields for proposal {}, using minimal",
                            proposal_id
                        );
                        cached_proposals.push(CachedProposal::new_minimal(
                            proposal_id,
                            *block_number,
                            *idx,
                        ));
                    }
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
