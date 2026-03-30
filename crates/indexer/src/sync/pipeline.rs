#![allow(clippy::type_complexity)]

use std::collections::{HashMap, HashSet};

/// Bounded channel capacity for the live sync pipeline (Fetcher → Parser → Writer).
const PIPELINE_CHANNEL_CAPACITY: usize = 16;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

use ckbadger_store::types::{LiveCellInfo, MnftTypeIndex, PositionedCellInfo, SporeTypeIndex};

use crate::parser::block::BlockParser;
use crate::parser::cell::{CellParser, ParsedCell};
use crate::parser::dao::{DaoParser, DaoState};
use crate::parser::transaction::TransactionParser;
use crate::parser::udt::UdtStandard;
use crate::parser::{
    dotbit::{may_contain_das_witness, parse_dotbit_witness_bundle, DotbitWitnessBundle},
    DotbitParser, MnftParser, SporeParser, UdtParser,
};
use crate::rpc::BlockResponseWithCycles;
use ckbadger_store::types::{DOTBIT_SENTINEL_COLLECTION, SOLE_SPORES_SENTINEL_COLLECTION};

use ckb_store_reader::CkbChainReader;
use rayon::prelude::*;

use super::adaptive::*;
use super::batch::*;
use super::bulk_build::facts::{
    BlockFacts, CellFacts, CellSemanticTag, DaoCellState, FactsArena, OutPointKey, TxFacts,
};
use super::bulk_build::interner::IdentityInterner;
use super::dao_helpers::*;
use super::diagnostics::*;
use super::helpers::*;
use super::indexer::{
    blocks_behind_tip, next_start_block_from_db_tip, require_non_negative_block_number, Indexer,
};
use super::sync_mode::*;
use super::token_helpers::*;
use super::types::{AddressBalanceDelta, CachedCellInfo, ReorgAction, TxData};
use super::undo::*;
use crate::bulk_sync_perf::BatchSample;

#[derive(Debug, Default, Clone, Copy)]
struct ParserPrecomputePhaseMetrics {
    build_batch_cell_infos_ms: f64,
    compute_fee_ms: f64,
    cache_balance_and_script_ms: f64,
}

