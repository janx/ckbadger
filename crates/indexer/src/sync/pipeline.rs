#![allow(clippy::type_complexity)]

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

use ckbadger_store::types::{LiveCellInfo, NftTypeIndex, SporeTypeIndex};

use crate::db::writer::dotbit::DOTBIT_SENTINEL_COLLECTION;
use crate::parser::{
    analyze_spore_media_profile, DotbitParser, MnftParser, ParsedClusterCell,
    ParsedDotbitAccountOutput, ParsedMnftClass, ParsedMnftIssuer, ParsedMnftToken, ParsedSporeCell,
    SporeParser,
};
use crate::rpc::{BlockResponseWithCycles, CkbRpcClient};
use crate::runtime_diag::read_cgroup_memory_snapshot;

use ckb_store_reader::CkbChainReader;
use futures::stream::{FuturesOrdered, StreamExt};
use rayon::prelude::*;

use super::adaptive::*;
use super::batch::*;
use super::dao_helpers::*;
use super::diagnostics::*;
use super::helpers::*;
use super::indexer::{
    blocks_behind_tip, next_start_block_from_db_tip, require_non_negative_block_number, Indexer,
};
use super::nft_helpers::*;
use super::sync_mode::*;
use super::token_helpers::*;
use super::types::{CachedCellInfo, DotbitConsumptionEvent, PreParsedNftData, ReorgAction, TxData};
use super::undo::*;

fn requires_direct_reads_for_fetch(
    bulk_sync_allowed: bool,
    blocks_behind: u64,
    bulk_sync_threshold: u64,
) -> bool {
    bulk_sync_allowed && is_bulk_sync_active_by_lag(blocks_behind, bulk_sync_threshold)
}

