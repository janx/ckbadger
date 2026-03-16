#![allow(clippy::type_complexity)]

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

use ckbadger_store::types::{LiveCellInfo, ObjectTypeIndex, PositionedCellInfo, SporeTypeIndex};

use crate::parser::{
    analyze_spore_media_profile,
    dotbit::{parse_dotbit_witness_bundle, DotbitWitnessBundle},
    DotbitParser, MnftParser, ParsedClusterCell, ParsedDotbitAccountOutput, ParsedMnftClass,
    ParsedMnftIssuer, ParsedMnftToken, ParsedSporeCell, SporeParser,
};
use crate::rpc::BlockResponseWithCycles;
use crate::runtime_diag::read_cgroup_memory_snapshot;
use ckbadger_store::types::{DOTBIT_SENTINEL_COLLECTION, SOLE_SPORES_SENTINEL_COLLECTION};

use ckb_store_reader::CkbChainReader;
use rayon::prelude::*;

use super::adaptive::*;
use super::batch::*;
use super::checked_tx_count;
use super::dao_helpers::*;
use super::diagnostics::*;
use super::helpers::*;
use super::indexer::{
    blocks_behind_tip, next_start_block_from_db_tip, require_non_negative_block_number, Indexer,
};
use super::nft_helpers::*;
use super::sync_mode::*;
use super::token_helpers::*;
use super::types::{
    CachedCellInfo, DotbitConsumptionEvent, MnftConsumptionEvent, PreParsedNftData, ReorgAction,
    SporeConsumptionEvent, TxData,
};
use super::undo::*;
use crate::bulk_sync_perf::BatchSample;

#[derive(Debug, Default, Clone, Copy)]
struct ParserPrecomputePhaseMetrics {
    build_batch_cell_infos_ms: f64,
    compute_fee_ms: f64,
    cache_balance_and_script_ms: f64,
    spore_precompute_ms: f64,
    nft_precompute_ms: f64,
}