impl ParserPrecomputePhaseMetrics {
    fn total_ms(&self) -> f64 {
        self.build_batch_cell_infos_ms + self.compute_fee_ms + self.cache_balance_and_script_ms
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct ParserBatchPerfSample {
    parse_ms: f64,
    precompute_ms: f64,
}

fn classify_bulk_cell_semantic_tag(cell: &ParsedCell) -> CellSemanticTag {
    let Some(type_code_hash) = cell.type_code_hash.as_deref() else {
        return CellSemanticTag::Plain;
    };

    if DaoParser::is_dao_code_hash(type_code_hash) {
        return CellSemanticTag::Dao;
    }

    if let Some(hash_type) = cell.type_hash_type {
        if let Some(standard) = UdtParser::is_udt_code_hash_bytes(type_code_hash, hash_type) {
            return match standard {
                UdtStandard::Sudt => CellSemanticTag::Sudt,
                UdtStandard::Xudt => CellSemanticTag::Xudt,
            };
        }
    }

    if DotbitParser::is_account_cell_type_script(type_code_hash) {
        return CellSemanticTag::Dotbit;
    }

    if MnftParser::is_issuer_type_script(type_code_hash)
        || MnftParser::is_class_type_script(type_code_hash)
        || MnftParser::is_token_type_script(type_code_hash)
    {
        return CellSemanticTag::Mnft;
    }

    if SporeParser::is_cluster_type_script(type_code_hash) {
        return CellSemanticTag::Cluster;
    }

    if SporeParser::is_spore_type_script(type_code_hash) {
        return CellSemanticTag::Spore;
    }

    CellSemanticTag::Plain
}

fn parse_bulk_dao_cell_state(
    cell: &ParsedCell,
    semantic_tag: CellSemanticTag,
    tx_hash: &[u8; 32],
    output_index: i16,
) -> Result<Option<DaoCellState>> {
    if !matches!(semantic_tag, CellSemanticTag::Dao) {
        return Ok(None);
    }

    let state = DaoParser::parse_dao_state(&cell.data).ok_or_else(|| {
        anyhow!(
            "invalid DAO cell data in bulk facts: tx=0x{}, output_index={}, data_len={}",
            hex::encode(tx_hash),
            output_index,
            cell.data.len()
        )
    })?;

    Ok(Some(match state {
        DaoState::Deposit => DaoCellState::Deposit,
        DaoState::WithdrawRequest => {
            let deposit_block_number =
                DaoParser::parse_deposit_block_number(&cell.data).ok_or_else(|| {
                    anyhow!(
                        "missing DAO deposit block number in withdraw request: tx=0x{}, output_index={}, data_len={}",
                        hex::encode(tx_hash),
                        output_index,
                        cell.data.len()
                    )
                })?;
            DaoCellState::WithdrawRequest {
                deposit_block_number: i64::try_from(deposit_block_number).map_err(|_| {
                    anyhow!(
                        "DAO deposit block number exceeds i64 range in bulk facts: tx=0x{}, output_index={}, deposit_block_number={}",
                        hex::encode(tx_hash),
                        output_index,
                        deposit_block_number
                    )
                })?,
            }
        }
    }))
}

use super::bulk_build::facts::{parse_fixed_protocol_id, parse_protocol_facts};

pub(crate) fn build_bulk_facts_arena_from_blocks(
    blocks: &[BlockResponseWithCycles],
    interner: &IdentityInterner,
) -> Result<(
    FactsArena,
    super::bulk_build::binary_facts::FactsTimingBreakdown,
)> {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Instant;

    // Single O(n) pre-scan for cell counts, shared between fast-path
    // threshold check and parallel chunking.
    let cell_counts: Vec<usize> = blocks
        .iter()
        .map(|b| b.block.transactions.iter().map(|tx| tx.outputs.len()).sum())
        .collect();
    let total_cells_estimate: usize = cell_counts.iter().sum();

    // Serial fast-path: when total cells are small, rayon overhead exceeds
    // the parallelism benefit. Threshold from perf data: batches under ~50K
    // cells averaged 0.30x speedup (3.3x slowdown) with par_iter.
    if total_cells_estimate < 50_000 {
        let start = Instant::now();
        let mut arena = FactsArena::default();
        let mut cell_count: u64 = 0;

        for block in blocks {
            let (block_facts, txs, cells) = parse_single_block(block, interner)?;
            let tx_start = arena.txs.len();
            let cell_start = arena.cells.len();
            cell_count += cells.len() as u64;

            for mut tx in txs {
                tx.output_range =
                    (cell_start + tx.output_range.start)..(cell_start + tx.output_range.end);
                arena.txs.push(tx);
            }
            arena.cells.extend(cells);

            let tx_end = arena.txs.len();
            let mut block_f = block_facts;
            block_f.tx_range = tx_start..tx_end;
            arena.blocks.push(block_f);
        }

        let elapsed = start.elapsed();
        let elapsed_ms = elapsed.as_secs_f64() * 1000.0;
        let (intern_total, intern_slow) = interner.drain_counters();
        return Ok((
            arena,
            super::bulk_build::binary_facts::FactsTimingBreakdown {
                par_iter_ms: elapsed_ms,
                merge_ms: 0.0,
                serial_equivalent_ms: elapsed_ms,
                intern_slow_path_count: intern_slow,
                intern_total_count: intern_total,
                cell_count,
            },
        ));
    }

    // Parallel path: atomic counters only needed here.
    let serial_equivalent_us = AtomicU64::new(0);
    let total_cells = AtomicU64::new(0);

    // Cell-weighted chunks: delegate to shared implementation.
    let par_start = Instant::now();
    let chunk_ranges = super::bulk_build::binary_facts::compute_cell_weighted_chunks_from_counts(
        blocks.len(),
        &cell_counts,
        total_cells_estimate,
    );
    let chunk_results: Vec<Result<FactsArena>> = chunk_ranges
        .par_iter()
        .map(|&(start, end)| {
            let chunk = &blocks[start..end];
            let chunk_start = Instant::now();
            let mut sub_arena = FactsArena::default();
            let mut chunk_cells: u64 = 0;

            for block in chunk {
                let (block_facts, txs, cells) = parse_single_block(block, interner)?;
                let tx_start = sub_arena.txs.len();
                let cell_start = sub_arena.cells.len();
                chunk_cells += cells.len() as u64;

                for mut tx in txs {
                    tx.output_range =
                        (cell_start + tx.output_range.start)..(cell_start + tx.output_range.end);
                    sub_arena.txs.push(tx);
                }
                sub_arena.cells.extend(cells);

                let tx_end = sub_arena.txs.len();
                let mut block_f = block_facts;
                block_f.tx_range = tx_start..tx_end;
                sub_arena.blocks.push(block_f);
            }

            serial_equivalent_us
                .fetch_add(chunk_start.elapsed().as_micros() as u64, Ordering::Relaxed);
            total_cells.fetch_add(chunk_cells, Ordering::Relaxed);
            Ok(sub_arena)
        })
        .collect();
    let par_elapsed = par_start.elapsed();

    // Merge sub-arenas sequentially, remapping tx/cell offsets.
    let merge_start = Instant::now();
    let mut arena = FactsArena::default();
    for result in chunk_results {
        let sub = result?;
        let tx_offset = arena.txs.len();
        let cell_offset = arena.cells.len();

        for mut block in sub.blocks {
            block.tx_range = (tx_offset + block.tx_range.start)..(tx_offset + block.tx_range.end);
            arena.blocks.push(block);
        }

        for mut tx in sub.txs {
            tx.output_range =
                (cell_offset + tx.output_range.start)..(cell_offset + tx.output_range.end);
            arena.txs.push(tx);
        }

        arena.cells.extend(sub.cells);
    }
    let merge_elapsed = merge_start.elapsed();

    let (intern_total, intern_slow) = interner.drain_counters();

    let breakdown = super::bulk_build::binary_facts::FactsTimingBreakdown {
        par_iter_ms: par_elapsed.as_secs_f64() * 1000.0,
        merge_ms: merge_elapsed.as_secs_f64() * 1000.0,
        serial_equivalent_ms: serial_equivalent_us.load(Ordering::Relaxed) as f64 / 1000.0,
        intern_slow_path_count: intern_slow,
        intern_total_count: intern_total,
        cell_count: total_cells.load(Ordering::Relaxed),
    };

    Ok((arena, breakdown))
}

/// Parse a single block into local facts with output ranges starting at 0.
fn parse_single_block(
    block: &BlockResponseWithCycles,
    interner: &IdentityInterner,
) -> Result<(BlockFacts, Vec<TxFacts>, Vec<CellFacts>)> {
    let parsed_block = BlockParser::parse(&block.block)?;
    let block_hash =
        parse_fixed_protocol_id::<32>(&parsed_block.hash, "block_hash", &[0u8; 32], -1)?;
    let timestamp_ms = parsed_block.timestamp.timestamp_millis();
    let block_dao_ar =
        DaoParser::extract_ar_from_dao_field(&parsed_block.dao).ok_or_else(|| {
            anyhow!(
                "failed to extract block DAO AR in bulk facts: block={}, dao_len={}",
                parsed_block.number,
                parsed_block.dao.len()
            )
        })?;

    let mut local_txs = Vec::with_capacity(block.block.transactions.len());
    let mut local_cells = Vec::new();

    for (tx_position, tx) in block.block.transactions.iter().enumerate() {
        crate::parser::validate_outputs_data_len(&tx.outputs, &tx.outputs_data, &tx.hash);
        let parsed_tx = TransactionParser::parse(tx)?;
        let parsed_inputs = TransactionParser::parse_inputs(tx)?;
        let parsed_cells = CellParser::parse_outputs(tx)?;
        let witness_bundle = if may_contain_das_witness(&tx.witnesses) {
            parse_dotbit_witness_bundle(&tx.witnesses)
        } else {
            DotbitWitnessBundle::default()
        };
        let tx_index = i32::try_from(tx_position).map_err(|_| {
            anyhow!(
                "bulk facts tx index exceeds i32 range: block={} tx_position={}",
                parsed_block.number,
                tx_position
            )
        })?;
        let inputs_count = i16::try_from(parsed_tx.inputs_count).map_err(|_| {
            anyhow!(
                "bulk facts inputs_count exceeds i16 range: block={} tx=0x{} tx_index={} inputs_count={}",
                parsed_block.number,
                hex::encode(parsed_tx.hash),
                tx_index,
                parsed_tx.inputs_count
            )
        })?;
        let outputs_count = i16::try_from(parsed_tx.outputs_count).map_err(|_| {
            anyhow!(
                "bulk facts outputs_count exceeds i16 range: block={} tx=0x{} tx_index={} outputs_count={}",
                parsed_block.number,
                hex::encode(parsed_tx.hash),
                tx_index,
                parsed_tx.outputs_count
            )
        })?;
        let cycles =
            parse_bulk_tx_cycles(block, tx_position, parsed_block.number, &parsed_tx.hash)?;
        let output_start = local_cells.len();
        let input_outpoints = if parsed_tx.is_cellbase {
            Vec::new()
        } else {
            parsed_inputs
                .iter()
                .enumerate()
                .map(|(input_position, input)| {
                    let output_index =
                        u32::try_from(input.previous_output_index).map_err(|_| {
                            anyhow!(
                                "negative previous_output_index in bulk facts: block={} tx=0x{} tx_index={} input_index={} previous_output_index={}",
                                parsed_block.number,
                                hex::encode(parsed_tx.hash),
                                tx_index,
                                input_position,
                                input.previous_output_index
                            )
                        })?;
                    Ok(OutPointKey::new(input.previous_tx_hash, output_index))
                })
                .collect::<Result<Vec<_>>>()?
        };

        for (output_index, cell) in parsed_cells.iter().enumerate() {
            let output_index_i16 =
                checked_usize_to_i16(output_index, "bulk facts arena output index")?;
            let semantic_tag = classify_bulk_cell_semantic_tag(cell);
            local_cells.push(CellFacts {
                outpoint: OutPointKey::new(
                    parsed_tx.hash,
                    u32::try_from(output_index).unwrap_or_else(|_| {
                        panic!(
                            "bulk facts arena output index {} exceeds u32::MAX",
                            output_index
                        )
                    }),
                ),
                created_at_block: parsed_block.number,
                created_by_block_dao_ar: block_dao_ar,
                capacity: cell.capacity,
                lock_script_hash_id: interner.intern_bytes(cell.lock_script_hash.clone()),
                lock_code_hash_id: interner.intern_bytes(cell.lock_code_hash.clone()),
                lock_hash_type: cell.lock_hash_type,
                lock_args_id: interner.intern_bytes(cell.lock_args.clone()),
                type_script_hash_id: cell
                    .type_script_hash
                    .clone()
                    .map(|value| interner.intern_bytes(value)),
                type_code_hash_id: cell
                    .type_code_hash
                    .clone()
                    .map(|value| interner.intern_bytes(value)),
                type_hash_type: cell.type_hash_type,
                type_args_id: cell
                    .type_args
                    .clone()
                    .map(|value| interner.intern_bytes(value)),
                occupied_capacity: occupied_capacity_shannons_i64(
                    cell.lock_args.len(),
                    cell.type_args.as_ref().map(|args| args.len()),
                    cell.data_size,
                ),
                data_size: cell.data_size,
                data: cell.data.clone(),
                data_hash: Some(cell.data_hash),
                udt_amount: parse_parsed_cell_udt_amount(
                    cell,
                    &parsed_tx.hash,
                    output_index_i16,
                    None,
                )?,
                semantic_tag,
                dao_state: parse_bulk_dao_cell_state(
                    cell,
                    semantic_tag,
                    &parsed_tx.hash,
                    output_index_i16,
                )?,
                protocol_facts: parse_protocol_facts(
                    cell,
                    semantic_tag,
                    &witness_bundle,
                    &parsed_tx.hash,
                    output_index_i16,
                )?,
            });
        }

        local_txs.push(TxFacts {
            hash: parsed_tx.hash,
            block_number: parsed_block.number,
            block_hash,
            timestamp_ms,
            block_dao_ar,
            tx_index,
            is_cellbase: parsed_tx.is_cellbase,
            inputs_count,
            outputs_count,
            tx_size: parsed_tx.tx_size,
            cycles,
            dotbit_action: witness_bundle.action.clone(),
            input_outpoints,
            output_range: output_start..local_cells.len(),
        });
    }

    let parent_hash: [u8; 32] = parsed_block
        .parent_hash
        .as_slice()
        .try_into()
        .map_err(|_| {
            anyhow!(
                "parent_hash length mismatch: block={} len={}",
                parsed_block.number,
                parsed_block.parent_hash.len()
            )
        })?;
    let block_facts = BlockFacts {
        number: parsed_block.number,
        hash: block_hash,
        parent_hash,
        timestamp_ms,
        epoch_number: parsed_block.epoch_number,
        epoch_index: parsed_block.epoch_index,
        epoch_length: parsed_block.epoch_length,
        dao: parsed_block.dao,
        compact_target: u32::try_from(parsed_block.compact_target).map_err(|_| {
            anyhow!(
                "compact_target exceeds u32 range: block={} compact_target={}",
                parsed_block.number,
                parsed_block.compact_target
            )
        })?,
        uncles_count: parsed_block.uncles_count,
        transactions_count: parsed_block.transactions_count,
        // Placeholder tx_range; remapped in the merge phase.
        tx_range: 0..local_txs.len(),
    };

    Ok((block_facts, local_txs, local_cells))
}

fn parse_bulk_tx_cycles(
    block: &BlockResponseWithCycles,
    tx_position: usize,
    block_number: i64,
    tx_hash: &[u8; 32],
) -> Result<Option<i64>> {
    let Some(cycles) = block.cycles.as_ref() else {
        return Ok(None);
    };

    // Cellbase (tx_position 0) has no cycles — CKB never runs VM on cellbase
    if tx_position == 0 {
        return Ok(None);
    }

    // CKB returns cycles for non-cellbase transactions only, so
    // cycles.len() == block.block.transactions.len() - 1
    let expected_len = block.block.transactions.len().saturating_sub(1);
    if cycles.len() != expected_len {
        return Err(anyhow!(
            "bulk facts cycles length mismatch: block={} tx_count={} expected_cycles={} actual_cycles={}",
            block_number,
            block.block.transactions.len(),
            expected_len,
            cycles.len()
        ));
    }

    // cycles[0] corresponds to tx_position 1 (first non-cellbase tx)
    let cycles_index = tx_position - 1;
    let raw_cycles = cycles.get(cycles_index).ok_or_else(|| {
        anyhow!(
            "bulk facts cycles missing tx position: block={} tx=0x{} tx_position={} cycles_index={} cycles_count={}",
            block_number,
            hex::encode(tx_hash),
            tx_position,
            cycles_index,
            cycles.len()
        )
    })?;
    let parsed_cycles = parse_prefixed_hex_u64(raw_cycles).map_err(|e| {
        anyhow!(
            "invalid cycles hex in bulk facts: block={} tx=0x{} tx_position={} raw='{}' error={}",
            block_number,
            hex::encode(tx_hash),
            tx_position,
            raw_cycles,
            e
        )
    })?;
    let cycles_i64 = i64::try_from(parsed_cycles).map_err(|_| {
        anyhow!(
            "bulk facts cycles exceed i64 range: block={} tx=0x{} tx_position={} cycles={}",
            block_number,
            hex::encode(tx_hash),
            tx_position,
            parsed_cycles
        )
    })?;
    Ok(Some(cycles_i64))
}

fn parser_cache_committed_tip_from_sync_tip(sync_tip: i64) -> i64 {
    sync_tip
        .checked_sub(1)
        .expect("sync tip underflow while deriving parser cache committed tip")
}

fn parse_script_reference_hash_type(
    hash_type: i16,
    script_kind: &str,
    block_number: i64,
    tx_hash: &[u8; 32],
) -> Result<u8> {
    match hash_type {
        0 | 1 | 2 | 4 => Ok(hash_type as u8),
        _ => Err(anyhow!(
            "invalid {} script reference hash_type in pipeline cache pass: block={}, tx=0x{}, hash_type={}, expected_one_of=[0,1,2,4]",
            script_kind,
            block_number,
            hex::encode(tx_hash),
            hash_type
        )),
    }
}

impl Indexer {
    pub(crate) async fn run_pipeline(&self) -> Result<()> {
        use tokio::sync::mpsc;

        type FetchedBatch = (u64, u64, u64, u64, Arc<Vec<BlockResponseWithCycles>>);

        struct ParsedBatch {
            batch_epoch: u64,
            start_block: u64,
            end_block: u64,
            chain_tip: u64,
            batch_tx_count: u64,
            blocks: Arc<Vec<BlockResponseWithCycles>>,
            parsed_blocks: Vec<crate::parser::block::ParsedBlock>,
            all_tx_data: Vec<TxData>,
            input_cell_info: HashMap<(Vec<u8>, i16), PositionedCellInfo>,
            batch_cell_infos: HashMap<(Vec<u8>, i16), PositionedCellInfo>,
            address_balance_changes: HashMap<Vec<u8>, AddressBalanceDelta>,
            script_usage_changes: ScriptUsageChanges,
            script_reference_usage_changes: ScriptReferenceUsageChanges,
            script_daily_changes: HashMap<(Vec<u8>, bool, u32), (i128, i128)>,
            token_daily_changes: HashMap<(Vec<u8>, u32), (i128, i128)>,
            spore_type_index_changes: HashMap<Vec<u8>, SporeTypeIndex>,
            spore_daily_changes: HashMap<(Vec<u8>, u32), (i128, i128)>,
            cluster_daily_changes: HashMap<(Vec<u8>, u32), (i128, i128)>,
            object_type_index_changes: HashMap<Vec<u8>, MnftTypeIndex>,
            object_daily_changes: HashMap<(Vec<u8>, u32), (i128, i128)>,
            parser_perf_sample: ParserBatchPerfSample,
        }

        let (fetch_tx, mut fetch_rx) = mpsc::channel::<FetchedBatch>(PIPELINE_CHANNEL_CAPACITY);
        let (parse_tx, mut parse_rx) = mpsc::channel::<ParsedBatch>(PIPELINE_CHANNEL_CAPACITY);
        let parse_tx_pending_txs = Arc::new(AtomicU64::new(0));
        let parser_exit_reason = Arc::new(std::sync::Mutex::new(None::<String>));
        let fetcher_exit_reason = Arc::new(std::sync::Mutex::new(None::<String>));
        // Initialize committed_tip one below sync_tip so that cells at the
        // tip block are retained in parser caches until the writer commits them.
        let committed_tip_for_cache = Arc::new(AtomicI64::new(
            parser_cache_committed_tip_from_sync_tip(self.repo.get_sync_tip().await?.0),
        ));
        self.pipeline_perf
            .set_queue_capacities(PIPELINE_CHANNEL_CAPACITY, PIPELINE_CHANNEL_CAPACITY);

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

                let dynamic_span = adaptive_batch_controller_for_fetcher.estimate_block_span();
                let end_block = std::cmp::min(start_block + dynamic_span - 1, chain_tip);

                debug!(
                    "Fetcher: fetching blocks {} to {} (chain_tip={}, next_block={:?}, span={})",
                    start_block, end_block, chain_tip, next_block, dynamic_span
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

                let batch_tx_count: usize = blocks
                    .iter()
                    .map(|block| block.block.transactions.len())
                    .sum();
                adaptive_batch_controller_for_fetcher
                    .observe_tx_density(batch_tx_count, blocks.len());

                if fetch_tx
                    .send((
                        fetch_cycle_epoch,
                        start_block,
                        end_block,
                        chain_tip,
                        Arc::new(blocks),
                    ))
                    .await
                    .is_err()
                {
                    record_worker_exit_reason(
                        &fetcher_exit_reason_for_fetcher,
                        format!(
                            "failed to send fetched batch to parser: range={}-{}, chain_tip={}, pipeline_epoch={}",
                            start_block, end_block, chain_tip, fetch_cycle_epoch
                        ),
                    );
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
                let current_epoch = pipeline_epoch_for_parser.load(Ordering::SeqCst);
                if batch_epoch != current_epoch {
                    debug!(
                        batch_epoch,
                        current_epoch, "Skipping stale fetched batch {}-{}", start_block, end_block
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
                                    data_hash: Some(cell.data_hash.to_vec()),
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
                            let output_index = match parsed_input_outpoint_index_i16(
                                input.previous_output_index,
                                "sync_indexer",
                            ) {
                                Ok(v) => v,
                                Err(e) => {
                                    record_worker_exit_reason(
                                        &parser_exit_reason_for_parser,
                                        format!(
                                            "parsed_input_outpoint_index_i16 failed for range {}-{}: {}",
                                            start_block, end_block, e
                                        ),
                                    );
                                    return;
                                }
                            };
                            let key = (input.previous_tx_hash.to_vec(), output_index);
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
                let mut address_balance_changes: HashMap<Vec<u8>, AddressBalanceDelta> =
                    HashMap::new();
                let mut script_usage_changes: ScriptUsageChanges = HashMap::new();
                let mut script_reference_usage_changes: ScriptReferenceUsageChanges =
                    HashMap::new();
                let mut script_daily_changes: HashMap<(Vec<u8>, bool, u32), (i128, i128)> =
                    HashMap::new();
                let mut token_daily_changes: HashMap<(Vec<u8>, u32), (i128, i128)> = HashMap::new();
                let mut spore_type_index_changes: HashMap<Vec<u8>, SporeTypeIndex> = HashMap::new();
                let mut spore_daily_changes: HashMap<(Vec<u8>, u32), (i128, i128)> = HashMap::new();
                let mut cluster_daily_changes: HashMap<(Vec<u8>, u32), (i128, i128)> =
                    HashMap::new();
                let mut object_type_index_changes: HashMap<Vec<u8>, MnftTypeIndex> = HashMap::new();
                let mut object_daily_changes: HashMap<(Vec<u8>, u32), (i128, i128)> =
                    HashMap::new();
                let mut spore_type_index_cache: HashMap<Vec<u8>, Option<SporeTypeIndex>> =
                    HashMap::new();
                let mut object_type_index_cache: HashMap<Vec<u8>, Option<MnftTypeIndex>> =
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
                        let lock_hash_type = match parse_script_reference_hash_type(
                            cell.lock_hash_type,
                            "lock",
                            tx_data.block_number,
                            &tx_data.hash,
                        ) {
                            Ok(v) => v,
                            Err(e) => {
                                record_worker_exit_reason(
                                    &parser_exit_reason_for_parser,
                                    format!(
                                        "invalid output lock hash_type for range {}-{}: {}",
                                        start_block, end_block, e
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
                        let reference_entry = script_reference_usage_changes
                            .entry((cell.lock_code_hash.clone(), lock_hash_type, false))
                            .or_insert((0, 0, 0, 0, 0, 0));
                        reference_entry.0 += 1;
                        reference_entry.1 += 1;
                        reference_entry.2 += i128::from(cell.capacity);
                        reference_entry.3 += i128::from(cell.capacity);
                        reference_entry.4 += i128::from(cell_occupied);
                        reference_entry.5 += i128::from(cell_occupied);
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
                            let type_hash_type = match cell.type_hash_type {
                                Some(hash_type) => match parse_script_reference_hash_type(
                                    hash_type,
                                    "type",
                                    tx_data.block_number,
                                    &tx_data.hash,
                                ) {
                                    Ok(v) => v,
                                    Err(e) => {
                                        record_worker_exit_reason(
                                            &parser_exit_reason_for_parser,
                                            format!(
                                                "invalid output type hash_type for range {}-{}: {}",
                                                start_block, end_block, e
                                            ),
                                        );
                                        return;
                                    }
                                },
                                None => {
                                    record_worker_exit_reason(
                                        &parser_exit_reason_for_parser,
                                        format!(
                                            "missing output type hash_type for range {}-{}: block={}, tx=0x{}",
                                            start_block,
                                            end_block,
                                            tx_data.block_number,
                                            hex::encode(tx_data.hash)
                                        ),
                                    );
                                    return;
                                }
                            };
                            let type_key = (type_code_hash.clone(), true);
                            let reference_entry = script_reference_usage_changes
                                .entry((type_code_hash.clone(), type_hash_type, true))
                                .or_insert((0, 0, 0, 0, 0, 0));
                            reference_entry.0 += 1;
                            reference_entry.1 += 1;
                            reference_entry.2 += i128::from(cell.capacity);
                            reference_entry.3 += i128::from(cell.capacity);
                            reference_entry.4 += i128::from(cell_occupied);
                            reference_entry.5 += i128::from(cell_occupied);
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
                        if let (Some(ref type_script_hash), Some(ref type_code_hash)) =
                            (&cell.type_script_hash, &cell.type_code_hash)
                        {
                            if cell
                                .type_hash_type
                                .and_then(|ht| {
                                    UdtParser::is_udt_code_hash_bytes(type_code_hash, ht)
                                })
                                .is_some()
                            {
                                let daily_entry = token_daily_changes
                                    .entry((type_script_hash.clone(), date_yyyymmdd))
                                    .or_insert((0, 0));
                                daily_entry.0 += i128::from(cell.capacity);
                                daily_entry.1 += i128::from(cell_occupied);
                            }
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
                                let index = MnftTypeIndex {
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
                            let output_index = match parsed_input_outpoint_index_i16(
                                input.previous_output_index,
                                "sync_indexer",
                            ) {
                                Ok(v) => v,
                                Err(e) => {
                                    record_worker_exit_reason(
                                        &parser_exit_reason_for_parser,
                                        format!(
                                            "parsed_input_outpoint_index_i16 failed for range {}-{}: {}",
                                            start_block, end_block, e
                                        ),
                                    );
                                    return;
                                }
                            };
                            let key = (input.previous_tx_hash.to_vec(), output_index);
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
                                let lock_hash_type = match parse_script_reference_hash_type(
                                    info.lock_hash_type,
                                    "lock",
                                    tx_data.block_number,
                                    &tx_data.hash,
                                ) {
                                    Ok(v) => v,
                                    Err(e) => {
                                        record_worker_exit_reason(
                                            &parser_exit_reason_for_parser,
                                            format!(
                                                "invalid input lock hash_type for range {}-{}: {}",
                                                start_block, end_block, e
                                            ),
                                        );
                                        return;
                                    }
                                };
                                let reference_entry = script_reference_usage_changes
                                    .entry((info.lock_code_hash.clone(), lock_hash_type, false))
                                    .or_insert((0, 0, 0, 0, 0, 0));
                                reference_entry.1 -= 1;
                                reference_entry.3 -= i128::from(info.capacity);
                                reference_entry.5 -= i128::from(info.occupied_capacity);
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
                                    let type_hash_type = match info.type_hash_type {
                                        Some(hash_type) => match parse_script_reference_hash_type(
                                            hash_type,
                                            "type",
                                            tx_data.block_number,
                                            &tx_data.hash,
                                        ) {
                                            Ok(v) => v,
                                            Err(e) => {
                                                record_worker_exit_reason(
                                                    &parser_exit_reason_for_parser,
                                                    format!(
                                                        "invalid input type hash_type for range {}-{}: {}",
                                                        start_block, end_block, e
                                                    ),
                                                );
                                                return;
                                            }
                                        },
                                        None => {
                                            record_worker_exit_reason(
                                                &parser_exit_reason_for_parser,
                                                format!(
                                                    "missing input type hash_type for range {}-{}: block={}, tx=0x{}",
                                                    start_block,
                                                    end_block,
                                                    tx_data.block_number,
                                                    hex::encode(tx_data.hash)
                                                ),
                                            );
                                            return;
                                        }
                                    };
                                    let type_key = (type_code_hash.clone(), true);
                                    let reference_entry = script_reference_usage_changes
                                        .entry((type_code_hash.clone(), type_hash_type, true))
                                        .or_insert((0, 0, 0, 0, 0, 0));
                                    reference_entry.1 -= 1;
                                    reference_entry.3 -= i128::from(info.capacity);
                                    reference_entry.5 -= i128::from(info.occupied_capacity);
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
                                if let (Some(ref type_script_hash), Some(ref type_code_hash)) =
                                    (&info.type_script_hash, &info.type_code_hash)
                                {
                                    if info
                                        .type_hash_type
                                        .and_then(|ht| {
                                            UdtParser::is_udt_code_hash_bytes(type_code_hash, ht)
                                        })
                                        .is_some()
                                    {
                                        let daily_entry = token_daily_changes
                                            .entry((type_script_hash.clone(), date_yyyymmdd))
                                            .or_insert((0, 0));
                                        daily_entry.0 -= i128::from(info.capacity);
                                        daily_entry.1 -= i128::from(info.occupied_capacity);
                                    }
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
                                                            .get_mnft_type_index(type_script_hash)
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
                        let entry = address_balance_changes.entry(lock_hash.clone()).or_insert(
                            AddressBalanceDelta {
                                balance_delta: 0,
                                live_delta: 0,
                                total_delta: 0,
                                tx_delta: 0,
                                used_delta: 0,
                                first_seen_block: tx_data.block_number,
                                first_seen_tx: tx_data.hash.to_vec(),
                                last_activity_block: tx_data.block_number,
                                last_activity_tx: tx_data.hash.to_vec(),
                            },
                        );
                        entry.balance_delta += balance_change;
                        entry.live_delta += cells_created - cells_consumed;
                        entry.total_delta += cells_created;
                        entry.tx_delta += 1;
                        entry.last_activity_block = tx_data.block_number;
                        entry.last_activity_tx = tx_data.hash.to_vec();
                        entry.used_delta += occupied_change;
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

                let current_epoch = pipeline_epoch_for_parser.load(Ordering::SeqCst);
                if batch_epoch != current_epoch {
                    debug!(
                        batch_epoch,
                        current_epoch,
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
                };
                // Increment pending tx counter BEFORE sending to the channel.
                // The writer decrements this counter immediately upon receiving
                // a batch. If we increment after send, the writer can race ahead
                // and observe counter=0 before our fetch_add executes, causing
                // an underflow panic.
                parse_tx_pending_txs_for_parser.fetch_add(batch_tx_count_u64, Ordering::Relaxed);
                if parse_tx
                    .send(ParsedBatch {
                        batch_epoch,
                        start_block,
                        end_block,
                        chain_tip,
                        batch_tx_count: batch_tx_count_u64,
                        blocks,
                        parsed_blocks: all_parsed_blocks,
                        all_tx_data,
                        input_cell_info,
                        batch_cell_infos,
                        address_balance_changes,
                        script_usage_changes,
                        script_reference_usage_changes,
                        script_daily_changes,
                        token_daily_changes,
                        spore_type_index_changes,
                        spore_daily_changes,
                        cluster_daily_changes,
                        object_type_index_changes,
                        object_daily_changes,
                        parser_perf_sample,
                    })
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
            }
        });

        // === Writer loop ===
        let committed_tip_for_cache_for_writer = Arc::clone(&committed_tip_for_cache);
        let mut consecutive_idle_timeouts: u64 = 0;
        let mut pipeline_batch_index: u64 = 0;

        // Resolve disk device once for per-batch I/O delta tracking
        let disk_device = crate::sys_info::detect_disk_device(&self.config.domain_data_path);
        let mut disk_tracker = crate::sys_info::DiskStatsTracker::new(disk_device);

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
                Ok(Some(ParsedBatch {
                    batch_epoch,
                    start_block,
                    end_block,
                    chain_tip,
                    batch_tx_count: parsed_batch_tx_count_u64,
                    blocks,
                    parsed_blocks: all_parsed_blocks,
                    all_tx_data,
                    input_cell_info,
                    batch_cell_infos,
                    address_balance_changes,
                    script_usage_changes,
                    script_reference_usage_changes,
                    script_daily_changes,
                    token_daily_changes,
                    spore_type_index_changes,
                    spore_daily_changes,
                    cluster_daily_changes,
                    object_type_index_changes,
                    object_daily_changes,
                    parser_perf_sample,
                })) => {
                    consecutive_idle_timeouts = 0;
                    let recv_wait_ms = t_recv.elapsed().as_secs_f64() * 1000.0;
                    let current_epoch = self.pipeline_reset_epoch.load(Ordering::SeqCst);
                    if batch_epoch != current_epoch {
                        // Stale batch from a previous pipeline epoch — skip it
                        // without decrementing the counter (which was already
                        // reset to 0 by the drain that bumped the epoch).
                        debug!(
                            batch_epoch,
                            current_epoch,
                            "Dropping stale parsed batch {}-{}",
                            start_block,
                            end_block
                        );
                        continue;
                    }
                    atomic_checked_sub_u64(
                        &parse_tx_pending_txs_for_writer,
                        parsed_batch_tx_count_u64,
                    );
                    let (db_tip, db_tip_hash) = self.repo.get_sync_tip().await?;
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

                    // Validate that the batch's first block is a child of db_tip.
                    // The position check (start_block == expected_start) alone cannot
                    // detect a reorg that replaced blocks above db_tip while this
                    // batch was in the parse queue. Comparing parent_hash catches
                    // stale-fork batches without an extra RPC round-trip.
                    //
                    // When a mismatch is found, we immediately run reorg handling
                    // instead of just resetting the pipeline. Without this, the
                    // fetcher re-fetches the same blocks, the parser re-parses them,
                    // and the writer detects the same mismatch — an infinite loop.
                    if let Some(ref stored_hash) = db_tip_hash {
                        if let Some(first_parsed) = all_parsed_blocks.first() {
                            if first_parsed.parent_hash != *stored_hash {
                                warn!(
                                    run_id = %self.run_id,
                                    pipeline_epoch = current_epoch,
                                    db_tip,
                                    start_block,
                                    expected_parent = %hex::encode(stored_hash),
                                    actual_parent = %hex::encode(&first_parsed.parent_hash),
                                    "Stale fork batch detected: first block parent_hash does not match db_tip hash, triggering reorg check"
                                );

                                // The parent_hash mismatch is direct proof of a reorg.
                                // Immediately check and handle it, bypassing the
                                // blocks-behind lag gate which would skip reorg handling
                                // during bulk sync catch-up.
                                if db_tip > 0 {
                                    let db_tip_u64 = require_non_negative_block_number(
                                        db_tip,
                                        "stale fork reorg tip",
                                    )?;
                                    match self
                                        .check_and_handle_reorg(db_tip_u64, stored_hash)
                                        .await?
                                    {
                                        Some(ReorgAction::Handled) => {
                                            self.cell_cache.clear();
                                            self.udt_cell_cache.clear();
                                            let (reorg_tip, _) = self.repo.get_sync_tip().await?;
                                            committed_tip_for_cache_for_writer.store(
                                                parser_cache_committed_tip_from_sync_tip(reorg_tip),
                                                Ordering::SeqCst,
                                            );
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
                                                "Stale fork reorg handled, caches cleared, trackers reconciled, draining stale parsed batches"
                                            );
                                            self.request_pipeline_reset(
                                                "stale fork reorg handled",
                                                None,
                                                None,
                                                None,
                                            );
                                            let drained = Self::drain_channel(&mut parse_rx).await;
                                            parse_tx_pending_txs_for_writer
                                                .store(0, Ordering::Relaxed);
                                            info!(
                                                run_id = %self.run_id,
                                                pipeline_epoch = current_epoch,
                                                drained,
                                                "Stale fork reorg drain completed"
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
                                                "Stale fork triggered deep fork detection, sync paused"
                                            );
                                            self.request_pipeline_reset(
                                                "stale fork deep fork paused",
                                                None,
                                                None,
                                                None,
                                            );
                                            let drained = Self::drain_channel(&mut parse_rx).await;
                                            parse_tx_pending_txs_for_writer
                                                .store(0, Ordering::Relaxed);
                                            info!(
                                                run_id = %self.run_id,
                                                pipeline_epoch = current_epoch,
                                                drained,
                                                "Stale fork deep fork pause drain completed"
                                            );
                                            sleep(Duration::from_secs(30)).await;
                                            continue;
                                        }
                                        None => {
                                            // check_and_handle_reorg returned None: the chain
                                            // hash at db_tip matches our stored hash (possible
                                            // RPC race). Pipeline reset and retry.
                                            info!(
                                                run_id = %self.run_id,
                                                pipeline_epoch = current_epoch,
                                                db_tip,
                                                "Stale fork detected but reorg check found no divergence (RPC race), resetting pipeline"
                                            );
                                        }
                                    }
                                }

                                self.request_pipeline_reset(
                                    "stale fork batch (parent_hash mismatch)",
                                    Some(expected_start),
                                    Some(start_block),
                                    None,
                                );
                                let drained = Self::drain_channel(&mut parse_rx).await;
                                parse_tx_pending_txs_for_writer.store(0, Ordering::Relaxed);
                                info!(
                                    run_id = %self.run_id,
                                    pipeline_epoch = current_epoch,
                                    drained,
                                    "Stale fork drain completed"
                                );
                                continue;
                            }
                        }
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
                                    Some(ReorgAction::Handled) => {
                                        self.cell_cache.clear();
                                        self.udt_cell_cache.clear();
                                        let (reorg_tip, _) = self.repo.get_sync_tip().await?;
                                        committed_tip_for_cache_for_writer.store(
                                            parser_cache_committed_tip_from_sync_tip(reorg_tip),
                                            Ordering::SeqCst,
                                        );
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
                            script_reference_usage_changes,
                            script_daily_changes,
                            token_daily_changes,
                            spore_type_index_changes,
                            spore_daily_changes,
                            cluster_daily_changes,
                            object_type_index_changes,
                            object_daily_changes,
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
                                return Err(cleanup_err).with_context(|| {
                                    format!(
                                        "failed to cleanup partial batch {}-{} (chain_tip={}): \
                                         cannot recover from write failure; delete RocksDB and restart from genesis",
                                        start_block, end_block, chain_tip
                                    )
                                });
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
                        let blocks_remaining = self.progress.blocks_remaining();
                        let perf_stats = self.writer.store().memory_stats();
                        let batch_env = crate::sys_info::read_batch_environment(&mut disk_tracker);
                        self.record_bulk_sync_perf_batch_sample(BatchSample {
                            start_block,
                            end_block,
                            batch_index: pipeline_batch_index,
                            bottleneck: None,
                            txs: write_metrics.txs,
                            cells: write_metrics.cells,
                            inputs: write_metrics.inputs,
                            parse_ms: parser_perf_sample.parse_ms,
                            precompute_ms: parser_perf_sample.precompute_ms,
                            build_ms: write_metrics.write_ms,
                            prefetch_ms: write_metrics.prefetch_ms,
                            finalize_ms: write_metrics.finalize_ms,
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
                            "Wrote blocks {} to {} ({} remaining, {:.2}s, commit={:.0}ms, q={}, wait={:.0}ms) {}",
                            start_block,
                            end_block,
                            blocks_remaining,
                            db_elapsed.as_secs_f64(),
                            write_metrics.commit_ms,
                            writer_queue,
                            recv_wait_ms,
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
                    pipeline_batch_index += 1;
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

    /// Fetch blocks from CKB RocksDB using scoped threads for parallel I/O.
    ///
    /// Uses `std::thread::scope` instead of rayon so that blocking RocksDB
    /// reads run on temporary threads that don't compete with the global
    /// rayon pool used by CPU-bound build phases (facts/reduce/history).
    pub(crate) fn fetch_blocks_direct(
        store: &CkbChainReader,
        start: u64,
        end: u64,
    ) -> Result<Vec<BlockResponseWithCycles>> {
        let default_threads = std::thread::available_parallelism()
            .map(|n| (n.get() / 2).max(2))
            .unwrap_or(4);
        let block_numbers: Vec<u64> = (start..=end).collect();
        Self::scoped_parallel_fetch(&block_numbers, default_threads, |&num| {
            let hash = store
                .get_block_hash(num)
                .ok_or_else(|| anyhow::anyhow!("Block {} hash not found in CKB RocksDB", num))?;
            let block = store
                .get_block(&hash)
                .ok_or_else(|| anyhow::anyhow!("Block {} data not found in CKB RocksDB", num))?;
            let rpc_block = ckb_store_reader::block_view_to_rpc(&block, store);
            Ok(rpc_block.into())
        })
    }

    /// Fetch blocks from CKB RocksDB as raw molecule `BlockView` + cycles.
    /// Bypasses `block_view_to_rpc` hex encoding for binary-native bulk sync.
    ///
    /// Uses batch `multi_get_cf` per thread chunk to reduce RocksDB lock
    /// contention: 3 multi_get calls per chunk instead of 5N individual gets.
    pub(crate) fn fetch_blocks_direct_binary(
        store: &CkbChainReader,
        start: u64,
        end: u64,
        max_threads: u32,
    ) -> Result<Vec<crate::sync::bulk_build::binary_facts::RawCkbBlock>> {
        let block_numbers: Vec<u64> = (start..=end).collect();
        if block_numbers.is_empty() {
            return Ok(Vec::new());
        }

        let thread_count = (max_threads as usize).max(1).min(block_numbers.len());
        let chunk_size = block_numbers.len().div_ceil(thread_count);

        std::thread::scope(|s| {
            let handles: Vec<_> = block_numbers
                .chunks(chunk_size)
                .map(|chunk| {
                    s.spawn(move || -> Result<Vec<crate::sync::bulk_build::binary_facts::RawCkbBlock>> {
                        // Batch 1: multi_get all block hashes
                        let hash_opts = store.get_block_hashes_batch(chunk);
                        let mut hashes = Vec::with_capacity(chunk.len());
                        for (i, h) in hash_opts.into_iter().enumerate() {
                            hashes.push(h.ok_or_else(|| {
                                anyhow::anyhow!(
                                    "Block {} hash not found in CKB RocksDB",
                                    chunk[i]
                                )
                            })?);
                        }

                        // Batch 2: multi_get headers + uncles + proposals (3N keys),
                        // body still per-block prefix iteration
                        let block_opts = store.get_blocks_batch(&hashes);

                        // Batch 3: multi_get all block extensions (N keys)
                        let ext_opts = store.get_block_exts_batch(&hashes);

                        // Assemble RawCkbBlock results
                        let mut results = Vec::with_capacity(chunk.len());
                        for (i, (block_opt, ext_opt)) in
                            block_opts.into_iter().zip(ext_opts).enumerate()
                        {
                            let block = block_opt.ok_or_else(|| {
                                anyhow::anyhow!(
                                    "Block {} data not found in CKB RocksDB",
                                    chunk[i]
                                )
                            })?;
                            let cycles = ext_opt
                                .and_then(|(_, cycles_vec)| {
                                    if cycles_vec.is_empty() {
                                        None
                                    } else {
                                        Some(cycles_vec)
                                    }
                                })
                                .unwrap_or_default();
                            results.push(
                                crate::sync::bulk_build::binary_facts::RawCkbBlock {
                                    block,
                                    cycles,
                                },
                            );
                        }
                        Ok(results)
                    })
                })
                .collect();

            let mut results = Vec::with_capacity(block_numbers.len());
            for handle in handles {
                results.extend(
                    handle
                        .join()
                        .map_err(|e| anyhow::anyhow!("fetch thread panicked: {:?}", e))??,
                );
            }
            Ok(results)
        })
    }

    /// Run `f` over `items` on temporary scoped threads, collecting results
    /// in order.  `max_threads` controls parallelism — set by the bottleneck
    /// controller for bulk build, or a reasonable default for pipeline fetch.
    /// Threads are destroyed after the call — no persistent pool, no rayon
    /// contention with CPU-bound build work.
    fn scoped_parallel_fetch<T, F>(items: &[u64], max_threads: usize, f: F) -> Result<Vec<T>>
    where
        T: Send,
        F: Fn(&u64) -> Result<T> + Sync,
    {
        if items.is_empty() {
            return Ok(Vec::new());
        }
        let thread_count = max_threads.max(1).min(items.len());
        let chunk_size = items.len().div_ceil(thread_count);

        std::thread::scope(|s| {
            let handles: Vec<_> = items
                .chunks(chunk_size)
                .map(|chunk| s.spawn(|| chunk.iter().map(&f).collect::<Result<Vec<T>>>()))
                .collect();
            let mut results = Vec::with_capacity(items.len());
            for handle in handles {
                results.extend(
                    handle
                        .join()
                        .map_err(|e| anyhow::anyhow!("fetch thread panicked: {:?}", e))??,
                );
            }
            Ok(results)
        })
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
    use super::super::bulk_build::facts::{CellProtocolFacts, CellSemanticTag};
    use super::super::bulk_build::interner::IdentityInterner;
    use super::*;
    use crate::parser::dotbit::DOTBIT_ACCOUNT_CELL_TYPE_ID;
    use crate::parser::udt::SUDT_CODE_HASH;
    use crate::rpc::{
        BlockResponseWithCycles, BlockView, CellInput, CellOutput, HeaderView, OutPoint, Script,
        TransactionView,
    };

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

    fn create_spore_type_script(spore_id: &[u8; 32]) -> Script {
        Script {
            code_hash: crate::parser::spore::SPORE_CODE_HASH_MAINNET_V2.to_string(),
            hash_type: "data1".to_string(),
            args: format!("0x{}", hex::encode(spore_id)),
        }
    }

    fn create_cluster_type_script(cluster_id: &[u8; 32]) -> Script {
        Script {
            code_hash: crate::parser::spore::CLUSTER_CODE_HASH_MAINNET_V2.to_string(),
            hash_type: "data1".to_string(),
            args: format!("0x{}", hex::encode(cluster_id)),
        }
    }

    fn create_spore_data(
        content_type: &str,
        content: &[u8],
        cluster_id: Option<&[u8; 32]>,
    ) -> Vec<u8> {
        let content_type_bytes = encode_molecule_bytes(content_type.as_bytes());
        let content_bytes = encode_molecule_bytes(content);
        let cluster_id_bytes = cluster_id.map(|id| encode_molecule_bytes(id));

        let offset_content_type = 16u32;
        let offset_content = offset_content_type + content_type_bytes.len() as u32;
        let offset_cluster_id = offset_content + content_bytes.len() as u32;
        let total_size =
            offset_cluster_id + cluster_id_bytes.as_ref().map(|b| b.len()).unwrap_or(0) as u32;

        let mut data = Vec::new();
        data.extend_from_slice(&total_size.to_le_bytes());
        data.extend_from_slice(&offset_content_type.to_le_bytes());
        data.extend_from_slice(&offset_content.to_le_bytes());
        data.extend_from_slice(&offset_cluster_id.to_le_bytes());
        data.extend_from_slice(&content_type_bytes);
        data.extend_from_slice(&content_bytes);
        if let Some(cluster_id_bytes) = cluster_id_bytes {
            data.extend_from_slice(&cluster_id_bytes);
        }
        data
    }

    #[test]
    fn parse_script_reference_hash_type_rejects_unsupported_value() {
        let err = parse_script_reference_hash_type(3, "lock", 42, &[0x11; 32]).unwrap_err();
        assert!(err.to_string().contains("expected_one_of=[0,1,2,4]"));
    }

    fn create_cluster_data(name: &str, description: &str) -> Vec<u8> {
        let name_bytes = encode_molecule_bytes(name.as_bytes());
        let description_bytes = encode_molecule_bytes(description.as_bytes());
        let offset_name = 16u32;
        let offset_description = offset_name + name_bytes.len() as u32;
        let offset_end = offset_description + description_bytes.len() as u32;

        let mut data = Vec::new();
        data.extend_from_slice(&offset_end.to_le_bytes());
        data.extend_from_slice(&offset_name.to_le_bytes());
        data.extend_from_slice(&offset_description.to_le_bytes());
        data.extend_from_slice(&offset_end.to_le_bytes());
        data.extend_from_slice(&name_bytes);
        data.extend_from_slice(&description_bytes);
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

    fn create_facts_fixture_header(number: u64) -> HeaderView {
        HeaderView {
            version: "0x0".to_string(),
            compact_target: "0x1a08a97e".to_string(),
            timestamp: "0x18c7b3b2b00".to_string(),
            number: format!("0x{number:x}"),
            epoch: "0x7080006000028".to_string(),
            parent_hash: format!("0x{}", "11".repeat(32)),
            transactions_root: format!("0x{}", "22".repeat(32)),
            proposals_hash: format!("0x{}", "33".repeat(32)),
            extra_hash: format!("0x{}", "44".repeat(32)),
            dao: format!("0x{}", "00".repeat(32)),
            nonce: "0x1".to_string(),
            hash: format!("0x{}", "55".repeat(32)),
        }
    }

    fn create_facts_fixture_block_with_two_txs() -> BlockResponseWithCycles {
        let tx0 = TransactionView {
            hash: format!("0x{}", "aa".repeat(32)),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![CellInput {
                since: "0x0".to_string(),
                previous_output: OutPoint {
                    tx_hash: format!("0x{}", "00".repeat(32)),
                    index: "0xffffffff".to_string(),
                },
            }],
            outputs: vec![CellOutput {
                capacity: "0x174876e800".to_string(),
                lock: create_lock_script(),
                type_: None,
            }],
            outputs_data: vec!["0x".to_string()],
            witnesses: vec!["0x".to_string()],
        };
        let tx1 = TransactionView {
            hash: format!("0x{}", "bb".repeat(32)),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![CellInput {
                since: "0x0".to_string(),
                previous_output: OutPoint {
                    tx_hash: format!("0x{}", "cc".repeat(32)),
                    index: "0x0".to_string(),
                },
            }],
            outputs: vec![CellOutput {
                capacity: "0x174876e800".to_string(),
                lock: create_lock_script(),
                type_: Some(Script {
                    code_hash: SUDT_CODE_HASH.to_string(),
                    hash_type: "type".to_string(),
                    args: format!("0x{}", "12".repeat(32)),
                }),
            }],
            outputs_data: vec![format!("0x{}", hex::encode(42u128.to_le_bytes()))],
            witnesses: vec!["0x".to_string()],
        };

        BlockResponseWithCycles {
            block: BlockView {
                header: create_facts_fixture_header(14_000_123),
                uncles: vec![],
                transactions: vec![tx0, tx1],
                proposals: vec![],
            },
            cycles: None,
        }
    }

    #[test]
    fn build_facts_arena_captures_exact_cell_semantics() {
        let blocks = vec![create_facts_fixture_block_with_two_txs()];
        let interner = IdentityInterner::default();
        let (arena, _breakdown) =
            build_bulk_facts_arena_from_blocks(&blocks, &interner).expect("facts");
        let frozen = interner.snapshot_for_reads();
        let sudt_cell = arena
            .cells
            .iter()
            .find(|cell| matches!(cell.semantic_tag, CellSemanticTag::Sudt))
            .expect("sudt cell");

        assert_eq!(arena.txs.len(), 2);
        assert!(arena.cells.iter().any(|cell| cell.occupied_capacity > 0));
        assert_eq!(sudt_cell.lock_hash_type, 1);
        assert_eq!(frozen.resolve_bytes(sudt_cell.lock_args_id).len(), 20);
        assert_eq!(sudt_cell.type_hash_type, Some(1));
        assert_eq!(
            frozen.resolve_bytes(sudt_cell.type_args_id.expect("type args")),
            &[0x12; 32]
        );
        assert!(
            arena.cells.iter().all(|cell| {
                matches!(
                    cell.semantic_tag,
                    CellSemanticTag::Plain
                        | CellSemanticTag::Dao
                        | CellSemanticTag::Sudt
                        | CellSemanticTag::Xudt
                        | CellSemanticTag::Dotbit
                        | CellSemanticTag::Mnft
                        | CellSemanticTag::Spore
                        | CellSemanticTag::Cluster
                )
            }),
            "all cells must have semantic tags"
        );
    }

    #[test]
    fn build_facts_arena_captures_protocol_facts_and_tx_metadata_for_bulk_build() {
        let cluster_id = [0x31; 32];
        let spore_id = [0x41; 32];
        let dotbit_account_id = [0x51; 20];
        let block = BlockResponseWithCycles {
            block: BlockView {
                header: HeaderView {
                    version: "0x0".to_string(),
                    compact_target: "0x1a08a97e".to_string(),
                    timestamp: "0x18c7b3b2b88".to_string(),
                    number: "0xd59f87".to_string(),
                    epoch: "0x7080006000028".to_string(),
                    parent_hash: format!("0x{}", "11".repeat(32)),
                    transactions_root: format!("0x{}", "22".repeat(32)),
                    proposals_hash: format!("0x{}", "33".repeat(32)),
                    extra_hash: format!("0x{}", "44".repeat(32)),
                    dao: format!("0x{}", "00".repeat(32)),
                    nonce: "0x1".to_string(),
                    hash: format!("0x{}", "66".repeat(32)),
                },
                uncles: vec![],
                transactions: vec![
                    TransactionView {
                        hash: format!("0x{}", "a1".repeat(32)),
                        version: "0x0".to_string(),
                        cell_deps: vec![],
                        header_deps: vec![],
                        inputs: vec![CellInput {
                            since: "0x0".to_string(),
                            previous_output: OutPoint {
                                tx_hash: format!("0x{}", "00".repeat(32)),
                                index: "0xffffffff".to_string(),
                            },
                        }],
                        outputs: vec![CellOutput {
                            capacity: "0x174876e800".to_string(),
                            lock: create_lock_script(),
                            type_: Some(create_cluster_type_script(&cluster_id)),
                        }],
                        outputs_data: vec![format!(
                            "0x{}",
                            hex::encode(create_cluster_data(
                                "Genesis Cluster",
                                "cluster description"
                            ))
                        )],
                        witnesses: vec!["0x".to_string()],
                    },
                    TransactionView {
                        hash: format!("0x{}", "a2".repeat(32)),
                        version: "0x0".to_string(),
                        cell_deps: vec![],
                        header_deps: vec![],
                        inputs: vec![CellInput {
                            since: "0x0".to_string(),
                            previous_output: OutPoint {
                                tx_hash: format!("0x{}", "10".repeat(32)),
                                index: "0x0".to_string(),
                            },
                        }],
                        outputs: vec![
                            CellOutput {
                                capacity: "0x174876e800".to_string(),
                                lock: create_lock_script(),
                                type_: Some(create_spore_type_script(&spore_id)),
                            },
                            CellOutput {
                                capacity: "0x174876e800".to_string(),
                                lock: create_lock_script(),
                                type_: Some(create_dotbit_account_cell_type_script(
                                    &dotbit_account_id,
                                )),
                            },
                        ],
                        outputs_data: vec![
                            format!(
                                "0x{}",
                                hex::encode(create_spore_data(
                                    "image/png",
                                    b"spore-content",
                                    Some(&cluster_id)
                                ))
                            ),
                            format!(
                                "0x{}",
                                hex::encode(create_dotbit_account_cell_data(&dotbit_account_id))
                            ),
                        ],
                        witnesses: vec![
                            encode_dotbit_account_cell_witness(&dotbit_account_id, "alice.bit"),
                            encode_das_action_witness("transfer_account"),
                        ],
                    },
                ],
                proposals: vec![],
            },
            cycles: None,
        };

        let interner = IdentityInterner::default();
        let (arena, _breakdown) =
            build_bulk_facts_arena_from_blocks(&[block], &interner).expect("facts");
        let cluster_cell = arena
            .cells
            .iter()
            .find(|cell| matches!(cell.semantic_tag, CellSemanticTag::Cluster))
            .expect("cluster cell");
        let spore_cell = arena
            .cells
            .iter()
            .find(|cell| matches!(cell.semantic_tag, CellSemanticTag::Spore))
            .expect("spore cell");
        let dotbit_cell = arena
            .cells
            .iter()
            .find(|cell| matches!(cell.semantic_tag, CellSemanticTag::Dotbit))
            .expect("dotbit cell");
        let dotbit_tx = arena
            .txs
            .iter()
            .find(|tx| tx.hash == [0xa2; 32])
            .expect("dotbit tx");

        match cluster_cell
            .protocol_facts
            .as_ref()
            .expect("cluster protocol facts")
        {
            CellProtocolFacts::Cluster(cluster) => {
                assert_eq!(cluster.cluster_id, cluster_id);
                assert_eq!(cluster.name.as_deref(), Some("Genesis Cluster"));
                assert_eq!(cluster.description.as_deref(), Some("cluster description"));
            }
            other => panic!("expected cluster facts, got {other:?}"),
        }

        match spore_cell
            .protocol_facts
            .as_ref()
            .expect("spore protocol facts")
        {
            CellProtocolFacts::Spore(spore) => {
                assert_eq!(spore.spore_id, spore_id);
                assert_eq!(spore.cluster_id, Some(cluster_id));
                assert_eq!(spore.content_type, "image/png");
                assert_eq!(spore.content, b"spore-content");
                assert!(!spore.is_did);
            }
            other => panic!("expected spore facts, got {other:?}"),
        }

        match dotbit_cell
            .protocol_facts
            .as_ref()
            .expect("dotbit protocol facts")
        {
            CellProtocolFacts::Dotbit(dotbit) => {
                assert_eq!(dotbit.account_id, dotbit_account_id);
                assert_eq!(dotbit.account.as_deref(), Some("alice.bit"));
            }
            other => panic!("expected dotbit facts, got {other:?}"),
        }

        assert_eq!(dotbit_tx.block_hash, [0x66; 32]);
        assert_eq!(dotbit_tx.timestamp_ms, 1_702_874_524_552);
        assert_eq!(dotbit_tx.dotbit_action.as_deref(), Some("transfer_account"));
    }

    fn make_dao_parsed_cell(data: Vec<u8>) -> ParsedCell {
        ParsedCell {
            capacity: 200_00000000,
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![0x22; 20],
            lock_script_hash: vec![0x33; 32],
            type_code_hash: Some(crate::rpc::parse_hex_to_bytes(
                crate::parser::dao::DAO_CODE_HASH,
            )),
            type_hash_type: Some(1),
            type_args: Some(vec![]),
            type_script_hash: Some(vec![0x44; 32]),
            data_hash: [0x55; 32],
            data_size: i32::try_from(data.len()).expect("data size"),
            data,
        }
    }

    #[test]
    fn parse_bulk_dao_cell_state_recognizes_withdraw_request_block_number() {
        let state = parse_bulk_dao_cell_state(
            &make_dao_parsed_cell(123u64.to_le_bytes().to_vec()),
            CellSemanticTag::Dao,
            &[0xaa; 32],
            0,
        )
        .expect("dao state");

        assert_eq!(
            state,
            Some(DaoCellState::WithdrawRequest {
                deposit_block_number: 123,
            })
        );
    }

    #[test]
    fn parse_bulk_dao_cell_state_rejects_invalid_dao_data_length() {
        let err = parse_bulk_dao_cell_state(
            &make_dao_parsed_cell(vec![0x11; 7]),
            CellSemanticTag::Dao,
            &[0xbb; 32],
            1,
        )
        .unwrap_err();

        assert!(err
            .to_string()
            .contains("invalid DAO cell data in bulk facts"));
    }

    #[test]
    fn test_parser_precompute_phase_metrics_total_ms_sums_live_phases() {
        let metrics = super::ParserPrecomputePhaseMetrics {
            build_batch_cell_infos_ms: 10.0,
            compute_fee_ms: 20.0,
            cache_balance_and_script_ms: 30.0,
        };

        assert!((metrics.total_ms() - 60.0).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_bulk_tx_cycles_excludes_cellbase_and_offsets_correctly() {
        use crate::rpc::*;

        let block = BlockResponseWithCycles {
            block: BlockView {
                header: create_facts_fixture_header(100),
                uncles: vec![],
                transactions: vec![
                    // tx0: cellbase
                    TransactionView {
                        hash: format!("0x{}", "aa".repeat(32)),
                        version: "0x0".to_string(),
                        cell_deps: vec![],
                        header_deps: vec![],
                        inputs: vec![CellInput {
                            since: "0x0".to_string(),
                            previous_output: OutPoint {
                                tx_hash: format!("0x{}", "00".repeat(32)),
                                index: "0xffffffff".to_string(),
                            },
                        }],
                        outputs: vec![],
                        outputs_data: vec![],
                        witnesses: vec![],
                    },
                    // tx1: regular tx
                    TransactionView {
                        hash: format!("0x{}", "bb".repeat(32)),
                        version: "0x0".to_string(),
                        cell_deps: vec![],
                        header_deps: vec![],
                        inputs: vec![CellInput {
                            since: "0x0".to_string(),
                            previous_output: OutPoint {
                                tx_hash: format!("0x{}", "cc".repeat(32)),
                                index: "0x0".to_string(),
                            },
                        }],
                        outputs: vec![],
                        outputs_data: vec![],
                        witnesses: vec![],
                    },
                ],
                proposals: vec![],
            },
            // CKB returns cycles for non-cellbase txs only (1 entry for 2 txs)
            cycles: Some(vec!["0x1f4".to_string()]),
        };

        let cellbase_hash = [0xaa; 32];
        let tx1_hash = [0xbb; 32];

        // Cellbase (tx_position=0) => None
        let result = super::parse_bulk_tx_cycles(&block, 0, 100, &cellbase_hash).unwrap();
        assert_eq!(result, None);

        // Non-cellbase tx (tx_position=1) => Some(500) (0x1f4)
        let result = super::parse_bulk_tx_cycles(&block, 1, 100, &tx1_hash).unwrap();
        assert_eq!(result, Some(500));
    }

    #[test]
    fn parse_bulk_tx_cycles_detects_length_mismatch() {
        use crate::rpc::*;

        let block = BlockResponseWithCycles {
            block: BlockView {
                header: create_facts_fixture_header(100),
                uncles: vec![],
                transactions: vec![
                    TransactionView {
                        hash: format!("0x{}", "aa".repeat(32)),
                        version: "0x0".to_string(),
                        cell_deps: vec![],
                        header_deps: vec![],
                        inputs: vec![],
                        outputs: vec![],
                        outputs_data: vec![],
                        witnesses: vec![],
                    },
                    TransactionView {
                        hash: format!("0x{}", "bb".repeat(32)),
                        version: "0x0".to_string(),
                        cell_deps: vec![],
                        header_deps: vec![],
                        inputs: vec![],
                        outputs: vec![],
                        outputs_data: vec![],
                        witnesses: vec![],
                    },
                ],
                proposals: vec![],
            },
            // Wrong: 2 cycles for 2 txs (should be 1 for non-cellbase only)
            cycles: Some(vec!["0x0".to_string(), "0x1f4".to_string()]),
        };

        let tx1_hash = [0xbb; 32];
        let err = super::parse_bulk_tx_cycles(&block, 1, 100, &tx1_hash).unwrap_err();
        assert!(err.to_string().contains("cycles length mismatch"));
    }

    #[test]
    fn build_facts_arena_skips_unparseable_dotbit_cell_instead_of_crashing() {
        // Regression: a DotBit AccountCell with data < 52 bytes caused a fatal
        // bail! that crashed the entire bulk sync.  The fix returns Ok(None)
        // so the cell is skipped with a warning instead.
        let short_data_account_id = [0x51; 20];
        let block = BlockResponseWithCycles {
            block: BlockView {
                header: HeaderView {
                    version: "0x0".to_string(),
                    compact_target: "0x1a08a97e".to_string(),
                    timestamp: "0x18c7b3b2b88".to_string(),
                    number: "0x4a3800".to_string(),
                    epoch: "0x7080006000028".to_string(),
                    parent_hash: format!("0x{}", "11".repeat(32)),
                    transactions_root: format!("0x{}", "22".repeat(32)),
                    proposals_hash: format!("0x{}", "33".repeat(32)),
                    extra_hash: format!("0x{}", "44".repeat(32)),
                    dao: format!("0x{}", "00".repeat(32)),
                    nonce: "0x1".to_string(),
                    hash: format!("0x{}", "66".repeat(32)),
                },
                uncles: vec![],
                transactions: vec![TransactionView {
                    hash: format!("0x{}", "a1".repeat(32)),
                    version: "0x0".to_string(),
                    cell_deps: vec![],
                    header_deps: vec![],
                    inputs: vec![CellInput {
                        since: "0x0".to_string(),
                        previous_output: OutPoint {
                            tx_hash: format!("0x{}", "00".repeat(32)),
                            index: "0xffffffff".to_string(),
                        },
                    }],
                    outputs: vec![CellOutput {
                        capacity: "0x174876e800".to_string(),
                        lock: create_lock_script(),
                        type_: Some(create_dotbit_account_cell_type_script(
                            &short_data_account_id,
                        )),
                    }],
                    // Only 10 bytes of data — far below the 52-byte minimum
                    outputs_data: vec![format!("0x{}", hex::encode([0xffu8; 10]))],
                    witnesses: vec!["0x".to_string()],
                }],
                proposals: vec![],
            },
            cycles: None,
        };

        let interner = IdentityInterner::default();
        let (arena, _breakdown) =
            build_bulk_facts_arena_from_blocks(&[block], &interner).expect("should not crash");

        let dotbit_cell = arena
            .cells
            .iter()
            .find(|cell| matches!(cell.semantic_tag, CellSemanticTag::Dotbit));
        // Cell is tagged as Dotbit but protocol_facts is None (skipped)
        let cell = dotbit_cell.expect("cell should exist with Dotbit tag");
        assert!(
            cell.protocol_facts.is_none(),
            "unparseable DotBit cell should have no protocol facts"
        );
    }
}