impl Indexer {
    pub(super) async fn run_pipeline(&self) -> Result<()> {
        use tokio::sync::mpsc;

        type FetchedBatch = (u64, u64, u64, u64, Arc<Vec<BlockResponseWithCycles>>);
        /// Pre-parsed spore/cluster data per-tx (flattened across all blocks).
        /// Each entry corresponds to one tx in all_tx_data, containing
        /// (parsed_spores, parsed_clusters) for that transaction.
        type PreParsedSporeData = Vec<(Vec<ParsedSporeCell>, Vec<ParsedClusterCell>)>;

        type ParsedBatch = (
            u64,
            u64,
            u64,
            u64,
            u64, // batch_tx_count
            Arc<Vec<BlockResponseWithCycles>>,
            Vec<crate::parser::block::ParsedBlock>,
            Vec<TxData>,
            HashMap<(Vec<u8>, i16), LiveCellInfo>,
            // Pre-computed in parser stage:
            HashMap<(Vec<u8>, i16), LiveCellInfo>, // batch_cell_infos
            HashMap<Vec<u8>, (i128, i32, i32, i64, i64, Vec<u8>, i128)>, // address_balance_changes
            ScriptUsageChanges,                    // script_usage_changes
            HashMap<(Vec<u8>, bool, u32), (i128, i128)>, // script_daily_changes
            HashMap<(Vec<u8>, u32), (i128, i128)>, // token_daily_changes
            HashMap<Vec<u8>, SporeTypeIndex>,      // spore_type_index_changes
            HashMap<(Vec<u8>, u32), (i128, i128)>, // spore_daily_changes
            HashMap<(Vec<u8>, u32), (i128, i128)>, // cluster_daily_changes
            HashMap<Vec<u8>, NftTypeIndex>,        // nft_type_index_changes
            HashMap<(Vec<u8>, u32), (i128, i128)>, // nft_daily_changes
            PreParsedSporeData,                    // pre-parsed spore/cluster data
            PreParsedNftData,                      // pre-parsed mNFT/DotBit data
        );

        let (fetch_tx, mut fetch_rx) = mpsc::channel::<FetchedBatch>(self.config.pipeline_buffer);
        let (parse_tx, mut parse_rx) = mpsc::channel::<ParsedBatch>(self.config.pipeline_buffer);
        let parse_tx_pending_txs = Arc::new(AtomicU64::new(0));
        let parser_exit_reason = Arc::new(std::sync::Mutex::new(None::<String>));
        let fetcher_exit_reason = Arc::new(std::sync::Mutex::new(None::<String>));
        let committed_tip_for_cache = Arc::new(AtomicI64::new(self.repo.get_sync_tip().await?.0));
        self.pipeline_perf
            .set_queue_capacities(self.config.pipeline_buffer, self.config.pipeline_buffer);

        let rpc = self.rpc.clone();
        let config = self.config.clone();
        let progress = Arc::clone(&self.progress);
        let repo = self.repo.clone();
        let run_id_for_fetcher = self.run_id.clone();
        let rebuild_pause = Arc::clone(&self.rebuild_pause_flag);
        let pipeline_reset_notify = Arc::clone(&self.pipeline_reset_notify_flag);
        let pipeline_reset_reason_code = Arc::clone(&self.pipeline_reset_reason_code);
        let pipeline_epoch_for_fetcher = Arc::clone(&self.pipeline_reset_epoch);
        let ckb_store = self.ckb_store.clone();
        let bulk_sync_allowed_for_fetcher = self.bulk_sync_allowed.load(Ordering::SeqCst);
        let pipeline_perf_for_fetcher = Arc::clone(&self.pipeline_perf);
        let adaptive_batch_controller_for_fetcher = Arc::clone(&self.adaptive_batch_controller);
        let parse_tx_for_fetcher_depth = parse_tx.clone();
        let parse_tx_pending_txs_for_parser = Arc::clone(&parse_tx_pending_txs);
        let parse_tx_pending_txs_for_writer = Arc::clone(&parse_tx_pending_txs);
        let fetcher_exit_reason_for_fetcher = Arc::clone(&fetcher_exit_reason);

        // === Fetcher task ===
        let fetcher = tokio::spawn(async move {
            let mut next_block: Option<u64> = None;
            let mut was_paused = false;

            loop {
                if rebuild_pause.load(Ordering::SeqCst) {
                    debug!("Fetcher paused for index rebuild");
                    was_paused = true;
                    sleep(Duration::from_millis(500)).await;
                    continue;
                }
                if was_paused {
                    info!("Fetcher resuming from pause, resetting next_block to re-query DB state");
                    next_block = None;
                    was_paused = false;
                }
                if pipeline_reset_notify.swap(false, Ordering::SeqCst) {
                    let reason = decode_pipeline_reset_reason(
                        pipeline_reset_reason_code.load(Ordering::SeqCst),
                    );
                    let pipeline_epoch = pipeline_epoch_for_fetcher.load(Ordering::SeqCst);
                    info!(
                        run_id = %run_id_for_fetcher,
                        pipeline_epoch,
                        reason,
                        "Fetcher received pipeline reset notification, resetting next_block"
                    );
                    next_block = None;
                }

                if let Some(ref store) = ckb_store {
                    if let Err(e) = store.refresh() {
                        error!("Failed to refresh CKB RocksDB secondary: {}", e);
                        sleep(Duration::from_secs(5)).await;
                        continue;
                    }
                }

                let chain_tip = if let Some(ref store) = ckb_store {
                    match store.tip_number() {
                        Some(tip) => tip,
                        None => {
                            error!("Failed to get chain tip from CKB RocksDB");
                            sleep(Duration::from_secs(5)).await;
                            continue;
                        }
                    }
                } else {
                    match rpc.get_tip_block_number().await {
                        Ok(tip) => tip,
                        Err(e) => {
                            error!("Failed to get chain tip: {}", e);
                            sleep(Duration::from_secs(5)).await;
                            continue;
                        }
                    }
                };
                progress.update_target(chain_tip);

                let start_block = match next_block {
                    Some(nb) => nb,
                    None => {
                        let (db_tip, db_tip_hash) = match repo.get_sync_tip().await {
                            Ok(tip) => tip,
                            Err(e) => {
                                error!("Failed to get DB tip: {}", e);
                                sleep(Duration::from_secs(5)).await;
                                continue;
                            }
                        };
                        match next_start_block_from_db_tip(
                            db_tip,
                            &db_tip_hash,
                            "pipeline fetcher start_block",
                        ) {
                            Ok(start) => start,
                            Err(e) => {
                                error!("Failed to compute fetch start block: {}", e);
                                sleep(Duration::from_secs(5)).await;
                                continue;
                            }
                        }
                    }
                };

                if start_block > chain_tip {
                    debug!(
                        "Fetcher waiting: start_block {} > chain_tip {}",
                        start_block, chain_tip
                    );
                    sleep(Duration::from_millis(config.poll_interval_ms)).await;
                    continue;
                }

                if let Some((previous_target_batch_txs, new_target_batch_txs)) =
                    adaptive_batch_controller_for_fetcher
                        .maybe_apply_early_height_boost(start_block)
                {
                    info!(
                        start_block,
                        cutoff_height = ADAPTIVE_BATCH_EARLY_HEIGHT_CUTOFF,
                        previous_target_batch_txs,
                        new_target_batch_txs,
                        "Adaptive batch warmup: boosted target batch txs for early-chain bulk sync"
                    );
                }

                let adaptive_snapshot = adaptive_batch_controller_for_fetcher.snapshot();
                let fetch_queue_depth_now = sender_queue_depth(&fetch_tx);
                let parse_queue_depth_now = sender_queue_depth(&parse_tx_for_fetcher_depth);
                let inflight_batches = fetch_queue_depth_now.saturating_add(parse_queue_depth_now);
                if inflight_batches >= adaptive_snapshot.inflight_limit {
                    sleep(Duration::from_millis(50)).await;
                    continue;
                }

                let dynamic_span = adaptive_batch_controller_for_fetcher
                    .estimate_block_span(config.batch_size as u64);
                let end_block = std::cmp::min(start_block + dynamic_span - 1, chain_tip);

                debug!(
                    "Fetcher: fetching blocks {} to {} (chain_tip={}, next_block={:?}, adaptive_txs={}, adaptive_min_txs={}, inflight_limit={}, inflight_batches={})",
                    start_block,
                    end_block,
                    chain_tip,
                    next_block,
                    adaptive_snapshot.target_batch_txs,
                    adaptive_snapshot.min_target_batch_txs,
                    adaptive_snapshot.inflight_limit,
                    inflight_batches
                );

                let fetch_cycle_epoch = pipeline_epoch_for_fetcher.load(Ordering::SeqCst);
                let fetch_started = Instant::now();
                let blocks = if let Some(ref store) = ckb_store {
                    let store = Arc::clone(store);
                    let sb = start_block;
                    let eb = end_block;
                    match tokio::task::spawn_blocking(move || {
                        Self::fetch_blocks_direct(&store, sb, eb)
                    })
                    .await
                    {
                        Ok(Ok(blocks)) => blocks,
                        Ok(Err(e)) => {
                            error!("Failed to fetch blocks from RocksDB: {}", e);
                            sleep(Duration::from_secs(5)).await;
                            next_block = None;
                            continue;
                        }
                        Err(e) => {
                            error!("Block fetch task panicked: {}", e);
                            sleep(Duration::from_secs(5)).await;
                            next_block = None;
                            continue;
                        }
                    }
                } else {
                    let blocks_behind = chain_tip.saturating_sub(start_block);
                    if requires_direct_reads_for_fetch(
                        bulk_sync_allowed_for_fetcher,
                        blocks_behind,
                        config.bulk_sync_threshold,
                    ) {
                        record_worker_exit_reason(
                            &fetcher_exit_reason_for_fetcher,
                            format!(
                                "bulk sync requires direct RocksDB reads but CKB_DATA_PATH is not set: range={}-{}, chain_tip={}",
                                start_block, end_block, chain_tip
                            ),
                        );
                        error!(
                            "bulk sync requires direct RocksDB reads but CKB_DATA_PATH is not set \
                             (blocks {}-{}). Set CKB_DATA_PATH to the CKB node data directory",
                            start_block, end_block
                        );
                        break;
                    }
                    match Self::fetch_blocks_with_config(
                        &rpc,
                        start_block,
                        end_block,
                        config.parallel_fetch_size,
                    )
                    .await
                    {
                        Ok(blocks) => blocks,
                        Err(e) => {
                            error!("Failed to fetch blocks: {}", e);
                            sleep(Duration::from_secs(5)).await;
                            next_block = None;
                            continue;
                        }
                    }
                };
                let fetch_elapsed = fetch_started.elapsed();

                // Split into sub-batches if too many transactions.
                // IMPORTANT: move blocks into sub-batches to avoid cloning large vectors.
                let max_txs = adaptive_sub_batch_tx_cap(
                    adaptive_snapshot.target_batch_txs,
                    adaptive_snapshot.min_target_batch_txs,
                );
                let max_inputs = adaptive_sub_batch_input_cap(
                    adaptive_snapshot.target_batch_txs,
                    adaptive_snapshot.min_target_batch_txs,
                );
                let mut send_failed_reason: Option<String> = None;
                let tx_counts: Vec<usize> = blocks
                    .iter()
                    .map(|block| block.block.transactions.len())
                    .collect();
                let input_counts: Vec<usize> = blocks
                    .iter()
                    .map(|block| {
                        block
                            .block
                            .transactions
                            .iter()
                            .map(|tx| tx.inputs.len())
                            .sum::<usize>()
                    })
                    .collect();
                adaptive_batch_controller_for_fetcher
                    .observe_tx_density(tx_counts.iter().sum(), tx_counts.len());
                let sub_batch_plan =
                    plan_fetch_sub_batches(&tx_counts, &input_counts, max_txs, max_inputs);
                let mut block_iter = blocks.into_iter();
                let mut sub_start_block = start_block;

                for (idx, (sub_block_count, sub_txs, sub_inputs)) in
                    sub_batch_plan.into_iter().enumerate()
                {
                    let sub_blocks: Vec<_> = block_iter.by_ref().take(sub_block_count).collect();
                    if sub_blocks.len() != sub_block_count {
                        error!(
                            expected = sub_block_count,
                            actual = sub_blocks.len(),
                            "Fetcher: planned sub-batch size mismatch"
                        );
                        send_failed_reason = Some(format!(
                            "planned sub-batch size mismatch: expected={}, actual={}, range={}-{}",
                            sub_block_count,
                            sub_blocks.len(),
                            start_block,
                            end_block
                        ));
                        break;
                    }

                    let sub_end_block = sub_start_block + sub_blocks.len() as u64 - 1;
                    if idx > 0 {
                        debug!(
                            sub_start_block,
                            sub_end_block,
                            txs = sub_txs,
                            inputs = sub_inputs,
                            "Fetcher: sending sub-batch"
                        );
                    }

                    if fetch_tx
                        .send((
                            fetch_cycle_epoch,
                            sub_start_block,
                            sub_end_block,
                            chain_tip,
                            Arc::new(sub_blocks),
                        ))
                        .await
                        .is_err()
                    {
                        send_failed_reason = Some(format!(
                            "failed to send fetched sub-batch to parser: sub_range={}-{}, chain_tip={}, pipeline_epoch={}",
                            sub_start_block, sub_end_block, chain_tip, fetch_cycle_epoch
                        ));
                        break;
                    }

                    sub_start_block = sub_end_block + 1;
                }

                if send_failed_reason.is_none() && block_iter.next().is_some() {
                    error!("Fetcher: leftover blocks after planned sub-batch splitting");
                    send_failed_reason = Some(format!(
                        "leftover blocks after planned sub-batch splitting: range={}-{}",
                        start_block, end_block
                    ));
                }

                if let Some(reason) = send_failed_reason {
                    record_worker_exit_reason(&fetcher_exit_reason_for_fetcher, reason);
                    break;
                }

                let fetch_queue_depth = fetch_tx.max_capacity() - fetch_tx.capacity();
                pipeline_perf_for_fetcher.record_fetch(
                    fetch_elapsed,
                    fetch_queue_depth,
                    fetch_tx.max_capacity(),
                );

                next_block = Some(next_fetch_start_after_batch(end_block));
            }
        });

        // === Parser task ===
        let writer_for_parser = self.writer.clone();
        let rpc_for_parser = self.rpc.clone();
        let cell_cache_for_parser = Arc::clone(&self.cell_cache);
        let committed_tip_for_cache_for_parser = Arc::clone(&committed_tip_for_cache);
        let pipeline_perf_for_parser = Arc::clone(&self.pipeline_perf);
        let pipeline_epoch_for_parser = Arc::clone(&self.pipeline_reset_epoch);
        let parser_exit_reason_for_parser = Arc::clone(&parser_exit_reason);
        let bulk_sync_threshold_for_parser = self.config.bulk_sync_threshold;

        let parse_tx_for_writer_depth = parse_tx.clone();
        let parser = tokio::spawn(async move {
            'parser_batches: while let Some((
                batch_epoch,
                start_block,
                end_block,
                chain_tip,
                blocks,
            )) = fetch_rx.recv().await
            {
                if batch_epoch != pipeline_epoch_for_parser.load(Ordering::SeqCst) {
                    debug!(
                        batch_epoch,
                        "Skipping stale fetched batch {}-{}", start_block, end_block
                    );
                    continue;
                }
                let t_parser = Instant::now();

                let blocks_ref = Arc::clone(&blocks);
                let (all_parsed_blocks, mut all_tx_data, all_input_outpoints) =
                    match tokio::task::spawn_blocking(move || parse_blocks_parallel(&blocks_ref))
                        .await
                    {
                        Ok(Ok(parsed)) => parsed,
                        Ok(Err(e)) => {
                            error!(
                                start_block,
                                end_block, "Parser: parse_blocks_parallel failed: {}", e
                            );
                            record_worker_exit_reason(
                                &parser_exit_reason_for_parser,
                                format!(
                                    "parse_blocks_parallel failed for range {}-{}: {}",
                                    start_block, end_block, e
                                ),
                            );
                            return;
                        }
                        Err(e) => {
                            error!(
                                start_block,
                                end_block, "Parser: parse_blocks_parallel task panicked: {}", e
                            );
                            record_worker_exit_reason(
                                &parser_exit_reason_for_parser,
                                format!(
                                    "parse_blocks_parallel task panicked for range {}-{}: {}",
                                    start_block, end_block, e
                                ),
                            );
                            return;
                        }
                    };

                if all_parsed_blocks.is_empty() {
                    continue;
                }

                let t_parse_ms = t_parser.elapsed().as_secs_f64() * 1000.0;

                let mut batch_cells: HashMap<(Vec<u8>, i16), ()> = HashMap::new();
                for td in &all_tx_data {
                    for (idx, _) in td.cells.iter().enumerate() {
                        let output_index = match checked_usize_to_i16(
                            idx,
                            "pipeline parser batch cell output index",
                        ) {
                            Ok(v) => v,
                            Err(e) => {
                                record_worker_exit_reason(
                                        &parser_exit_reason_for_parser,
                                        format!(
                                            "pipeline parser batch cell output index conversion failed for range {}-{}: tx_hash=0x{}, output_index={}, error={}",
                                            start_block,
                                            end_block,
                                            hex::encode(td.hash),
                                            idx,
                                            e
                                        ),
                                    );
                                return;
                            }
                        };
                        batch_cells.insert((td.hash.to_vec(), output_index), ());
                    }
                }

                let t_cell_lookup = Instant::now();
                let mut unresolved_retry_count: usize = 0;
                let resolved_input_cells: Option<(
                    HashMap<(Vec<u8>, i16), LiveCellInfo>,
                    usize,
                    usize,
                )> = loop {
                    let current_epoch = pipeline_epoch_for_parser.load(Ordering::SeqCst);
                    if should_abort_unresolved_retry_on_epoch_change(batch_epoch, current_epoch) {
                        info!(
                            start_block,
                            end_block,
                            batch_epoch,
                            current_epoch,
                            retries = unresolved_retry_count,
                            "Parser: aborting unresolved input retry because batch became stale after pipeline reset"
                        );
                        break None;
                    }

                    let mut attempt_cache_hits: usize = 0;
                    let mut attempt_input_cell_info: HashMap<(Vec<u8>, i16), LiveCellInfo> =
                        HashMap::new();
                    for (tx_hash, idx) in &all_input_outpoints {
                        let hash_arr = match tx_hash_key32(
                            tx_hash,
                            "pipeline parser input cell cache lookup",
                        ) {
                            Ok(v) => v,
                            Err(e) => {
                                record_worker_exit_reason(
                                    &parser_exit_reason_for_parser,
                                    format!(
                                        "pipeline parser input cell cache key conversion failed for range {}-{}: tx_hash_len={}, error={}",
                                        start_block,
                                        end_block,
                                        tx_hash.len(),
                                        e
                                    ),
                                );
                                return;
                            }
                        };
                        let key = (hash_arr, *idx);
                        if let Some(cached) = cell_cache_for_parser.get(&key) {
                            attempt_cache_hits += 1;
                            attempt_input_cell_info.insert(
                                (tx_hash.clone(), *idx),
                                LiveCellInfo {
                                    capacity: cached.capacity,
                                    created_at_block: cached.created_at_block,
                                    lock_script_hash: cached.lock_script_hash.clone(),
                                    lock_code_hash: cached.lock_code_hash.clone(),
                                    lock_hash_type: cached.lock_hash_type,
                                    lock_args: cached.lock_args.clone(),
                                    type_script_hash: cached.type_script_hash.clone(),
                                    type_code_hash: cached.type_code_hash.clone(),
                                    type_args: cached.type_args.clone(),
                                    data_size: cached.data_size,
                                    occupied_capacity: cached.occupied_capacity,
                                    udt_amount: cached.udt_amount,
                                },
                            );
                        }
                    }

                    let missing_outpoints = collect_missing_input_outpoints(
                        &all_input_outpoints,
                        &attempt_input_cell_info,
                        &batch_cells,
                    );

                    let mut db_lookups = 0usize;
                    let mut db_lookup_failed = false;
                    if !missing_outpoints.is_empty() {
                        db_lookups = missing_outpoints.len();
                        let bulk_sync_mode = is_bulk_sync_batch(
                            chain_tip,
                            end_block,
                            bulk_sync_threshold_for_parser,
                        );
                        let wr = writer_for_parser.clone();
                        let missing_owned: Vec<(Vec<u8>, i16)> = missing_outpoints
                            .iter()
                            .map(|(h, i)| (h.clone(), *i))
                            .collect();
                        let db_query = tokio::task::spawn_blocking(move || {
                            let refs: Vec<(&[u8], i16)> = missing_owned
                                .iter()
                                .map(|(h, i)| (h.as_slice(), *i))
                                .collect();
                            wr.get_full_cells_info_batch(&refs, bulk_sync_mode)
                        });
                        match tokio::time::timeout(Duration::from_secs(30), db_query).await {
                            Ok(Ok(Ok(db_info))) => {
                                for ((tx_hash, idx), info) in db_info {
                                    attempt_input_cell_info.insert((tx_hash, idx), info);
                                }
                            }
                            Ok(Ok(Err(e))) => {
                                error!("Parser: DB error fetching cell info: {}", e);
                                db_lookup_failed = true;
                            }
                            Ok(Err(e)) => {
                                error!("Parser: Failed to fetch cell info from DB: {}", e);
                                db_lookup_failed = true;
                            }
                            Err(_) => {
                                warn!(
                                    "Parser: DB query for cell info timed out after 30s, forcing batch retry"
                                );
                                db_lookup_failed = true;
                            }
                        }
                    }

                    let unresolved_outpoints = collect_missing_input_outpoints(
                        &all_input_outpoints,
                        &attempt_input_cell_info,
                        &batch_cells,
                    );

                    if !db_lookup_failed && unresolved_outpoints.is_empty() {
                        break Some((attempt_input_cell_info, attempt_cache_hits, db_lookups));
                    }

                    unresolved_retry_count += 1;
                    if should_log_unresolved_retry(unresolved_retry_count) {
                        let unresolved_local_probe = classify_unresolved_local_probe(
                            &writer_for_parser,
                            &unresolved_outpoints,
                            PARSER_UNRESOLVED_PROBE_SAMPLE_SIZE,
                        )
                        .format_for_log();
                        let unresolved_rpc_probe = match tokio::time::timeout(
                            Duration::from_secs(PARSER_UNRESOLVED_RPC_PROBE_TIMEOUT_SECS),
                            collect_unresolved_rpc_probe(
                                &rpc_for_parser,
                                &unresolved_outpoints,
                                PARSER_UNRESOLVED_PROBE_SAMPLE_SIZE,
                            ),
                        )
                        .await
                        {
                            Ok(summary) => summary.format_for_log(),
                            Err(_) => format!(
                                "timeout_after={}s",
                                PARSER_UNRESOLVED_RPC_PROBE_TIMEOUT_SECS
                            ),
                        };
                        warn!(
                            start_block,
                            end_block,
                            retry = unresolved_retry_count,
                            unresolved_count = unresolved_outpoints.len(),
                            unresolved_sample = %format_outpoint_sample(&unresolved_outpoints, 5),
                            db_lookup_failed,
                            unresolved_local_probe = %unresolved_local_probe,
                            unresolved_rpc_probe = %unresolved_rpc_probe,
                            "Parser: unresolved input cells detected; waiting for writer progress and retrying same batch"
                        );
                    }

                    if unresolved_retry_count >= PARSER_UNRESOLVED_MAX_RETRIES {
                        let unresolved_local_probe = classify_unresolved_local_probe(
                            &writer_for_parser,
                            &unresolved_outpoints,
                            PARSER_UNRESOLVED_PROBE_SAMPLE_SIZE,
                        )
                        .format_for_log();
                        let unresolved_rpc_probe = match tokio::time::timeout(
                            Duration::from_secs(PARSER_UNRESOLVED_RPC_PROBE_TIMEOUT_SECS),
                            collect_unresolved_rpc_probe(
                                &rpc_for_parser,
                                &unresolved_outpoints,
                                PARSER_UNRESOLVED_PROBE_SAMPLE_SIZE,
                            ),
                        )
                        .await
                        {
                            Ok(summary) => summary.format_for_log(),
                            Err(_) => format!(
                                "timeout_after={}s",
                                PARSER_UNRESOLVED_RPC_PROBE_TIMEOUT_SECS
                            ),
                        };
                        error!(
                            start_block,
                            end_block,
                            retries = unresolved_retry_count,
                            unresolved_count = unresolved_outpoints.len(),
                            unresolved_sample = %format_outpoint_sample(&unresolved_outpoints, 5),
                            db_lookup_failed,
                            unresolved_local_probe = %unresolved_local_probe,
                            unresolved_rpc_probe = %unresolved_rpc_probe,
                            "Parser: unresolved input cells persisted after max retries; stopping parser task"
                        );
                        record_worker_exit_reason(
                            &parser_exit_reason_for_parser,
                            format!(
                                "unresolved input cells after max retries for range {}-{}: retries={}, unresolved_count={}, db_lookup_failed={}",
                                start_block,
                                end_block,
                                unresolved_retry_count,
                                unresolved_outpoints.len(),
                                db_lookup_failed
                            ),
                        );
                        return;
                    }

                    sleep(Duration::from_millis(PARSER_UNRESOLVED_RETRY_DELAY_MS)).await;
                };
                let Some((input_cell_info, cache_hits, cache_misses)) = resolved_input_cells else {
                    continue 'parser_batches;
                };

                let cell_lookup_ms = t_cell_lookup.elapsed().as_secs_f64() * 1000.0;

                // Pre-compute batch_cell_infos, fees, cell_cache, balance/script changes
                // (moved from writer to overlap with pipeline buffering)
                let t_precompute_parser = Instant::now();
                let mut udt_standard_hint_cache: HashMap<Vec<u8>, Option<String>> = HashMap::new();

                // Pass 1: Build batch_cell_infos
                let mut batch_cell_infos: HashMap<(Vec<u8>, i16), LiveCellInfo> = HashMap::new();
                for tx_data in &all_tx_data {
                    for (output_index, cell) in tx_data.cells.iter().enumerate() {
                        let output_index_i16 = match checked_usize_to_i16(
                            output_index,
                            "pipeline parser batch cell output index",
                        ) {
                            Ok(v) => v,
                            Err(e) => {
                                error!(
                                    block_number = tx_data.block_number,
                                    tx_hash = %hex::encode(tx_data.hash),
                                    output_index,
                                    "Parser: {}",
                                    e
                                );
                                record_worker_exit_reason(
                                    &parser_exit_reason_for_parser,
                                    format!(
                                        "invalid output index while precomputing batch_cell_infos: block={}, tx=0x{}, output_index={}, error={}",
                                        tx_data.block_number,
                                        hex::encode(tx_data.hash),
                                        output_index,
                                        e
                                    ),
                                );
                                return;
                            }
                        };
                        let standard_hint = if let Some(type_hash) = cell.type_script_hash.as_ref()
                        {
                            if let Some(cached) = udt_standard_hint_cache.get(type_hash) {
                                cached.clone()
                            } else {
                                let looked_up = match writer_for_parser.store().get_token(type_hash)
                                {
                                    Ok(token) => token.map(|info| info.standard),
                                    Err(e) => {
                                        error!(
                                            block_number = tx_data.block_number,
                                            tx_hash = %hex::encode(tx_data.hash),
                                            output_index,
                                            type_script_hash = %hex::encode(type_hash),
                                            "Parser: token metadata lookup failed while parsing UDT output hint: {}",
                                            e
                                        );
                                        record_worker_exit_reason(
                                            &parser_exit_reason_for_parser,
                                            format!(
                                                "token metadata lookup failed while parsing UDT output hint: block={}, tx=0x{}, output_index={}, error={}",
                                                tx_data.block_number,
                                                hex::encode(tx_data.hash),
                                                output_index,
                                                e
                                            ),
                                        );
                                        return;
                                    }
                                };
                                udt_standard_hint_cache
                                    .insert(type_hash.clone(), looked_up.clone());
                                looked_up
                            }
                        } else {
                            None
                        };
                        let udt_amount = match parse_parsed_cell_udt_amount(
                            cell,
                            &tx_data.hash,
                            output_index_i16,
                            standard_hint.as_deref(),
                        ) {
                            Ok(v) => v,
                            Err(e) => {
                                error!(
                                    block_number = tx_data.block_number,
                                    tx_hash = %hex::encode(tx_data.hash),
                                    output_index,
                                    "Parser: {}",
                                    e
                                );
                                record_worker_exit_reason(
                                    &parser_exit_reason_for_parser,
                                    format!(
                                        "failed to parse UDT amount while precomputing batch_cell_infos: block={}, tx=0x{}, output_index={}, error={}",
                                        tx_data.block_number,
                                        hex::encode(tx_data.hash),
                                        output_index,
                                        e
                                    ),
                                );
                                return;
                            }
                        };
                        let occupied_capacity = occupied_capacity_shannons_i64(
                            cell.lock_args.len(),
                            cell.type_args.as_ref().map(|args| args.len()),
                            cell.data_size,
                        );
                        batch_cell_infos.insert(
                            (tx_data.hash.to_vec(), output_index_i16),
                            LiveCellInfo {
                                capacity: cell.capacity,
                                created_at_block: tx_data.block_number,
                                lock_script_hash: cell.lock_script_hash.clone(),
                                lock_code_hash: cell.lock_code_hash.clone(),
                                lock_hash_type: cell.lock_hash_type,
                                lock_args: cell.lock_args.clone(),
                                type_script_hash: cell.type_script_hash.clone(),
                                type_code_hash: cell.type_code_hash.clone(),
                                type_args: cell.type_args.clone(),
                                data_size: cell.data_size,
                                occupied_capacity,
                                udt_amount,
                            },
                        );
                    }
                }

                // Pass 2: Compute input capacity + fee
                let dao_code_hash =
                    crate::rpc::parse_hex_to_bytes(crate::parser::dao::DAO_CODE_HASH);
                for tx_data in &mut all_tx_data {
                    if !tx_data.is_cellbase {
                        let mut has_dao_input = false;
                        for input in &tx_data.inputs {
                            let key = (
                                input.previous_tx_hash.to_vec(),
                                parsed_input_outpoint_index_i16(
                                    input.previous_output_index,
                                    "sync_indexer",
                                ),
                            );
                            if let Some(info) = input_cell_info.get(&key) {
                                tx_data.total_input_capacity += info.capacity;
                                if info.type_code_hash.as_deref() == Some(dao_code_hash.as_slice())
                                {
                                    has_dao_input = true;
                                }
                            } else if let Some(info) = batch_cell_infos.get(&key) {
                                tx_data.total_input_capacity += info.capacity;
                                if info.type_code_hash.as_deref() == Some(dao_code_hash.as_slice())
                                {
                                    has_dao_input = true;
                                }
                            }
                        }
                        tx_data.fee = match checked_tx_fee(
                            tx_data.total_input_capacity,
                            tx_data.total_output_capacity,
                            has_dao_input,
                            &tx_data.hash,
                            tx_data.block_number,
                        ) {
                            Ok(fee) => fee,
                            Err(err) => {
                                error!(
                                    start_block,
                                    end_block,
                                    tx_hash = %hex::encode(tx_data.hash),
                                    block_number = tx_data.block_number,
                                    "Parser: invalid tx fee accounting: {}",
                                    err
                                );
                                record_worker_exit_reason(
                                    &parser_exit_reason_for_parser,
                                    format!(
                                        "invalid tx fee accounting: range {}-{}, block={}, tx=0x{}, error={}",
                                        start_block,
                                        end_block,
                                        tx_data.block_number,
                                        hex::encode(tx_data.hash),
                                        err
                                    ),
                                );
                                return;
                            }
                        };
                    }
                }

                // Pass 3: cell_cache update + address_balance_changes + script_usage_changes
                let mut address_balance_changes: HashMap<
                    Vec<u8>,
                    (i128, i32, i32, i64, i64, Vec<u8>, i128),
                > = HashMap::new();
                let mut script_usage_changes: ScriptUsageChanges = HashMap::new();
                let mut script_daily_changes: HashMap<(Vec<u8>, bool, u32), (i128, i128)> =
                    HashMap::new();
                let mut token_daily_changes: HashMap<(Vec<u8>, u32), (i128, i128)> = HashMap::new();
                let mut spore_type_index_changes: HashMap<Vec<u8>, SporeTypeIndex> = HashMap::new();
                let mut spore_daily_changes: HashMap<(Vec<u8>, u32), (i128, i128)> = HashMap::new();
                let mut cluster_daily_changes: HashMap<(Vec<u8>, u32), (i128, i128)> =
                    HashMap::new();
                let mut nft_type_index_changes: HashMap<Vec<u8>, NftTypeIndex> = HashMap::new();
                let mut nft_daily_changes: HashMap<(Vec<u8>, u32), (i128, i128)> = HashMap::new();
                let mut spore_type_index_cache: HashMap<Vec<u8>, Option<SporeTypeIndex>> =
                    HashMap::new();
                let mut nft_type_index_cache: HashMap<Vec<u8>, Option<NftTypeIndex>> =
                    HashMap::new();

                for tx_data in &all_tx_data {
                    let date_yyyymmdd = ckbadger_store::keys::timestamp_ms_to_date(
                        tx_data.timestamp.timestamp_millis(),
                    );
                    // cell_cache update
                    for (output_index, cell) in tx_data.cells.iter().enumerate() {
                        let output_index_i16 = match checked_usize_to_i16(
                            output_index,
                            "pipeline parser cache update output index",
                        ) {
                            Ok(v) => v,
                            Err(e) => {
                                error!(
                                    block_number = tx_data.block_number,
                                    tx_hash = %hex::encode(tx_data.hash),
                                    output_index,
                                    "Parser: {}",
                                    e
                                );
                                record_worker_exit_reason(
                                    &parser_exit_reason_for_parser,
                                    format!(
                                        "invalid output index while updating parser cache: block={}, tx=0x{}, output_index={}, error={}",
                                        tx_data.block_number,
                                        hex::encode(tx_data.hash),
                                        output_index,
                                        e
                                    ),
                                );
                                return;
                            }
                        };
                        let standard_hint = if let Some(type_hash) = cell.type_script_hash.as_ref()
                        {
                            if let Some(cached) = udt_standard_hint_cache.get(type_hash) {
                                cached.clone()
                            } else {
                                let looked_up = match writer_for_parser.store().get_token(type_hash)
                                {
                                    Ok(token) => token.map(|info| info.standard),
                                    Err(e) => {
                                        error!(
                                            block_number = tx_data.block_number,
                                            tx_hash = %hex::encode(tx_data.hash),
                                            output_index,
                                            type_script_hash = %hex::encode(type_hash),
                                            "Parser: token metadata lookup failed while updating UDT cache hint: {}",
                                            e
                                        );
                                        record_worker_exit_reason(
                                            &parser_exit_reason_for_parser,
                                            format!(
                                                "token metadata lookup failed while updating UDT cache hint: block={}, tx=0x{}, output_index={}, error={}",
                                                tx_data.block_number,
                                                hex::encode(tx_data.hash),
                                                output_index,
                                                e
                                            ),
                                        );
                                        return;
                                    }
                                };
                                udt_standard_hint_cache
                                    .insert(type_hash.clone(), looked_up.clone());
                                looked_up
                            }
                        } else {
                            None
                        };
                        let udt_amount = match parse_parsed_cell_udt_amount(
                            cell,
                            &tx_data.hash,
                            output_index_i16,
                            standard_hint.as_deref(),
                        ) {
                            Ok(v) => v,
                            Err(e) => {
                                error!(
                                    block_number = tx_data.block_number,
                                    tx_hash = %hex::encode(tx_data.hash),
                                    output_index,
                                    "Parser: {}",
                                    e
                                );
                                record_worker_exit_reason(
                                    &parser_exit_reason_for_parser,
                                    format!(
                                        "failed to parse UDT amount while updating parser cache: block={}, tx=0x{}, output_index={}, error={}",
                                        tx_data.block_number,
                                        hex::encode(tx_data.hash),
                                        output_index,
                                        e
                                    ),
                                );
                                return;
                            }
                        };
                        let cell_occupied = occupied_capacity_shannons_i64(
                            cell.lock_args.len(),
                            cell.type_args.as_ref().map(|args| args.len()),
                            cell.data_size,
                        );
                        cell_cache_for_parser.insert(
                            (tx_data.hash, output_index_i16),
                            CachedCellInfo {
                                capacity: cell.capacity,
                                created_at_block: tx_data.block_number,
                                lock_script_hash: cell.lock_script_hash.clone(),
                                lock_code_hash: cell.lock_code_hash.clone(),
                                lock_hash_type: cell.lock_hash_type,
                                lock_args: cell.lock_args.clone(),
                                type_script_hash: cell.type_script_hash.clone(),
                                type_code_hash: cell.type_code_hash.clone(),
                                type_args: cell.type_args.clone(),
                                data_size: cell.data_size,
                                occupied_capacity: cell_occupied,
                                udt_amount,
                            },
                        );
                    }

                    // script_usage_changes - outputs
                    for cell in &tx_data.cells {
                        let lock_key = (cell.lock_code_hash.clone(), false);
                        let cell_occupied = occupied_capacity_shannons_i64(
                            cell.lock_args.len(),
                            cell.type_args.as_ref().map(|args| args.len()),
                            cell.data_size,
                        );
                        let entry = script_usage_changes
                            .entry(lock_key)
                            .or_insert((0, 0, 0, 0, 0, 0));
                        entry.0 += 1;
                        entry.1 += 1;
                        entry.2 += i128::from(cell.capacity);
                        entry.3 += i128::from(cell.capacity);
                        entry.4 += i128::from(cell_occupied);
                        entry.5 += i128::from(cell_occupied);
                        let daily_entry = script_daily_changes
                            .entry((cell.lock_code_hash.clone(), false, date_yyyymmdd))
                            .or_insert((0, 0));
                        daily_entry.0 += i128::from(cell.capacity);
                        daily_entry.1 += i128::from(cell_occupied);
                        if let Some(ref type_code_hash) = cell.type_code_hash {
                            let type_key = (type_code_hash.clone(), true);
                            let entry = script_usage_changes
                                .entry(type_key)
                                .or_insert((0, 0, 0, 0, 0, 0));
                            entry.0 += 1;
                            entry.1 += 1;
                            entry.2 += i128::from(cell.capacity);
                            entry.3 += i128::from(cell.capacity);
                            entry.4 += i128::from(cell_occupied);
                            entry.5 += i128::from(cell_occupied);
                            let daily_entry = script_daily_changes
                                .entry((type_code_hash.clone(), true, date_yyyymmdd))
                                .or_insert((0, 0));
                            daily_entry.0 += i128::from(cell.capacity);
                            daily_entry.1 += i128::from(cell_occupied);
                        }
                        if let Some(ref type_script_hash) = cell.type_script_hash {
                            let daily_entry = token_daily_changes
                                .entry((type_script_hash.clone(), date_yyyymmdd))
                                .or_insert((0, 0));
                            daily_entry.0 += i128::from(cell.capacity);
                            daily_entry.1 += i128::from(cell_occupied);
                        }
                        if let (Some(type_script_hash), Some(type_code_hash), Some(type_args)) = (
                            cell.type_script_hash.as_ref(),
                            cell.type_code_hash.as_ref(),
                            cell.type_args.as_ref(),
                        ) {
                            if type_args.len() >= 32
                                && SporeParser::is_spore_nft_type_script(type_code_hash)
                            {
                                let spore_id = type_args[..32].to_vec();
                                let cluster_id =
                                    SporeParser::parse_spore_cluster_id_from_data(&cell.data);
                                let index = SporeTypeIndex {
                                    spore_id: spore_id.clone(),
                                    cluster_id: cluster_id.clone(),
                                };
                                spore_type_index_cache
                                    .insert(type_script_hash.clone(), Some(index.clone()));
                                spore_type_index_changes.insert(type_script_hash.clone(), index);

                                let spore_daily = spore_daily_changes
                                    .entry((spore_id, date_yyyymmdd))
                                    .or_insert((0, 0));
                                spore_daily.0 += i128::from(cell.capacity);
                                spore_daily.1 += i128::from(cell_occupied);

                                if let Some(cluster_id) = cluster_id {
                                    let cluster_daily = cluster_daily_changes
                                        .entry((cluster_id, date_yyyymmdd))
                                        .or_insert((0, 0));
                                    cluster_daily.0 += i128::from(cell.capacity);
                                    cluster_daily.1 += i128::from(cell_occupied);
                                }
                            }
                        }
                        if let (Some(type_script_hash), Some(type_code_hash), Some(type_args)) = (
                            cell.type_script_hash.as_ref(),
                            cell.type_code_hash.as_ref(),
                            cell.type_args.as_ref(),
                        ) {
                            let collection_id =
                                classify_nft_collection_id(type_code_hash, type_args);
                            if let Some(collection_id) = collection_id {
                                let index = NftTypeIndex {
                                    collection_id: collection_id.clone(),
                                };
                                nft_type_index_cache
                                    .insert(type_script_hash.clone(), Some(index.clone()));
                                nft_type_index_changes.insert(type_script_hash.clone(), index);

                                let nft_daily = nft_daily_changes
                                    .entry((collection_id, date_yyyymmdd))
                                    .or_insert((0, 0));
                                nft_daily.0 += i128::from(cell.capacity);
                                nft_daily.1 += i128::from(cell_occupied);
                            }
                        }
                    }

                    // Per-tx balance/consumption tracking
                    let mut tx_balance_changes: HashMap<Vec<u8>, i128> = HashMap::new();
                    let mut tx_cells_consumed: HashMap<Vec<u8>, i32> = HashMap::new();
                    let mut tx_occupied_changes: HashMap<Vec<u8>, i128> = HashMap::new();

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
                                *tx_balance_changes
                                    .entry(info.lock_script_hash.clone())
                                    .or_default() -= i128::from(info.capacity);
                                *tx_cells_consumed
                                    .entry(info.lock_script_hash.clone())
                                    .or_default() += 1;
                                *tx_occupied_changes
                                    .entry(info.lock_script_hash.clone())
                                    .or_default() -= i128::from(info.occupied_capacity);
                                // script usage - inputs
                                let lock_key = (info.lock_code_hash.clone(), false);
                                let entry = script_usage_changes
                                    .entry(lock_key)
                                    .or_insert((0, 0, 0, 0, 0, 0));
                                entry.1 -= 1;
                                entry.3 -= i128::from(info.capacity);
                                entry.5 -= i128::from(info.occupied_capacity);
                                let daily_entry = script_daily_changes
                                    .entry((info.lock_code_hash.clone(), false, date_yyyymmdd))
                                    .or_insert((0, 0));
                                daily_entry.0 -= i128::from(info.capacity);
                                daily_entry.1 -= i128::from(info.occupied_capacity);
                                if let Some(ref type_code_hash) = info.type_code_hash {
                                    let type_key = (type_code_hash.clone(), true);
                                    let entry = script_usage_changes
                                        .entry(type_key)
                                        .or_insert((0, 0, 0, 0, 0, 0));
                                    entry.1 -= 1;
                                    entry.3 -= i128::from(info.capacity);
                                    entry.5 -= i128::from(info.occupied_capacity);
                                    let daily_entry = script_daily_changes
                                        .entry((type_code_hash.clone(), true, date_yyyymmdd))
                                        .or_insert((0, 0));
                                    daily_entry.0 -= i128::from(info.capacity);
                                    daily_entry.1 -= i128::from(info.occupied_capacity);
                                }
                                if let Some(ref type_script_hash) = info.type_script_hash {
                                    let daily_entry = token_daily_changes
                                        .entry((type_script_hash.clone(), date_yyyymmdd))
                                        .or_insert((0, 0));
                                    daily_entry.0 -= i128::from(info.capacity);
                                    daily_entry.1 -= i128::from(info.occupied_capacity);
                                }
                                if let (Some(type_script_hash), Some(type_code_hash)) =
                                    (info.type_script_hash.as_ref(), info.type_code_hash.as_ref())
                                {
                                    if SporeParser::is_spore_nft_type_script(type_code_hash) {
                                        let spore_index = match load_optional_index_from_store(
                                            &mut spore_type_index_cache,
                                            type_script_hash,
                                            "spore_type",
                                            || {
                                                writer_for_parser
                                                    .store()
                                                    .get_spore_type_index(type_script_hash)
                                            },
                                        ) {
                                            Ok(index) => index,
                                            Err(e) => {
                                                error!(
                                                    start_block,
                                                    end_block,
                                                    "Parser: failed to load spore type index: {}",
                                                    e
                                                );
                                                record_worker_exit_reason(
                                                    &parser_exit_reason_for_parser,
                                                    format!(
                                                        "failed to load spore type index for range {}-{}: {}",
                                                        start_block, end_block, e
                                                    ),
                                                );
                                                return;
                                            }
                                        };
                                        if let Some(index) = spore_index {
                                            let spore_daily = spore_daily_changes
                                                .entry((index.spore_id.clone(), date_yyyymmdd))
                                                .or_insert((0, 0));
                                            spore_daily.0 -= i128::from(info.capacity);
                                            spore_daily.1 -= i128::from(info.occupied_capacity);

                                            if let Some(cluster_id) = index.cluster_id {
                                                let cluster_daily = cluster_daily_changes
                                                    .entry((cluster_id, date_yyyymmdd))
                                                    .or_insert((0, 0));
                                                cluster_daily.0 -= i128::from(info.capacity);
                                                cluster_daily.1 -=
                                                    i128::from(info.occupied_capacity);
                                            }
                                        }
                                    }
                                    if DotbitParser::is_account_cell_type_script(type_code_hash)
                                        || MnftParser::is_token_type_script(type_code_hash)
                                        || SporeParser::is_did_type_script(type_code_hash)
                                    {
                                        let collection_id =
                                            if DotbitParser::is_account_cell_type_script(
                                                type_code_hash,
                                            ) {
                                                Some(DOTBIT_SENTINEL_COLLECTION.to_vec())
                                            } else if SporeParser::is_did_type_script(
                                                type_code_hash,
                                            ) {
                                                Some(DID_CKB_SENTINEL_COLLECTION.to_vec())
                                            } else if let Some(cached) =
                                                nft_type_index_cache.get(type_script_hash)
                                            {
                                                cached.clone().map(|idx| idx.collection_id)
                                            } else {
                                                match load_optional_index_from_store(
                                                    &mut nft_type_index_cache,
                                                    type_script_hash,
                                                    "nft_type",
                                                    || {
                                                        writer_for_parser
                                                            .store()
                                                            .get_nft_type_index(type_script_hash)
                                                    },
                                                ) {
                                                    Ok(loaded) => {
                                                        loaded.map(|idx| idx.collection_id)
                                                    }
                                                    Err(e) => {
                                                        error!(
                                                            start_block,
                                                            end_block,
                                                            "Parser: failed to load nft type index: {}",
                                                            e
                                                        );
                                                        record_worker_exit_reason(
                                                            &parser_exit_reason_for_parser,
                                                            format!(
                                                                "failed to load nft type index for range {}-{}: {}",
                                                                start_block, end_block, e
                                                            ),
                                                        );
                                                        return;
                                                    }
                                                }
                                            };
                                        if let Some(collection_id) = collection_id {
                                            let nft_daily = nft_daily_changes
                                                .entry((collection_id, date_yyyymmdd))
                                                .or_insert((0, 0));
                                            nft_daily.0 -= i128::from(info.capacity);
                                            nft_daily.1 -= i128::from(info.occupied_capacity);
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // address_balance_changes - outputs + merge
                    let mut tx_cells_created: HashMap<Vec<u8>, i32> = HashMap::new();
                    for cell in &tx_data.cells {
                        *tx_balance_changes
                            .entry(cell.lock_script_hash.clone())
                            .or_default() += i128::from(cell.capacity);
                        *tx_cells_created
                            .entry(cell.lock_script_hash.clone())
                            .or_default() += 1;
                        let cell_occupied = occupied_capacity_shannons_i128(
                            cell.lock_args.len(),
                            cell.type_args.as_ref().map(|args| args.len()),
                            cell.data_size,
                        );
                        *tx_occupied_changes
                            .entry(cell.lock_script_hash.clone())
                            .or_default() += cell_occupied;
                    }
                    let all_addresses: HashSet<Vec<u8>> = tx_balance_changes
                        .keys()
                        .chain(tx_cells_created.keys())
                        .chain(tx_cells_consumed.keys())
                        .chain(tx_occupied_changes.keys())
                        .cloned()
                        .collect();
                    for lock_hash in all_addresses {
                        let balance_change =
                            tx_balance_changes.get(&lock_hash).copied().unwrap_or(0);
                        let cells_created = tx_cells_created.get(&lock_hash).copied().unwrap_or(0);
                        let cells_consumed =
                            tx_cells_consumed.get(&lock_hash).copied().unwrap_or(0);
                        let occupied_change =
                            tx_occupied_changes.get(&lock_hash).copied().unwrap_or(0);
                        let entry = address_balance_changes.entry(lock_hash.clone()).or_insert((
                            0,
                            0,
                            0,
                            0,
                            tx_data.block_number,
                            tx_data.hash.to_vec(),
                            0,
                        ));
                        entry.0 += balance_change;
                        entry.1 += cells_created - cells_consumed;
                        entry.2 += cells_created;
                        entry.3 += 1;
                        entry.4 = tx_data.block_number;
                        entry.5 = tx_data.hash.to_vec();
                        entry.6 += occupied_change;
                    }
                }
                // NOTE: Do NOT clear cell_cache here. In pipeline mode, parser
                // runs ahead of writer. We only evict entries guaranteed to be
                // committed in DB according to writer-reported committed tip.
                let committed_tip = committed_tip_for_cache_for_parser.load(Ordering::SeqCst);
                let mut cache_evicted = 0usize;
                if should_trim_cell_cache(cell_cache_for_parser.len()) {
                    cache_evicted = evict_committed_cell_cache_entries(
                        cell_cache_for_parser.as_ref(),
                        committed_tip,
                    );
                }

                // Spore precompute: parse all spores/clusters and compute media
                // profiles in the parser stage to offload T6 writer thread.
                let pre_parsed_spore_data: PreParsedSporeData = {
                    // Pass 1: Parse all clusters and spores per-tx.
                    let mut per_tx: Vec<(Vec<ParsedSporeCell>, Vec<ParsedClusterCell>)> =
                        Vec::with_capacity(all_tx_data.len());
                    let mut batch_cluster_descriptions: HashMap<Vec<u8>, Option<String>> =
                        HashMap::new();
                    let mut missing_cluster_ids: Vec<Vec<u8>> = Vec::new();
                    for block_response in blocks.iter() {
                        for tx in &block_response.block.transactions {
                            let clusters = SporeParser::parse_clusters(tx);
                            let spores = SporeParser::parse_spores(tx);

                            // Record cluster descriptions from this batch.
                            for cluster in &clusters {
                                batch_cluster_descriptions.insert(
                                    cluster.cluster_id.clone(),
                                    cluster.description.clone(),
                                );
                            }

                            // Collect cluster IDs referenced by spores that aren't
                            // in this batch yet — we'll fetch them from DB.
                            for spore in &spores {
                                if let Some(ref cid) = spore.cluster_id {
                                    if !batch_cluster_descriptions.contains_key(cid) {
                                        missing_cluster_ids.push(cid.clone());
                                    }
                                }
                            }
                            per_tx.push((spores, clusters));
                        }
                    }

                    // Batch-fetch missing cluster descriptions from DB.
                    if !missing_cluster_ids.is_empty() {
                        missing_cluster_ids.sort();
                        missing_cluster_ids.dedup();
                        let stored_clusters = match writer_for_parser
                            .store()
                            .get_spores_batch(&missing_cluster_ids)
                        {
                            Ok(rows) => rows,
                            Err(e) => {
                                error!(
                                    start_block,
                                    end_block,
                                    "Parser: get_spores_batch failed while preloading cluster descriptions: {}",
                                    e
                                );
                                record_worker_exit_reason(
                                    &parser_exit_reason_for_parser,
                                    format!(
                                        "get_spores_batch failed while preloading cluster descriptions for range {}-{}: {}",
                                        start_block, end_block, e
                                    ),
                                );
                                return;
                            }
                        };
                        for (id, entry) in stored_clusters {
                            let desc = entry.and_then(|e| {
                                if e.standard == ckbadger_store::types::DobStandard::SporeCluster {
                                    e.description
                                } else {
                                    None
                                }
                            });
                            batch_cluster_descriptions.insert(id, desc);
                        }
                    }

                    // Pass 2: Compute media profiles with cluster descriptions.
                    for (spores, _clusters) in &mut per_tx {
                        for spore in spores.iter_mut() {
                            if spore.is_did {
                                continue;
                            }
                            let cluster_desc = spore
                                .cluster_id
                                .as_ref()
                                .and_then(|cid| batch_cluster_descriptions.get(cid))
                                .and_then(|d| d.as_deref());
                            spore.media_profile = Some(analyze_spore_media_profile(
                                &spore.content_type,
                                &spore.content,
                                cluster_desc,
                            ));
                        }
                    }

                    per_tx
                };

                // mNFT/DotBit precompute: parse all mNFT issuers/classes/tokens and DotBit
                // accounts in the parser stage to offload t6b writer thread.
                // Also pre-identify consumed DotBit inputs using input_cell_info
                // type_code_hash (zero DB reads).
                let pre_parsed_nft_data: PreParsedNftData = {
                    let mut mnft_issuers: Vec<(usize, ParsedMnftIssuer)> = Vec::new();
                    let mut mnft_classes: Vec<(usize, usize, ParsedMnftClass)> = Vec::new();
                    let mut mnft_tokens: Vec<(usize, usize, ParsedMnftToken)> = Vec::new();
                    let mut dotbit_accounts: Vec<(usize, ParsedDotbitAccountOutput)> = Vec::new();
                    let mut dotbit_tx_actions: HashMap<usize, String> = HashMap::new();
                    let mut batch_dotbit_outpoints: HashMap<(Vec<u8>, i16), Vec<u8>> =
                        HashMap::new();
                    let mut batch_dotbit_latest_create_order: HashMap<Vec<u8>, u64> =
                        HashMap::new();

                    // Phase 1: Parse mNFT/DotBit outputs
                    let mut block_tx_idx = 0usize;
                    for (block_idx, block_response) in blocks.iter().enumerate() {
                        let parsed = &all_parsed_blocks[block_idx];
                        let tx_count_for_block = parsed.transactions_count as usize;
                        let tx_slice =
                            &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                        for (tx_idx, tx_data) in tx_slice.iter().enumerate() {
                            let tx_global_index = block_tx_idx + tx_idx;
                            let tx = &block_response.block.transactions[tx_idx];
                            for issuer in MnftParser::parse_issuers(tx) {
                                mnft_issuers.push((tx_global_index, issuer));
                            }
                            for (output_index, class) in
                                MnftParser::parse_classes_with_output_indices(tx)
                            {
                                mnft_classes.push((tx_global_index, output_index, class));
                            }
                            for (output_index, token) in
                                MnftParser::parse_tokens_with_output_indices(tx)
                            {
                                mnft_tokens.push((tx_global_index, output_index, token));
                            }
                            let parsed_accounts = match DotbitParser::parse_accounts(tx) {
                                Ok(accounts) => accounts,
                                Err(e) => {
                                    error!(
                                        "Parser: DotBit parse_accounts failed at block {}, tx 0x{}: {}",
                                        parsed.number, hex::encode(tx_data.hash), e
                                    );
                                    record_worker_exit_reason(
                                        &parser_exit_reason_for_parser,
                                        format!(
                                            "DotBit parse_accounts failed: block={}, tx=0x{}, error={}",
                                            parsed.number,
                                            hex::encode(tx_data.hash),
                                            e
                                        ),
                                    );
                                    return;
                                }
                            };
                            let dotbit_create_order =
                                match dotbit_create_event_order(tx_global_index) {
                                    Ok(order) => order,
                                    Err(e) => {
                                        error!("Parser: dotbit_create_event_order overflow: {}", e);
                                        record_worker_exit_reason(
                                            &parser_exit_reason_for_parser,
                                            format!("dotbit_create_event_order overflow: {}", e),
                                        );
                                        return;
                                    }
                                };
                            if !parsed_accounts.is_empty() {
                                if let Some(action) = DotbitParser::parse_das_action(&tx.witnesses)
                                {
                                    dotbit_tx_actions.insert(tx_global_index, action);
                                }
                            }
                            for account in parsed_accounts {
                                batch_dotbit_outpoints.insert(
                                    (tx_data.hash.to_vec(), account.output_index),
                                    account.account.account_id.clone(),
                                );
                                batch_dotbit_latest_create_order
                                    .entry(account.account.account_id.clone())
                                    .and_modify(|current| {
                                        if dotbit_create_order > *current {
                                            *current = dotbit_create_order;
                                        }
                                    })
                                    .or_insert(dotbit_create_order);
                                dotbit_accounts.push((tx_global_index, account));
                            }
                        }
                        block_tx_idx += tx_count_for_block;
                    }

                    // Phase 2: Pre-identify consumed DotBit inputs using
                    // input_cell_info type_code_hash (zero DB reads).
                    let mut consumed_dotbit: Vec<DotbitConsumptionEvent> = Vec::new();
                    let mut block_tx_idx = 0usize;
                    for parsed in all_parsed_blocks.iter().take(blocks.len()) {
                        let tx_count_for_block = parsed.transactions_count as usize;
                        let tx_slice =
                            &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                        for (tx_idx, tx_data) in tx_slice.iter().enumerate() {
                            if tx_data.is_cellbase || tx_data.inputs.is_empty() {
                                continue;
                            }
                            let tx_global_index = block_tx_idx + tx_idx;
                            let dotbit_consume_order =
                                match dotbit_consume_event_order(tx_global_index) {
                                    Ok(order) => order,
                                    Err(e) => {
                                        error!(
                                            "Parser: dotbit_consume_event_order overflow: {}",
                                            e
                                        );
                                        record_worker_exit_reason(
                                            &parser_exit_reason_for_parser,
                                            format!("dotbit_consume_event_order overflow: {}", e),
                                        );
                                        return;
                                    }
                                };
                            for input in &tx_data.inputs {
                                let key = (
                                    input.previous_tx_hash.to_vec(),
                                    parsed_input_outpoint_index_i16(
                                        input.previous_output_index,
                                        "sync_indexer",
                                    ),
                                );

                                // 1. Check same-batch first
                                if let Some(account_id) = batch_dotbit_outpoints.get(&key) {
                                    let latest_create_order =
                                        batch_dotbit_latest_create_order.get(account_id).copied();
                                    if should_consume_dotbit_account(
                                        latest_create_order,
                                        dotbit_consume_order,
                                    ) {
                                        consumed_dotbit.push(DotbitConsumptionEvent {
                                            account_id: account_id.clone(),
                                            block_number: parsed.number,
                                            consuming_tx_hash: tx_data.hash,
                                            tx_idx: checked_usize_to_i32(tx_idx, "tx_idx"),
                                            ts_ms: parsed.timestamp.timestamp_millis(),
                                        });
                                    }
                                    continue;
                                }

                                // 2. Check input_cell_info / batch_cell_infos type_code_hash
                                let cell_info = input_cell_info
                                    .get(&key)
                                    .or_else(|| batch_cell_infos.get(&key));
                                let is_dotbit = cell_info
                                    .and_then(|info| info.type_code_hash.as_ref())
                                    .map(|tc| DotbitParser::is_account_cell_type_script(tc))
                                    .unwrap_or(false);
                                if !is_dotbit {
                                    continue;
                                }

                                // 3. Extract account_id from type_args (20 bytes, non-zero)
                                let account_id = cell_info
                                    .and_then(|info| info.type_args.as_ref())
                                    .filter(|args| {
                                        args.len() == 20 && !args.iter().all(|&b| b == 0)
                                    })
                                    .cloned();
                                if let Some(account_id) = account_id {
                                    let latest_create_order =
                                        batch_dotbit_latest_create_order.get(&account_id).copied();
                                    if should_consume_dotbit_account(
                                        latest_create_order,
                                        dotbit_consume_order,
                                    ) {
                                        consumed_dotbit.push(DotbitConsumptionEvent {
                                            account_id,
                                            block_number: parsed.number,
                                            consuming_tx_hash: tx_data.hash,
                                            tx_idx: checked_usize_to_i32(tx_idx, "tx_idx"),
                                            ts_ms: parsed.timestamp.timestamp_millis(),
                                        });
                                    }
                                }
                            }
                        }
                        block_tx_idx += tx_count_for_block;
                    }

                    PreParsedNftData {
                        mnft_issuers,
                        mnft_classes,
                        mnft_tokens,
                        dotbit_accounts,
                        consumed_dotbit,
                        dotbit_tx_actions,
                    }
                };

                let precompute_parser_ms = t_precompute_parser.elapsed().as_secs_f64() * 1000.0;
                let total_parser_ms = t_parser.elapsed().as_secs_f64() * 1000.0;
                let tx_count: usize = all_tx_data.len();
                let cell_count: usize = all_tx_data.iter().map(|t| t.cells.len()).sum();
                let input_count: usize = all_tx_data
                    .iter()
                    .filter(|t| !t.is_cellbase)
                    .map(|t| t.inputs.len())
                    .sum();
                let queue_depth = parse_tx.max_capacity() - parse_tx.capacity();
                pipeline_perf_for_parser.record_parse(
                    t_parser.elapsed(),
                    queue_depth,
                    parse_tx.max_capacity(),
                );
                let cache_total = cache_hits + cache_misses;
                let hit_rate = if cache_total > 0 {
                    cache_hits as f64 / cache_total as f64 * 100.0
                } else {
                    0.0
                };
                info!(
                    parse_ms = format!("{:.1}", t_parse_ms),
                    cell_lookup_ms = format!("{:.1}", cell_lookup_ms),
                    precompute_ms = format!("{:.1}", precompute_parser_ms),
                    total_ms = format!("{:.1}", total_parser_ms),
                    txs = tx_count,
                    cells = cell_count,
                    inputs = input_count,
                    cache_hits,
                    cache_misses,
                    cache_hit_pct = format!("{:.0}", hit_rate),
                    committed_tip,
                    cache_evicted,
                    cache_size = cell_cache_for_parser.len(),
                    queue_depth,
                    "Parser batch {}-{}",
                    start_block,
                    end_block,
                );

                if batch_epoch != pipeline_epoch_for_parser.load(Ordering::SeqCst) {
                    debug!(
                        batch_epoch,
                        "Dropping parsed stale batch {}-{} before writer handoff",
                        start_block,
                        end_block
                    );
                    continue;
                }

                let batch_tx_count_u64 =
                    u64::try_from(tx_count).expect("parsed batch tx count exceeds u64");
                if parse_tx
                    .send((
                        batch_epoch,
                        start_block,
                        end_block,
                        chain_tip,
                        batch_tx_count_u64,
                        blocks,
                        all_parsed_blocks,
                        all_tx_data,
                        input_cell_info,
                        batch_cell_infos,
                        address_balance_changes,
                        script_usage_changes,
                        script_daily_changes,
                        token_daily_changes,
                        spore_type_index_changes,
                        spore_daily_changes,
                        cluster_daily_changes,
                        nft_type_index_changes,
                        nft_daily_changes,
                        pre_parsed_spore_data,
                        pre_parsed_nft_data,
                    ))
                    .await
                    .is_err()
                {
                    record_worker_exit_reason(
                        &parser_exit_reason_for_parser,
                        format!(
                            "failed to send parsed batch to writer: range={}-{}, chain_tip={}, pipeline_epoch={}",
                            start_block, end_block, chain_tip, batch_epoch
                        ),
                    );
                    break;
                }
                parse_tx_pending_txs_for_parser.fetch_add(batch_tx_count_u64, Ordering::Relaxed);
            }
        });

        // === Writer loop ===
        let committed_tip_for_cache_for_writer = Arc::clone(&committed_tip_for_cache);
        let mut consecutive_idle_timeouts: u64 = 0;
        loop {
            // Bulk sync is an optimistic rebuild path and must not run reorg/deep-fork handling.
            let should_handle_reorg =
                self.should_handle_reorg_for_lag(self.progress.blocks_remaining());
            if should_handle_reorg && self.repo.has_unresolved_deep_fork()? {
                let drained = Self::drain_channel(&mut parse_rx).await;
                parse_tx_pending_txs_for_writer.store(0, Ordering::Relaxed);
                if let Some(repeat) = self.repeated_warning_snapshot(
                    "pipeline_deep_fork_unresolved",
                    Duration::from_secs(120),
                ) {
                    warn!(
                        run_id = %self.run_id,
                        pipeline_epoch = self.pipeline_reset_epoch.load(Ordering::SeqCst),
                        drained,
                        repeat_count = repeat.total_count,
                        suppressed_since_last = repeat.suppressed_since_last_emit,
                        first_seen_secs_ago = repeat.first_seen_secs_ago,
                        "Deep fork unresolved, sync paused. Waiting for manual intervention..."
                    );
                }
                sleep(Duration::from_secs(30)).await;
                continue;
            }

            let recv_timeout = Duration::from_millis(self.config.poll_interval_ms * 2);
            let t_recv = Instant::now();
            match tokio::time::timeout(recv_timeout, parse_rx.recv()).await {
                Ok(Some((
                    batch_epoch,
                    start_block,
                    end_block,
                    chain_tip,
                    parsed_batch_tx_count_u64,
                    blocks,
                    all_parsed_blocks,
                    all_tx_data,
                    input_cell_info,
                    batch_cell_infos,
                    address_balance_changes,
                    script_usage_changes,
                    script_daily_changes,
                    token_daily_changes,
                    spore_type_index_changes,
                    spore_daily_changes,
                    cluster_daily_changes,
                    nft_type_index_changes,
                    nft_daily_changes,
                    pre_parsed_spore_data,
                    pre_parsed_nft_data,
                ))) => {
                    consecutive_idle_timeouts = 0;
                    atomic_saturating_sub_u64(
                        &parse_tx_pending_txs_for_writer,
                        parsed_batch_tx_count_u64,
                    );
                    let recv_wait_ms = t_recv.elapsed().as_secs_f64() * 1000.0;
                    let current_epoch = self.pipeline_reset_epoch.load(Ordering::SeqCst);
                    if batch_epoch != current_epoch {
                        debug!(
                            batch_epoch,
                            current_epoch,
                            "Dropping stale parsed batch {}-{}",
                            start_block,
                            end_block
                        );
                        continue;
                    }
                    let (db_tip, db_tip_hash) = self.repo.get_sync_tip().await?;
                    committed_tip_for_cache_for_writer.store(db_tip, Ordering::SeqCst);
                    let expected_start = next_start_block_from_db_tip(
                        db_tip,
                        &db_tip_hash,
                        "pipeline writer expected_start",
                    )?;

                    if start_block != expected_start {
                        let writer_queue_depth = parse_tx_for_writer_depth.max_capacity()
                            - parse_tx_for_writer_depth.capacity();
                        let pipeline = self.pipeline_perf.snapshot();
                        let blocks_behind = blocks_behind_tip(
                            chain_tip,
                            db_tip,
                            "pipeline mismatch blocks_behind",
                        )?;
                        if let Some(repeat) = self.repeated_warning_snapshot(
                            "pipeline_batch_mismatch",
                            Duration::from_secs(5),
                        ) {
                            warn!(
                                run_id = %self.run_id,
                                pipeline_epoch = current_epoch,
                                db_tip,
                                chain_tip,
                                blocks_behind,
                                expected_start,
                                got_start = start_block,
                                writer_queue_depth,
                                repeat_count = repeat.total_count,
                                suppressed_since_last = repeat.suppressed_since_last_emit,
                                first_seen_secs_ago = repeat.first_seen_secs_ago,
                                parse_queue_depth = ?pipeline.as_ref().and_then(|p| p.parse_queue_depth),
                                parse_queue_capacity = ?pipeline.as_ref().and_then(|p| p.parse_queue_capacity),
                                writer_wait_ms = ?pipeline.as_ref().and_then(|p| p.writer_wait_ms),
                                "Pipeline batch mismatch: reset and drain stale parsed batches"
                            );
                        }
                        self.request_pipeline_reset(
                            "pipeline batch mismatch",
                            Some(expected_start),
                            Some(start_block),
                            Some(writer_queue_depth),
                        );
                        let drained = Self::drain_channel(&mut parse_rx).await;
                        parse_tx_pending_txs_for_writer.store(0, Ordering::Relaxed);
                        info!(
                            run_id = %self.run_id,
                            pipeline_epoch = current_epoch,
                            drained,
                            expected_start,
                            got_start = start_block,
                            "Pipeline mismatch drain completed"
                        );
                        continue;
                    }

                    let blocks_behind =
                        blocks_behind_tip(chain_tip, db_tip, "pipeline writer reorg check")?;
                    if self.should_handle_reorg_for_lag(blocks_behind) {
                        if let Some(ref stored_hash) = db_tip_hash {
                            if db_tip > 0 {
                                let db_tip_u64 = require_non_negative_block_number(
                                    db_tip,
                                    "pipeline writer reorg tip",
                                )?;
                                match self.check_and_handle_reorg(db_tip_u64, stored_hash).await? {
                                    Some(ReorgAction::Handled(_)) => {
                                        self.cell_cache.clear();
                                        self.udt_cell_cache.clear();
                                        let (reorg_tip, _) = self.repo.get_sync_tip().await?;
                                        self.reconcile_hodl_tracker_with_tip(reorg_tip)?;
                                        let current_epoch =
                                            self.pipeline_reset_epoch.load(Ordering::SeqCst);
                                        info!(
                                            run_id = %self.run_id,
                                            pipeline_epoch = current_epoch,
                                            db_tip,
                                            chain_tip,
                                            reorg_tip,
                                            "Reorg handled, caches cleared, HODL tracker reconciled, draining stale parsed batches"
                                        );
                                        self.request_pipeline_reset(
                                            "reorg handled",
                                            None,
                                            None,
                                            None,
                                        );
                                        let drained = Self::drain_channel(&mut parse_rx).await;
                                        parse_tx_pending_txs_for_writer.store(0, Ordering::Relaxed);
                                        info!(
                                            run_id = %self.run_id,
                                            pipeline_epoch = current_epoch,
                                            drained,
                                            "Reorg drain completed"
                                        );
                                        continue;
                                    }
                                    Some(ReorgAction::DeepForkPaused) => {
                                        let current_epoch =
                                            self.pipeline_reset_epoch.load(Ordering::SeqCst);
                                        warn!(
                                            run_id = %self.run_id,
                                            pipeline_epoch = current_epoch,
                                            db_tip,
                                            chain_tip,
                                            "Deep fork detected, sync paused"
                                        );
                                        self.request_pipeline_reset(
                                            "deep fork paused",
                                            None,
                                            None,
                                            None,
                                        );
                                        let drained = Self::drain_channel(&mut parse_rx).await;
                                        parse_tx_pending_txs_for_writer.store(0, Ordering::Relaxed);
                                        info!(
                                            run_id = %self.run_id,
                                            pipeline_epoch = current_epoch,
                                            drained,
                                            "Deep fork pause drain completed"
                                        );
                                        sleep(Duration::from_secs(30)).await;
                                        continue;
                                    }
                                    None => {}
                                }
                            }
                        }
                    }

                    let db_start = Instant::now();
                    let batch_tx_count = all_tx_data.len();
                    let batch_tx_count_u64 =
                        u64::try_from(batch_tx_count).expect("batch tx count exceeds u64");
                    let write_metrics = match self
                        .write_parsed_batch(
                            &blocks,
                            &all_parsed_blocks,
                            all_tx_data,
                            input_cell_info,
                            batch_cell_infos,
                            address_balance_changes,
                            script_usage_changes,
                            script_daily_changes,
                            token_daily_changes,
                            spore_type_index_changes,
                            spore_daily_changes,
                            cluster_daily_changes,
                            nft_type_index_changes,
                            nft_daily_changes,
                            pre_parsed_spore_data,
                            pre_parsed_nft_data,
                            chain_tip,
                        )
                        .await
                    {
                        Ok(metrics) => metrics,
                        Err(e) => {
                            let incident_id = self.report_incident(
                                "pipeline_batch_write_failed",
                                format!(
                                    "start_block={} end_block={} chain_tip={} error={:?}",
                                    start_block, end_block, chain_tip, e
                                ),
                            );
                            error!(
                                run_id = %self.run_id,
                                incident_id = %incident_id,
                                start_block,
                                end_block,
                                chain_tip,
                                error = ?e,
                                "Sync error while writing parsed batch"
                            );
                            let bulk_sync_mode = is_bulk_sync_batch(
                                chain_tip,
                                end_block,
                                self.config.bulk_sync_threshold,
                            );
                            if bulk_sync_mode {
                                return Err(e).with_context(|| {
                                format!(
                                    "bulk sync fail-fast for range {}-{} (chain_tip={}): \
                                     no rollback cleanup/retry in bulk mode; delete RocksDB and restart from genesis",
                                    start_block, end_block, chain_tip
                                )
                            });
                            }
                            if let Err(cleanup_err) = self.writer.cleanup_batch_range(
                                self.append_only_store.as_ref(),
                                i64::try_from(start_block).map_err(|_| {
                                    anyhow!(
                                        "batch cleanup start_block exceeds i64: {}",
                                        start_block
                                    )
                                })?,
                                i64::try_from(end_block).map_err(|_| {
                                    anyhow!("batch cleanup end_block exceeds i64: {}", end_block)
                                })?,
                            ) {
                                error!("Failed to cleanup partial batch: {:?}", cleanup_err);
                            } else {
                                let cleanup_tip = i64::try_from(start_block).map_err(|_| {
                                    anyhow!(
                                        "batch cleanup start_block exceeds i64 for hodl consistency check: {}",
                                        start_block
                                    )
                                })? - 1;
                                rollback_undo_log_after_batch_cleanup(
                                    self.writer.store().as_ref(),
                                    self.append_only_store.as_ref(),
                                    cleanup_tip,
                                    &format!(
                                        "pipeline range {}-{} (chain_tip={})",
                                        start_block, end_block, chain_tip
                                    ),
                                )?;
                                if let Err(consistency_err) =
                                    self.reconcile_hodl_tracker_with_tip(cleanup_tip)
                                {
                                    error!(
                                        cleanup_tip,
                                        "HODL tracker consistency check failed after batch cleanup: {:?}",
                                        consistency_err
                                    );
                                    return Err(consistency_err).with_context(|| {
                                        format!(
                                            "HODL tracker inconsistent after batch cleanup to tip {}. automatic rebuild is disabled; delete RocksDB and re-sync from genesis",
                                            cleanup_tip
                                        )
                                    });
                                }
                            }
                            self.request_pipeline_reset("batch write failed", None, None, None);
                            let drained = Self::drain_channel(&mut parse_rx).await;
                            parse_tx_pending_txs_for_writer.store(0, Ordering::Relaxed);
                            info!(
                                run_id = %self.run_id,
                                pipeline_epoch = self.pipeline_reset_epoch.load(Ordering::SeqCst),
                                drained,
                                "Batch write failure drain completed"
                            );
                            sleep(Duration::from_secs(5)).await;
                            continue;
                        }
                    };
                    let db_elapsed = db_start.elapsed();
                    self.perf.add_db_write(db_elapsed);
                    self.perf
                        .add_db_commit(duration_from_millis(write_metrics.commit_ms));

                    if db_elapsed.as_secs() >= 5 {
                        let stats = self.writer.store().memory_stats();
                        warn!(
                            db_stage_ms = format!("{:.1}", db_elapsed.as_secs_f64() * 1000.0),
                            commit_ms = format!("{:.1}", write_metrics.commit_ms),
                            compaction_pending_mb = stats.compaction_pending_bytes / (1024 * 1024),
                            running_compactions = stats.num_running_compactions,
                            l0_total = stats.l0_files_count,
                            l0_max = stats.l0_files_max,
                            l0_worst_cf = stats.l0_worst_cf,
                            memtable_mb = stats.memtable_bytes / (1024 * 1024),
                            imm_memtables = stats.immutable_memtables,
                            "Slow write stage detected"
                        );
                    }

                    if let Some(last_block) = all_parsed_blocks.last() {
                        committed_tip_for_cache_for_writer
                            .store(last_block.number, Ordering::SeqCst);
                        self.progress.record_batch(
                            last_block.number as u64,
                            all_parsed_blocks.len() as u64,
                            batch_tx_count_u64,
                        );

                        let mode = if self.is_bulk_sync_active() {
                            "[BULK]"
                        } else {
                            ""
                        };
                        let partition_range = format_partition_range(start_block, end_block);
                        let boundary_info = if crosses_partition_boundary(start_block, end_block) {
                            " (crosses boundary)"
                        } else {
                            ""
                        };
                        let writer_queue = parse_tx_for_writer_depth.max_capacity()
                            - parse_tx_for_writer_depth.capacity();
                        self.pipeline_perf.record_write(
                            db_elapsed,
                            write_metrics.commit_ms,
                            recv_wait_ms,
                            writer_queue,
                            parse_tx_for_writer_depth.max_capacity(),
                        );
                        let adaptive_snapshot_before = self.adaptive_batch_controller.snapshot();
                        let parse_queue_pending_txs =
                            parse_tx_pending_txs_for_writer.load(Ordering::Relaxed);
                        let parse_queue_capacity_txs = parse_queue_capacity_txs(
                            parse_tx_for_writer_depth.max_capacity(),
                            adaptive_snapshot_before.target_batch_txs,
                            adaptive_snapshot_before.min_target_batch_txs,
                        );
                        // Model queue pressure by pending tx volume to avoid over-reacting to
                        // temporary bursts with dense transactions.
                        let parse_queue_fill_pct = queue_fill_percentage(
                            Some(parse_queue_pending_txs),
                            Some(parse_queue_capacity_txs),
                        );
                        let writer_queue_fill_pct = parse_queue_fill_pct;
                        let memory_ratio_pct =
                            cgroup_memory_ratio_pct(&read_cgroup_memory_snapshot());
                        let (l0_files_max, compaction_pending_bytes, immutable_memtables) =
                            self.writer.store().compaction_pressure();
                        let blocks_remaining = self.progress.blocks_remaining();
                        let db_stage_ms = db_elapsed.as_secs_f64() * 1000.0;
                        let write_us_per_tx = if batch_tx_count > 0 && db_stage_ms > 0.0 {
                            Some((db_stage_ms * 1000.0) / batch_tx_count as f64)
                        } else {
                            None
                        };
                        let mem_profile = self.writer.store().memory_profile();
                        if let Some(adjustment) =
                            self.adaptive_batch_controller
                                .update_after_write(AdaptiveBatchInput {
                                    write_ms: db_stage_ms,
                                    commit_ms: write_metrics.commit_ms,
                                    batch_tx_count,
                                    blocks_remaining,
                                    parse_queue_fill_pct,
                                    writer_queue_fill_pct,
                                    memory_ratio_pct,
                                    l0_files_max: Some(l0_files_max),
                                    compaction_pending_bytes: Some(compaction_pending_bytes),
                                    immutable_memtables: Some(immutable_memtables),
                                    severe_pending_threshold: mem_profile
                                        .severe_compaction_pending_bytes,
                                    moderate_pending_threshold: mem_profile
                                        .moderate_compaction_pending_bytes,
                                    severe_imm_threshold: mem_profile.severe_immutable_memtables,
                                    moderate_imm_threshold: mem_profile
                                        .moderate_immutable_memtables,
                                })
                        {
                            info!(
                                run_id = %self.run_id,
                                reason = adjustment.reason,
                                previous_target_batch_txs = adjustment.previous_target_batch_txs,
                                new_target_batch_txs = adjustment.new_target_batch_txs,
                                previous_inflight_limit = adjustment.previous_inflight_limit,
                                new_inflight_limit = adjustment.new_inflight_limit,
                                previous_min_target_batch_txs = adjustment.previous_min_target_batch_txs,
                                new_min_target_batch_txs = adjustment.new_min_target_batch_txs,
                                parse_queue_fill_pct = parse_queue_fill_pct.map(|v| format!("{:.1}", v)),
                                writer_queue_fill_pct = writer_queue_fill_pct.map(|v| format!("{:.1}", v)),
                                parse_queue_pending_txs,
                                parse_queue_capacity_txs,
                                memory_ratio_pct = memory_ratio_pct.map(|v| format!("{:.1}", v)),
                                write_us_per_tx = write_us_per_tx.map(|v| format!("{:.1}", v)),
                                adaptive_backoff_streak = self.adaptive_batch_controller.snapshot().backoff_streak,
                                blocks_remaining,
                                db_stage_ms = format!("{:.1}", db_stage_ms),
                                db_commit_ms = format!("{:.1}", write_metrics.commit_ms),
                                "Adaptive batch controller adjusted"
                            );
                        }
                        let adaptive_snapshot = self.adaptive_batch_controller.snapshot();
                        info!(
                            "Wrote blocks {} to {} ({} remaining, {:.2}s, commit={:.0}ms, q={}, wait={:.0}ms, adaptive_txs={}, adaptive_min_txs={}, inflight_limit={}) {}{} {}",
                            start_block,
                            end_block,
                            blocks_remaining,
                            db_elapsed.as_secs_f64(),
                            write_metrics.commit_ms,
                            writer_queue,
                            recv_wait_ms,
                            adaptive_snapshot.target_batch_txs,
                            adaptive_snapshot.min_target_batch_txs,
                            adaptive_snapshot.inflight_limit,
                            partition_range,
                            boundary_info,
                            mode
                        );

                        self.maybe_invalidate_chart_caches(end_block).await;
                        self.check_bulk_sync_completion().await;
                        self.ensure_compaction_mode(blocks_remaining);
                    }

                    self.perf
                        .blocks_count
                        .fetch_add(all_parsed_blocks.len() as u64, Ordering::Relaxed);
                    self.perf.report_and_reset();
                }
                Ok(None) => {
                    let parser_reason = get_worker_exit_reason(&parser_exit_reason);
                    let fetcher_reason = get_worker_exit_reason(&fetcher_exit_reason);
                    fetcher.abort();
                    parser.abort();
                    return Err(anyhow::anyhow!(
                        "Pipeline channel closed: {}",
                        format_pipeline_worker_termination_message(
                            parser.is_finished(),
                            fetcher.is_finished(),
                            parser_reason.as_deref(),
                            fetcher_reason.as_deref(),
                        )
                    ));
                }
                Err(_timeout) => {
                    // Idle timeout - no pending batches. If any worker exited, fail fast
                    // instead of spinning forever with stale progress.
                    consecutive_idle_timeouts = consecutive_idle_timeouts.saturating_add(1);
                    let parser_finished = parser.is_finished();
                    let fetcher_finished = fetcher.is_finished();
                    let writer_queue_depth = parse_tx_for_writer_depth.max_capacity()
                        - parse_tx_for_writer_depth.capacity();
                    let pipeline = self.pipeline_perf.snapshot();
                    let fetch_fill_pct = queue_fill_percentage(
                        pipeline.as_ref().and_then(|p| p.fetch_queue_depth),
                        pipeline.as_ref().and_then(|p| p.fetch_queue_capacity),
                    );
                    let parse_fill_pct = queue_fill_percentage(
                        pipeline.as_ref().and_then(|p| p.parse_queue_depth),
                        pipeline.as_ref().and_then(|p| p.parse_queue_capacity),
                    );
                    if should_log_pipeline_idle_timeout(consecutive_idle_timeouts) {
                        if let Some(repeat) = self.repeated_warning_snapshot(
                            "pipeline_idle_timeout",
                            Duration::from_secs(10),
                        ) {
                            warn!(
                                run_id = %self.run_id,
                                pipeline_epoch = self.pipeline_reset_epoch.load(Ordering::SeqCst),
                                idle_timeouts = consecutive_idle_timeouts,
                                parser_finished,
                                fetcher_finished,
                                repeat_count = repeat.total_count,
                                suppressed_since_last = repeat.suppressed_since_last_emit,
                                first_seen_secs_ago = repeat.first_seen_secs_ago,
                                current_block = self.progress.current(),
                                target_block = self.progress.target(),
                                blocks_remaining = self.progress.blocks_remaining(),
                                writer_queue_depth,
                                fetch_queue_depth = ?pipeline.as_ref().and_then(|p| p.fetch_queue_depth),
                                fetch_queue_capacity = ?pipeline.as_ref().and_then(|p| p.fetch_queue_capacity),
                                fetch_queue_fill_pct = ?fetch_fill_pct.map(|v| format!("{:.1}", v)),
                                parse_queue_depth = ?pipeline.as_ref().and_then(|p| p.parse_queue_depth),
                                parse_queue_capacity = ?pipeline.as_ref().and_then(|p| p.parse_queue_capacity),
                                parse_queue_fill_pct = ?parse_fill_pct.map(|v| format!("{:.1}", v)),
                                writer_wait_ms = ?pipeline.as_ref().and_then(|p| p.writer_wait_ms),
                                "Pipeline idle timeout while waiting for parsed batches"
                            );
                        }
                    }
                    if should_abort_pipeline_on_idle_timeout(parser_finished, fetcher_finished) {
                        let parser_reason = get_worker_exit_reason(&parser_exit_reason);
                        let fetcher_reason = get_worker_exit_reason(&fetcher_exit_reason);
                        let incident_id = self.report_incident(
                            "pipeline_worker_terminated",
                            format!(
                                "idle_timeouts={} parser_finished={} fetcher_finished={} writer_queue_depth={} current={} target={} parser_reason={} fetcher_reason={}",
                                consecutive_idle_timeouts,
                                parser_finished,
                                fetcher_finished,
                                writer_queue_depth,
                                self.progress.current(),
                                self.progress.target(),
                                parser_reason.as_deref().unwrap_or("unknown"),
                                fetcher_reason.as_deref().unwrap_or("unknown")
                            ),
                        );
                        fetcher.abort();
                        parser.abort();
                        error!(
                            run_id = %self.run_id,
                            incident_id = %incident_id,
                            pipeline_epoch = self.pipeline_reset_epoch.load(Ordering::SeqCst),
                            idle_timeouts = consecutive_idle_timeouts,
                            parser_finished,
                            fetcher_finished,
                            parser_exit_reason = ?parser_reason,
                            fetcher_exit_reason = ?fetcher_reason,
                            writer_queue_depth,
                            "Pipeline worker terminated unexpectedly after idle timeout"
                        );
                        return Err(anyhow::anyhow!(
                            "Pipeline worker terminated unexpectedly: {}",
                            format_pipeline_worker_termination_message(
                                parser_finished,
                                fetcher_finished,
                                parser_reason.as_deref(),
                                fetcher_reason.as_deref(),
                            )
                        ));
                    }
                }
            }
        }
    }

    async fn fetch_blocks_with_config(
        rpc: &CkbRpcClient,
        start: u64,
        end: u64,
        parallel_fetch_size: usize,
    ) -> Result<Vec<BlockResponseWithCycles>> {
        let mut blocks = Vec::with_capacity((end - start + 1) as usize);
        let mut current = start;
        while current <= end {
            let batch_end = std::cmp::min(current + parallel_fetch_size as u64 - 1, end);
            let mut futures = FuturesOrdered::new();
            for block_num in current..=batch_end {
                futures.push_back(
                    async move { (block_num, rpc.get_block_by_number(block_num).await) },
                );
            }
            while let Some((block_num, result)) = futures.next().await {
                match result {
                    Ok(Some(block)) => blocks.push(block),
                    Ok(None) => return Err(anyhow::anyhow!("Block {} not found", block_num)),
                    Err(e) => return Err(e),
                }
            }
            current = batch_end + 1;
        }
        Ok(blocks)
    }

    fn fetch_blocks_direct(
        store: &CkbChainReader,
        start: u64,
        end: u64,
    ) -> Result<Vec<BlockResponseWithCycles>> {
        let block_numbers: Vec<u64> = (start..=end).collect();
        let results: Vec<Result<BlockResponseWithCycles>> = block_numbers
            .par_iter()
            .map(|&num| {
                let hash = store.get_block_hash(num).ok_or_else(|| {
                    anyhow::anyhow!("Block {} hash not found in CKB RocksDB", num)
                })?;
                let block = store.get_block(&hash).ok_or_else(|| {
                    anyhow::anyhow!("Block {} data not found in CKB RocksDB", num)
                })?;
                let rpc_block = ckb_store_reader::block_view_to_rpc(&block, store);
                Ok(rpc_block.into())
            })
            .collect();
        results.into_iter().collect()
    }

    async fn drain_channel<T>(rx: &mut tokio::sync::mpsc::Receiver<T>) -> usize {
        let mut drained = 0;
        while rx.try_recv().is_ok() {
            drained += 1;
        }
        drained
    }

    pub(super) async fn fetch_blocks_parallel(
        &self,
        start: u64,
        end: u64,
    ) -> Result<Vec<BlockResponseWithCycles>> {
        if let Some(ref store) = self.ckb_store {
            let store = Arc::clone(store);
            tokio::task::spawn_blocking(move || Self::fetch_blocks_direct(&store, start, end))
                .await
                .map_err(|e| anyhow::anyhow!("Block fetch task panicked: {}", e))?
        } else if self.is_bulk_sync_active() {
            bail!(
                "bulk sync requires direct RocksDB reads but CKB_DATA_PATH is not set \
                 (blocks {}-{}). Set CKB_DATA_PATH to the CKB node data directory",
                start,
                end
            )
        } else {
            Self::fetch_blocks_with_config(&self.rpc, start, end, self.config.parallel_fetch_size)
                .await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::requires_direct_reads_for_fetch;

    #[test]
    fn test_requires_direct_reads_for_fetch_only_when_bulk_allowed_and_lagging() {
        assert!(requires_direct_reads_for_fetch(true, 1001, 1000));
        assert!(!requires_direct_reads_for_fetch(false, 1001, 1000));
        assert!(!requires_direct_reads_for_fetch(true, 1000, 1000));
        assert!(!requires_direct_reads_for_fetch(true, 0, 1000));
    }
}