impl ParserPrecomputePhaseMetrics {
    fn total_ms(&self) -> f64 {
        self.build_batch_cell_infos_ms
            + self.compute_fee_ms
            + self.cache_balance_and_script_ms
            + self.spore_precompute_ms
            + self.nft_precompute_ms
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct ParserBatchPerfSample {
    parse_ms: f64,
    precompute_ms: f64,
    nft_precompute_ms: f64,
}

#[derive(Debug, Default)]
struct ScannedPreParsedNftTx {
    mnft_issuers: Vec<ParsedMnftIssuer>,
    mnft_classes: Vec<(usize, ParsedMnftClass)>,
    mnft_tokens: Vec<(usize, ParsedMnftToken)>,
    dotbit_accounts: Vec<ParsedDotbitAccountOutput>,
    dotbit_action: Option<String>,
}

fn scan_preparsed_nft_tx(
    tx_data: &TxData,
    witness_bundle: &DotbitWitnessBundle,
) -> Result<ScannedPreParsedNftTx> {
    let mut scanned = ScannedPreParsedNftTx {
        mnft_issuers: Vec::new(),
        mnft_classes: Vec::new(),
        mnft_tokens: Vec::new(),
        dotbit_accounts: Vec::new(),
        dotbit_action: witness_bundle.action.clone(),
    };
    let mut missing_name_count = 0usize;
    let mut missing_name_samples: Vec<String> = Vec::new();

    for (output_index, cell) in tx_data.cells.iter().enumerate() {
        if let Some(issuer) = MnftParser::parse_issuer_parsed_cell(cell) {
            scanned.mnft_issuers.push(issuer);
        }
        if let Some(class) = MnftParser::parse_class_parsed_cell(cell) {
            scanned.mnft_classes.push((output_index, class));
        }
        if let Some(token) = MnftParser::parse_token_parsed_cell(cell) {
            scanned.mnft_tokens.push((output_index, token));
        }
        let Some(mut account) = DotbitParser::parse_account_parsed_cell(cell) else {
            continue;
        };
        let output_index = i16::try_from(output_index).map_err(|_| {
            anyhow!(
                "dotbit output index exceeds i16 range: tx_hash=0x{}, output_index={}",
                hex::encode(tx_data.hash),
                output_index
            )
        })?;
        if let Some(wd) = witness_bundle.accounts.get(&account.account_id) {
            account.account = wd.name.clone();
            account.registered_at = wd.registered_at;
            account.status = wd.status;
        }
        if account.account.is_none() {
            missing_name_count += 1;
            if missing_name_samples.len() < 5 {
                missing_name_samples.push(format!("0x{}", hex::encode(&account.account_id)));
            }
        }
        scanned.dotbit_accounts.push(ParsedDotbitAccountOutput {
            output_index,
            account,
        });
    }

    if missing_name_count > 0 {
        return Err(anyhow!(
            "dotbit account name missing in DAS witness: tx_hash=0x{}, missing_account_name_count={}, missing_account_name_samples={}",
            hex::encode(tx_data.hash),
            missing_name_count,
            missing_name_samples.join(",")
        ));
    }

    Ok(scanned)
}

fn run_nft_precompute(
    all_parsed_blocks: &[crate::parser::block::ParsedBlock],
    all_tx_data: &[TxData],
    input_cell_info: &HashMap<(Vec<u8>, i16), PositionedCellInfo>,
    batch_cell_infos: &HashMap<(Vec<u8>, i16), PositionedCellInfo>,
    dotbit_outpoint_fallback: &HashMap<(Vec<u8>, i16), Vec<u8>>,
) -> Result<PreParsedNftData> {
    let mut mnft_issuers: Vec<(usize, ParsedMnftIssuer)> = Vec::new();
    let mut mnft_classes: Vec<(usize, usize, ParsedMnftClass)> = Vec::new();
    let mut mnft_tokens: Vec<(usize, usize, ParsedMnftToken)> = Vec::new();
    let mut dotbit_accounts: Vec<(usize, ParsedDotbitAccountOutput)> = Vec::new();
    let mut dotbit_tx_actions: HashMap<usize, String> = HashMap::new();
    let mut batch_dotbit_outpoints: HashMap<(Vec<u8>, i16), Vec<u8>> = HashMap::new();
    let mut batch_dotbit_latest_create_order: HashMap<Vec<u8>, u64> = HashMap::new();
    let mut batch_spore_latest_create: HashMap<Vec<u8>, usize> = HashMap::new();
    let mut batch_mnft_latest_create: HashMap<Vec<u8>, usize> = HashMap::new();

    let dotbit_code_hash =
        crate::rpc::parse_hex_to_bytes(crate::parser::dotbit::DOTBIT_ACCOUNT_CELL_TYPE_ID);

    let mut block_tx_idx = 0usize;
    for parsed in all_parsed_blocks {
        let tx_count_for_block = checked_tx_count(parsed.transactions_count, parsed.number)?;
        let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
        for (tx_idx, tx_data) in tx_slice.iter().enumerate() {
            let tx_global_index = block_tx_idx + tx_idx;
            let has_dotbit_output = tx_data.cells.iter().any(|cell| {
                cell.type_code_hash
                    .as_ref()
                    .is_some_and(|tc| tc.as_slice() == dotbit_code_hash.as_slice())
            });
            let witness_bundle = if has_dotbit_output {
                parse_dotbit_witness_bundle(&tx_data.witnesses)
            } else {
                DotbitWitnessBundle::default()
            };
            let scanned = scan_preparsed_nft_tx(tx_data, &witness_bundle).with_context(|| {
                format!(
                    "block={}, tx=0x{}",
                    parsed.number,
                    hex::encode(tx_data.hash)
                )
            })?;
            let dotbit_create_order = dotbit_create_event_order(tx_global_index)
                .map_err(|e| anyhow!("dotbit_create_event_order overflow: {}", e))?;

            for issuer in scanned.mnft_issuers {
                mnft_issuers.push((tx_global_index, issuer));
            }
            for (output_index, class) in scanned.mnft_classes {
                mnft_classes.push((tx_global_index, output_index, class));
            }
            for (output_index, token) in scanned.mnft_tokens {
                mnft_tokens.push((tx_global_index, output_index, token));
            }
            if !scanned.dotbit_accounts.is_empty() {
                if let Some(action) = scanned.dotbit_action {
                    dotbit_tx_actions.insert(tx_global_index, action);
                }
            }
            for account in scanned.dotbit_accounts {
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

            // Track spore and mNFT output creations for same-batch ordering
            for cell in &tx_data.cells {
                if let Some(ref tc) = cell.type_code_hash {
                    if SporeParser::is_spore_type_script(tc) {
                        if let Some(ref type_args) = cell.type_args {
                            if !type_args.is_empty() {
                                batch_spore_latest_create
                                    .entry(type_args.clone())
                                    .and_modify(|current| {
                                        if tx_global_index > *current {
                                            *current = tx_global_index;
                                        }
                                    })
                                    .or_insert(tx_global_index);
                            }
                        }
                    } else if MnftParser::is_token_type_script(tc) {
                        if let Some(ref type_args) = cell.type_args {
                            if type_args.len() >= 28 {
                                batch_mnft_latest_create
                                    .entry(type_args.clone())
                                    .and_modify(|current| {
                                        if tx_global_index > *current {
                                            *current = tx_global_index;
                                        }
                                    })
                                    .or_insert(tx_global_index);
                            }
                        }
                    }
                }
            }
        }
        block_tx_idx += tx_count_for_block;
    }

    let mut consumed_dotbit: Vec<DotbitConsumptionEvent> = Vec::new();
    let mut consumed_spore: Vec<SporeConsumptionEvent> = Vec::new();
    let mut consumed_mnft: Vec<MnftConsumptionEvent> = Vec::new();
    let mut block_tx_idx = 0usize;
    for parsed in all_parsed_blocks {
        let tx_count_for_block = checked_tx_count(parsed.transactions_count, parsed.number)?;
        let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
        for (tx_idx, tx_data) in tx_slice.iter().enumerate() {
            if tx_data.is_cellbase || tx_data.inputs.is_empty() {
                continue;
            }
            let tx_global_index = block_tx_idx + tx_idx;
            let dotbit_consume_order = dotbit_consume_event_order(tx_global_index)
                .map_err(|e| anyhow!("dotbit_consume_event_order overflow: {}", e))?;
            for input in &tx_data.inputs {
                let key = (
                    input.previous_tx_hash.to_vec(),
                    parsed_input_outpoint_index_i16(input.previous_output_index, "sync_indexer"),
                );

                if let Some(account_id) = batch_dotbit_outpoints.get(&key) {
                    let latest_create_order =
                        batch_dotbit_latest_create_order.get(account_id).copied();
                    if should_consume_dotbit_account(latest_create_order, dotbit_consume_order) {
                        consumed_dotbit.push(DotbitConsumptionEvent {
                            account_id: account_id.clone(),
                            block_number: parsed.number,
                            consuming_tx_hash: tx_data.hash,
                            tx_global_index,
                        });
                    }
                    continue;
                }

                let cell_info = input_cell_info
                    .get(&key)
                    .or_else(|| batch_cell_infos.get(&key));

                if let Some(cell_info) = cell_info {
                    if let Some(ref tc) = cell_info.type_code_hash {
                        // DotBit check (existing)
                        if DotbitParser::is_account_cell_type_script(tc) {
                            let account_id = cell_info
                                .type_args
                                .as_ref()
                                .filter(|args| args.len() == 20 && !args.iter().all(|&b| b == 0))
                                .cloned()
                                .or_else(|| dotbit_outpoint_fallback.get(&key).cloned());
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
                                        tx_global_index,
                                    });
                                }
                            }
                        }
                        // Spore check
                        else if SporeParser::is_spore_type_script(tc) {
                            if let Some(ref type_args) = cell_info.type_args {
                                if !type_args.is_empty() {
                                    let latest_create =
                                        batch_spore_latest_create.get(type_args).copied();
                                    if should_consume_spore(latest_create, tx_global_index) {
                                        consumed_spore.push(SporeConsumptionEvent {
                                            spore_id: type_args.clone(),
                                            block_number: parsed.number,
                                            consuming_tx_hash: tx_data.hash,
                                            tx_global_index,
                                        });
                                    }
                                }
                            }
                        }
                        // mNFT token check
                        else if MnftParser::is_token_type_script(tc) {
                            if let Some(ref type_args) = cell_info.type_args {
                                if type_args.len() >= 28 {
                                    let latest_create =
                                        batch_mnft_latest_create.get(type_args).copied();
                                    if should_consume_mnft_token(latest_create, tx_global_index) {
                                        consumed_mnft.push(MnftConsumptionEvent {
                                            token_id: type_args.clone(),
                                            block_number: parsed.number,
                                            consuming_tx_hash: tx_data.hash,
                                            tx_global_index,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        block_tx_idx += tx_count_for_block;
    }

    Ok(PreParsedNftData {
        mnft_issuers,
        mnft_classes,
        mnft_tokens,
        dotbit_accounts,
        consumed_dotbit,
        consumed_spore,
        consumed_mnft,
        dotbit_tx_actions,
    })
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
            HashMap<(Vec<u8>, i16), PositionedCellInfo>,
            // Pre-computed in parser stage:
            HashMap<(Vec<u8>, i16), PositionedCellInfo>, // batch_cell_infos
            HashMap<Vec<u8>, (i128, i32, i32, i64, i64, Vec<u8>, i128)>, // address_balance_changes
            ScriptUsageChanges,                          // script_usage_changes
            HashMap<(Vec<u8>, bool, u32), (i128, i128)>, // script_daily_changes
            HashMap<(Vec<u8>, u32), (i128, i128)>,       // token_daily_changes
            HashMap<Vec<u8>, SporeTypeIndex>,            // spore_type_index_changes
            HashMap<(Vec<u8>, u32), (i128, i128)>,       // spore_daily_changes
            HashMap<(Vec<u8>, u32), (i128, i128)>,       // cluster_daily_changes
            HashMap<Vec<u8>, ObjectTypeIndex>,           // object_type_index_changes
            HashMap<(Vec<u8>, u32), (i128, i128)>,       // object_daily_changes
            PreParsedSporeData,                          // pre-parsed spore/cluster data
            PreParsedNftData,                            // pre-parsed mNFT/DotBit data
            ParserBatchPerfSample,                       // parser hotpath timings
        );

        let (fetch_tx, mut fetch_rx) = mpsc::channel::<FetchedBatch>(self.config.pipeline_buffer);
        let (parse_tx, mut parse_rx) = mpsc::channel::<ParsedBatch>(self.config.pipeline_buffer);
        let parse_tx_pending_txs = Arc::new(AtomicU64::new(0));
        let parser_exit_reason = Arc::new(std::sync::Mutex::new(None::<String>));
        let fetcher_exit_reason = Arc::new(std::sync::Mutex::new(None::<String>));
        let committed_tip_for_cache = Arc::new(AtomicI64::new(self.repo.get_sync_tip().await?.0));
        self.pipeline_perf
            .set_queue_capacities(self.config.pipeline_buffer, self.config.pipeline_buffer);

        let config = self.config.clone();
        let progress = Arc::clone(&self.progress);
        let repo = self.repo.clone();
        let run_id_for_fetcher = self.run_id.clone();
        let rebuild_pause = Arc::clone(&self.rebuild_pause_flag);
        let pipeline_reset_notify = Arc::clone(&self.pipeline_reset_notify_flag);
        let pipeline_reset_reason_code = Arc::clone(&self.pipeline_reset_reason_code);
        let pipeline_epoch_for_fetcher = Arc::clone(&self.pipeline_reset_epoch);
        let ckb_store = self
            .ckb_store
            .clone()
            .expect("ckb_store must exist before pipeline starts");
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

                if let Err(e) = ckb_store.refresh() {
                    error!("Failed to refresh CKB RocksDB secondary: {}", e);
                    sleep(Duration::from_secs(5)).await;
                    continue;
                }

                let chain_tip = match ckb_store.tip_number() {
                    Some(tip) => tip,
                    None => {
                        error!("Failed to get chain tip from CKB RocksDB");
                        sleep(Duration::from_secs(5)).await;
                        continue;
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
                let store = Arc::clone(&ckb_store);
                let sb = start_block;
                let eb = end_block;
                let blocks = match tokio::task::spawn_blocking(move || {
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
                    HashMap<(Vec<u8>, i16), PositionedCellInfo>,
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
                    let mut attempt_input_cell_info: HashMap<(Vec<u8>, i16), PositionedCellInfo> =
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
                                PositionedCellInfo::new(
                                    LiveCellInfo {
                                        capacity: cached.capacity,
                                        lock_script_hash: cached.lock_script_hash.clone(),
                                        lock_code_hash: cached.lock_code_hash.clone(),
                                        lock_hash_type: cached.lock_hash_type,
                                        lock_args: cached.lock_args.clone(),
                                        type_script_hash: cached.type_script_hash.clone(),
                                        type_code_hash: cached.type_code_hash.clone(),
                                        type_hash_type: cached.type_hash_type,
                                        type_args: cached.type_args.clone(),
                                        data_size: cached.data_size,
                                        occupied_capacity: cached.occupied_capacity,
                                        udt_amount: cached.udt_amount,
                                        data_hash: cached.data_hash.clone(),
                                    },
                                    cached.created_at_block,
                                ),
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
                            wr.get_full_cells_info_batch(&refs)
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

                let mut precompute_phase_metrics = ParserPrecomputePhaseMetrics::default();

                // Pass 1: Build batch_cell_infos
                let build_batch_cell_infos_started = Instant::now();
                let mut batch_cell_infos: HashMap<(Vec<u8>, i16), PositionedCellInfo> =
                    HashMap::new();
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
                            PositionedCellInfo::new(
                                LiveCellInfo {
                                    capacity: cell.capacity,
                                    lock_script_hash: cell.lock_script_hash.clone(),
                                    lock_code_hash: cell.lock_code_hash.clone(),
                                    lock_hash_type: cell.lock_hash_type,
                                    lock_args: cell.lock_args.clone(),
                                    type_script_hash: cell.type_script_hash.clone(),
                                    type_code_hash: cell.type_code_hash.clone(),
                                    type_hash_type: cell.type_hash_type,
                                    type_args: cell.type_args.clone(),
                                    data_size: cell.data_size,
                                    occupied_capacity,
                                    udt_amount,
                                    data_hash: if cell.data_hash.is_empty() {
                                        None
                                    } else {
                                        Some(cell.data_hash.clone())
                                    },
                                },
                                tx_data.block_number,
                            ),
                        );
                    }
                }
                precompute_phase_metrics.build_batch_cell_infos_ms =
                    build_batch_cell_infos_started.elapsed().as_secs_f64() * 1000.0;

                // Pass 2: Compute input capacity + fee
                let compute_fee_started = Instant::now();
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
                precompute_phase_metrics.compute_fee_ms =
                    compute_fee_started.elapsed().as_secs_f64() * 1000.0;

                // Pass 3: cell_cache update + address_balance_changes + script_usage_changes
                let cache_balance_and_script_started = Instant::now();
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
                let mut object_type_index_changes: HashMap<Vec<u8>, ObjectTypeIndex> =
                    HashMap::new();
                let mut object_daily_changes: HashMap<(Vec<u8>, u32), (i128, i128)> =
                    HashMap::new();
                let mut spore_type_index_cache: HashMap<Vec<u8>, Option<SporeTypeIndex>> =
                    HashMap::new();
                let mut object_type_index_cache: HashMap<Vec<u8>, Option<ObjectTypeIndex>> =
                    HashMap::new();

                for tx_data in &all_tx_data {
                    let date_yyyymmdd = ckbadger_store::keys::timestamp_ms_to_date(
                        tx_data.timestamp.timestamp_millis(),
                    );
                    // cell_cache update
                    for (output_index, _cell) in tx_data.cells.iter().enumerate() {
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
                        let key = (tx_data.hash.to_vec(), output_index_i16);
                        let info = match batch_cell_infos.get(&key) {
                            Some(info) => info,
                            None => {
                                error!(
                                    block_number = tx_data.block_number,
                                    tx_hash = %hex::encode(tx_data.hash),
                                    output_index,
                                    "Parser: missing precomputed cell info while updating parser cache"
                                );
                                record_worker_exit_reason(
                                    &parser_exit_reason_for_parser,
                                    format!(
                                        "missing precomputed cell info while updating parser cache: block={}, tx=0x{}, output_index={}",
                                        tx_data.block_number,
                                        hex::encode(tx_data.hash),
                                        output_index
                                    ),
                                );
                                return;
                            }
                        };
                        cell_cache_for_parser.insert(
                            (tx_data.hash, output_index_i16),
                            CachedCellInfo {
                                capacity: info.capacity,
                                created_at_block: info.created_at_block,
                                lock_script_hash: info.lock_script_hash.clone(),
                                lock_code_hash: info.lock_code_hash.clone(),
                                lock_hash_type: info.lock_hash_type,
                                lock_args: info.lock_args.clone(),
                                type_script_hash: info.type_script_hash.clone(),
                                type_code_hash: info.type_code_hash.clone(),
                                type_hash_type: info.type_hash_type,
                                type_args: info.type_args.clone(),
                                data_size: info.data_size,
                                occupied_capacity: info.occupied_capacity,
                                udt_amount: info.udt_amount,
                                data_hash: info.data_hash.clone(),
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

                                {
                                    let effective_cluster_id = cluster_id.unwrap_or_else(|| {
                                        SOLE_SPORES_SENTINEL_COLLECTION.to_vec()
                                    });
                                    let cluster_daily = cluster_daily_changes
                                        .entry((effective_cluster_id, date_yyyymmdd))
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
                                let index = ObjectTypeIndex {
                                    collection_id: collection_id.clone(),
                                };
                                object_type_index_cache
                                    .insert(type_script_hash.clone(), Some(index.clone()));
                                object_type_index_changes.insert(type_script_hash.clone(), index);

                                let object_daily = object_daily_changes
                                    .entry((collection_id, date_yyyymmdd))
                                    .or_insert((0, 0));
                                object_daily.0 += i128::from(cell.capacity);
                                object_daily.1 += i128::from(cell_occupied);
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

                                            {
                                                let effective_cluster_id =
                                                    index.cluster_id.unwrap_or_else(|| {
                                                        SOLE_SPORES_SENTINEL_COLLECTION.to_vec()
                                                    });
                                                let cluster_daily = cluster_daily_changes
                                                    .entry((effective_cluster_id, date_yyyymmdd))
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
                                                object_type_index_cache.get(type_script_hash)
                                            {
                                                cached.clone().map(|idx| idx.collection_id)
                                            } else {
                                                match load_optional_index_from_store(
                                                    &mut object_type_index_cache,
                                                    type_script_hash,
                                                    "nft_type",
                                                    || {
                                                        writer_for_parser
                                                            .store()
                                                            .get_object_type_index(type_script_hash)
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
                                            let object_daily = object_daily_changes
                                                .entry((collection_id, date_yyyymmdd))
                                                .or_insert((0, 0));
                                            object_daily.0 -= i128::from(info.capacity);
                                            object_daily.1 -= i128::from(info.occupied_capacity);
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
                precompute_phase_metrics.cache_balance_and_script_ms =
                    cache_balance_and_script_started.elapsed().as_secs_f64() * 1000.0;

                // Spore precompute: parse all spores/clusters and compute media
                // profiles in the parser stage to offload T6 writer thread.
                let spore_precompute_started = Instant::now();
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
                                if e.standard == ckbadger_store::types::ObjectStandard::SporeCluster
                                {
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
                precompute_phase_metrics.spore_precompute_ms =
                    spore_precompute_started.elapsed().as_secs_f64() * 1000.0;

                // mNFT/DotBit precompute: parse all mNFT issuers/classes/tokens and DotBit
                // accounts in the parser stage to offload t6b writer thread.
                // Also pre-identify consumed DotBit inputs using input_cell_info
                // type_code_hash + type_args, with DB fallback for historical cells
                // whose type_args is empty (account_id stored only in cell data).
                let nft_precompute_started = Instant::now();
                let dotbit_outpoint_fallback = {
                    let dotbit_code_hash = crate::rpc::parse_hex_to_bytes(
                        crate::parser::dotbit::DOTBIT_ACCOUNT_CELL_TYPE_ID,
                    );
                    let mut needs_resolution: Vec<(Vec<u8>, i16)> = Vec::new();
                    for ((tx_hash, idx), info) in &input_cell_info {
                        let is_dotbit = info
                            .type_code_hash
                            .as_ref()
                            .map(|tc| tc.as_slice() == dotbit_code_hash.as_slice())
                            .unwrap_or(false);
                        if !is_dotbit {
                            continue;
                        }
                        let has_valid_type_args = info
                            .type_args
                            .as_ref()
                            .filter(|args| args.len() == 20 && !args.iter().all(|&b| b == 0))
                            .is_some();
                        if !has_valid_type_args {
                            needs_resolution.push((tx_hash.clone(), *idx));
                        }
                    }
                    if needs_resolution.is_empty() {
                        HashMap::new()
                    } else {
                        let wr = writer_for_parser.clone();
                        let nr = needs_resolution;
                        let db_query = tokio::task::spawn_blocking(move || {
                            let refs: Vec<(&[u8], i16)> =
                                nr.iter().map(|(h, i)| (h.as_slice(), *i)).collect();
                            wr.store().get_dotbit_account_ids_by_outpoints_batch(&refs)
                        });
                        match tokio::time::timeout(Duration::from_secs(10), db_query).await {
                            Ok(Ok(Ok(resolved))) => {
                                let mut map = HashMap::with_capacity(resolved.len());
                                for (tx_hash, idx, account_id) in resolved {
                                    map.insert((tx_hash, idx), account_id);
                                }
                                map
                            }
                            Ok(Ok(Err(e))) => {
                                let msg = format!(
                                    "dotbit outpoint fallback DB query failed for range {}-{}: {}",
                                    start_block, end_block, e
                                );
                                error!(start_block, end_block, "{}", msg);
                                record_worker_exit_reason(&parser_exit_reason_for_parser, msg);
                                return;
                            }
                            Ok(Err(e)) => {
                                let msg = format!(
                                    "dotbit outpoint fallback task failed for range {}-{}: {}",
                                    start_block, end_block, e
                                );
                                error!(start_block, end_block, "{}", msg);
                                record_worker_exit_reason(&parser_exit_reason_for_parser, msg);
                                return;
                            }
                            Err(_) => {
                                let msg = format!(
                                    "dotbit outpoint fallback timed out for range {}-{}",
                                    start_block, end_block
                                );
                                error!(start_block, end_block, "{}", msg);
                                record_worker_exit_reason(&parser_exit_reason_for_parser, msg);
                                return;
                            }
                        }
                    }
                };
                let pre_parsed_nft_data: PreParsedNftData = match run_nft_precompute(
                    &all_parsed_blocks[..blocks.len()],
                    &all_tx_data,
                    &input_cell_info,
                    &batch_cell_infos,
                    &dotbit_outpoint_fallback,
                ) {
                    Ok(data) => data,
                    Err(e) => {
                        error!(
                            start_block,
                            end_block, "Parser: nft precompute failed: {}", e
                        );
                        record_worker_exit_reason(
                            &parser_exit_reason_for_parser,
                            format!(
                                "nft precompute failed for range {}-{}: {}",
                                start_block, end_block, e
                            ),
                        );
                        return;
                    }
                };
                precompute_phase_metrics.nft_precompute_ms =
                    nft_precompute_started.elapsed().as_secs_f64() * 1000.0;

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
                    build_batch_cell_infos_ms =
                        format!("{:.1}", precompute_phase_metrics.build_batch_cell_infos_ms),
                    compute_fee_ms = format!("{:.1}", precompute_phase_metrics.compute_fee_ms),
                    cache_balance_and_script_ms = format!(
                        "{:.1}",
                        precompute_phase_metrics.cache_balance_and_script_ms
                    ),
                    spore_precompute_ms =
                        format!("{:.1}", precompute_phase_metrics.spore_precompute_ms),
                    nft_precompute_ms =
                        format!("{:.1}", precompute_phase_metrics.nft_precompute_ms),
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
                    precompute_phase_total_ms =
                        format!("{:.1}", precompute_phase_metrics.total_ms()),
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
                let parser_perf_sample = ParserBatchPerfSample {
                    parse_ms: t_parse_ms,
                    precompute_ms: precompute_parser_ms,
                    nft_precompute_ms: precompute_phase_metrics.nft_precompute_ms,
                };
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
                        object_type_index_changes,
                        object_daily_changes,
                        pre_parsed_spore_data,
                        pre_parsed_nft_data,
                        parser_perf_sample,
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

        // Resolve disk device once for per-batch I/O delta tracking
        let disk_device = {
            let mounts = std::fs::read_to_string("/proc/mounts").unwrap_or_default();
            crate::sys_info::parse_mount_info(&mounts, &self.config.domain_data_path)
                .map(|(dev, _fs)| dev)
                .unwrap_or_default()
        };
        let mut disk_tracker = crate::sys_info::DiskStatsTracker::new(disk_device);
        let mut batches_since_last_flush: u32 = 0;
        let mut compaction_checkpoint_done = false;

        loop {
            if self.shutdown_requested.load(Ordering::SeqCst) {
                info!(run_id = %self.run_id, "Shutdown requested, aborting pipeline");
                fetcher.abort();
                parser.abort();
                return Ok(());
            }

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
                    object_type_index_changes,
                    object_daily_changes,
                    pre_parsed_spore_data,
                    pre_parsed_nft_data,
                    parser_perf_sample,
                ))) => {
                    consecutive_idle_timeouts = 0;
                    atomic_checked_sub_u64(
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
                                        self.reconcile_cell_dist_tracker_with_tip(reorg_tip)?;
                                        let current_epoch =
                                            self.pipeline_reset_epoch.load(Ordering::SeqCst);
                                        info!(
                                            run_id = %self.run_id,
                                            pipeline_epoch = current_epoch,
                                            db_tip,
                                            chain_tip,
                                            reorg_tip,
                                            "Reorg handled, caches cleared, trackers reconciled, draining stale parsed batches"
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
                            object_type_index_changes,
                            object_daily_changes,
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
                            let bulk_sync_mode = is_effective_bulk_sync_batch(
                                chain_tip,
                                end_block,
                                self.config.bulk_sync_threshold,
                                self.bulk_sync_allowed.load(Ordering::SeqCst),
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
                                if let Err(consistency_err) =
                                    self.reconcile_cell_dist_tracker_with_tip(cleanup_tip)
                                {
                                    error!(
                                        cleanup_tip,
                                        "Cell distribution tracker consistency check failed after batch cleanup: {:?}",
                                        consistency_err
                                    );
                                    return Err(consistency_err).with_context(|| {
                                        format!(
                                            "Cell distribution tracker inconsistent after batch cleanup to tip {}. automatic rebuild is disabled; delete RocksDB and re-sync from genesis",
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
                        let block_num = require_non_negative_block_number(
                            last_block.number,
                            "pipeline writer record_batch",
                        )?;
                        self.progress.record_batch(
                            block_num,
                            all_parsed_blocks.len() as u64,
                            batch_tx_count_u64,
                        );

                        let mode = if self.is_bulk_sync_active() {
                            "[BULK]"
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
                        let queue_pressure = build_queue_pressure_snapshot(
                            parse_queue_pending_txs,
                            parse_queue_capacity_txs,
                            writer_queue as u64,
                            parse_tx_for_writer_depth.max_capacity() as u64,
                        );
                        let memory_ratio_pct =
                            cgroup_memory_ratio_pct(&read_cgroup_memory_snapshot());
                        let compaction_pressure = self.writer.store().compaction_pressure();
                        let blocks_remaining = self.progress.blocks_remaining();
                        let db_stage_ms = db_elapsed.as_secs_f64() * 1000.0;
                        let write_us_per_tx = if batch_tx_count > 0 && db_stage_ms > 0.0 {
                            Some((db_stage_ms * 1000.0) / batch_tx_count as f64)
                        } else {
                            None
                        };
                        let mem_profile = self.writer.store().memory_profile();
                        let is_bulk = self.writer.store().is_bulk_sync_mode();
                        if let Some(adjustment) =
                            self.adaptive_batch_controller
                                .update_after_write(AdaptiveBatchInput {
                                    write_ms: db_stage_ms,
                                    commit_ms: write_metrics.commit_ms,
                                    batch_tx_count,
                                    blocks_remaining,
                                    parse_queue_fill_pct: queue_pressure.parse_queue_fill_pct,
                                    writer_queue_fill_pct: queue_pressure.writer_queue_fill_pct,
                                    memory_ratio_pct,
                                    l0_files_total: Some(compaction_pressure.l0_files_total),
                                    l0_files_max: Some(compaction_pressure.l0_files_max),
                                    compaction_pending_bytes: Some(
                                        compaction_pressure.compaction_pending_bytes,
                                    ),
                                    immutable_memtables: Some(
                                        compaction_pressure.immutable_memtables,
                                    ),
                                    severe_pending_threshold: if is_bulk {
                                        mem_profile.severe_compaction_pending_bytes_bulk
                                    } else {
                                        mem_profile.severe_compaction_pending_bytes
                                    },
                                    moderate_pending_threshold: if is_bulk {
                                        mem_profile.moderate_compaction_pending_bytes_bulk
                                    } else {
                                        mem_profile.moderate_compaction_pending_bytes
                                    },
                                    severe_imm_threshold: mem_profile.severe_immutable_memtables,
                                    moderate_imm_threshold: mem_profile
                                        .moderate_immutable_memtables,
                                    is_bulk_sync: is_bulk,
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
                                parse_queue_fill_pct = queue_pressure.parse_queue_fill_pct.map(|v| format!("{:.1}", v)),
                                writer_queue_fill_pct = queue_pressure.writer_queue_fill_pct.map(|v| format!("{:.1}", v)),
                                parse_queue_pending_txs = queue_pressure.parse_queue_pending_txs,
                                parse_queue_capacity_txs = queue_pressure.parse_queue_capacity_txs,
                                writer_queue_depth = queue_pressure.writer_queue_depth,
                                writer_queue_capacity = queue_pressure.writer_queue_capacity,
                                l0_files_total = compaction_pressure.l0_files_total,
                                l0_files_max = compaction_pressure.l0_files_max,
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
                        let perf_stats = self.writer.store().memory_stats();
                        let batch_env = crate::sys_info::read_batch_environment(&mut disk_tracker);
                        self.record_bulk_sync_perf_batch_sample(BatchSample {
                            txs: write_metrics.txs,
                            cells: write_metrics.cells,
                            inputs: write_metrics.inputs,
                            parse_ms: parser_perf_sample.parse_ms,
                            precompute_ms: parser_perf_sample.precompute_ms,
                            nft_precompute_ms: parser_perf_sample.nft_precompute_ms,
                            write_ms: write_metrics.write_ms,
                            prefetch_ms: write_metrics.prefetch_ms,
                            finalize_ms: write_metrics.finalize_ms,
                            t1_ms: write_metrics.t1_ms,
                            t1b_ms: write_metrics.t1b_ms,
                            t2_ms: write_metrics.t2_ms,
                            t4_ms: write_metrics.t4_ms,
                            t5_ms: write_metrics.t5_ms,
                            t6a_ms: write_metrics.t6a_ms,
                            t6b_ms: write_metrics.t6b_ms,
                            t7_ms: write_metrics.t7_ms,
                            t_act_ms: write_metrics.t_act_ms,
                            t_track_ms: write_metrics.t_track_ms,
                            ..BatchSample::new(
                                u64::try_from(all_parsed_blocks.len())
                                    .expect("parsed block count exceeds u64"),
                                db_elapsed.as_secs_f64(),
                                write_metrics.commit_ms,
                                perf_stats.compaction_pending_bytes / (1024 * 1024),
                                perf_stats.l0_files_count,
                                perf_stats.immutable_memtables,
                                chrono::Utc::now()
                                    .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                                    .to_string(),
                                batch_env.load_avg_1m,
                                batch_env.mem_available_mb,
                                batch_env.disk_read_mb,
                                batch_env.disk_write_mb,
                            )
                        });
                        info!(
                            "Wrote blocks {} to {} ({} remaining, {:.2}s, commit={:.0}ms, q={}, wait={:.0}ms, adaptive_txs={}, adaptive_min_txs={}, inflight_limit={}) {}",
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
                            mode
                        );

                        self.maybe_invalidate_chart_caches(end_block).await;
                        self.check_bulk_sync_completion().await;
                        self.ensure_compaction_mode(blocks_remaining);

                        // Memory-pressure flush + compaction checkpoint
                        batches_since_last_flush += 1;
                        if self.writer.store().is_bulk_sync_mode() {
                            let mem_flush_threshold_mb =
                                self.writer.store().memory_profile().system_ram_bytes
                                    / (1024 * 1024)
                                    / 5; // 20% of total RAM
                            if batch_env.mem_available_mb < mem_flush_threshold_mb
                                && batches_since_last_flush >= 30
                            {
                                info!(
                                    mem_available_mb = batch_env.mem_available_mb,
                                    threshold_mb = mem_flush_threshold_mb,
                                    "Memory pressure detected, flushing memtables"
                                );
                                if let Err(e) = self.writer.store().flush_all_memtables() {
                                    warn!(error = %e, "Memory-pressure flush failed");
                                }
                                batches_since_last_flush = 0;

                                let cp_pending_mb =
                                    compaction_pressure.compaction_pending_bytes / (1024 * 1024);
                                if cp_pending_mb > 6000 && !compaction_checkpoint_done {
                                    info!(
                                        compaction_pending_mb = cp_pending_mb,
                                        "Compaction checkpoint: compacting hot CFs"
                                    );
                                    self.writer.store().compact_hot_cfs();

                                    // Poll until pressure drains or timeout
                                    let checkpoint_start = std::time::Instant::now();
                                    loop {
                                        let p = self.writer.store().compaction_pressure();
                                        let pending_mb = p.compaction_pending_bytes / (1024 * 1024);
                                        if pending_mb < 2000
                                            || checkpoint_start.elapsed().as_secs() > 120
                                        {
                                            info!(
                                                pending_mb,
                                                elapsed_s = checkpoint_start.elapsed().as_secs(),
                                                "Compaction checkpoint complete"
                                            );
                                            break;
                                        }
                                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                                    }
                                    compaction_checkpoint_done = true;
                                }
                            }
                        }
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
                    let blocks_remaining = self.progress.blocks_remaining();
                    let pipeline = self.pipeline_perf.snapshot();
                    let fetch_fill_pct = queue_fill_percentage(
                        pipeline.as_ref().and_then(|p| p.fetch_queue_depth),
                        pipeline.as_ref().and_then(|p| p.fetch_queue_capacity),
                    );
                    let parse_fill_pct = queue_fill_percentage(
                        pipeline.as_ref().and_then(|p| p.parse_queue_depth),
                        pipeline.as_ref().and_then(|p| p.parse_queue_capacity),
                    );
                    // When caught up (blocks_remaining == 0) and both workers are alive,
                    // idle is the expected state — don't warn.
                    let caught_up_and_idle =
                        blocks_remaining == 0 && !parser_finished && !fetcher_finished;
                    if !caught_up_and_idle
                        && should_log_pipeline_idle_timeout(consecutive_idle_timeouts)
                    {
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
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::super::types::TxData;
    use super::*;
    use crate::parser::block::ParsedBlock;
    use crate::parser::cell::CellParser;
    use crate::parser::dotbit::{parse_dotbit_witness_bundle, DOTBIT_ACCOUNT_CELL_TYPE_ID};
    use crate::parser::mnft::MNFT_TOKEN_CODE_HASH;
    use crate::parser::transaction::TransactionParser;
    use crate::rpc::{CellDep, CellInput, CellOutput, OutPoint, Script, TransactionView};

    fn create_lock_script() -> Script {
        Script {
            code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8"
                .to_string(),
            hash_type: "type".to_string(),
            args: "0x927f3e74dceb87c81ba65a19da4f098b4de75a0d".to_string(),
        }
    }

    fn create_dotbit_account_cell_type_script(account_id: &[u8; 20]) -> Script {
        Script {
            code_hash: DOTBIT_ACCOUNT_CELL_TYPE_ID.to_string(),
            hash_type: "type".to_string(),
            args: format!("0x{}", hex::encode(account_id)),
        }
    }

    fn create_dotbit_account_cell_data(account_id: &[u8; 20]) -> Vec<u8> {
        let mut data = vec![0u8; 32];
        data.extend_from_slice(account_id);
        data.extend_from_slice(&[0u8; 20]);
        data.extend_from_slice(&0u64.to_le_bytes());
        data
    }

    fn create_mnft_token_type_script(class_id: &[u8], token_index: u32) -> Script {
        let mut args = class_id.to_vec();
        args.extend_from_slice(&token_index.to_le_bytes());
        Script {
            code_hash: MNFT_TOKEN_CODE_HASH.to_string(),
            hash_type: "type".to_string(),
            args: format!("0x{}", hex::encode(args)),
        }
    }

    fn create_mnft_token_data(characteristic: &[u8; 8], configure: u8, state: u8) -> Vec<u8> {
        let mut data = vec![0u8];
        data.extend_from_slice(characteristic);
        data.push(configure);
        data.push(state);
        data
    }

    fn encode_molecule_table(fields: &[Vec<u8>]) -> Vec<u8> {
        let header_size = 4 + fields.len() * 4;
        let total_size: usize = header_size + fields.iter().map(|f| f.len()).sum::<usize>();
        let mut out = Vec::with_capacity(total_size);
        out.extend_from_slice(&(total_size as u32).to_le_bytes());
        let mut offset = header_size as u32;
        for field in fields {
            out.extend_from_slice(&offset.to_le_bytes());
            offset += field.len() as u32;
        }
        for field in fields {
            out.extend_from_slice(field);
        }
        out
    }

    fn encode_molecule_bytes(payload: &[u8]) -> Vec<u8> {
        let total_size = 4 + payload.len();
        let mut out = Vec::with_capacity(total_size);
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(payload);
        out
    }

    fn encode_molecule_dynvec(items: &[Vec<u8>]) -> Vec<u8> {
        if items.is_empty() {
            return 4u32.to_le_bytes().to_vec();
        }
        let header_size = 4 + items.len() * 4;
        let total_size: usize = header_size + items.iter().map(|item| item.len()).sum::<usize>();
        let mut out = Vec::with_capacity(total_size);
        out.extend_from_slice(&(total_size as u32).to_le_bytes());

        let mut offset = header_size as u32;
        for item in items {
            out.extend_from_slice(&offset.to_le_bytes());
            offset += item.len() as u32;
        }
        for item in items {
            out.extend_from_slice(item);
        }
        out
    }

    fn encode_dotbit_account_cell_witness(account_id: &[u8; 20], account: &str) -> String {
        let mut account_items = Vec::new();
        let account_without_suffix = account.strip_suffix(".bit").unwrap_or(account);
        for ch in account_without_suffix.chars() {
            let char_table = encode_molecule_table(&[
                2u32.to_le_bytes().to_vec(),
                encode_molecule_bytes(ch.to_string().as_bytes()),
            ]);
            account_items.push(char_table);
        }
        let account_chars = encode_molecule_dynvec(&account_items);
        let records_empty = encode_molecule_dynvec(&[]);

        let entity = encode_molecule_table(&[
            account_id.to_vec(),
            account_chars,
            0u64.to_le_bytes().to_vec(),
            0u64.to_le_bytes().to_vec(),
            0u64.to_le_bytes().to_vec(),
            0u64.to_le_bytes().to_vec(),
            vec![0],
            records_empty,
            vec![0],
            0u64.to_le_bytes().to_vec(),
        ]);

        let data_entity = encode_molecule_table(&[
            0u32.to_le_bytes().to_vec(),
            3u32.to_le_bytes().to_vec(),
            encode_molecule_bytes(&entity),
        ]);

        let data = encode_molecule_table(&[Vec::new(), Vec::new(), data_entity]);

        let mut witness = Vec::new();
        witness.extend_from_slice(b"das");
        witness.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
        witness.extend_from_slice(&data);
        format!("0x{}", hex::encode(witness))
    }

    fn encode_das_action_witness(action: &str) -> String {
        let action_bytes = encode_molecule_bytes(action.as_bytes());
        let params_bytes = encode_molecule_bytes(&[]);
        let action_data = encode_molecule_table(&[action_bytes, params_bytes]);

        let mut witness = Vec::new();
        witness.extend_from_slice(b"das");
        witness.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        witness.extend_from_slice(&action_data);
        format!("0x{}", hex::encode(witness))
    }

    fn create_test_tx_data(tx: &TransactionView) -> TxData {
        let parsed_tx = TransactionParser::parse(tx).expect("parse tx");
        TxData {
            hash: parsed_tx.hash,
            block_number: 14_000_000,
            tx_index: 0,
            inputs_count: tx.inputs.len() as i16,
            outputs_count: tx.outputs.len() as i16,
            is_cellbase: parsed_tx.is_cellbase,
            inputs: TransactionParser::parse_inputs(tx).expect("parse inputs"),
            cell_deps: TransactionParser::parse_cell_deps(tx).expect("parse cell deps"),
            cells: CellParser::parse_outputs(tx).expect("parse outputs"),
            witnesses: tx.witnesses.clone(),
            outputs_data: tx.outputs_data.clone(),
            total_input_capacity: 0,
            total_output_capacity: 0,
            fee: 0,
            tx_size: parsed_tx.tx_size,
            cycles: None,
            timestamp: Utc.timestamp_millis_opt(0).single().expect("timestamp"),
        }
    }

    fn create_mixed_nft_tx() -> TransactionView {
        let dotbit_account_id = [0x11u8; 20];
        let issuer_id = [0x22u8; 20];
        let mut class_id = issuer_id.to_vec();
        class_id.extend_from_slice(&7u32.to_le_bytes());

        TransactionView {
            hash: "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            version: "0x0".to_string(),
            cell_deps: Vec::<CellDep>::new(),
            header_deps: Vec::new(),
            inputs: vec![CellInput {
                since: "0x0".to_string(),
                previous_output: OutPoint {
                    tx_hash: "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                        .to_string(),
                    index: "0x0".to_string(),
                },
            }],
            outputs: vec![
                CellOutput {
                    capacity: "0x174876e800".to_string(),
                    lock: create_lock_script(),
                    type_: Some(create_dotbit_account_cell_type_script(&dotbit_account_id)),
                },
                CellOutput {
                    capacity: "0x174876e800".to_string(),
                    lock: create_lock_script(),
                    type_: Some(create_mnft_token_type_script(&class_id, 42)),
                },
            ],
            outputs_data: vec![
                format!(
                    "0x{}",
                    hex::encode(create_dotbit_account_cell_data(&dotbit_account_id))
                ),
                format!(
                    "0x{}",
                    hex::encode(create_mnft_token_data(&[1, 2, 3, 4, 5, 6, 7, 8], 1, 0))
                ),
            ],
            witnesses: vec![
                encode_dotbit_account_cell_witness(&dotbit_account_id, "alice.bit"),
                encode_das_action_witness("transfer_account"),
            ],
        }
    }

    fn create_dotbit_consume_tx(created_tx_hash: &str) -> TransactionView {
        TransactionView {
            hash: "0xcccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".to_string(),
            version: "0x0".to_string(),
            cell_deps: Vec::<CellDep>::new(),
            header_deps: Vec::new(),
            inputs: vec![CellInput {
                since: "0x0".to_string(),
                previous_output: OutPoint {
                    tx_hash: created_tx_hash.to_string(),
                    index: "0x0".to_string(),
                },
            }],
            outputs: vec![CellOutput {
                capacity: "0x174876e800".to_string(),
                lock: create_lock_script(),
                type_: None,
            }],
            outputs_data: vec!["0x".to_string()],
            witnesses: vec![encode_das_action_witness("transfer_account")],
        }
    }

    fn create_test_parsed_block(number: i64, tx_count: i32, timestamp_ms: i64) -> ParsedBlock {
        ParsedBlock {
            number,
            hash: vec![0x11; 32],
            parent_hash: vec![0x22; 32],
            timestamp: Utc
                .timestamp_millis_opt(timestamp_ms)
                .single()
                .expect("timestamp"),
            version: 0,
            compact_target: 0,
            transactions_count: tx_count,
            proposals_count: 0,
            uncles_count: 0,
            epoch_number: 0,
            epoch_index: 0,
            epoch_length: 0,
            dao: vec![0; 32],
            nonce: vec![0; 16],
            extra_hash: vec![0; 32],
            proposals_hash: vec![0; 32],
            transactions_root: vec![0; 32],
            proposals: Vec::new(),
        }
    }

    #[test]
    fn test_parser_precompute_phase_metrics_total_ms_sums_all_phases() {
        let metrics = super::ParserPrecomputePhaseMetrics {
            build_batch_cell_infos_ms: 10.0,
            compute_fee_ms: 20.0,
            cache_balance_and_script_ms: 30.0,
            spore_precompute_ms: 40.0,
            nft_precompute_ms: 50.0,
        };

        assert!((metrics.total_ms() - 150.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_scan_preparsed_nft_tx_single_pass_collects_mnft_and_dotbit_outputs() {
        let tx = create_mixed_nft_tx();
        let tx_data = create_test_tx_data(&tx);
        let witness_bundle = parse_dotbit_witness_bundle(&tx.witnesses);

        let scanned = scan_preparsed_nft_tx(&tx_data, &witness_bundle).expect("scan");

        assert_eq!(scanned.mnft_tokens.len(), 1);
        assert_eq!(scanned.mnft_tokens[0].0, 1);
        assert_eq!(scanned.dotbit_accounts.len(), 1);
        assert_eq!(scanned.dotbit_accounts[0].output_index, 0);
        assert_eq!(
            scanned.dotbit_accounts[0].account.account.as_deref(),
            Some("alice.bit")
        );
        assert_eq!(scanned.dotbit_action.as_deref(), Some("transfer_account"));
    }

    #[test]
    fn test_run_nft_precompute_single_pass_preserves_preparsed_bridge_shape() {
        let create_tx = create_mixed_nft_tx();
        let consume_tx = create_dotbit_consume_tx(&create_tx.hash);
        let all_tx_data = vec![
            create_test_tx_data(&create_tx),
            create_test_tx_data(&consume_tx),
        ];
        let parsed_blocks = vec![create_test_parsed_block(14_000_000, 2, 0)];

        let output = run_nft_precompute(
            &parsed_blocks,
            &all_tx_data,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        )
        .expect("precompute");

        assert_eq!(output.mnft_tokens.len(), 1);
        assert_eq!(output.dotbit_accounts.len(), 1);
        assert_eq!(output.consumed_dotbit.len(), 1);
        assert_eq!(
            output
                .dotbit_tx_actions
                .get(&0)
                .map(std::string::String::as_str),
            Some("transfer_account")
        );
    }

    /// Regression test: cross-batch consume of a .bit cell whose input_cell_info
    /// has empty type_args (historical DAS cells stored account_id only in cell
    /// data, not in type_args).  The consume must still be detected.
    #[test]
    fn test_run_nft_precompute_cross_batch_dotbit_consume_empty_type_args() {
        let dotbit_account_id = [0x11u8; 20];
        let dotbit_code_hash = crate::rpc::parse_hex_to_bytes(DOTBIT_ACCOUNT_CELL_TYPE_ID);

        // TX that consumes a dotbit cell created in a previous batch.
        // The consuming TX has no dotbit outputs (pure recycle).
        let prev_tx_hash_hex = "0xdddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
        let prev_tx_hash = crate::rpc::parse_hex_to_bytes(prev_tx_hash_hex);
        let consume_tx = TransactionView {
            hash: "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee".to_string(),
            version: "0x0".to_string(),
            cell_deps: Vec::<CellDep>::new(),
            header_deps: Vec::new(),
            inputs: vec![CellInput {
                since: "0x0".to_string(),
                previous_output: OutPoint {
                    tx_hash: prev_tx_hash_hex.to_string(),
                    index: "0x0".to_string(),
                },
            }],
            outputs: vec![CellOutput {
                capacity: "0x174876e800".to_string(),
                lock: create_lock_script(),
                type_: None,
            }],
            outputs_data: vec!["0x".to_string()],
            witnesses: vec![],
        };
        let all_tx_data = vec![create_test_tx_data(&consume_tx)];
        let parsed_blocks = vec![create_test_parsed_block(14_000_100, 1, 0)];

        // Simulate cross-batch input_cell_info: the consumed cell IS a dotbit
        // cell (type_code_hash matches) but has EMPTY type_args (historical cell).
        let mut input_cell_info: HashMap<(Vec<u8>, i16), PositionedCellInfo> = HashMap::new();
        input_cell_info.insert(
            (prev_tx_hash.clone(), 0),
            PositionedCellInfo::new(
                LiveCellInfo {
                    capacity: 100_00000000,
                    lock_script_hash: vec![0xAA; 32],
                    lock_code_hash: vec![0xBB; 32],
                    lock_hash_type: 1,
                    lock_args: vec![0xCC; 20],
                    type_script_hash: Some(vec![0xDD; 32]),
                    type_code_hash: Some(dotbit_code_hash),
                    type_hash_type: Some(1),
                    type_args: Some(vec![]), // EMPTY — historical cell
                    data_size: 80,
                    occupied_capacity: 61_00000000,
                    udt_amount: None,
                    data_hash: None,
                },
                14_000_000,
            ),
        );

        // Simulate the DB fallback: the outpoint → account_id mapping that
        // was written when the cell was first indexed as an output.
        let mut dotbit_outpoint_fallback: HashMap<(Vec<u8>, i16), Vec<u8>> = HashMap::new();
        dotbit_outpoint_fallback.insert((prev_tx_hash.clone(), 0), dotbit_account_id.to_vec());

        let output = run_nft_precompute(
            &parsed_blocks,
            &all_tx_data,
            &input_cell_info,
            &HashMap::new(),
            &dotbit_outpoint_fallback,
        )
        .expect("precompute");

        // The consume should be detected even though type_args is empty.
        // The account_id should be resolved via the fallback map.
        assert_eq!(
            output.consumed_dotbit.len(),
            1,
            "cross-batch dotbit consume with empty type_args must be detected"
        );
        assert_eq!(
            output.consumed_dotbit[0].account_id,
            dotbit_account_id.to_vec()
        );
    }

    // ── Helpers for spore/mNFT consumption tests ──────────────────────

    fn make_cellbase_tx_data(block_number: i64) -> TxData {
        TxData {
            hash: [0x00; 32],
            block_number,
            tx_index: 0,
            inputs_count: 1,
            outputs_count: 1,
            is_cellbase: true,
            inputs: vec![],
            cell_deps: vec![],
            cells: vec![],
            witnesses: vec![],
            outputs_data: vec![],
            total_input_capacity: 0,
            total_output_capacity: 0,
            fee: 0,
            tx_size: 0,
            cycles: None,
            timestamp: Utc.timestamp_millis_opt(0).single().expect("timestamp"),
        }
    }

    fn make_consuming_tx_data(
        tx_hash: [u8; 32],
        block_number: i64,
        prev_tx_hash: [u8; 32],
        prev_output_index: i32,
    ) -> TxData {
        use crate::parser::transaction::ParsedInput;
        TxData {
            hash: tx_hash,
            block_number,
            tx_index: 0,
            inputs_count: 1,
            outputs_count: 0,
            is_cellbase: false,
            inputs: vec![ParsedInput {
                previous_tx_hash: prev_tx_hash,
                previous_output_index: prev_output_index,
                since: 0,
            }],
            cell_deps: vec![],
            cells: vec![],
            witnesses: vec![],
            outputs_data: vec![],
            total_input_capacity: 0,
            total_output_capacity: 0,
            fee: 0,
            tx_size: 0,
            cycles: None,
            timestamp: Utc.timestamp_millis_opt(0).single().expect("timestamp"),
        }
    }

    fn make_creating_tx_data(
        tx_hash: [u8; 32],
        block_number: i64,
        type_code_hash: Vec<u8>,
        type_args: Vec<u8>,
    ) -> TxData {
        use crate::parser::cell::ParsedCell;
        TxData {
            hash: tx_hash,
            block_number,
            tx_index: 0,
            inputs_count: 0,
            outputs_count: 1,
            is_cellbase: false,
            inputs: vec![],
            cell_deps: vec![],
            cells: vec![ParsedCell {
                capacity: 100_00000000,
                lock_code_hash: vec![0xBB; 32],
                lock_hash_type: 1,
                lock_args: vec![0xCC; 20],
                lock_script_hash: vec![0xDD; 32],
                type_code_hash: Some(type_code_hash),
                type_hash_type: Some(1),
                type_args: Some(type_args),
                type_script_hash: Some(vec![0xEE; 32]),
                data_hash: vec![0xFF; 32],
                data_size: 0,
                data: vec![],
            }],
            witnesses: vec![],
            outputs_data: vec![],
            total_input_capacity: 0,
            total_output_capacity: 0,
            fee: 0,
            tx_size: 0,
            cycles: None,
            timestamp: Utc.timestamp_millis_opt(0).single().expect("timestamp"),
        }
    }

    fn make_positioned_cell_info(
        type_code_hash: Vec<u8>,
        type_args: Vec<u8>,
    ) -> PositionedCellInfo {
        PositionedCellInfo::new(
            LiveCellInfo {
                capacity: 100_00000000,
                lock_script_hash: vec![0xAA; 32],
                lock_code_hash: vec![0xBB; 32],
                lock_hash_type: 1,
                lock_args: vec![0xCC; 20],
                type_script_hash: Some(vec![0xDD; 32]),
                type_code_hash: Some(type_code_hash),
                type_hash_type: Some(1),
                type_args: Some(type_args),
                data_size: 0,
                occupied_capacity: 61_00000000,
                udt_amount: None,
                data_hash: None,
            },
            14_000_000,
        )
    }

    // ── Spore/mNFT consumption identification tests ───────────────────

    #[test]
    fn test_precompute_identifies_spore_consumption() {
        use crate::parser::spore::SPORE_CODE_HASH_MAINNET_V2;

        let spore_code_hash = crate::rpc::parse_hex_to_bytes(SPORE_CODE_HASH_MAINNET_V2);
        let spore_id = vec![0x55u8; 32];
        let prev_tx_hash = [0x11u8; 32];

        // 1 block, 2 transactions: TX0=cellbase, TX1=consume spore
        let all_tx_data = vec![
            make_cellbase_tx_data(14_000_000),
            make_consuming_tx_data([0xAA; 32], 14_000_000, prev_tx_hash, 0),
        ];
        let parsed_blocks = vec![create_test_parsed_block(14_000_000, 2, 0)];

        // The consumed cell is a spore cell from a previous batch
        let mut input_cell_info: HashMap<(Vec<u8>, i16), PositionedCellInfo> = HashMap::new();
        input_cell_info.insert(
            (prev_tx_hash.to_vec(), 0),
            make_positioned_cell_info(spore_code_hash, spore_id.clone()),
        );

        let output = run_nft_precompute(
            &parsed_blocks,
            &all_tx_data,
            &input_cell_info,
            &HashMap::new(),
            &HashMap::new(),
        )
        .expect("precompute");

        assert_eq!(
            output.consumed_spore.len(),
            1,
            "should identify one spore consumption"
        );
        assert_eq!(output.consumed_spore[0].spore_id, spore_id);
        assert_eq!(output.consumed_spore[0].block_number, 14_000_000);
        assert_eq!(output.consumed_spore[0].consuming_tx_hash, [0xAA; 32]);
    }

    #[test]
    fn test_precompute_identifies_mnft_consumption() {
        let mnft_code_hash = crate::rpc::parse_hex_to_bytes(MNFT_TOKEN_CODE_HASH);
        // mNFT token_id: 20-byte issuer_id + 4-byte class_id + 4-byte token_index = 28 bytes
        let mut token_id = vec![0x22u8; 20];
        token_id.extend_from_slice(&7u32.to_le_bytes());
        token_id.extend_from_slice(&42u32.to_le_bytes());
        let prev_tx_hash = [0x11u8; 32];

        // 1 block, 2 transactions: TX0=cellbase, TX1=consume mNFT
        let all_tx_data = vec![
            make_cellbase_tx_data(14_000_000),
            make_consuming_tx_data([0xBB; 32], 14_000_000, prev_tx_hash, 0),
        ];
        let parsed_blocks = vec![create_test_parsed_block(14_000_000, 2, 0)];

        // The consumed cell is an mNFT token cell from a previous batch
        let mut input_cell_info: HashMap<(Vec<u8>, i16), PositionedCellInfo> = HashMap::new();
        input_cell_info.insert(
            (prev_tx_hash.to_vec(), 0),
            make_positioned_cell_info(mnft_code_hash, token_id.clone()),
        );

        let output = run_nft_precompute(
            &parsed_blocks,
            &all_tx_data,
            &input_cell_info,
            &HashMap::new(),
            &HashMap::new(),
        )
        .expect("precompute");

        assert_eq!(
            output.consumed_mnft.len(),
            1,
            "should identify one mNFT consumption"
        );
        assert_eq!(output.consumed_mnft[0].token_id, token_id);
        assert_eq!(output.consumed_mnft[0].block_number, 14_000_000);
        assert_eq!(output.consumed_mnft[0].consuming_tx_hash, [0xBB; 32]);
    }

    #[test]
    fn test_precompute_same_batch_spore_transfer_not_consumed() {
        use crate::parser::spore::SPORE_CODE_HASH_MAINNET_V2;

        let spore_code_hash = crate::rpc::parse_hex_to_bytes(SPORE_CODE_HASH_MAINNET_V2);
        let spore_id = vec![0x55u8; 32];
        let prev_tx_hash = [0x11u8; 32];

        // 1 block, 3 transactions:
        //   TX0 (index 0) = cellbase
        //   TX1 (index 1) = consumes a spore cell
        //   TX2 (index 2) = creates a new cell with the SAME spore_id (transfer/re-creation)
        let all_tx_data = vec![
            make_cellbase_tx_data(14_000_000),
            make_consuming_tx_data([0xAA; 32], 14_000_000, prev_tx_hash, 0),
            make_creating_tx_data(
                [0xCC; 32],
                14_000_000,
                spore_code_hash.clone(),
                spore_id.clone(),
            ),
        ];
        let parsed_blocks = vec![create_test_parsed_block(14_000_000, 3, 0)];

        // The consumed cell from a previous batch
        let mut input_cell_info: HashMap<(Vec<u8>, i16), PositionedCellInfo> = HashMap::new();
        input_cell_info.insert(
            (prev_tx_hash.to_vec(), 0),
            make_positioned_cell_info(spore_code_hash, spore_id.clone()),
        );

        let output = run_nft_precompute(
            &parsed_blocks,
            &all_tx_data,
            &input_cell_info,
            &HashMap::new(),
            &HashMap::new(),
        )
        .expect("precompute");

        // The spore was transferred (re-created in TX2 after being consumed in TX1),
        // so it should NOT be marked as consumed.
        assert!(
            output.consumed_spore.is_empty(),
            "spore transferred within same batch must NOT be marked as consumed, got {} events",
            output.consumed_spore.len()
        );
    }

    #[test]
    fn test_precompute_same_batch_mnft_transfer_not_consumed() {
        let mnft_code_hash = crate::rpc::parse_hex_to_bytes(MNFT_TOKEN_CODE_HASH);
        let mut token_id = vec![0x22u8; 20];
        token_id.extend_from_slice(&7u32.to_le_bytes());
        token_id.extend_from_slice(&42u32.to_le_bytes());
        let prev_tx_hash = [0x11u8; 32];

        // 1 block, 3 transactions:
        //   TX0 (index 0) = cellbase
        //   TX1 (index 1) = consumes an mNFT token cell
        //   TX2 (index 2) = creates a new cell with the SAME token_id (transfer)
        let all_tx_data = vec![
            make_cellbase_tx_data(14_000_000),
            make_consuming_tx_data([0xBB; 32], 14_000_000, prev_tx_hash, 0),
            make_creating_tx_data(
                [0xDD; 32],
                14_000_000,
                mnft_code_hash.clone(),
                token_id.clone(),
            ),
        ];
        let parsed_blocks = vec![create_test_parsed_block(14_000_000, 3, 0)];

        // The consumed cell from a previous batch
        let mut input_cell_info: HashMap<(Vec<u8>, i16), PositionedCellInfo> = HashMap::new();
        input_cell_info.insert(
            (prev_tx_hash.to_vec(), 0),
            make_positioned_cell_info(mnft_code_hash, token_id.clone()),
        );

        let output = run_nft_precompute(
            &parsed_blocks,
            &all_tx_data,
            &input_cell_info,
            &HashMap::new(),
            &HashMap::new(),
        )
        .expect("precompute");

        // The mNFT was transferred (re-created in TX2 after being consumed in TX1),
        // so it should NOT be marked as consumed.
        assert!(
            output.consumed_mnft.is_empty(),
            "mNFT transferred within same batch must NOT be marked as consumed, got {} events",
            output.consumed_mnft.len()
        );
    }
}
