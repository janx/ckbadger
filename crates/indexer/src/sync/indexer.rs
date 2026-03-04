#![allow(clippy::type_complexity)]

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Timelike, Utc};
use ckbadger_common::{LabelImportConfig, PipelineProgressData};
use dashmap::DashMap;
use futures::stream::{FuturesOrdered, StreamExt};
use rayon::prelude::*;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::keys;
use ckbadger_store::types::{
    DaoDailySnapshot, HodlTrackerState, LiveCellInfo, NftTypeIndex, SporeTypeIndex,
};
use ckbadger_store::{CkbadgerStore, CF_ADDR_TXS};

use crate::cache::CacheInvalidator;
use crate::config::{Config, DEEP_FORK_DEPTH};
use crate::db::writer::dotbit::{resolve_dotbit_tx_activity, DOTBIT_SENTINEL_COLLECTION};
use crate::db::writer::hodl_wave::HodlWaveTracker;
use crate::db::writer::nft_activity_acc::NftCollectionActivityAccumulator;
use crate::db::{BatchWriter, DaoWithdrawalContext, Repository};
use crate::parser::{
    analyze_spore_media_profile, BlockParser, CellParser, DaoParser, DotbitParser, MnftParser,
    ParsedClusterCell, ParsedDotbitAccountOutput, ParsedMnftClass, ParsedMnftIssuer,
    ParsedMnftToken, ParsedSporeCell, ScriptParser, SporeParser, TransactionParser, UdtParser,
};
use ckb_store_reader::CkbChainReader;

use crate::rpc::{BlockResponseWithCycles, CkbRpcClient};
use crate::runtime_diag::{generate_incident_id, read_cgroup_memory_snapshot, FlightRecorder};

use super::adaptive::*;
use super::dao_helpers::*;
use super::diagnostics::*;
use super::helpers::*;
use super::nft_helpers::*;
use super::sync_mode::*;
use super::token_helpers::*;
use super::types::{
    BatchWriteMetrics, CachedCellInfo, CachedUdtCellInfo, DotbitConsumptionEvent,
    DotbitTxActivityData, PreParsedNftData, ReorgAction, SyncAction, TxData, UndoSeqScope,
    UnresolvedLocalProbeSummary, UnresolvedRpcProbeSummary,
};
use super::undo::*;
use super::SyncProgress;

fn ensure_hodl_tracker_state_consistent(
    state: Option<&HodlTrackerState>,
    tip_block: i64,
) -> Result<()> {
    if tip_block <= 0 {
        return Ok(());
    }
    let state = state.ok_or_else(|| {
        anyhow!(
            "missing HODL tracker state at tip {}. automatic rebuild is disabled; delete RocksDB and re-sync from genesis",
            tip_block
        )
    })?;
    if state.date_transitions.is_empty() {
        bail!(
            "invalid HODL tracker state: empty date_transitions at tip {}. automatic rebuild is disabled; delete RocksDB and re-sync from genesis",
            tip_block
        );
    }
    if let Some((last_block, _)) = state.date_transitions.last() {
        if *last_block > tip_block {
            bail!(
                "invalid HODL tracker state: last transition block {} ahead of sync tip {}. automatic rebuild is disabled; delete RocksDB and re-sync from genesis",
                last_block,
                tip_block
            );
        }
    }
    Ok(())
}

fn rebuild_hodl_tracker_from_state(
    state: Option<HodlTrackerState>,
    tip_block: i64,
) -> Result<HodlWaveTracker> {
    ensure_hodl_tracker_state_consistent(state.as_ref(), tip_block)?;
    if tip_block <= 0 {
        return Ok(HodlWaveTracker::new());
    }
    Ok(state
        .map(HodlWaveTracker::from_state)
        .unwrap_or_else(HodlWaveTracker::new))
}

fn collect_missing_input_outpoints<T>(
    all_input_outpoints: &[(Vec<u8>, i16)],
    input_cell_info: &HashMap<(Vec<u8>, i16), LiveCellInfo>,
    same_batch_cells: &HashMap<(Vec<u8>, i16), T>,
) -> Vec<(Vec<u8>, i16)> {
    let mut seen = HashSet::new();
    all_input_outpoints
        .iter()
        .filter_map(|(tx_hash, output_index)| {
            let key = (tx_hash.clone(), *output_index);
            if input_cell_info.contains_key(&key) || same_batch_cells.contains_key(&key) {
                None
            } else if seen.insert(key.clone()) {
                Some(key)
            } else {
                None
            }
        })
        .collect()
}

fn build_activity_input_views(
    store: &CkbadgerStore,
    tx_data: &TxData,
    block_number: i64,
    input_cell_info: &HashMap<(Vec<u8>, i16), LiveCellInfo>,
    batch_cell_infos: &HashMap<(Vec<u8>, i16), LiveCellInfo>,
) -> Result<Vec<crate::db::writer::activities::InputCellView>> {
    if tx_data.is_cellbase {
        return Ok(Vec::new());
    }

    let dao_code_hash = crate::rpc::parse_hex_to_bytes(crate::parser::dao::DAO_CODE_HASH);

    tx_data
        .inputs
        .iter()
        .enumerate()
        .map(|(input_index, input)| {
            let previous_output_index =
                i16::try_from(input.previous_output_index).map_err(|_| {
                    anyhow!(
                        "input previous output index out of i16 range while building activities: block={}, tx_hash=0x{}, tx_index={}, input_index={}, previous_output_index={}",
                        block_number,
                        hex::encode(tx_data.hash),
                        tx_data.tx_index,
                        input_index,
                        input.previous_output_index
                    )
                })?;
            let key = (input.previous_tx_hash.to_vec(), previous_output_index);
            let info = input_cell_info
                .get(&key)
                .or_else(|| batch_cell_infos.get(&key))
                .ok_or_else(|| {
                    anyhow!(
                        "missing input cell info while building activities: block={}, tx_hash=0x{}, tx_index={}, input_index={}, prev_outpoint=0x{}:{}",
                        block_number,
                        hex::encode(tx_data.hash),
                        tx_data.tx_index,
                        input_index,
                        hex::encode(input.previous_tx_hash),
                        previous_output_index
                    )
                })?;
            let outpoint_key = keys::encode_outpoint(&input.previous_tx_hash, previous_output_index);
            let is_dao_withdraw_request = if info.type_code_hash.as_deref()
                == Some(dao_code_hash.as_slice())
            {
                store
                    .get_cf(store.cf_dao_by_withdraw_tx(), &outpoint_key)
                    .map_err(|e| {
                        anyhow!(
                            "failed to check DAO withdraw index while building activities: block={}, tx_hash=0x{}, tx_index={}, input_index={}, prev_outpoint=0x{}:{}, error={}",
                            block_number,
                            hex::encode(tx_data.hash),
                            tx_data.tx_index,
                            input_index,
                            hex::encode(input.previous_tx_hash),
                            previous_output_index,
                            e
                        )
                    })?
                    .is_some()
            } else {
                false
            };

            Ok(crate::db::writer::activities::InputCellView {
                lock_script_hash: info.lock_script_hash.clone(),
                capacity: info.capacity,
                occupied_capacity: info.occupied_capacity,
                type_code_hash: info.type_code_hash.clone(),
                type_script_hash: info.type_script_hash.clone(),
                type_args: info.type_args.clone(),
                udt_amount: info.udt_amount,
                data: Vec::new(),
                is_dao_withdraw_request,
            })
        })
        .collect()
}

fn parse_udt_cells_with_store_fallback_inner<F>(
    tx: &crate::rpc::TransactionView,
    mut standard_lookup: F,
) -> Result<Vec<(i16, crate::parser::ParsedUdtCell)>>
where
    F: FnMut(&[u8]) -> Result<Option<String>>,
{
    if tx.outputs.len() != tx.outputs_data.len() {
        bail!(
            "transaction outputs mismatch while parsing UDT outputs with store fallback: tx_hash={}, outputs={}, outputs_data={}",
            tx.hash,
            tx.outputs.len(),
            tx.outputs_data.len()
        );
    }
    let mut parsed = Vec::new();
    let mut standard_cache: HashMap<Vec<u8>, Option<String>> = HashMap::new();

    for (output_index, (output, data_hex)) in
        tx.outputs.iter().zip(tx.outputs_data.iter()).enumerate()
    {
        if let Some(cell) = UdtParser::parse_udt_cell(output, data_hex) {
            let output_index_i16 =
                checked_usize_to_i16(output_index, "UDT output index while parsing outputs")
                    .map_err(|e| anyhow!("{}: tx_hash={}", e, tx.hash))?;
            parsed.push((output_index_i16, cell));
            continue;
        }

        let Some(type_script) = output.type_.as_ref() else {
            continue;
        };

        let type_script_hash = ScriptParser::compute_script_hash(type_script).map_err(|e| {
            anyhow!(
                "compute_script_hash failed for type script in tx {}: {}",
                tx.hash,
                e
            )
        })?;
        let standard_hint = if let Some(cached) = standard_cache.get(&type_script_hash) {
            cached.clone()
        } else {
            let looked_up = standard_lookup(&type_script_hash)?;
            standard_cache.insert(type_script_hash.clone(), looked_up.clone());
            looked_up
        };

        let Some(standard_hint) = standard_hint else {
            continue;
        };

        if let Some(cell) =
            UdtParser::parse_udt_cell_with_standard_hint(output, data_hex, Some(&standard_hint))
        {
            let output_index_i16 = checked_usize_to_i16(
                output_index,
                "UDT output index while parsing hinted outputs",
            )
            .map_err(|e| anyhow!("{}: tx_hash={}", e, tx.hash))?;
            parsed.push((output_index_i16, cell));
        }
    }

    Ok(parsed)
}

type UdtInputInfo = (Vec<u8>, Vec<u8>, i16, Vec<u8>, Vec<u8>, u128, String);

/// Resolve input UDT cells only from live_cells (plus same-batch cells at call sites).
/// Intentionally never trusts in-memory UDT cache for input validity because cache entries can
/// outlive cell consumption and reintroduce spent outpoints.
fn resolve_input_udt_info_from_live_cells(
    writer: &BatchWriter,
    udt_cache: &DashMap<([u8; 32], i16), CachedUdtCellInfo>,
    all_input_outpoints_udt: &[(Vec<u8>, i16)],
) -> Result<HashMap<(Vec<u8>, i16), UdtInputInfo>> {
    if all_input_outpoints_udt.is_empty() {
        return Ok(HashMap::new());
    }

    let unique_outpoints: Vec<(Vec<u8>, i16)> = {
        let mut seen = HashSet::new();
        all_input_outpoints_udt
            .iter()
            .filter_map(|x| {
                if seen.insert(x.clone()) {
                    Some(x.clone())
                } else {
                    None
                }
            })
            .collect()
    };

    if unique_outpoints.is_empty() {
        return Ok(HashMap::new());
    }

    let outpoint_refs: Vec<(&[u8], i16)> = unique_outpoints
        .iter()
        .map(|(h, i)| (h.as_slice(), *i))
        .collect();
    let db_results = writer.get_udt_cells_info_batch(&outpoint_refs)?;

    for ((tx_hash, idx), (tsh, tch, tht, ta, lsh, am, std)) in &db_results {
        let key = tx_hash_key32(
            tx_hash,
            "resolve_input_udt_info_from_live_cells cache insert",
        )?;
        udt_cache.insert(
            (key, *idx),
            CachedUdtCellInfo {
                type_script_hash: tsh.clone(),
                type_code_hash: tch.clone(),
                type_hash_type: *tht,
                type_args: ta.clone(),
                lock_script_hash: lsh.clone(),
                amount: *am,
                standard: std.clone(),
            },
        );
    }

    if udt_cache.len() > UDT_CELL_CACHE_CAPACITY * 2 {
        udt_cache.clear();
    }

    Ok(db_results)
}

fn classify_unresolved_local_probe(
    writer: &BatchWriter,
    unresolved_outpoints: &[(Vec<u8>, i16)],
    sample_limit: usize,
) -> UnresolvedLocalProbeSummary {
    let mut summary = UnresolvedLocalProbeSummary::default();
    let sampled = unresolved_outpoints.iter().take(sample_limit);
    let store = writer.store();

    for (tx_hash, output_index) in sampled {
        summary.sampled += 1;
        let outpoint_label = format!("0x{}:{}", short_tx_hash(tx_hash), output_index);

        let live_exists = match store.get_cell(tx_hash, *output_index) {
            Ok(Some(_)) => true,
            Ok(None) => false,
            Err(_) => {
                summary.store_errors += 1;
                summary
                    .sample_details
                    .push(format!("{}=live_read_error", outpoint_label));
                continue;
            }
        };
        if live_exists {
            summary.live_hits += 1;
            summary
                .sample_details
                .push(format!("{}=live_cell_exists", outpoint_label));
            continue;
        }

        let consumed_exists = match store.get_consumed_cell(tx_hash, *output_index) {
            Ok(Some(_)) => true,
            Ok(None) => false,
            Err(_) => {
                summary.store_errors += 1;
                summary
                    .sample_details
                    .push(format!("{}=consumed_read_error", outpoint_label));
                continue;
            }
        };
        if consumed_exists {
            summary.consumed_hits += 1;
            summary
                .sample_details
                .push(format!("{}=consumed_cell_exists", outpoint_label));
            continue;
        }

        match store.get_tx_location(tx_hash) {
            Ok(Some((block_num, tx_idx))) => {
                summary.tx_location_hits += 1;
                summary.sample_details.push(format!(
                    "{}=tx_location_exists({}:{})",
                    outpoint_label, block_num, tx_idx
                ));
            }
            Ok(None) => {
                summary.missing_everywhere += 1;
                summary
                    .sample_details
                    .push(format!("{}=tx_location_missing", outpoint_label));
            }
            Err(_) => {
                summary.store_errors += 1;
                summary
                    .sample_details
                    .push(format!("{}=tx_location_read_error", outpoint_label));
            }
        }
    }

    summary
}

async fn collect_unresolved_rpc_probe(
    rpc: &CkbRpcClient,
    unresolved_outpoints: &[(Vec<u8>, i16)],
    sample_limit: usize,
) -> UnresolvedRpcProbeSummary {
    let mut summary = UnresolvedRpcProbeSummary::default();
    let mut seen = HashSet::new();
    let mut sampled_hashes: Vec<Vec<u8>> = Vec::new();
    for (tx_hash, _) in unresolved_outpoints {
        if seen.insert(tx_hash.clone()) {
            sampled_hashes.push(tx_hash.clone());
        }
        if sampled_hashes.len() >= sample_limit {
            break;
        }
    }

    for tx_hash in sampled_hashes {
        summary.sampled_tx_hashes += 1;
        let tx_hash_hex = format!("0x{}", hex::encode(&tx_hash));
        let tx_hash_short = short_tx_hash(&tx_hash);
        match rpc.get_transaction(&tx_hash_hex).await {
            Ok(Some(tx_with_status)) => {
                let status = tx_with_status.tx_status.status.to_lowercase();
                let block_number = tx_with_status
                    .tx_status
                    .block_number
                    .unwrap_or_else(|| "none".to_string());
                match status.as_str() {
                    "committed" => summary.committed += 1,
                    "pending" => summary.pending += 1,
                    "proposed" => summary.proposed += 1,
                    "rejected" => summary.rejected += 1,
                    _ => summary.unknown_status += 1,
                }
                summary
                    .sample_details
                    .push(format!("0x{}={}#{}", tx_hash_short, status, block_number));
            }
            Ok(None) => {
                summary.rpc_null += 1;
                summary
                    .sample_details
                    .push(format!("0x{}=rpc_null", tx_hash_short));
            }
            Err(_) => {
                summary.rpc_errors += 1;
                summary
                    .sample_details
                    .push(format!("0x{}=rpc_error", tx_hash_short));
            }
        }
    }

    summary
}

fn commit_phase_no_wal(
    phase: &'static str,
    batch_start: i64,
    batch_end: i64,
    batch: StoreBatch<'_>,
) -> Result<f64> {
    debug!(phase, batch_start, batch_end, "Bulk phase commit start");
    let commit_started = Instant::now();
    batch.commit_no_wal().with_context(|| {
        format!(
            "bulk phase commit failed: phase={} blocks {}-{}",
            phase, batch_start, batch_end
        )
    })?;
    let commit_ms = commit_started.elapsed().as_secs_f64() * 1000.0;
    if commit_ms >= BULK_PHASE_COMMIT_SLOW_WARN_MS {
        warn!(
            phase,
            batch_start,
            batch_end,
            commit_ms = format!("{:.1}", commit_ms),
            "Bulk phase commit slow"
        );
    } else {
        debug!(
            phase,
            batch_start,
            batch_end,
            commit_ms = format!("{:.1}", commit_ms),
            "Bulk phase commit done"
        );
    }
    Ok(commit_ms)
}

fn should_abort_unresolved_retry_on_epoch_change(batch_epoch: u64, current_epoch: u64) -> bool {
    batch_epoch != current_epoch
}

fn require_non_negative_block_number(value: i64, context: &str) -> Result<u64> {
    u64::try_from(value).map_err(|_| anyhow!("negative block number in {}: {}", context, value))
}

fn next_start_block_from_db_tip(
    db_tip: i64,
    db_tip_hash: &Option<Vec<u8>>,
    context: &str,
) -> Result<u64> {
    if db_tip == 0 && db_tip_hash.is_none() {
        return Ok(0);
    }

    let db_tip_u64 = require_non_negative_block_number(db_tip, context)?;
    db_tip_u64
        .checked_add(1)
        .ok_or_else(|| anyhow!("db_tip overflow in {}: {}", context, db_tip))
}

fn blocks_behind_tip(chain_tip: u64, base_tip: i64, context: &str) -> Result<u64> {
    let base_tip_u64 = require_non_negative_block_number(base_tip, context)?;
    chain_tip.checked_sub(base_tip_u64).ok_or_else(|| {
        anyhow!(
            "invalid tip ordering in {}: base_tip={} exceeds chain_tip={}",
            context,
            base_tip,
            chain_tip
        )
    })
}

fn require_chain_tip_number(tip: Option<u64>, source: &str) -> Result<u64> {
    tip.ok_or_else(|| anyhow!("Failed to get chain tip from {}", source))
}

fn load_optional_index_from_store<T, F>(
    cache: &mut HashMap<Vec<u8>, Option<T>>,
    type_script_hash: &[u8],
    index_name: &str,
    load: F,
) -> Result<Option<T>>
where
    T: Clone,
    F: FnOnce() -> Result<Option<T>>,
{
    if let Some(cached) = cache.get(type_script_hash) {
        return Ok(cached.clone());
    }

    let loaded = load().with_context(|| {
        format!(
            "failed to load {} index: type_script_hash=0x{}",
            index_name,
            hex::encode(type_script_hash)
        )
    })?;
    cache.insert(type_script_hash.to_vec(), loaded.clone());
    Ok(loaded)
}

fn load_latest_dao_daily_snapshot(store: &CkbadgerStore) -> Result<Option<DaoDailySnapshot>> {
    let snapshots = store
        .list_dao_daily_snapshots()
        .context("failed to list dao daily snapshots while building cumulative snapshot")?;
    Ok(snapshots.last().cloned())
}

fn startup_header_gap_fail_fast_message(
    first_header_gap: i64,
    start_block: i64,
    header_tip: Option<i64>,
    tx_tip: Option<i64>,
) -> String {
    format!(
        "startup fail-fast: detected internal block header gap at block {} (sync_tip={}, header_tip={:?}, tx_tip={:?}). \
         automatic gap replay is disabled because it is equivalent to deep reorg handling; delete RocksDB and re-sync from genesis",
        first_header_gap, start_block, header_tip, tx_tip
    )
}

fn mempool_short_tx_id(tx_hash: &str) -> Result<&str> {
    let raw_hash = tx_hash.strip_prefix("0x").ok_or_else(|| {
        anyhow!(
            "mempool tx hash missing 0x prefix in proposal cache: tx_hash={}",
            tx_hash
        )
    })?;
    if raw_hash.len() < 20 {
        bail!(
            "mempool tx hash too short in proposal cache: tx_hash={}, hex_len={}",
            tx_hash,
            raw_hash.len()
        );
    }
    if !raw_hash.is_ascii() {
        bail!(
            "mempool tx hash must be ASCII hex in proposal cache: tx_hash={}",
            tx_hash
        );
    }
    Ok(&raw_hash[..20])
}

fn collect_committed_proposal_ids(txs: &[TxData]) -> Vec<String> {
    let mut ids = HashSet::new();
    for tx in txs {
        if tx.is_cellbase {
            continue;
        }
        // CKB proposal id is the first 10 bytes (20 hex chars) of tx hash.
        ids.insert(hex::encode(&tx.hash[..10]));
    }

    let mut collected: Vec<String> = ids.into_iter().collect();
    collected.sort();
    collected
}

fn bump_pipeline_reset_epoch(epoch: &AtomicU64) -> u64 {
    epoch.fetch_add(1, Ordering::SeqCst) + 1
}

const STARTUP_CONTINUITY_WINDOW_BLOCKS: i64 = 512;

fn block_time_to_bucket(block_time_seconds: i64) -> i32 {
    if block_time_seconds < 1 {
        0
    } else if block_time_seconds < 30 {
        block_time_seconds as i32
    } else {
        30
    }
}

fn truncate_to_hour(dt: DateTime<Utc>) -> DateTime<Utc> {
    dt.with_minute(0)
        .and_then(|d| d.with_second(0))
        .and_then(|d| d.with_nanosecond(0))
        .unwrap_or(dt)
}

type ScriptUsageChanges = HashMap<(Vec<u8>, bool), (i64, i64, i128, i128, i128, i128)>;

fn parse_blocks_parallel(
    blocks: &[BlockResponseWithCycles],
) -> Result<(
    Vec<crate::parser::block::ParsedBlock>,
    Vec<TxData>,
    Vec<(Vec<u8>, i16)>,
)> {
    let parsed_results_raw: Vec<Result<(usize, crate::parser::block::ParsedBlock, Vec<TxData>)>> =
        blocks
            .par_iter()
            .enumerate()
            .map(|(block_idx, block_response)| -> Result<_> {
                let block = &block_response.block;
                let parsed = BlockParser::parse(block);
                let tx_data_for_block_raw: Vec<Result<TxData>> = block
                    .transactions
                    .par_iter()
                    .enumerate()
                    .map(|(tx_index, tx)| -> Result<_> {
                        let parsed_tx = TransactionParser::parse(tx).map_err(|e| {
                            anyhow!(
                                "failed to parse tx metadata for tx {} in block {}: {}",
                                tx.hash,
                                parsed.number,
                                e
                            )
                        })?;
                        let inputs = TransactionParser::parse_inputs(tx).map_err(|e| {
                            anyhow!(
                                "failed to parse tx inputs for tx {} in block {}: {}",
                                tx.hash,
                                parsed.number,
                                e
                            )
                        })?;
                        let cells = CellParser::parse_outputs(tx).map_err(|e| {
                            anyhow!(
                                "failed to parse tx outputs for tx {} in block {}: {}",
                                tx.hash,
                                parsed.number,
                                e
                            )
                        })?;
                        let witnesses: Vec<String> = tx.witnesses.clone();
                        let outputs_data: Vec<String> = tx.outputs_data.clone();
                        let total_output_capacity: i64 = cells.iter().map(|c| c.capacity).sum();
                        let cycles = if tx_index == 0 {
                            None
                        } else {
                            parse_tx_cycles(
                                block_response
                                    .cycles
                                    .as_ref()
                                    .and_then(|c| c.get(tx_index - 1)),
                                &tx.hash,
                                parsed.number,
                            )?
                        };
                        Ok(TxData {
                            hash: parsed_tx.hash,
                            block_number: parsed.number,
                            block_hash: parsed.hash.clone(),
                            tx_index: tx_index as i32,
                            version: parsed_tx.version,
                            inputs_count: i16::try_from(parsed_tx.inputs_count).map_err(|_| {
                                anyhow!(
                                    "tx inputs count exceeds i16 range: tx_hash=0x{}, block={}, inputs_count={}",
                                    hex::encode(parsed_tx.hash),
                                    parsed.number,
                                    parsed_tx.inputs_count
                                )
                            })?,
                            outputs_count: i16::try_from(parsed_tx.outputs_count).map_err(|_| {
                                anyhow!(
                                    "tx outputs count exceeds i16 range: tx_hash=0x{}, block={}, outputs_count={}",
                                    hex::encode(parsed_tx.hash),
                                    parsed.number,
                                    parsed_tx.outputs_count
                                )
                            })?,
                            witnesses_count: i16::try_from(parsed_tx.witnesses_count).map_err(
                                |_| {
                                    anyhow!(
                                        "tx witnesses count exceeds i16 range: tx_hash=0x{}, block={}, witnesses_count={}",
                                        hex::encode(parsed_tx.hash),
                                        parsed.number,
                                        parsed_tx.witnesses_count
                                    )
                                },
                            )?,
                            cell_deps_count: i16::try_from(parsed_tx.cell_deps_count).map_err(
                                |_| {
                                    anyhow!(
                                        "tx cell_deps count exceeds i16 range: tx_hash=0x{}, block={}, cell_deps_count={}",
                                        hex::encode(parsed_tx.hash),
                                        parsed.number,
                                        parsed_tx.cell_deps_count
                                    )
                                },
                            )?,
                            header_deps_count: i16::try_from(parsed_tx.header_deps_count)
                                .map_err(|_| {
                                    anyhow!(
                                        "tx header_deps count exceeds i16 range: tx_hash=0x{}, block={}, header_deps_count={}",
                                        hex::encode(parsed_tx.hash),
                                        parsed.number,
                                        parsed_tx.header_deps_count
                                    )
                                })?,
                            is_cellbase: parsed_tx.is_cellbase,
                            inputs,
                            cells,
                            witnesses,
                            outputs_data,
                            total_input_capacity: 0,
                            total_output_capacity,
                            fee: 0,
                            tx_size: parsed_tx.tx_size,
                            cycles,
                            timestamp: parsed.timestamp,
                        })
                    })
                    .collect();
                let mut tx_data_for_block = Vec::with_capacity(tx_data_for_block_raw.len());
                for tx_data in tx_data_for_block_raw {
                    tx_data_for_block.push(tx_data?);
                }
                tx_data_for_block.sort_by_key(|td| td.tx_index);
                Ok((block_idx, parsed, tx_data_for_block))
            })
            .collect();
    let mut parsed_results: Vec<(usize, crate::parser::block::ParsedBlock, Vec<TxData>)> =
        Vec::with_capacity(parsed_results_raw.len());
    for parsed in parsed_results_raw {
        parsed_results.push(parsed?);
    }
    parsed_results.sort_by_key(|(idx, _, _)| *idx);

    let mut all_parsed_blocks = Vec::with_capacity(parsed_results.len());
    let mut all_tx_data = Vec::new();
    let mut all_input_outpoints = Vec::new();
    for (_, parsed, tx_data_list) in parsed_results {
        for tx_data in &tx_data_list {
            if !tx_data.is_cellbase {
                for input in &tx_data.inputs {
                    let previous_output_index =
                        i16::try_from(input.previous_output_index).map_err(|_| {
                            anyhow!(
                                "input previous_output_index exceeds i16 range while collecting outpoints: tx_hash=0x{}, block={}, previous_output_index={}",
                                hex::encode(tx_data.hash),
                                tx_data.block_number,
                                input.previous_output_index
                            )
                        })?;
                    all_input_outpoints
                        .push((input.previous_tx_hash.to_vec(), previous_output_index));
                }
            }
        }
        all_tx_data.extend(tx_data_list);
        all_parsed_blocks.push(parsed);
    }
    Ok((all_parsed_blocks, all_tx_data, all_input_outpoints))
}

const CACHE_INVALIDATION_INTERVAL: u64 = 10_000;
pub struct Indexer {
    run_id: String,
    config: Config,
    rpc: CkbRpcClient,
    repo: Repository,
    writer: BatchWriter,
    append_only_store: Arc<CkbadgerStore>,
    progress: Arc<SyncProgress>,
    cell_cache: Arc<DashMap<([u8; 32], i16), CachedCellInfo>>,
    udt_cell_cache: Arc<DashMap<([u8; 32], i16), CachedUdtCellInfo>>,
    perf: PerfStats,
    pipeline_perf: Arc<PipelinePerfStats>,
    adaptive_batch_controller: Arc<AdaptiveBatchController>,
    cache_invalidator: CacheInvalidator,
    last_cache_invalidation: tokio::sync::Mutex<u64>,
    was_bulk_sync_active: std::sync::atomic::AtomicBool,
    rebuild_pause_flag: Arc<std::sync::atomic::AtomicBool>,
    pipeline_reset_notify_flag: Arc<std::sync::atomic::AtomicBool>,
    pipeline_reset_reason_code: Arc<AtomicU8>,
    startup_phase: AtomicU8,
    pipeline_reset_epoch: Arc<AtomicU64>,
    incident_seq: AtomicU64,
    flight_recorder: FlightRecorder,
    repeated_warning_tracker: RepeatedWarningTracker,
    incident_dir: PathBuf,
    label_import_started: std::sync::atomic::AtomicBool,
    ckb_store: Option<Arc<CkbChainReader>>,
    hodl_tracker: std::sync::Mutex<HodlWaveTracker>,
}

impl Indexer {
    pub async fn new(
        run_id: String,
        config: Config,
        store: Arc<CkbadgerStore>,
        append_only_store: Arc<CkbadgerStore>,
    ) -> Result<Self> {
        let rpc = CkbRpcClient::new(&config.ckb_rpc_url);
        let cache_invalidator = CacheInvalidator::new(config.redis_url.as_deref()).await;

        let ckb_store = match config.ckb_data_path.as_deref() {
            Some(path) => {
                let reader = CkbChainReader::open(path)?;
                info!("CKB direct RocksDB reader opened at {}", path);
                Some(Arc::new(reader))
            }
            None => None,
        };
        let repo = Repository::with_cache(store.clone(), cache_invalidator.clone());
        let writer = BatchWriter::with_cache(
            store.clone(),
            config.fast_sync_mode,
            cache_invalidator.clone(),
        );

        let (tip_number, _) = repo.get_sync_tip().await?;
        let chain_tip = if let Some(ref store) = ckb_store {
            require_chain_tip_number(store.tip_number(), "CKB RocksDB during indexer startup")?
        } else {
            rpc.get_tip_block_number().await?
        };

        let tip_number_u64 =
            require_non_negative_block_number(tip_number, "indexer startup sync tip")?;
        let progress = Arc::new(SyncProgress::new(tip_number_u64, chain_tip));
        progress.start_refresher();
        let cell_cache = Arc::new(DashMap::with_capacity(CELL_CACHE_CAPACITY));
        let udt_cell_cache = Arc::new(DashMap::with_capacity(UDT_CELL_CACHE_CAPACITY));
        let adaptive_batch_controller =
            Arc::new(AdaptiveBatchController::new(config.pipeline_buffer as u64));

        let was_bulk = progress.blocks_remaining() > config.bulk_sync_threshold;
        let hodl_tracker = match store.get_hodl_tracker_state()? {
            Some(state) => {
                info!(
                    "Restored HODL tracker: {} date entries, {} transitions, holder_count={}",
                    state.capacity_by_date.len(),
                    state.date_transitions.len(),
                    state.holder_count,
                );
                HodlWaveTracker::from_state(state)
            }
            None => {
                info!("Starting fresh HODL wave tracker");
                HodlWaveTracker::new()
            }
        };

        let incident_dir = PathBuf::from(&config.domain_data_path).join("incidents");

        Ok(Self {
            run_id,
            config,
            rpc,
            repo,
            writer,
            append_only_store,
            progress,
            cell_cache,
            udt_cell_cache,
            perf: PerfStats::default(),
            pipeline_perf: Arc::new(PipelinePerfStats::default()),
            adaptive_batch_controller,
            cache_invalidator,
            last_cache_invalidation: tokio::sync::Mutex::new(0),
            was_bulk_sync_active: std::sync::atomic::AtomicBool::new(was_bulk),
            rebuild_pause_flag: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            pipeline_reset_notify_flag: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            pipeline_reset_reason_code: Arc::new(AtomicU8::new(PIPELINE_RESET_REASON_UNKNOWN)),
            startup_phase: AtomicU8::new(STARTUP_PHASE_NONE),
            pipeline_reset_epoch: Arc::new(AtomicU64::new(0)),
            incident_seq: AtomicU64::new(0),
            flight_recorder: FlightRecorder::new(FLIGHT_RECORDER_CAPACITY),
            repeated_warning_tracker: RepeatedWarningTracker::default(),
            incident_dir,
            label_import_started: std::sync::atomic::AtomicBool::new(false),
            ckb_store,
            hodl_tracker: std::sync::Mutex::new(hodl_tracker),
        })
    }

    pub fn progress(&self) -> Arc<SyncProgress> {
        Arc::clone(&self.progress)
    }

    pub fn cache_invalidator(&self) -> &CacheInvalidator {
        &self.cache_invalidator
    }

    pub fn writer(&self) -> &BatchWriter {
        &self.writer
    }

    /// Parse UDT outputs from a transaction, with a fallback for label-known
    /// token standards such as `xudt_compatible`.
    fn parse_udt_cells_with_store_fallback(
        &self,
        tx: &crate::rpc::TransactionView,
    ) -> Result<Vec<(i16, crate::parser::ParsedUdtCell)>> {
        parse_udt_cells_with_store_fallback_inner(tx, |type_script_hash| {
            self.writer
                .store()
                .get_token(type_script_hash)
                .map(|info| info.map(|token| token.standard))
        })
    }

    pub fn rebuild_pause_flag(&self) -> Arc<std::sync::atomic::AtomicBool> {
        Arc::clone(&self.rebuild_pause_flag)
    }

    pub fn is_bulk_sync_active(&self) -> bool {
        self.progress.blocks_remaining() > self.config.bulk_sync_threshold
    }

    /// Dynamically switch RocksDB compaction options based on how far behind tip we are.
    ///
    /// - **Enter bulk**: blocks_behind > threshold and not already in bulk compaction mode.
    /// - **Exit bulk**: blocks_behind <= threshold and currently in bulk compaction mode,
    ///   BUT only if compaction pressure has drained (L0 files < 10, pending < 2 GB).
    ///   Otherwise defers the transition and logs.
    fn ensure_compaction_mode(&self, blocks_behind: u64) {
        let domain_store = self.writer.store();
        let append_store = &self.append_only_store;
        let in_bulk = domain_store.is_bulk_sync_mode();
        let should_be_bulk = blocks_behind > self.config.bulk_sync_threshold;

        if should_be_bulk && !in_bulk {
            info!(
                blocks_behind,
                threshold = self.config.bulk_sync_threshold,
                "Re-entering bulk compaction mode"
            );
            domain_store.set_bulk_sync_compaction_options();
            append_store.set_bulk_sync_compaction_options();
        } else if !should_be_bulk && in_bulk {
            let (l0_files_max, compaction_pending_bytes, _imm) = domain_store.compaction_pressure();
            const DRAIN_L0_THRESHOLD: u64 = 10;
            let drain_pending_threshold =
                domain_store.memory_profile().drain_pending_bytes_threshold;
            if l0_files_max < DRAIN_L0_THRESHOLD
                && compaction_pending_bytes < drain_pending_threshold
            {
                info!(
                    l0_files_max,
                    compaction_pending_mb = compaction_pending_bytes / (1024 * 1024),
                    "Compaction drained, restoring normal compaction options"
                );
                domain_store.restore_normal_compaction_options();
                append_store.restore_normal_compaction_options();
            } else {
                debug!(
                    l0_files_max,
                    compaction_pending_mb = compaction_pending_bytes / (1024 * 1024),
                    "Deferring normal compaction: pressure still high"
                );
            }
        }
    }

    pub fn is_direct_db_read(&self) -> bool {
        self.ckb_store.is_some()
    }

    pub fn ckb_store(&self) -> Option<Arc<CkbChainReader>> {
        self.ckb_store.clone()
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    pub fn record_runtime_heartbeat(
        &self,
        current_block: u64,
        target_block: u64,
        stage: Option<&str>,
    ) {
        let current_block_i64 = match i64::try_from(current_block) {
            Ok(v) => v,
            Err(_) => {
                warn!(
                    run_id = %self.run_id,
                    current_block,
                    "Skipping runtime heartbeat: current_block exceeds i64 range"
                );
                return;
            }
        };
        let target_block_i64 = match i64::try_from(target_block) {
            Ok(v) => v,
            Err(_) => {
                warn!(
                    run_id = %self.run_id,
                    target_block,
                    "Skipping runtime heartbeat: target_block exceeds i64 range"
                );
                return;
            }
        };
        let cgroup = read_cgroup_memory_snapshot();
        if let Err(e) = self.writer.store().mark_runtime_heartbeat_with_diag(
            &self.run_id,
            current_block_i64,
            target_block_i64,
            stage,
            cgroup.oom_events,
            cgroup.oom_kill_events,
        ) {
            warn!(
                run_id = %self.run_id,
                current_block,
                target_block,
                error = %e,
                "Failed to persist runtime heartbeat"
            );
        }
    }

    pub fn mark_runtime_shutdown(&self, reason: &str, exit_code: i32) {
        if let Err(e) = self
            .writer
            .store()
            .mark_runtime_shutdown(&self.run_id, reason, exit_code)
        {
            warn!(
                run_id = %self.run_id,
                reason,
                exit_code,
                error = %e,
                "Failed to persist runtime shutdown reason"
            );
        }
    }

    fn record_flight_event(&self, event: &str, detail: impl Into<String>) {
        self.flight_recorder.record(event, detail);
    }

    fn next_incident_id(&self) -> String {
        let sequence = self.incident_seq.fetch_add(1, Ordering::SeqCst) + 1;
        generate_incident_id(&self.run_id, sequence)
    }

    fn write_incident_report(
        &self,
        incident_id: &str,
        reason: &str,
        detail: &str,
    ) -> anyhow::Result<PathBuf> {
        let sync_status = self.writer.store().get_sync_status()?;
        let report = IncidentReport {
            incident_id: incident_id.to_string(),
            run_id: self.run_id.clone(),
            created_at: chrono::Utc::now().timestamp(),
            reason: reason.to_string(),
            detail: detail.to_string(),
            startup_phase: self.startup_phase(),
            pipeline_reset_epoch: self.pipeline_reset_epoch.load(Ordering::SeqCst),
            sync_tip_block: sync_status.tip_block_number,
            sync_tip_hash: if sync_status.tip_block_hash.is_empty() {
                "0x".to_string()
            } else {
                format!("0x{}", hex::encode(sync_status.tip_block_hash))
            },
            cgroup_memory: read_cgroup_memory_snapshot(),
            recent_events: self.flight_recorder.snapshot(),
        };

        std::fs::create_dir_all(&self.incident_dir)?;
        let path = self.incident_dir.join(format!("{}.json", incident_id));
        let encoded = serde_json::to_vec_pretty(&report)?;
        std::fs::write(&path, encoded)?;
        Ok(path)
    }

    fn report_incident(&self, reason: &str, detail: impl Into<String>) -> String {
        let detail = detail.into();
        let incident_id = self.next_incident_id();
        self.record_flight_event(
            "incident",
            format!(
                "incident_id={} reason={} detail={}",
                incident_id, reason, detail
            ),
        );

        if let Err(e) =
            self.writer
                .store()
                .mark_runtime_incident(&self.run_id, &incident_id, reason)
        {
            warn!(
                run_id = %self.run_id,
                incident_id = %incident_id,
                error = %e,
                "Failed to persist runtime incident marker"
            );
        }

        match self.write_incident_report(&incident_id, reason, &detail) {
            Ok(path) => {
                info!(
                    run_id = %self.run_id,
                    incident_id = %incident_id,
                    path = %path.display(),
                    "Incident report written"
                );
            }
            Err(e) => {
                warn!(
                    run_id = %self.run_id,
                    incident_id = %incident_id,
                    error = %e,
                    "Failed to write incident report"
                );
            }
        }

        incident_id
    }

    fn repeated_warning_snapshot(
        &self,
        key: &'static str,
        min_emit_interval: Duration,
    ) -> Option<RepeatedWarningSnapshot> {
        self.repeated_warning_tracker.record(key, min_emit_interval)
    }

    fn request_pipeline_reset(
        &self,
        reason: &'static str,
        expected_start: Option<u64>,
        got_start: Option<u64>,
        writer_queue_depth: Option<usize>,
    ) {
        let reason_code = encode_pipeline_reset_reason(reason);
        let epoch = bump_pipeline_reset_epoch(&self.pipeline_reset_epoch);
        self.pipeline_reset_reason_code
            .store(reason_code, Ordering::SeqCst);
        self.pipeline_reset_notify_flag
            .store(true, Ordering::SeqCst);
        info!(
            run_id = %self.run_id,
            epoch,
            reason,
            reason_code,
            expected_start = ?expected_start,
            got_start = ?got_start,
            writer_queue_depth = ?writer_queue_depth,
            "Pipeline reset requested"
        );
        self.record_flight_event(
            "pipeline_reset",
            format!(
                "epoch={} reason={} expected_start={:?} got_start={:?} writer_queue_depth={:?}",
                epoch, reason, expected_start, got_start, writer_queue_depth
            ),
        );
    }

    /// Snapshot the current perf stats: (fetch_ms, db_stage_write_ms, db_commit_ms).
    pub fn perf_snapshot_ms(&self) -> (f64, f64, f64) {
        self.perf.snapshot_ms()
    }

    pub fn pipeline_progress_snapshot(&self) -> Option<PipelineProgressData> {
        if !self.config.pipeline_enabled {
            return None;
        }
        self.pipeline_perf.snapshot()
    }

    pub fn adaptive_batch_snapshot(&self) -> Option<AdaptiveBatchProgressSnapshot> {
        if !self.config.pipeline_enabled {
            return None;
        }
        let snapshot = self.adaptive_batch_controller.snapshot();
        Some(AdaptiveBatchProgressSnapshot {
            target_batch_txs: snapshot.target_batch_txs,
            inflight_limit: snapshot.inflight_limit,
            min_target_batch_txs: snapshot.min_target_batch_txs,
            cooldown_steps: snapshot.cooldown_steps,
            last_reason: decode_adaptive_batch_reason(snapshot.last_reason_code)
                .map(str::to_string),
            adjustment_seq: snapshot.adjustment_seq,
            backoff_streak: snapshot.backoff_streak,
            last_adjusted_at: snapshot.last_adjusted_at,
        })
    }

    pub fn pipeline_reset_snapshot(&self) -> Option<(u64, String)> {
        if !self.config.pipeline_enabled {
            return None;
        }
        let epoch = self.pipeline_reset_epoch.load(Ordering::SeqCst);
        if epoch == 0 {
            return None;
        }
        let reason =
            decode_pipeline_reset_reason(self.pipeline_reset_reason_code.load(Ordering::SeqCst))
                .to_string();
        Some((epoch, reason))
    }

    pub fn startup_phase(&self) -> Option<String> {
        decode_startup_phase(self.startup_phase.load(Ordering::SeqCst)).map(str::to_string)
    }

    pub fn get_memory_stats(&self) -> ckbadger_common::MemoryStatsData {
        let stats = self.writer.store().memory_stats();
        let sync_status = self.writer.store().get_sync_status().unwrap_or_else(|e| {
            panic!(
                "failed to read sync_status while collecting memory stats: {}",
                e
            )
        });
        ckbadger_common::MemoryStatsData {
            live_cells_count: stats.live_cells_count as u64,
            consumed_cells_count: stats.consumed_cells_count as u64,
            consumed_cells_bytes: stats.consumed_cells_bytes as u64,
            consumed_cells_bytes_source: stats.consumed_cells_bytes_source.to_string(),
            rocksdb_memtable_bytes: stats.memtable_bytes as u64,
            rocksdb_block_cache_bytes: stats.block_cache_bytes as u64,
            rocksdb_table_readers_bytes: stats.table_readers_bytes as u64,
            rocksdb_total_bytes: stats.memory_bytes as u64,
            block_headers_count: stats.block_headers_count as u64,
            bulk_sync_cell_cache_enabled: false,
            bulk_sync_mode: self.is_bulk_sync_active(),
            compaction_pending_bytes: stats.compaction_pending_bytes,
            num_running_compactions: stats.num_running_compactions,
            sst_files_size: stats.sst_files_size,
            l0_files_count: stats.l0_files_count,
            l0_files_max: stats.l0_files_max,
            l0_worst_cf: stats.l0_worst_cf,
            immutable_memtables: stats.immutable_memtables,
            top_cf_sizes: stats.top_cf_sizes,
            wbm_usage_bytes: stats.wbm_usage_bytes as u64,
            wbm_budget_bytes: stats.wbm_budget_bytes as u64,
            total_transactions: sync_status.total_transactions,
            total_cells: sync_status.total_cells_created,
            total_live_cells: sync_status.total_cells_created - sync_status.total_cells_consumed,
            total_addresses: i64::try_from(stats.addr_balance_count).unwrap_or_else(|_| {
                panic!(
                    "addr_balance_count over i64 range in memory stats: {}",
                    stats.addr_balance_count
                )
            }),
            updated_at: chrono::Utc::now().timestamp(),
        }
    }

    // === run / run_sequential / run_pipeline ===

    pub async fn run(&self) -> Result<()> {
        let blocks_behind = self.progress.blocks_remaining();
        let bulk_sync_mode =
            is_bulk_sync_active_by_lag(blocks_behind, self.config.bulk_sync_threshold);
        info!(
            run_id = %self.run_id,
            "Starting indexer (pipeline={}, {} blocks behind, threshold={})",
            self.config.pipeline_enabled, blocks_behind, self.config.bulk_sync_threshold
        );
        self.record_flight_event(
            "run_start",
            format!(
                "pipeline_enabled={} blocks_behind={} bulk_threshold={}",
                self.config.pipeline_enabled, blocks_behind, self.config.bulk_sync_threshold
            ),
        );

        if bulk_sync_mode {
            info!(
                run_id = %self.run_id,
                "Bulk sync auto-enabled: {} blocks behind > {} threshold",
                blocks_behind, self.config.bulk_sync_threshold,
            );
            self.writer.store().set_bulk_sync_compaction_options();
            self.append_only_store.set_bulk_sync_compaction_options();
        }

        let (start_block, start_block_hash) = self.repo.get_sync_tip().await?;
        ensure_bulk_sync_fresh_start(bulk_sync_mode, start_block, &start_block_hash)?;
        let consistent_block = self.writer.find_last_consistent_block()?;
        let actual_start = match consistent_block {
            Some(cb) if cb < start_block => {
                warn!(
                    "Rolling back from block {} to {} due to data inconsistency",
                    start_block, cb
                );
                cb
            }
            _ => start_block,
        };
        let continuity_probe = self.writer.probe_startup_continuity(
            actual_start,
            STARTUP_CONTINUITY_WINDOW_BLOCKS,
            true,
        )?;
        if continuity_probe.has_inconsistency() {
            warn!(
                run_id = %self.run_id,
                startup_tip = continuity_probe.startup_tip,
                header_tip = ?continuity_probe.header_tip,
                tx_floor = ?continuity_probe.tx_floor,
                tx_tip = ?continuity_probe.tx_tip,
                first_header_gap = ?continuity_probe.first_header_gap,
                window_start = continuity_probe.recent_window_start,
                window_end = continuity_probe.recent_window_end,
                missing_header_sample = ?continuity_probe.missing_header_sample,
                missing_tx_block0_sample = ?continuity_probe.missing_tx_block0_sample,
                full_header_gap_scan = continuity_probe.full_header_gap_scan,
                "Startup continuity probe detected inconsistencies"
            );
        } else {
            info!(
                run_id = %self.run_id,
                startup_tip = continuity_probe.startup_tip,
                header_tip = ?continuity_probe.header_tip,
                tx_floor = ?continuity_probe.tx_floor,
                tx_tip = ?continuity_probe.tx_tip,
                window_start = continuity_probe.recent_window_start,
                window_end = continuity_probe.recent_window_end,
                full_header_gap_scan = continuity_probe.full_header_gap_scan,
                "Startup continuity probe passed"
            );
        }

        if let Some(first_header_gap) = continuity_probe.first_header_gap {
            bail!(
                "{}",
                startup_header_gap_fail_fast_message(
                    first_header_gap,
                    start_block,
                    continuity_probe.header_tip,
                    continuity_probe.tx_tip
                )
            );
        }

        if bulk_sync_mode && actual_start < start_block {
            bail!(
                "bulk sync fail-fast: inconsistent local DB state detected at startup (sync_tip={}, recovery_start={}). \
                 bulk sync does not auto-rollback; delete RocksDB and restart from genesis",
                start_block,
                actual_start
            );
        }

        let cleanup_needed = self
            .writer
            .needs_startup_cleanup_with_force(actual_start, self.config.force_startup_cleanup)?;

        if bulk_sync_mode && cleanup_needed {
            bail!(
                "bulk sync fail-fast: startup rollback cleanup required from block {}. \
                 bulk sync does not auto-cleanup; delete RocksDB and restart from genesis",
                actual_start + 1
            );
        }
        if cleanup_needed {
            self.startup_phase
                .store(STARTUP_PHASE_ROLLBACK_CLEANUP, Ordering::SeqCst);
            info!(
                run_id = %self.run_id,
                from_block = actual_start + 1,
                "Startup rollback cleanup phase started"
            );
            self.record_flight_event(
                "startup_cleanup_started",
                format!("from_block={}", actual_start + 1),
            );
        }

        let init_result = self.writer.init_sync_start_with_options(
            actual_start,
            bulk_sync_mode,
            self.config.force_startup_cleanup,
        );

        self.startup_phase
            .store(STARTUP_PHASE_NONE, Ordering::SeqCst);
        if cleanup_needed {
            info!(
                run_id = %self.run_id,
                "Startup rollback cleanup phase completed"
            );
            self.record_flight_event("startup_cleanup_completed", "ok");
        }
        init_result?;
        if cleanup_needed {
            info!(
                run_id = %self.run_id,
                rollback_to = actual_start,
                "Startup undo-log rollback phase started"
            );
            self.writer
                .store()
                .rollback_via_undo_log(self.append_only_store.as_ref(), actual_start)?;
            info!(
                run_id = %self.run_id,
                rollback_to = actual_start,
                "Startup undo-log rollback phase completed"
            );
            if !self.writer.store().has_cf(CF_ADDR_TXS) {
                let rebuilt = self
                    .writer
                    .store()
                    .rebuild_addr_balances_from_live_cells_with_tx_index_store(Some(
                        self.append_only_store.as_ref(),
                    ))?;
                info!(
                    run_id = %self.run_id,
                    rebuilt,
                    "Address balances rebuilt from append-only addr_txs after startup cleanup"
                );
            }
        }
        self.reconcile_hodl_tracker_with_tip(actual_start)?;

        self.maybe_start_label_import();

        // Periodic 24h transfer refresh
        let store_for_task = Arc::clone(self.writer.store());
        let fast_sync_mode = self.config.fast_sync_mode;
        let progress_for_task = Arc::clone(&self.progress);
        let bulk_sync_threshold = self.config.bulk_sync_threshold;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(600));
            loop {
                interval.tick().await;
                let blocks_remaining = progress_for_task.blocks_remaining();
                if blocks_remaining > bulk_sync_threshold {
                    debug!(
                        "Skipping token 24h refresh ({} blocks remaining > {} threshold)",
                        blocks_remaining, bulk_sync_threshold
                    );
                    continue;
                }
                let writer =
                    BatchWriter::with_fast_sync_mode(store_for_task.clone(), fast_sync_mode);
                match writer.refresh_token_24h_transfers() {
                    Ok(count) => info!("Refreshed 24h transfers for {} tokens", count),
                    Err(e) => warn!("Failed to refresh token 24h transfers: {}", e),
                }
                match writer.refresh_mnft_24h_transfers() {
                    Ok(count) => info!("Refreshed 24h transfers for {} NFT classes", count),
                    Err(e) => warn!("Failed to refresh NFT 24h transfers: {}", e),
                }
            }
        });

        if self.config.pipeline_enabled {
            self.run_pipeline().await
        } else {
            self.run_sequential().await
        }
    }

    async fn run_sequential(&self) -> Result<()> {
        loop {
            if self.rebuild_pause_flag.load(Ordering::SeqCst) {
                debug!("Sync paused for index rebuild");
                sleep(Duration::from_millis(500)).await;
                continue;
            }

            // Bulk sync is an optimistic rebuild path and must not run reorg/deep-fork handling.
            let should_handle_reorg = should_run_reorg_handling(
                self.progress.blocks_remaining(),
                self.config.bulk_sync_threshold,
            );
            if should_handle_reorg && self.repo.has_unresolved_deep_fork()? {
                if let Some(repeat) = self.repeated_warning_snapshot(
                    "sequential_deep_fork_unresolved",
                    Duration::from_secs(120),
                ) {
                    warn!(
                        run_id = %self.run_id,
                        repeat_count = repeat.total_count,
                        suppressed_since_last = repeat.suppressed_since_last_emit,
                        first_seen_secs_ago = repeat.first_seen_secs_ago,
                        "Deep fork unresolved, sync paused. Waiting for manual intervention..."
                    );
                }
                sleep(Duration::from_secs(30)).await;
                continue;
            }

            match self.sync_batch().await {
                Ok(SyncAction::CaughtUp) => {
                    sleep(Duration::from_millis(self.config.poll_interval_ms)).await;
                }
                Ok(SyncAction::Continue) => {}
                Ok(SyncAction::ReorgHandled) => {
                    self.cell_cache.clear();
                    self.udt_cell_cache.clear();
                    let (reorg_tip, _) = self.repo.get_sync_tip().await?;
                    self.reconcile_hodl_tracker_with_tip(reorg_tip)?;
                    let new_epoch = bump_pipeline_reset_epoch(&self.pipeline_reset_epoch);
                    info!(
                        epoch = new_epoch,
                        reorg_tip,
                        "Reorg handled, caches cleared, HODL tracker reconciled, epoch bumped, continuing sync from fork point"
                    );
                }
                Ok(SyncAction::DeepForkPaused) => {
                    warn!("Deep fork detected, sync paused");
                    sleep(Duration::from_secs(30)).await;
                }
                Err(e) => {
                    let incident_id =
                        self.report_incident("sync_batch_failed", format!("error={:?}", e));
                    error!(
                        run_id = %self.run_id,
                        incident_id = %incident_id,
                        error = ?e,
                        "Sync error"
                    );
                    sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }

    async fn run_pipeline(&self) -> Result<()> {
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
                    if is_bulk_sync_active_by_lag(blocks_behind, config.bulk_sync_threshold) {
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
                let mut send_failed_reason: Option<String> = None;
                let tx_counts: Vec<usize> = blocks
                    .iter()
                    .map(|block| block.block.transactions.len())
                    .collect();
                adaptive_batch_controller_for_fetcher
                    .observe_tx_density(tx_counts.iter().sum(), tx_counts.len());
                let sub_batch_plan = plan_fetch_sub_batches(&tx_counts, max_txs);
                let mut block_iter = blocks.into_iter();
                let mut sub_start_block = start_block;

                for (idx, (sub_block_count, sub_txs)) in sub_batch_plan.into_iter().enumerate() {
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
            let should_handle_reorg = should_run_reorg_handling(
                self.progress.blocks_remaining(),
                self.config.bulk_sync_threshold,
            );
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
                    if should_run_reorg_handling(blocks_behind, self.config.bulk_sync_threshold) {
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

    async fn maybe_invalidate_chart_caches(&self, current_block: u64) {
        if !self.cache_invalidator.is_enabled() {
            return;
        }
        let blocks_remaining = self.progress.blocks_remaining();
        if !should_invalidate_chart_caches_for_lag(blocks_remaining) {
            return;
        }
        let mut last_invalidation = self.last_cache_invalidation.lock().await;
        if current_block >= *last_invalidation + CACHE_INVALIDATION_INTERVAL {
            self.cache_invalidator.invalidate_chart_caches().await;
            *last_invalidation = current_block;
        }
    }
    // === sync_batch, check_bulk_sync_completion, task submission ===

    async fn sync_batch(&self) -> Result<SyncAction> {
        let chain_tip = self.get_chain_tip().await?;
        self.progress.update_target(chain_tip);

        let (db_tip, db_tip_hash) = self.repo.get_sync_tip().await?;
        let start_block =
            next_start_block_from_db_tip(db_tip, &db_tip_hash, "sequential sync start_block")?;

        if start_block > chain_tip {
            return Ok(SyncAction::CaughtUp);
        }

        let blocks_behind = chain_tip.saturating_sub(start_block);
        if should_run_reorg_handling(blocks_behind, self.config.bulk_sync_threshold) {
            if let Some(ref stored_hash) = db_tip_hash {
                if db_tip > 0 {
                    let db_tip_u64 =
                        require_non_negative_block_number(db_tip, "sequential reorg tip")?;
                    match self.check_and_handle_reorg(db_tip_u64, stored_hash).await? {
                        Some(ReorgAction::Handled(_)) => return Ok(SyncAction::ReorgHandled),
                        Some(ReorgAction::DeepForkPaused) => return Ok(SyncAction::DeepForkPaused),
                        None => {}
                    }
                }
            }
        }

        let mut end_block =
            std::cmp::min(start_block + self.config.batch_size as u64 - 1, chain_tip);

        if start_block > end_block {
            return Ok(SyncAction::CaughtUp);
        }

        // Live sync accumulation
        if end_block == start_block && blocks_behind <= self.config.bulk_sync_threshold {
            let accumulation_timeout = Duration::from_secs(2);
            let max_accumulate = 5u64;
            let deadline = Instant::now() + accumulation_timeout;
            while Instant::now() < deadline {
                sleep(Duration::from_millis(200)).await;
                if let Ok(new_tip) = self.get_chain_tip().await {
                    if new_tip > end_block {
                        end_block = std::cmp::min(
                            start_block + max_accumulate - 1,
                            std::cmp::min(new_tip, start_block + self.config.batch_size as u64 - 1),
                        );
                        self.progress.update_target(new_tip);
                        if end_block - start_block + 1 >= max_accumulate {
                            break;
                        }
                    }
                }
            }
        }

        let fetch_start = Instant::now();
        let blocks = self.fetch_blocks_parallel(start_block, end_block).await?;
        self.perf.add_fetch(fetch_start.elapsed());

        let db_start = Instant::now();
        let write_metrics = match self.sync_blocks_batch(&blocks, chain_tip).await {
            Ok(metrics) => metrics,
            Err(e) => {
                let bulk_sync_mode =
                    is_bulk_sync_batch(chain_tip, end_block, self.config.bulk_sync_threshold);
                if bulk_sync_mode {
                    return Err(e).with_context(|| {
                        format!(
                            "bulk sync fail-fast for range {}-{} (chain_tip={}): \
                             no rollback cleanup/retry in bulk mode; delete RocksDB and restart from genesis",
                            start_block, end_block, chain_tip
                        )
                    });
                }
                self.cleanup_failed_batch_range(start_block, end_block, chain_tip, "sequential")?;
                return Err(e).with_context(|| {
                    format!(
                        "sync_blocks_batch failed for range {}-{} (chain_tip={})",
                        start_block, end_block, chain_tip
                    )
                });
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
                "Slow write stage detected"
            );
        }

        if let Some(last_block_response) = blocks.last() {
            let last_block_number = BlockParser::parse_block_number(&last_block_response.block);
            let batch_block_count = u64::try_from(blocks.len()).expect("batch blocks exceed u64");
            let batch_tx_count: u64 = blocks
                .iter()
                .map(|block_response| {
                    u64::try_from(block_response.block.transactions.len())
                        .expect("batch tx count exceeds u64")
                })
                .sum();
            self.progress
                .record_batch(last_block_number, batch_block_count, batch_tx_count);

            let partition_range = format_partition_range(start_block, end_block);
            let boundary_info = if crosses_partition_boundary(start_block, end_block) {
                " (crosses boundary)"
            } else {
                ""
            };
            info!(
                "Wrote blocks {} to {} ({} remaining, {:.2}s, commit={:.0}ms) {}{}",
                start_block,
                end_block,
                self.progress.blocks_remaining(),
                db_elapsed.as_secs_f64(),
                write_metrics.commit_ms,
                partition_range,
                boundary_info
            );
        }
        self.perf
            .blocks_count
            .fetch_add(blocks.len() as u64, Ordering::Relaxed);
        self.perf.report_and_reset();

        if !blocks.is_empty() {
            self.maybe_invalidate_chart_caches(end_block).await;
        }

        self.check_bulk_sync_completion().await;
        self.ensure_compaction_mode(self.progress.blocks_remaining());

        Ok(SyncAction::Continue)
    }

    async fn check_bulk_sync_completion(&self) {
        let currently_bulk = self.is_bulk_sync_active();
        let was_bulk = self.was_bulk_sync_active.load(Ordering::SeqCst);

        if was_bulk && !currently_bulk {
            let stats = self.writer.store().memory_stats();
            let current = self.progress.current();
            let chain_tip = self.progress.target();
            let chain_tip_i64 = i64::try_from(chain_tip).unwrap_or_else(|_| {
                panic!(
                    "chain tip over i64 range while marking bulk sync complete: {} (max={})",
                    chain_tip,
                    i64::MAX
                )
            });
            let sst_gb = stats.sst_files_size as f64 / (1024.0 * 1024.0 * 1024.0);

            self.cache_invalidator
                .update_sync_status(|status| {
                    status.mark_bulk_sync_completed(chain_tip_i64);
                })
                .await;

            let elapsed = self
                .cache_invalidator
                .get_sync_status()
                .await
                .and_then(|s| s.bulk_sync_total_seconds());
            let avg_bps = elapsed
                .filter(|&e| e > 0)
                .map(|e| current as f64 / e as f64);
            info!(
                blocks_synced = current,
                elapsed_secs = elapsed.unwrap_or(0),
                avg_bps = format!("{:.1}", avg_bps.unwrap_or(0.0)),
                sst_size_gb = format!("{:.1}", sst_gb),
                "Bulk sync completed"
            );

            self.cache_invalidator.invalidate_chart_caches().await;

            // Compaction mode transition is now handled by ensure_compaction_mode()
            // which runs after every batch and includes a drain guard.
        }

        self.was_bulk_sync_active
            .store(currently_bulk, Ordering::SeqCst);
    }

    fn cleanup_failed_batch_range(
        &self,
        start_block: u64,
        end_block: u64,
        chain_tip: u64,
        mode: &str,
    ) -> Result<()> {
        if let Err(cleanup_err) = self.writer.cleanup_batch_range(
            i64::try_from(start_block)
                .map_err(|_| anyhow!("batch cleanup start_block exceeds i64: {}", start_block))?,
            i64::try_from(end_block)
                .map_err(|_| anyhow!("batch cleanup end_block exceeds i64: {}", end_block))?,
        ) {
            error!("Failed to cleanup partial batch: {:?}", cleanup_err);
            return Ok(());
        }

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
                "{} range {}-{} (chain_tip={})",
                mode, start_block, end_block, chain_tip
            ),
        )?;
        if let Err(consistency_err) = self.reconcile_hodl_tracker_with_tip(cleanup_tip) {
            error!(
                cleanup_tip,
                "HODL tracker consistency check failed after batch cleanup: {:?}", consistency_err
            );
            return Err(consistency_err).with_context(|| {
                format!(
                    "HODL tracker inconsistent after batch cleanup to tip {}. automatic rebuild is disabled; delete RocksDB and re-sync from genesis",
                    cleanup_tip
                )
            });
        }

        Ok(())
    }

    fn maybe_start_label_import(&self) {
        let token_labels_path = self.config.token_labels_path.clone();
        if !std::path::Path::new(&token_labels_path)
            .join("information")
            .exists()
        {
            debug!(
                "Token labels directory not found at {}, skipping label import",
                token_labels_path
            );
            return;
        }

        if self.label_import_started.swap(true, Ordering::SeqCst) {
            debug!("Label import already started in this process, skipping");
            return;
        }

        let config = LabelImportConfig {
            token_labels_path,
            ..Default::default()
        };
        let core_store = Arc::clone(self.writer.store());
        let ckb_store = self.ckb_store.clone();

        tokio::spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                crate::label_import::run_label_import_staged(
                    core_store.as_ref(),
                    ckb_store.as_deref(),
                    &config,
                )
            })
            .await;

            match result {
                Ok(Ok(summary)) => info!(
                    "Background label import finished: {} UDT, {} scripts, {} errors",
                    summary.udt_labels_imported,
                    summary.script_labels_imported,
                    summary.errors.len()
                ),
                Ok(Err(e)) => warn!("Background label import failed: {}", e),
                Err(e) => warn!("Background label import task panicked: {}", e),
            }
        });
    }

    async fn fetch_blocks_parallel(
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

    // === sync_blocks_batch (sequential path) ===

    async fn sync_blocks_batch(
        &self,
        blocks: &[BlockResponseWithCycles],
        chain_tip: u64,
    ) -> Result<BatchWriteMetrics> {
        if blocks.is_empty() {
            return Ok(BatchWriteMetrics::default());
        }

        let blocks_clone: Vec<BlockResponseWithCycles> = blocks.to_vec();
        let (all_parsed_blocks, mut all_tx_data, all_input_outpoints) =
            tokio::task::spawn_blocking(move || parse_blocks_parallel(&blocks_clone))
                .await
                .map_err(|e| anyhow!("parse_blocks_parallel task panicked: {}", e))??;

        let end_block = all_parsed_blocks
            .last()
            .map(|b| b.number as u64)
            .unwrap_or(0);
        let bulk_sync_mode =
            is_bulk_sync_batch(chain_tip, end_block, self.config.bulk_sync_threshold);
        let mut commit_ms = 0.0_f64;
        let mut udt_standard_hint_cache: HashMap<Vec<u8>, Option<String>> = HashMap::new();

        let mut batch_cell_infos: HashMap<(Vec<u8>, i16), LiveCellInfo> = HashMap::new();
        for tx_data in &all_tx_data {
            for (output_index, cell) in tx_data.cells.iter().enumerate() {
                let output_index_i16 =
                    checked_usize_to_i16(output_index, "sync batch output index")?;
                let standard_hint = if let Some(type_hash) = cell.type_script_hash.as_ref() {
                    if let Some(cached) = udt_standard_hint_cache.get(type_hash) {
                        cached.clone()
                    } else {
                        let looked_up = self
                            .writer
                            .store()
                            .get_token(type_hash)
                            .map(|info| info.map(|token| token.standard))?;
                        udt_standard_hint_cache.insert(type_hash.clone(), looked_up.clone());
                        looked_up
                    }
                } else {
                    None
                };
                let udt_amount = parse_parsed_cell_udt_amount(
                    cell,
                    &tx_data.hash,
                    output_index_i16,
                    standard_hint.as_deref(),
                )?;
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

        let mut input_cell_info: HashMap<(Vec<u8>, i16), LiveCellInfo> = HashMap::new();
        for (tx_hash, idx) in &all_input_outpoints {
            let hash_arr = tx_hash_key32(tx_hash, "sync batch input cell cache lookup")?;
            let key = (hash_arr, *idx);
            if let Some(cached) = self.cell_cache.get(&key) {
                input_cell_info.insert(
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
            &input_cell_info,
            &batch_cell_infos,
        );

        if !missing_outpoints.is_empty() {
            let missing_refs: Vec<(&[u8], i16)> = missing_outpoints
                .iter()
                .map(|(h, i)| (h.as_slice(), *i))
                .collect();
            let db_info = self
                .writer
                .get_full_cells_info_batch(&missing_refs, bulk_sync_mode)?;
            for ((tx_hash, idx), info) in db_info {
                input_cell_info.insert((tx_hash, idx), info);
            }
        }
        let unresolved_outpoints = collect_missing_input_outpoints(
            &all_input_outpoints,
            &input_cell_info,
            &batch_cell_infos,
        );
        if !unresolved_outpoints.is_empty() {
            let first_block = all_parsed_blocks.first().map(|b| b.number).unwrap_or(0);
            let last_block = all_parsed_blocks.last().map(|b| b.number).unwrap_or(0);
            return Err(anyhow::anyhow!(
                "sync batch {}-{} has {} unresolved input cells (sample: {})",
                first_block,
                last_block,
                unresolved_outpoints.len(),
                format_outpoint_sample(&unresolved_outpoints, 5)
            ));
        }

        let dao_code_hash = crate::rpc::parse_hex_to_bytes(crate::parser::dao::DAO_CODE_HASH);
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
                        if info.type_code_hash.as_deref() == Some(dao_code_hash.as_slice()) {
                            has_dao_input = true;
                        }
                    } else if let Some(info) = batch_cell_infos.get(&key) {
                        tx_data.total_input_capacity += info.capacity;
                        if info.type_code_hash.as_deref() == Some(dao_code_hash.as_slice()) {
                            has_dao_input = true;
                        }
                    }
                }
                tx_data.fee = checked_tx_fee(
                    tx_data.total_input_capacity,
                    tx_data.total_output_capacity,
                    has_dao_input,
                    &tx_data.hash,
                    tx_data.block_number,
                )?;
            }
        }

        for tx_data in &all_tx_data {
            for (output_index, cell) in tx_data.cells.iter().enumerate() {
                let output_index_i16 =
                    checked_usize_to_i16(output_index, "sync batch output index for cache insert")?;
                let standard_hint = if let Some(type_hash) = cell.type_script_hash.as_ref() {
                    if let Some(cached) = udt_standard_hint_cache.get(type_hash) {
                        cached.clone()
                    } else {
                        let looked_up = self
                            .writer
                            .store()
                            .get_token(type_hash)
                            .map(|info| info.map(|token| token.standard))?;
                        udt_standard_hint_cache.insert(type_hash.clone(), looked_up.clone());
                        looked_up
                    }
                } else {
                    None
                };
                let udt_amount = parse_parsed_cell_udt_amount(
                    cell,
                    &tx_data.hash,
                    output_index_i16,
                    standard_hint.as_deref(),
                )?;
                let cell_occupied = occupied_capacity_shannons_i64(
                    cell.lock_args.len(),
                    cell.type_args.as_ref().map(|args| args.len()),
                    cell.data_size,
                );
                self.cell_cache.insert(
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
        }
        if self.cell_cache.len() > CELL_CACHE_CAPACITY * 2 {
            // In pipeline mode, the parser runs concurrently and may need
            // cache entries from batches not yet committed to DB. Only evict
            // entries from blocks already committed (before this batch).
            let safe_cutoff = all_parsed_blocks.first().map(|b| b.number).unwrap_or(0);
            self.cell_cache
                .retain(|_, v| v.created_at_block >= safe_cutoff);
        }

        // Prepare all data for insertion
        let block_refs: Vec<&crate::parser::block::ParsedBlock> =
            all_parsed_blocks.iter().collect();

        let txs_for_batch: Vec<_> = all_tx_data
            .iter()
            .map(|tx_data| {
                (
                    tx_data.hash.as_slice(),
                    tx_data.block_number,
                    tx_data.block_hash.as_slice(),
                    tx_data.tx_index,
                    tx_data.version,
                    tx_data.inputs_count,
                    tx_data.outputs_count,
                    tx_data.witnesses_count,
                    tx_data.cell_deps_count,
                    tx_data.header_deps_count,
                    tx_data.total_input_capacity,
                    tx_data.total_output_capacity,
                    tx_data.fee,
                    Some(tx_data.tx_size),
                    tx_data.cycles,
                    tx_data.is_cellbase,
                    tx_data.timestamp,
                )
            })
            .collect();

        let mut all_cells: Vec<(&[u8], i16, &crate::parser::cell::ParsedCell, i64)> = Vec::new();
        for tx_data in &all_tx_data {
            for (output_index, cell) in tx_data.cells.iter().enumerate() {
                let output_index_i16 =
                    checked_usize_to_i16(output_index, "sync batch output index for all_cells")
                        .map_err(|e| {
                            anyhow!(
                                "{}: tx_hash=0x{}, block={}",
                                e,
                                hex::encode(tx_data.hash),
                                tx_data.block_number
                            )
                        })?;
                all_cells.push((
                    tx_data.hash.as_slice(),
                    output_index_i16,
                    cell,
                    tx_data.block_number,
                ));
            }
        }

        // Write txs/cells first; block headers are committed in finalization as
        // the per-batch progress marker.
        let t_headers = Instant::now();
        {
            let mut batch = StoreBatch::new(self.writer.store());
            let mut tx_undo_seq_by_block: HashMap<i64, u64> = HashMap::new();
            if !all_tx_data.is_empty() {
                put_tx_context_undo_entries(&mut batch, &mut tx_undo_seq_by_block, &all_tx_data)?;
            }
            if !txs_for_batch.is_empty() {
                self.writer
                    .insert_transactions_batch(&txs_for_batch, &mut batch)?;
            }
            if !all_cells.is_empty() {
                self.writer
                    .insert_cells_batch(&all_cells, &mut batch, false)?;
            }
            let commit_started = Instant::now();
            batch.commit()?;
            commit_ms += commit_started.elapsed().as_secs_f64() * 1000.0;
        }
        let headers_ms = t_headers.elapsed().as_secs_f64() * 1000.0;

        // Block proposals are sourced from ckb-store-reader; only update cache indices here.
        let mut batch_proposals: Vec<(Vec<u8>, i64, i16)> = Vec::new();
        for parsed_block in &all_parsed_blocks {
            if !parsed_block.proposals.is_empty() && !self.is_bulk_sync_active() {
                for (idx, proposal_id) in parsed_block.proposals.iter().enumerate() {
                    let proposal_index = checked_usize_to_i16(
                        idx,
                        "proposal index while populating proposal cache batch",
                    )
                    .map_err(|e| {
                        anyhow!(
                            "{}: block_number={}, proposal_count={}",
                            e,
                            parsed_block.number,
                            parsed_block.proposals.len()
                        )
                    })?;
                    batch_proposals.push((
                        proposal_id.clone(),
                        parsed_block.number,
                        proposal_index,
                    ));
                }
            }
        }
        if !batch_proposals.is_empty() {
            let last_bn = all_parsed_blocks.last().map(|b| b.number).unwrap_or(0);
            tokio::spawn(Self::run_proposal_cache_batch(
                self.rpc.clone(),
                self.cache_invalidator.clone(),
                batch_proposals,
                last_bn,
            ));
        }

        // Consume cells
        let t_cells = Instant::now();
        let mut all_consumptions: Vec<(&[u8], i16, i64, &[u8], i64, i16)> = Vec::new();
        for tx_data in &all_tx_data {
            if !tx_data.is_cellbase {
                for (input_index, input) in tx_data.inputs.iter().enumerate() {
                    let input_index_i16 =
                        checked_usize_to_i16(input_index, "sync batch input index for consume")
                            .map_err(|e| {
                                anyhow!(
                                    "{}: tx_hash=0x{}, block={}",
                                    e,
                                    hex::encode(tx_data.hash),
                                    tx_data.block_number
                                )
                            })?;
                    let key = (
                        input.previous_tx_hash.to_vec(),
                        parsed_input_outpoint_index_i16(
                            input.previous_output_index,
                            "sync_indexer",
                        ),
                    );
                    if let Some(info) = input_cell_info.get(&key) {
                        all_consumptions.push((
                            input.previous_tx_hash.as_slice(),
                            parsed_input_outpoint_index_i16(
                                input.previous_output_index,
                                "sync_indexer",
                            ),
                            info.created_at_block,
                            tx_data.hash.as_slice(),
                            tx_data.block_number,
                            input_index_i16,
                        ));
                    } else if let Some(info) = batch_cell_infos.get(&key) {
                        all_consumptions.push((
                            input.previous_tx_hash.as_slice(),
                            parsed_input_outpoint_index_i16(
                                input.previous_output_index,
                                "sync_indexer",
                            ),
                            info.created_at_block,
                            tx_data.hash.as_slice(),
                            tx_data.block_number,
                            input_index_i16,
                        ));
                    }
                }
            }
        }
        // Single batch for consume + address balances + script usage
        let mut consume_addr_batch = StoreBatch::new(self.writer.store());
        let mut domain_analytics_batch = StoreBatch::new(self.writer.store());
        let mut append_history_batch = StoreBatch::new(&self.append_only_store);
        let mut append_undo_seq_by_block: HashMap<i64, u64> = HashMap::new();
        if !all_consumptions.is_empty() {
            self.writer.consume_cells_batch_preloaded(
                &all_consumptions,
                &input_cell_info,
                &batch_cell_infos,
                &mut consume_addr_batch,
                false,
            )?;
        }

        // Address balances
        let mut address_balance_changes: HashMap<
            Vec<u8>,
            (i128, i32, i32, i64, i64, Vec<u8>, i128),
        > = HashMap::new();
        for tx_data in &all_tx_data {
            let mut tx_balance_changes: HashMap<Vec<u8>, i128> = HashMap::new();
            let mut tx_cells_created: HashMap<Vec<u8>, i32> = HashMap::new();
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
                    }
                }
            }
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
                let balance_change = tx_balance_changes.get(&lock_hash).copied().unwrap_or(0);
                let cells_created = tx_cells_created.get(&lock_hash).copied().unwrap_or(0);
                let cells_consumed = tx_cells_consumed.get(&lock_hash).copied().unwrap_or(0);
                let occupied_change = tx_occupied_changes.get(&lock_hash).copied().unwrap_or(0);
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

                // Index address → transaction
                put_addr_tx_with_undo_log(
                    &mut consume_addr_batch,
                    &mut append_history_batch,
                    &mut append_undo_seq_by_block,
                    &lock_hash,
                    tx_data.block_number,
                    tx_data.tx_index,
                    &tx_data.hash,
                );
            }
        }

        let changes_ref: HashMap<Vec<u8>, (i128, i32, i32, i64, i64, &[u8], i128)> =
            address_balance_changes
                .iter()
                .map(|(k, (a, b, c, d, e, f, g))| {
                    (k.clone(), (*a, *b, *c, *d, *e, f.as_slice(), *g))
                })
                .collect();

        // Script usage
        let mut script_usage_changes: ScriptUsageChanges = HashMap::new();
        let mut script_daily_changes: HashMap<(Vec<u8>, bool, u32), (i128, i128)> = HashMap::new();
        let mut token_daily_changes: HashMap<(Vec<u8>, u32), (i128, i128)> = HashMap::new();
        let mut spore_type_index_changes: HashMap<Vec<u8>, SporeTypeIndex> = HashMap::new();
        let mut spore_daily_changes: HashMap<(Vec<u8>, u32), (i128, i128)> = HashMap::new();
        let mut cluster_daily_changes: HashMap<(Vec<u8>, u32), (i128, i128)> = HashMap::new();
        let mut nft_type_index_changes: HashMap<Vec<u8>, NftTypeIndex> = HashMap::new();
        let mut nft_daily_changes: HashMap<(Vec<u8>, u32), (i128, i128)> = HashMap::new();
        let mut spore_type_index_cache: HashMap<Vec<u8>, Option<SporeTypeIndex>> = HashMap::new();
        let mut nft_type_index_cache: HashMap<Vec<u8>, Option<NftTypeIndex>> = HashMap::new();
        for tx_data in &all_tx_data {
            let date_yyyymmdd =
                ckbadger_store::keys::timestamp_ms_to_date(tx_data.timestamp.timestamp_millis());
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
                        let cluster_id = SporeParser::parse_spore_cluster_id_from_data(&cell.data);
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
                    let collection_id = classify_nft_collection_id(type_code_hash, type_args);
                    if let Some(collection_id) = collection_id {
                        let index = NftTypeIndex {
                            collection_id: collection_id.clone(),
                        };
                        nft_type_index_cache.insert(type_script_hash.clone(), Some(index.clone()));
                        nft_type_index_changes.insert(type_script_hash.clone(), index);

                        let nft_daily = nft_daily_changes
                            .entry((collection_id, date_yyyymmdd))
                            .or_insert((0, 0));
                        nft_daily.0 += i128::from(cell.capacity);
                        nft_daily.1 += i128::from(cell_occupied);
                    }
                }
            }
        }
        for tx_data in &all_tx_data {
            let date_yyyymmdd =
                ckbadger_store::keys::timestamp_ms_to_date(tx_data.timestamp.timestamp_millis());
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
                                let spore_index = if let Some(cached) =
                                    spore_type_index_cache.get(type_script_hash)
                                {
                                    cached.clone()
                                } else {
                                    let loaded = self
                                        .writer
                                        .store()
                                        .get_spore_type_index(type_script_hash)?;
                                    spore_type_index_cache
                                        .insert(type_script_hash.clone(), loaded.clone());
                                    loaded
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
                                        cluster_daily.1 -= i128::from(info.occupied_capacity);
                                    }
                                }
                            }
                            if DotbitParser::is_account_cell_type_script(type_code_hash)
                                || MnftParser::is_token_type_script(type_code_hash)
                                || SporeParser::is_did_type_script(type_code_hash)
                            {
                                let collection_id =
                                    if DotbitParser::is_account_cell_type_script(type_code_hash) {
                                        Some(DOTBIT_SENTINEL_COLLECTION.to_vec())
                                    } else if SporeParser::is_did_type_script(type_code_hash) {
                                        Some(DID_CKB_SENTINEL_COLLECTION.to_vec())
                                    } else if let Some(cached) =
                                        nft_type_index_cache.get(type_script_hash)
                                    {
                                        cached.clone().map(|idx| idx.collection_id)
                                    } else {
                                        let loaded = self
                                            .writer
                                            .store()
                                            .get_nft_type_index(type_script_hash)?;
                                        nft_type_index_cache
                                            .insert(type_script_hash.clone(), loaded.clone());
                                        loaded.map(|idx| idx.collection_id)
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
        }

        let skip_address_balances = should_skip_address_balances(bulk_sync_mode);

        // Parallel DB reads for address balances and script usage
        let lock_hash_keys: Vec<&Vec<u8>> = if !skip_address_balances && !changes_ref.is_empty() {
            changes_ref.keys().collect()
        } else {
            vec![]
        };
        let unique_code_hashes: Vec<Vec<u8>> = if !script_usage_changes.is_empty() {
            let mut seen = std::collections::HashSet::new();
            script_usage_changes
                .keys()
                .filter_map(|(code_hash, _)| {
                    if seen.insert(code_hash.clone()) {
                        Some(code_hash.clone())
                    } else {
                        None
                    }
                })
                .collect()
        } else {
            vec![]
        };
        let code_hash_refs: Vec<&Vec<u8>> = unique_code_hashes.iter().collect();

        let need_balances = !lock_hash_keys.is_empty();
        let need_scripts = !code_hash_refs.is_empty();
        let mut batch_new_addresses = 0i64;

        if need_balances || need_scripts {
            let writer = &self.writer;
            let (existing_balances, existing_scripts) = std::thread::scope(|s| {
                let bal = if need_balances {
                    Some(s.spawn(|| writer.read_address_balances(&lock_hash_keys)))
                } else {
                    None
                };
                let scr = if need_scripts {
                    Some(s.spawn(|| writer.read_script_info(&code_hash_refs)))
                } else {
                    None
                };
                (
                    bal.map(|h| h.join().unwrap()),
                    scr.map(|h| h.join().unwrap()),
                )
            });
            if let Some(existing) = existing_balances {
                let existing = existing?;
                batch_new_addresses = count_new_addresses(&changes_ref, &existing);
                self.writer.apply_address_balance_deltas(
                    &existing,
                    &changes_ref,
                    &mut consume_addr_batch,
                )?;
            }
            if let Some(existing) = existing_scripts {
                self.writer.apply_script_usage_deltas(
                    &existing?,
                    &script_usage_changes,
                    &mut domain_analytics_batch,
                )?;
            }
        }
        if !script_daily_changes.is_empty() {
            self.writer.update_script_daily_deltas_batch(
                &script_daily_changes,
                &mut domain_analytics_batch,
            )?;
        }
        if !token_daily_changes.is_empty() {
            self.writer.update_token_daily_deltas_batch(
                &token_daily_changes,
                &mut domain_analytics_batch,
            )?;
        }
        if !spore_type_index_changes.is_empty() {
            self.writer.update_spore_type_index_batch(
                &spore_type_index_changes,
                &mut consume_addr_batch,
            )?;
        }
        if !spore_daily_changes.is_empty() {
            self.writer.update_spore_daily_deltas_batch(
                &spore_daily_changes,
                &mut domain_analytics_batch,
            )?;
        }
        if !nft_type_index_changes.is_empty() {
            self.writer
                .update_nft_type_index_batch(&nft_type_index_changes, &mut consume_addr_batch)?;
        }
        if !nft_daily_changes.is_empty() {
            self.writer
                .update_nft_daily_deltas_batch(&nft_daily_changes, &mut domain_analytics_batch)?;
        }
        if !cluster_daily_changes.is_empty() {
            self.writer.update_cluster_daily_deltas_batch(
                &cluster_daily_changes,
                &mut domain_analytics_batch,
            )?;
        }
        {
            let commit_started = Instant::now();
            consume_addr_batch.commit()?;
            commit_ms += commit_started.elapsed().as_secs_f64() * 1000.0;
        }
        if !domain_analytics_batch.is_empty() {
            let commit_started = Instant::now();
            domain_analytics_batch.commit()?;
            commit_ms += commit_started.elapsed().as_secs_f64() * 1000.0;
        }
        if !append_history_batch.is_empty() {
            let commit_started = Instant::now();
            append_history_batch.commit()?;
            commit_ms += commit_started.elapsed().as_secs_f64() * 1000.0;
        }

        let cells_ms = t_cells.elapsed().as_secs_f64() * 1000.0;

        // Accumulate batch statistics
        let t_stats = Instant::now();
        let mut batch_stats = BatchStats::default();
        let mut prev_timestamp: Option<chrono::DateTime<Utc>> =
            if let Some(first_block) = all_parsed_blocks.first() {
                self.writer
                    .get_previous_block_timestamp(first_block.number)?
            } else {
                None
            };
        let mut prev_epoch: Option<(i64, chrono::DateTime<Utc>, f64)> =
            if let Some(first_block) = all_parsed_blocks.first() {
                self.writer
                    .get_last_epoch_start(first_block.number)?
                    .map(|(epoch_num, ts)| (epoch_num, ts, 0.0))
            } else {
                None
            };
        let mut prev_dao_cs: Option<(i128, i128)> =
            if let Some(first_block) = all_parsed_blocks.first() {
                if first_block.number > 0 {
                    self.writer
                        .store()
                        .get_block_header(first_block.number - 1)?
                        .and_then(|h| extract_dao_csu(&h.dao).map(|(c, s, _)| (c, s)))
                } else {
                    None
                }
            } else {
                None
            };

        // Pre-build consumed DAO deposit map for delta computation
        let dao_code_hash_for_seq_stats =
            crate::rpc::parse_hex_to_bytes(crate::parser::dao::DAO_CODE_HASH);
        let all_input_outpoints_for_seq_dao: Vec<(Vec<u8>, i16)> = all_tx_data
            .iter()
            .filter(|tx| !tx.is_cellbase)
            .flat_map(|tx| {
                tx.inputs.iter().map(|input| {
                    (
                        input.previous_tx_hash.to_vec(),
                        parsed_input_outpoint_index_i16(
                            input.previous_output_index,
                            "sync_indexer",
                        ),
                    )
                })
            })
            .collect();
        let consumed_dao_for_seq_stats = if !all_input_outpoints_for_seq_dao.is_empty() {
            let unique: Vec<(Vec<u8>, i16)> = {
                let mut seen = HashSet::new();
                all_input_outpoints_for_seq_dao
                    .into_iter()
                    .filter(|x| seen.insert(x.clone()))
                    .collect()
            };
            let refs: Vec<(&[u8], i16)> = unique.iter().map(|(h, i)| (h.as_slice(), *i)).collect();
            self.writer.find_consumed_dao_deposits_batch(&refs)?
        } else {
            HashMap::new()
        };
        let mut same_batch_dao_for_seq_stats: HashMap<(Vec<u8>, i16), i64> = HashMap::new();

        let mut block_tx_idx = 0usize;
        for parsed in &all_parsed_blocks {
            let block_date = ckbadger_common::block_date(parsed.timestamp);
            accumulate_secondary_issuance_deltas(
                &mut batch_stats,
                parsed,
                block_date,
                &mut prev_dao_cs,
            )?;
            let tx_count_for_block = parsed.transactions_count as usize;
            let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
            block_tx_idx += tx_count_for_block;

            let cells_created: i32 = tx_slice.iter().map(|tx| tx.cells.len() as i32).sum();
            let cells_consumed: i32 = tx_slice
                .iter()
                .filter(|tx| !tx.is_cellbase)
                .map(|tx| tx.inputs.len() as i32)
                .sum();
            let capacity_transferred: i128 = tx_slice
                .iter()
                .filter(|tx| !tx.is_cellbase)
                .map(|tx| i128::from(tx.total_output_capacity))
                .sum();
            let data_size_added: i64 = tx_slice
                .iter()
                .flat_map(|tx| tx.cells.iter())
                .map(|cell| cell.data_size as i64)
                .sum();
            let occupied_capacity_created: i128 = tx_slice
                .iter()
                .flat_map(|tx| tx.cells.iter())
                .map(|cell| {
                    occupied_capacity_shannons_i128(
                        cell.lock_args.len(),
                        cell.type_args.as_ref().map(|args| args.len()),
                        cell.data_size,
                    )
                })
                .sum();
            let data_size_consumed: i64 = tx_slice
                .iter()
                .filter(|tx| !tx.is_cellbase)
                .flat_map(|tx| tx.inputs.iter())
                .filter_map(|input| {
                    let key = (
                        input.previous_tx_hash.to_vec(),
                        parsed_input_outpoint_index_i16(
                            input.previous_output_index,
                            "sync_indexer",
                        ),
                    );
                    input_cell_info
                        .get(&key)
                        .map(|info| info.data_size as i64)
                        .or_else(|| batch_cell_infos.get(&key).map(|info| info.data_size as i64))
                })
                .sum();
            let occupied_capacity_consumed: i128 = tx_slice
                .iter()
                .filter(|tx| !tx.is_cellbase)
                .flat_map(|tx| tx.inputs.iter())
                .filter_map(|input| {
                    let key = (
                        input.previous_tx_hash.to_vec(),
                        parsed_input_outpoint_index_i16(
                            input.previous_output_index,
                            "sync_indexer",
                        ),
                    );
                    input_cell_info
                        .get(&key)
                        .map(|info| i128::from(info.occupied_capacity))
                        .or_else(|| {
                            batch_cell_infos
                                .get(&key)
                                .map(|info| i128::from(info.occupied_capacity))
                        })
                })
                .sum();

            batch_stats.sync_totals.0 += parsed.transactions_count as i64;
            batch_stats.sync_totals.1 += cells_created as i64;
            batch_stats.sync_totals.2 += cells_consumed as i64;
            batch_stats.last_block = Some((parsed.number, parsed.hash.clone()));

            {
                let entry = batch_stats.daily_stats.entry(block_date).or_default();
                entry.0 += 1;
                entry.1 += parsed.transactions_count;
                entry.2 += cells_created;
                entry.3 += cells_consumed;
                entry.4 = entry.4.checked_add(capacity_transferred).ok_or_else(|| {
                    anyhow!(
                        "daily capacity_transferred overflow: date={} block={}",
                        block_date,
                        parsed.number
                    )
                })?;
                entry.5 = entry
                    .5
                    .checked_add(occupied_capacity_created)
                    .ok_or_else(|| {
                        anyhow!(
                            "daily occupied_capacity_created overflow: date={} block={}",
                            block_date,
                            parsed.number
                        )
                    })?;
                entry.6 = entry
                    .6
                    .checked_add(occupied_capacity_consumed)
                    .ok_or_else(|| {
                        anyhow!(
                            "daily occupied_capacity_consumed overflow: date={} block={}",
                            block_date,
                            parsed.number
                        )
                    })?;
                entry.7 += data_size_added;
                entry.8 += data_size_consumed;
            }

            batch_stats
                .daily_dao_fields
                .insert(block_date, parsed.dao.clone());

            {
                let block_hour = truncate_to_hour(parsed.timestamp);
                let entry = batch_stats.hourly_stats.entry(block_hour).or_default();
                entry.0 += 1;
                entry.1 += parsed.transactions_count;
                entry.2 += cells_created;
                entry.3 += cells_consumed;
                entry.4 = entry.4.checked_add(capacity_transferred).ok_or_else(|| {
                    anyhow!(
                        "hourly capacity_transferred overflow: hour={} block={}",
                        block_hour,
                        parsed.number
                    )
                })?;
            }

            {
                let entry = batch_stats.daily_block_stats.entry(block_date).or_default();
                entry.0 += parsed.compact_target as i128;
                entry.1 += 1;
                entry.2 += parsed.uncles_count;
            }

            if let Some(first_tx) = tx_slice.first() {
                if first_tx.is_cellbase {
                    if let Some(first_cell) = first_tx.cells.first() {
                        let key = (block_date, first_cell.lock_script_hash.clone());
                        let entry = batch_stats.miner_stats.entry(key).or_insert((0, 0));
                        entry.0 += 1;
                        entry.1 = parsed.number;
                    }
                }
            }

            {
                let entry = batch_stats
                    .epoch_stats
                    .entry(parsed.epoch_number)
                    .or_insert_with(|| EpochAccum {
                        start_block: parsed.number,
                        end_block: parsed.number,
                        length: parsed.epoch_length,
                        start_ts: parsed.timestamp,
                        end_ts: parsed.timestamp,
                        tx_count: 0,
                        is_new: parsed.epoch_index == 0,
                    });
                entry.end_block = parsed.number;
                entry.end_ts = parsed.timestamp;
                entry.tx_count += parsed.transactions_count;
            }

            if let Some(prev_ts) = prev_timestamp {
                let block_time_seconds = (parsed.timestamp - prev_ts).num_seconds();
                if block_time_seconds >= 0 {
                    *batch_stats
                        .block_time_dist
                        .entry(block_time_to_bucket(block_time_seconds))
                        .or_default() += 1;
                    let block_time_ms = block_time_seconds * 1000;
                    let entry = batch_stats
                        .daily_block_times
                        .entry(block_date)
                        .or_insert((0, 0));
                    entry.0 += block_time_ms;
                    entry.1 += 1;
                }
            }
            prev_timestamp = Some(parsed.timestamp);

            if parsed.epoch_index == 0 && parsed.epoch_number > 0 {
                if let Some((prev_epoch_num, prev_start_ts, _)) = prev_epoch {
                    if prev_epoch_num == parsed.epoch_number - 1 {
                        let epoch_duration_minutes =
                            (parsed.timestamp - prev_start_ts).num_seconds() as f64 / 60.0;
                        let bucket_minutes = epoch_duration_minutes.round() as i32;
                        *batch_stats
                            .epoch_time_dist
                            .entry(bucket_minutes)
                            .or_default() += 1;
                    }
                }
            }
            if parsed.epoch_index == 0 {
                prev_epoch = Some((parsed.epoch_number, parsed.timestamp, 0.0));
            }

            // DAO per-day deltas for snapshot accumulation
            accumulate_dao_snapshot_deltas_for_txs(
                tx_slice,
                block_date,
                &dao_code_hash_for_seq_stats,
                &consumed_dao_for_seq_stats,
                &mut same_batch_dao_for_seq_stats,
                &mut batch_stats.dao_daily_active_delta,
                &mut batch_stats.dao_daily_gross_deposit_delta,
                &mut batch_stats.dao_daily_new_deposits_delta,
                &mut batch_stats.dao_daily_withdrawals_delta,
            )?;

            batch_stats.dao_snapshot_dates.insert(block_date);
        }
        batch_stats.dao_deltas_computed = true;

        // DAO processing
        let dao_code_hash = crate::rpc::parse_hex_to_bytes(crate::parser::dao::DAO_CODE_HASH);
        {
            let mut all_dao_deposits: Vec<(
                crate::parser::ParsedDaoDeposit,
                i64,
                chrono::DateTime<Utc>,
                i64,
            )> = Vec::new();
            let mut block_tx_idx = 0usize;
            for parsed in &all_parsed_blocks {
                let tx_count_for_block = parsed.transactions_count as usize;
                let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                block_tx_idx += tx_count_for_block;
                let ar = extract_ar_i64_from_dao(&parsed.dao, parsed.number)?;
                for tx_data in tx_slice {
                    let dao_deposits =
                        DaoParser::parse_deposits_from_cells(&tx_data.hash, &tx_data.cells);
                    for deposit in dao_deposits {
                        all_dao_deposits.push((deposit, parsed.number, parsed.timestamp, ar));
                    }
                }
            }
            if !all_dao_deposits.is_empty() {
                let mut batch = StoreBatch::new(self.writer.store());
                self.writer
                    .insert_dao_deposits_batch(&all_dao_deposits, &mut batch)?;
                let commit_started = Instant::now();
                batch.commit()?;
                commit_ms += commit_started.elapsed().as_secs_f64() * 1000.0;
            }
        }

        let mut block_tx_idx = 0usize;
        for parsed in &all_parsed_blocks {
            let tx_count_for_block = parsed.transactions_count as usize;
            let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
            block_tx_idx += tx_count_for_block;

            for tx_data in tx_slice {
                if tx_data.is_cellbase || tx_data.inputs.is_empty() {
                    continue;
                }

                let input_outpoints: Vec<(&[u8], i32)> = tx_data
                    .inputs
                    .iter()
                    .map(|i| (i.previous_tx_hash.as_slice(), i.previous_output_index))
                    .collect();

                let consumed_dao = self.writer.find_consumed_dao_deposits(&input_outpoints)?;
                if consumed_dao.is_empty() {
                    continue;
                }

                let mut new_dao_outputs: Vec<(Vec<u8>, i16, Vec<u8>, i64, u64)> = Vec::new();
                let mut candidate_withdraw_to_outputs: Vec<(i16, Vec<u8>)> = Vec::new();
                for (idx, cell) in tx_data.cells.iter().enumerate() {
                    let output_index = checked_usize_to_i16(
                        idx,
                        "DAO processing output index in same-batch withdrawal context",
                    )
                    .map_err(|e| {
                        anyhow!(
                            "{}: tx_hash=0x{}, block={}",
                            e,
                            hex::encode(tx_data.hash),
                            tx_data.block_number
                        )
                    })?;
                    if let Some(ref type_code_hash) = cell.type_code_hash {
                        if type_code_hash == &dao_code_hash && cell.data_size == 8 {
                            if let Some(data) = tx_data.outputs_data.get(idx) {
                                let data_bytes = crate::rpc::parse_hex_to_bytes(data);
                                if let Some(deposit_block) =
                                    DaoParser::parse_deposit_block_number(&data_bytes)
                                {
                                    new_dao_outputs.push((
                                        tx_data.hash.to_vec(),
                                        output_index,
                                        cell.lock_script_hash.clone(),
                                        cell.capacity,
                                        deposit_block,
                                    ));
                                }
                            }
                        } else {
                            candidate_withdraw_to_outputs
                                .push((output_index, cell.lock_script_hash.clone()));
                        }
                    } else {
                        candidate_withdraw_to_outputs
                            .push((output_index, cell.lock_script_hash.clone()));
                    }
                }

                let tx_input_outpoints: Vec<(Vec<u8>, i16)> = tx_data
                    .inputs
                    .iter()
                    .map(|input| {
                        let output_index = i16::try_from(input.previous_output_index).map_err(|_| {
                            anyhow!(
                                "DAO processing input index exceeds i16 range: tx_hash=0x{}, previous_output_index={}",
                                hex::encode(tx_data.hash),
                                input.previous_output_index
                            )
                        })?;
                        Ok((input.previous_tx_hash.to_vec(), output_index))
                    })
                    .collect::<Result<_>>()?;

                {
                    let mut batch = StoreBatch::new(self.writer.store());
                    self.writer.process_dao_withdrawals(
                        &consumed_dao,
                        &new_dao_outputs,
                        &candidate_withdraw_to_outputs,
                        &tx_input_outpoints,
                        parsed.number,
                        &tx_data.hash,
                        parsed.timestamp,
                        &mut batch,
                    )?;
                    let commit_started = Instant::now();
                    batch.commit()?;
                    commit_ms += commit_started.elapsed().as_secs_f64() * 1000.0;
                }
            }
        }

        // UDT processing
        let skip_token = false;
        let skip_spore = false;

        struct UdtTxContext {
            tx_hash: Vec<u8>,
            block_number: i64,
            #[allow(dead_code)]
            timestamp: chrono::DateTime<Utc>,
            output_udts: Vec<crate::parser::ParsedUdtCell>,
            input_outpoints: Vec<(Vec<u8>, i16)>,
        }

        let mut udt_tx_contexts: Vec<UdtTxContext> = Vec::new();
        let mut all_input_outpoints_udt: Vec<(Vec<u8>, i16)> = Vec::new();
        let mut batch_udt_cells: HashMap<(Vec<u8>, i16), crate::parser::ParsedUdtCell> =
            HashMap::new();

        struct TxInfoForUdt {
            tx_hash: Vec<u8>,
            block_number: i64,
            timestamp: chrono::DateTime<Utc>,
            output_udts: Vec<crate::parser::ParsedUdtCell>,
            input_outpoints: Vec<(Vec<u8>, i16)>,
        }
        let mut all_tx_infos_for_udt: Vec<TxInfoForUdt> = Vec::new();

        let mut block_tx_idx = 0usize;
        for (block_idx, block_response) in blocks.iter().enumerate() {
            let parsed = &all_parsed_blocks[block_idx];
            let tx_count_for_block = parsed.transactions_count as usize;
            let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
            block_tx_idx += tx_count_for_block;

            for (tx_idx, tx_data) in tx_slice.iter().enumerate() {
                if tx_data.is_cellbase {
                    continue;
                }
                let tx = &block_response.block.transactions[tx_idx];
                let mut output_udts: Vec<crate::parser::ParsedUdtCell> = Vec::new();
                for (output_index, udt_cell) in self.parse_udt_cells_with_store_fallback(tx)? {
                    batch_udt_cells.insert((tx_data.hash.to_vec(), output_index), udt_cell.clone());
                    self.udt_cell_cache.insert(
                        (tx_data.hash, output_index),
                        CachedUdtCellInfo {
                            type_script_hash: udt_cell.type_script_hash.clone(),
                            type_code_hash: udt_cell.type_code_hash.clone(),
                            type_hash_type: udt_cell.type_hash_type,
                            type_args: udt_cell.type_args.clone(),
                            lock_script_hash: udt_cell.lock_script_hash.clone(),
                            amount: udt_cell.amount,
                            standard: udt_cell.standard.as_str().to_string(),
                        },
                    );
                    output_udts.push(udt_cell);
                }
                let input_outpoints: Vec<(Vec<u8>, i16)> = tx_data
                    .inputs
                    .iter()
                    .map(|i| {
                        let previous_output_index =
                            i16::try_from(i.previous_output_index).map_err(|_| {
                                anyhow!(
                                    "UDT input previous_output_index exceeds i16 range: tx_hash=0x{}, block={}, previous_output_index={}",
                                    hex::encode(tx_data.hash),
                                    parsed.number,
                                    i.previous_output_index
                                )
                            })?;
                        Ok((i.previous_tx_hash.to_vec(), previous_output_index))
                    })
                    .collect::<Result<_>>()?;
                all_input_outpoints_udt.extend(input_outpoints.iter().cloned());
                all_tx_infos_for_udt.push(TxInfoForUdt {
                    tx_hash: tx_data.hash.to_vec(),
                    block_number: parsed.number,
                    timestamp: parsed.timestamp,
                    output_udts,
                    input_outpoints,
                });
            }
        }

        let mut input_udt_info: HashMap<
            (Vec<u8>, i16),
            (Vec<u8>, Vec<u8>, i16, Vec<u8>, Vec<u8>, u128, String),
        > = HashMap::new();
        if !skip_token && !all_input_outpoints_udt.is_empty() {
            input_udt_info = resolve_input_udt_info_from_live_cells(
                &self.writer,
                &self.udt_cell_cache,
                &all_input_outpoints_udt,
            )?;
        }

        for tx_info in all_tx_infos_for_udt {
            let has_udt_outputs = !tx_info.output_udts.is_empty();
            let has_udt_inputs = tx_info.input_outpoints.iter().any(|(tx_hash, idx)| {
                input_udt_info.contains_key(&(tx_hash.clone(), *idx))
                    || batch_udt_cells.contains_key(&(tx_hash.clone(), *idx))
            });
            if has_udt_outputs || has_udt_inputs {
                udt_tx_contexts.push(UdtTxContext {
                    tx_hash: tx_info.tx_hash,
                    block_number: tx_info.block_number,
                    timestamp: tx_info.timestamp,
                    output_udts: tx_info.output_udts,
                    input_outpoints: tx_info.input_outpoints,
                });
            }
        }

        if !skip_token && !udt_tx_contexts.is_empty() {
            let max_supply_observations = collect_token_max_supply_observations(&all_tx_data);
            let mut all_transfers: Vec<(crate::parser::ParsedUdtTransfer, Vec<u8>, i64)> =
                Vec::new();
            for ctx in &udt_tx_contexts {
                let mut input_udts: Vec<crate::parser::ParsedUdtCell> = Vec::new();
                for (tx_hash, idx) in &ctx.input_outpoints {
                    if let Some((
                        type_script_hash,
                        type_code_hash,
                        type_hash_type,
                        type_args,
                        lock_script_hash,
                        amount,
                        standard,
                    )) = input_udt_info.get(&(tx_hash.clone(), *idx))
                    {
                        input_udts.push(crate::parser::ParsedUdtCell {
                            type_script_hash: type_script_hash.clone(),
                            type_code_hash: type_code_hash.clone(),
                            type_hash_type: *type_hash_type,
                            type_args: type_args.clone(),
                            lock_script_hash: lock_script_hash.clone(),
                            amount: *amount,
                            standard: crate::parser::UdtStandard::parse(standard),
                        });
                    } else if let Some(udt_cell) = batch_udt_cells.get(&(tx_hash.clone(), *idx)) {
                        input_udts.push(udt_cell.clone());
                    }
                }

                for transfer in crate::parser::UdtParser::build_transfers_from_cells(
                    &input_udts,
                    &ctx.output_udts,
                ) {
                    all_transfers.push((transfer, ctx.tx_hash.clone(), ctx.block_number));
                }
            }

            if !all_transfers.is_empty() {
                let transfer_refs: Vec<_> = all_transfers
                    .iter()
                    .map(|(t, h, b)| (t, h.as_slice(), *b))
                    .collect();
                let block_timestamps: HashMap<i64, i64> = all_parsed_blocks
                    .iter()
                    .map(|p| (p.number, p.timestamp.timestamp_millis()))
                    .collect();
                {
                    let mut udt_state = self.writer.new_udt_batch_state();
                    let mut batch = StoreBatch::new(self.writer.store());
                    self.writer.process_udt_transfers_batch_with_state(
                        &transfer_refs,
                        &max_supply_observations,
                        &block_timestamps,
                        &mut batch,
                        &mut udt_state,
                    )?;
                    let commit_started = Instant::now();
                    batch.commit()?;
                    commit_ms += commit_started.elapsed().as_secs_f64() * 1000.0;
                }
            }
        }

        // NFT/Spore processing
        let mut batch_spore_ids: HashSet<Vec<u8>> = HashSet::new();
        let mut batch_dotbit_outpoints: HashMap<(Vec<u8>, i16), Vec<u8>> = HashMap::new();
        let mut dotbit_tx_activity_data: HashMap<[u8; 32], DotbitTxActivityData> = HashMap::new();
        let mut batch_dotbit_latest_create_order: HashMap<Vec<u8>, u64> = HashMap::new();

        let mut nft_activity_acc = NftCollectionActivityAccumulator::new();
        {
            let mut nft_batch = StoreBatch::new(self.writer.store());
            let mut spore_state = self.writer.new_spore_batch_state();
            let mut dotbit_state = self.writer.new_dotbit_batch_state();
            let mut mnft_state = self.writer.new_mnft_batch_state();
            let mut block_tx_idx = 0usize;
            for (block_idx, block_response) in blocks.iter().enumerate() {
                let parsed = &all_parsed_blocks[block_idx];
                let tx_count_for_block = parsed.transactions_count as usize;
                let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];

                for (tx_idx, tx_data) in tx_slice.iter().enumerate() {
                    let tx_global_index = block_tx_idx + tx_idx;
                    let dotbit_create_order = dotbit_create_event_order(tx_global_index)?;
                    let tx = &block_response.block.transactions[tx_idx];

                    if !skip_spore {
                        for cluster in SporeParser::parse_clusters(tx) {
                            self.writer.insert_spore_cluster(
                                &cluster,
                                parsed.number,
                                &tx_data.hash,
                                &mut nft_batch,
                                &mut spore_state,
                            )?;
                        }
                        for (output_index, spore) in
                            SporeParser::parse_spores(tx).iter().enumerate()
                        {
                            let output_index_i16 = checked_usize_to_i16(
                                output_index,
                                "spore output index while indexing parsed block",
                            )
                            .map_err(|e| {
                                anyhow!(
                                    "{}: block={}, tx_hash=0x{}",
                                    e,
                                    parsed.number,
                                    hex::encode(tx_data.hash)
                                )
                            })?;
                            batch_spore_ids.insert(spore.spore_id.clone());
                            self.writer.insert_spore_cell(
                                spore,
                                &tx_data.hash,
                                output_index_i16,
                                parsed.number,
                                parsed.timestamp.timestamp_millis(),
                                &mut nft_batch,
                                &mut spore_state,
                            )?;
                            let coll_id = if spore.is_did {
                                &DID_CKB_SENTINEL_COLLECTION[..]
                            } else if let Some(ref cid) = spore.cluster_id {
                                cid.as_slice()
                            } else {
                                continue;
                            };
                            nft_activity_acc.record(
                                coll_id,
                                &tx_data.hash,
                                &spore.spore_id,
                                parsed.number,
                                checked_usize_to_i32(tx_idx, "tx_idx"),
                                parsed.timestamp.timestamp_millis(),
                                true,
                            );
                        }
                    }

                    for issuer in MnftParser::parse_issuers(tx) {
                        self.writer.insert_mnft_issuer(
                            &issuer,
                            &tx_data.hash,
                            0,
                            parsed.number,
                            &mut nft_batch,
                        )?;
                    }
                    for (output_index, class) in MnftParser::parse_classes_with_output_indices(tx) {
                        let output_index = i16::try_from(output_index).map_err(|_| {
                            anyhow!(
                                "mNFT class output index exceeds i16 range: block={}, tx_hash=0x{}, output_index={}",
                                parsed.number,
                                hex::encode(tx_data.hash),
                                output_index
                            )
                        })?;
                        self.writer.insert_mnft_class_with_state(
                            &class,
                            &tx_data.hash,
                            output_index,
                            parsed.number,
                            &mut nft_batch,
                            &mut mnft_state,
                        )?;
                    }
                    for (output_index, token) in MnftParser::parse_tokens_with_output_indices(tx) {
                        let output_index = i16::try_from(output_index).map_err(|_| {
                            anyhow!(
                                "mNFT token output index exceeds i16 range: block={}, tx_hash=0x{}, output_index={}",
                                parsed.number,
                                hex::encode(tx_data.hash),
                                output_index
                            )
                        })?;
                        self.writer.insert_mnft_token_with_state(
                            &token,
                            &tx_data.hash,
                            output_index,
                            parsed.number,
                            parsed.timestamp.timestamp_millis(),
                            &mut nft_batch,
                            &mut mnft_state,
                        )?;
                        nft_activity_acc.record(
                            &token.class_id,
                            &tx_data.hash,
                            &token.token_id,
                            parsed.number,
                            checked_usize_to_i32(tx_idx, "tx_idx"),
                            parsed.timestamp.timestamp_millis(),
                            true,
                        );
                    }
                    let dotbit_accounts = DotbitParser::parse_accounts(tx)?;
                    if !dotbit_accounts.is_empty() {
                        let das_action = DotbitParser::parse_das_action(&tx.witnesses);
                        let mut created_ids = HashSet::new();
                        for account in &dotbit_accounts {
                            self.writer.insert_dotbit_account_with_state(
                                account,
                                &tx_data.hash,
                                parsed.number,
                                parsed.timestamp.timestamp_millis(),
                                &mut nft_batch,
                                &mut dotbit_state,
                            )?;
                            batch_dotbit_outpoints.insert(
                                (tx_data.hash.to_vec(), account.output_index),
                                account.account.account_id.clone(),
                            );
                            let account_id = account.account.account_id.clone();
                            batch_dotbit_latest_create_order
                                .entry(account_id.clone())
                                .and_modify(|current| {
                                    if dotbit_create_order > *current {
                                        *current = dotbit_create_order;
                                    }
                                })
                                .or_insert(dotbit_create_order);
                            created_ids.insert(account_id);
                        }
                        dotbit_tx_activity_data.insert(
                            tx_data.hash,
                            DotbitTxActivityData {
                                das_action,
                                created_account_ids: created_ids,
                                consumed_account_ids: HashSet::new(),
                                block_number: parsed.number,
                                tx_idx: checked_usize_to_i32(tx_idx, "tx_idx"),
                                timestamp_ms: parsed.timestamp.timestamp_millis(),
                            },
                        );
                    }
                }
                block_tx_idx += tx_count_for_block;
            }
            let commit_started = Instant::now();
            nft_batch.commit()?;
            commit_ms += commit_started.elapsed().as_secs_f64() * 1000.0;
        }

        // Spore consumption runs in live sync mode only, DotBit consumption runs in all sync modes.
        let bulk_sync_active = self.is_bulk_sync_active();
        let mut consume_batch = StoreBatch::new(self.writer.store());
        let mut append_undo_seq_by_block: HashMap<i64, u64> = HashMap::new();
        let mut spore_state = self.writer.new_spore_batch_state();
        let mut dotbit_state = self.writer.new_dotbit_batch_state();
        let mut block_tx_idx = 0usize;
        for (block_idx, block_response) in blocks.iter().enumerate() {
            let parsed = &all_parsed_blocks[block_idx];
            let tx_count_for_block = parsed.transactions_count as usize;
            let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];

            for (tx_idx, tx_data) in tx_slice.iter().enumerate() {
                if tx_data.is_cellbase || tx_data.inputs.is_empty() {
                    continue;
                }
                let tx_global_index = block_tx_idx + tx_idx;
                let dotbit_consume_order = dotbit_consume_event_order(tx_global_index)?;
                let tx = &block_response.block.transactions[tx_idx];
                for input in &tx.inputs {
                    let prev_tx_hash =
                        crate::rpc::parse_hex_to_bytes(&input.previous_output.tx_hash);
                    let prev_index = parse_outpoint_index_i16(
                        &input.previous_output.index,
                        "input.previous_output.index",
                    )
                    .map_err(|e| {
                        anyhow!(
                            "invalid consumed spore input index at block {}, tx 0x{}: {}",
                            parsed.number,
                            hex::encode(tx_data.hash),
                            e
                        )
                    })?;

                    if !bulk_sync_active {
                        let consumed_spore_id = self
                            .writer
                            .get_spore_id_by_outpoint(&prev_tx_hash, prev_index)?;
                        if let Some(spore_id) = consumed_spore_id {
                            if !batch_spore_ids.contains(&spore_id) {
                                if let Some(coll_id) = self.writer.consume_spore(
                                    &spore_id,
                                    parsed.number,
                                    &tx_data.hash,
                                    &mut consume_batch,
                                    &mut spore_state,
                                )? {
                                    nft_activity_acc.record(
                                        &coll_id,
                                        &tx_data.hash,
                                        &spore_id,
                                        parsed.number,
                                        checked_usize_to_i32(tx_idx, "tx_idx"),
                                        parsed.timestamp.timestamp_millis(),
                                        false,
                                    );
                                }
                            }
                        }
                    }

                    let consumed_dotbit_account_id = resolve_dotbit_account_id_for_outpoint(
                        self.writer
                            .get_dotbit_account_id_by_outpoint(&prev_tx_hash, prev_index)?,
                        &prev_tx_hash,
                        prev_index,
                        &batch_dotbit_outpoints,
                    );
                    if let Some(account_id) = consumed_dotbit_account_id {
                        let latest_create_order =
                            batch_dotbit_latest_create_order.get(&account_id).copied();
                        if should_consume_dotbit_account(latest_create_order, dotbit_consume_order)
                            && self
                                .writer
                                .consume_dotbit_account_with_state(
                                    &account_id,
                                    parsed.number,
                                    &tx_data.hash,
                                    &mut consume_batch,
                                    &mut dotbit_state,
                                )?
                                .is_some()
                        {
                            let activity = dotbit_tx_activity_data
                                .entry(tx_data.hash)
                                .or_insert_with(|| {
                                    let das_action = DotbitParser::parse_das_action(&tx.witnesses);
                                    DotbitTxActivityData {
                                        das_action,
                                        created_account_ids: HashSet::new(),
                                        consumed_account_ids: HashSet::new(),
                                        block_number: parsed.number,
                                        tx_idx: checked_usize_to_i32(tx_idx, "tx_idx"),
                                        timestamp_ms: parsed.timestamp.timestamp_millis(),
                                    }
                                });
                            activity.consumed_account_ids.insert(account_id);
                        }
                    }
                }
            }
            block_tx_idx += tx_count_for_block;
        }
        let mut nft_activity_batch = StoreBatch::new(&self.append_only_store);
        // Write .bit collection activities directly (bypassing accumulator)
        for (_tx_hash, activity) in &dotbit_tx_activity_data {
            let inserted = resolve_dotbit_tx_activity(
                activity.das_action.as_deref(),
                &activity.created_account_ids,
                &activity.consumed_account_ids,
                _tx_hash,
                activity.block_number,
                activity.tx_idx,
                activity.timestamp_ms,
                &mut nft_activity_batch,
            );
            if inserted {
                let append_key = keys::encode_nft_collection_activity_key(
                    &DOTBIT_SENTINEL_COLLECTION,
                    activity.block_number,
                    activity.tx_idx,
                );
                put_append_delete_undo_entry(
                    &mut consume_batch,
                    &mut append_undo_seq_by_block,
                    UndoSeqScope::AppendNftCollectionActivity,
                    activity.block_number,
                    ckbadger_store::CF_NFT_COLLECTION_ACTIVITIES,
                    &append_key,
                );
            }
        }
        for (collection_id, block_number, tx_idx) in nft_activity_acc.flush(&mut nft_activity_batch)
        {
            let append_key =
                keys::encode_nft_collection_activity_key(&collection_id, block_number, tx_idx);
            put_append_delete_undo_entry(
                &mut consume_batch,
                &mut append_undo_seq_by_block,
                UndoSeqScope::AppendNftCollectionActivity,
                block_number,
                ckbadger_store::CF_NFT_COLLECTION_ACTIVITIES,
                &append_key,
            );
        }
        let commit_started = Instant::now();
        consume_batch.commit()?;
        commit_ms += commit_started.elapsed().as_secs_f64() * 1000.0;
        if !nft_activity_batch.is_empty() {
            let commit_started = Instant::now();
            nft_activity_batch.commit()?;
            commit_ms += commit_started.elapsed().as_secs_f64() * 1000.0;
        }

        // Finalization: persist block headers last as the durable sync marker,
        // together with stats derived from this batch.
        {
            let mut core_batch = StoreBatch::new(self.writer.store());
            self.writer
                .insert_blocks_batch(&block_refs, &mut core_batch)?;
            let mut stats_batch = StoreBatch::new(self.writer.store());
            self.write_batch_stats_to_batch(&batch_stats, &mut stats_batch)?;
            if bulk_sync_mode {
                let commit_started = Instant::now();
                core_batch.commit_no_wal()?;
                stats_batch.commit_no_wal()?;
                commit_ms += commit_started.elapsed().as_secs_f64() * 1000.0;
            } else {
                let commit_started = Instant::now();
                core_batch.commit()?;
                stats_batch.commit()?;
                commit_ms += commit_started.elapsed().as_secs_f64() * 1000.0;
            }
        }

        // HODL wave tracker update
        self.update_hodl_wave(
            &all_parsed_blocks,
            &all_tx_data,
            &input_cell_info,
            &batch_cell_infos,
            &address_balance_changes,
        )?;

        if let Some((block_number, ref block_hash)) = batch_stats.last_block {
            let ema_rate = self.progress.ema_blocks_per_second();
            let ema_rate_opt = if ema_rate > 0.0 { Some(ema_rate) } else { None };
            self.writer
                .update_sync_status(
                    block_number,
                    block_hash,
                    batch_stats.sync_totals.0,
                    batch_stats.sync_totals.1,
                    batch_stats.sync_totals.2,
                    batch_new_addresses,
                    ema_rate_opt,
                )
                .await?;
        }

        if !bulk_sync_mode {
            let committed_proposal_ids = collect_committed_proposal_ids(&all_tx_data);
            if !committed_proposal_ids.is_empty() {
                self.cache_invalidator
                    .remove_committed_proposals(&committed_proposal_ids)
                    .await;
            }
        }

        let stats_ms = t_stats.elapsed().as_secs_f64() * 1000.0;
        debug!(
            headers_ms = format!("{:.1}", headers_ms),
            cells_ms = format!("{:.1}", cells_ms),
            stats_ms = format!("{:.1}", stats_ms),
            commit_ms = format!("{:.1}", commit_ms),
            "Batch write breakdown"
        );

        Ok(BatchWriteMetrics { commit_ms })
    }
    // === write_parsed_batch (pipeline path) ===
    // This is largely identical to sync_blocks_batch but receives pre-parsed data
    // from the pipeline parser stage and writes blocks LAST as a commit marker.

    #[allow(clippy::too_many_arguments)]
    async fn write_parsed_batch(
        &self,
        blocks: &[BlockResponseWithCycles],
        all_parsed_blocks: &[crate::parser::block::ParsedBlock],
        all_tx_data: Vec<TxData>,
        input_cell_info: HashMap<(Vec<u8>, i16), LiveCellInfo>,
        batch_cell_infos: HashMap<(Vec<u8>, i16), LiveCellInfo>,
        address_balance_changes: HashMap<Vec<u8>, (i128, i32, i32, i64, i64, Vec<u8>, i128)>,
        script_usage_changes: ScriptUsageChanges,
        script_daily_changes: HashMap<(Vec<u8>, bool, u32), (i128, i128)>,
        token_daily_changes: HashMap<(Vec<u8>, u32), (i128, i128)>,
        spore_type_index_changes: HashMap<Vec<u8>, SporeTypeIndex>,
        spore_daily_changes: HashMap<(Vec<u8>, u32), (i128, i128)>,
        cluster_daily_changes: HashMap<(Vec<u8>, u32), (i128, i128)>,
        nft_type_index_changes: HashMap<Vec<u8>, NftTypeIndex>,
        nft_daily_changes: HashMap<(Vec<u8>, u32), (i128, i128)>,
        pre_parsed_spore_data: Vec<(Vec<ParsedSporeCell>, Vec<ParsedClusterCell>)>,
        pre_parsed_nft_data: PreParsedNftData,
        chain_tip: u64,
    ) -> Result<BatchWriteMetrics> {
        if all_parsed_blocks.is_empty() {
            return Ok(BatchWriteMetrics::default());
        }

        let first_block = all_parsed_blocks.first().map(|b| b.number).unwrap_or(0);
        let last_block = all_parsed_blocks.last().map(|b| b.number).unwrap_or(0);
        let end_block = last_block as u64;
        let bulk_sync_mode =
            is_bulk_sync_batch(chain_tip, end_block, self.config.bulk_sync_threshold);

        let all_input_outpoints: Vec<(Vec<u8>, i16)> = all_tx_data
            .iter()
            .filter(|tx| !tx.is_cellbase)
            .flat_map(|tx| {
                tx.inputs.iter().map(|input| {
                    (
                        input.previous_tx_hash.to_vec(),
                        parsed_input_outpoint_index_i16(
                            input.previous_output_index,
                            "sync_indexer",
                        ),
                    )
                })
            })
            .collect();
        let unresolved_outpoints = collect_missing_input_outpoints(
            &all_input_outpoints,
            &input_cell_info,
            &batch_cell_infos,
        );
        if !unresolved_outpoints.is_empty() {
            return Err(anyhow::anyhow!(
                "pipeline batch {}-{} has {} unresolved input cells (sample: {})",
                first_block,
                last_block,
                unresolved_outpoints.len(),
                format_outpoint_sample(&unresolved_outpoints, 5)
            ));
        }

        let t_precompute = Instant::now();

        // Build reference vectors from pre-computed data (Passes 1-3 done in parser)
        let mut all_cells: Vec<(&[u8], i16, &crate::parser::cell::ParsedCell, i64)> = Vec::new();
        let mut all_consumptions: Vec<(&[u8], i16, i64, &[u8], i64, i16)> = Vec::new();
        let mut txs_for_batch: Vec<_> = Vec::with_capacity(all_tx_data.len());

        for tx_data in &all_tx_data {
            txs_for_batch.push((
                tx_data.hash.as_slice(),
                tx_data.block_number,
                tx_data.block_hash.as_slice(),
                tx_data.tx_index,
                tx_data.version,
                tx_data.inputs_count,
                tx_data.outputs_count,
                tx_data.witnesses_count,
                tx_data.cell_deps_count,
                tx_data.header_deps_count,
                tx_data.total_input_capacity,
                tx_data.total_output_capacity,
                tx_data.fee,
                Some(tx_data.tx_size),
                tx_data.cycles,
                tx_data.is_cellbase,
                tx_data.timestamp,
            ));

            for (output_index, cell) in tx_data.cells.iter().enumerate() {
                let output_index_i16 =
                    checked_usize_to_i16(output_index, "pipeline sync output index for all_cells")
                        .map_err(|e| {
                            anyhow!(
                                "{}: tx_hash=0x{}, block={}",
                                e,
                                hex::encode(tx_data.hash),
                                tx_data.block_number
                            )
                        })?;
                all_cells.push((
                    tx_data.hash.as_slice(),
                    output_index_i16,
                    cell,
                    tx_data.block_number,
                ));
            }

            if !tx_data.is_cellbase {
                for (input_index, input) in tx_data.inputs.iter().enumerate() {
                    let input_index_i16 = checked_usize_to_i16(
                        input_index,
                        "pipeline sync input index for consumptions",
                    )
                    .map_err(|e| {
                        anyhow!(
                            "{}: tx_hash=0x{}, block={}",
                            e,
                            hex::encode(tx_data.hash),
                            tx_data.block_number
                        )
                    })?;
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
                        all_consumptions.push((
                            input.previous_tx_hash.as_slice(),
                            parsed_input_outpoint_index_i16(
                                input.previous_output_index,
                                "sync_indexer",
                            ),
                            info.created_at_block,
                            tx_data.hash.as_slice(),
                            tx_data.block_number,
                            input_index_i16,
                        ));
                    }
                }
            }
        }

        // Pre-compute cell index keys for T1b (parallel cell index writes).
        // Each op holds the 74-byte encoded keys so T1b can write them without
        // re-encoding or needing access to ParsedCell / LiveCellInfo.
        struct CellIndexOp {
            lock_hash_key: Vec<u8>,
            lock_code_hash_key: Vec<u8>,
            type_hash_key: Option<Vec<u8>>,
            type_code_hash_key: Option<Vec<u8>>,
        }

        let cell_index_puts: Vec<CellIndexOp> =
            all_cells
                .iter()
                .map(|(tx_hash, output_index, cell, block_number)| CellIndexOp {
                    lock_hash_key: keys::encode_cell_index_key(
                        &cell.lock_script_hash,
                        *block_number,
                        tx_hash,
                        *output_index,
                    ),
                    lock_code_hash_key: keys::encode_cell_index_key(
                        &cell.lock_code_hash,
                        *block_number,
                        tx_hash,
                        *output_index,
                    ),
                    type_hash_key: cell.type_script_hash.as_ref().map(|h| {
                        keys::encode_cell_index_key(h, *block_number, tx_hash, *output_index)
                    }),
                    type_code_hash_key: cell.type_code_hash.as_ref().map(|h| {
                        keys::encode_cell_index_key(h, *block_number, tx_hash, *output_index)
                    }),
                })
                .collect();

        let cell_index_deletes: Vec<CellIndexOp> = all_consumptions
            .iter()
            .filter_map(
                |(
                    tx_hash,
                    output_index,
                    _created_at_block,
                    _consumed_by_tx,
                    _consumed_at_block,
                    _input_index,
                )| {
                    let key = (tx_hash.to_vec(), *output_index);
                    let info = input_cell_info
                        .get(&key)
                        .or_else(|| batch_cell_infos.get(&key));
                    info.map(|info| CellIndexOp {
                        lock_hash_key: keys::encode_cell_index_key(
                            &info.lock_script_hash,
                            info.created_at_block,
                            tx_hash,
                            *output_index,
                        ),
                        lock_code_hash_key: keys::encode_cell_index_key(
                            &info.lock_code_hash,
                            info.created_at_block,
                            tx_hash,
                            *output_index,
                        ),
                        type_hash_key: info.type_script_hash.as_ref().map(|h| {
                            keys::encode_cell_index_key(
                                h,
                                info.created_at_block,
                                tx_hash,
                                *output_index,
                            )
                        }),
                        type_code_hash_key: info.type_code_hash.as_ref().map(|h| {
                            keys::encode_cell_index_key(
                                h,
                                info.created_at_block,
                                tx_hash,
                                *output_index,
                            )
                        }),
                    })
                },
            )
            .collect();

        // Compute per-tx address entries for addr_txs index
        let mut addr_tx_entries: Vec<(Vec<u8>, i64, i32, Vec<u8>)> = Vec::new();
        for tx_data in &all_tx_data {
            let mut touched: HashSet<Vec<u8>> = HashSet::new();
            for cell in &tx_data.cells {
                touched.insert(cell.lock_script_hash.clone());
            }
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
                        touched.insert(info.lock_script_hash.clone());
                    }
                }
            }
            for lock_hash in touched {
                addr_tx_entries.push((
                    lock_hash,
                    tx_data.block_number,
                    tx_data.tx_index,
                    tx_data.hash.to_vec(),
                ));
            }
        }

        let changes_ref: HashMap<Vec<u8>, (i128, i32, i32, i64, i64, &[u8], i128)> =
            address_balance_changes
                .iter()
                .map(|(k, (a, b, c, d, e, f, g))| {
                    (k.clone(), (*a, *b, *c, *d, *e, f.as_slice(), *g))
                })
                .collect();

        let block_refs: Vec<&crate::parser::block::ParsedBlock> =
            all_parsed_blocks.iter().collect();

        // Pass 4: Proposals (iterates all_parsed_blocks, spawns background cache task)
        let mut batch_proposals: Vec<(Vec<u8>, i64, i16)> = Vec::new();
        let is_bulk = self.is_bulk_sync_active();
        let mut last_proposal_block: i64 = 0;
        for parsed_block in all_parsed_blocks {
            if !parsed_block.proposals.is_empty() {
                for (proposal_index, proposal_id) in parsed_block.proposals.iter().enumerate() {
                    if !is_bulk {
                        let proposal_index_i16 = checked_usize_to_i16(
                            proposal_index,
                            "pipeline proposal index while populating proposal cache batch",
                        )
                        .map_err(|e| {
                            anyhow!(
                                "{}: block_number={}, proposal_count={}",
                                e,
                                parsed_block.number,
                                parsed_block.proposals.len()
                            )
                        })?;
                        batch_proposals.push((
                            proposal_id.clone(),
                            parsed_block.number,
                            proposal_index_i16,
                        ));
                    }
                }
                if !is_bulk {
                    last_proposal_block = parsed_block.number;
                }
            }
        }
        if !batch_proposals.is_empty() {
            tokio::spawn(Self::run_proposal_cache_batch(
                self.rpc.clone(),
                self.cache_invalidator.clone(),
                batch_proposals,
                last_proposal_block,
            ));
        }

        let skip_address_balances = should_skip_address_balances(bulk_sync_mode);
        let skip_activities = false;

        let precompute_ms = t_precompute.elapsed().as_secs_f64() * 1000.0;

        // DAO, UDT, NFT processing flags
        let dao_code_hash = crate::rpc::parse_hex_to_bytes(crate::parser::dao::DAO_CODE_HASH);
        let skip_token = false;
        let skip_spore = false;

        // Pre-fetch DAO, UDT, address balance, and script info data outside thread::scope.
        // 4-way rayon::join overlaps all DB reads: takes max(dao, udt, addr, script).
        let t_prefetch = Instant::now();

        // Prepare address balance + script info keys for prefetch
        let lock_hash_keys: Vec<&Vec<u8>> = if !skip_address_balances {
            changes_ref.keys().collect()
        } else {
            Vec::new()
        };

        let unique_code_hashes: Vec<Vec<u8>> = {
            let mut seen = std::collections::HashSet::new();
            script_usage_changes
                .keys()
                .filter_map(|(code_hash, _)| {
                    if seen.insert(code_hash.clone()) {
                        Some(code_hash.clone())
                    } else {
                        None
                    }
                })
                .collect()
        };
        let code_hash_refs: Vec<&Vec<u8>> = unique_code_hashes.iter().collect();

        let (
            (
                consumed_dao_map,
                (prefetched_input_udt_info, prefetched_batch_udt_cells, prefetched_udt_tx_infos),
            ),
            (prefetched_addr_balances, prefetched_script_info),
        ) = if bulk_sync_mode {
            let writer = &self.writer;
            let udt_cache = &self.udt_cell_cache;
            let prefetch_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                rayon::join(
                    || {
                        rayon::join(
                            || {
                                // DAO: collect input outpoints, deduplicate, batch query DB
                                let mut all_input_outpoints_dao: Vec<(Vec<u8>, i16)> = Vec::new();
                                let mut block_tx_idx = 0usize;
                                for parsed in all_parsed_blocks.iter() {
                                    let tx_count_for_block = parsed.transactions_count as usize;
                                    let tx_slice = &all_tx_data
                                        [block_tx_idx..block_tx_idx + tx_count_for_block];
                                    block_tx_idx += tx_count_for_block;
                                    for tx_data in tx_slice {
                                        if tx_data.is_cellbase || tx_data.inputs.is_empty() {
                                            continue;
                                        }
                                        for input in &tx_data.inputs {
                                            all_input_outpoints_dao.push((
                                                input.previous_tx_hash.to_vec(),
                                                parsed_input_outpoint_index_i16(
                                                    input.previous_output_index,
                                                    "sync_indexer",
                                                ),
                                            ));
                                        }
                                    }
                                }
                                if !all_input_outpoints_dao.is_empty() {
                                    let unique_outpoints: Vec<(Vec<u8>, i16)> = {
                                        let mut seen = HashSet::new();
                                        all_input_outpoints_dao
                                            .into_iter()
                                            .filter(|x| seen.insert(x.clone()))
                                            .collect()
                                    };
                                    let outpoint_refs: Vec<(&[u8], i16)> = unique_outpoints
                                        .iter()
                                        .map(|(h, i)| (h.as_slice(), *i))
                                        .collect();
                                    writer
                                    .find_consumed_dao_deposits_batch(&outpoint_refs)
                                    .unwrap_or_else(|e| {
                                        panic!(
                                            "failed to prefetch consumed DAO deposits: outpoints={}, error={}",
                                            outpoint_refs.len(),
                                            e
                                        )
                                    })
                                } else {
                                    HashMap::new()
                                }
                            },
                            || {
                                // UDT: parse outputs, populate cache, collect input outpoints,
                                // cache lookup + DB fallback
                                struct TxInfoForUdt {
                                    tx_hash: Vec<u8>,
                                    block_number: i64,
                                    timestamp: chrono::DateTime<Utc>,
                                    output_udts: Vec<crate::parser::ParsedUdtCell>,
                                    input_outpoints: Vec<(Vec<u8>, i16)>,
                                }
                                let mut all_tx_infos_for_udt: Vec<TxInfoForUdt> = Vec::new();
                                let mut all_input_outpoints_udt: Vec<(Vec<u8>, i16)> = Vec::new();
                                let mut batch_udt_cells: HashMap<
                                    (Vec<u8>, i16),
                                    crate::parser::ParsedUdtCell,
                                > = HashMap::new();

                                let mut block_tx_idx = 0usize;
                                for (block_idx, block_response) in blocks.iter().enumerate() {
                                    let parsed = &all_parsed_blocks[block_idx];
                                    let tx_count_for_block = parsed.transactions_count as usize;
                                    let tx_slice = &all_tx_data
                                        [block_tx_idx..block_tx_idx + tx_count_for_block];
                                    block_tx_idx += tx_count_for_block;
                                    for (tx_idx, tx_data) in tx_slice.iter().enumerate() {
                                        if tx_data.is_cellbase {
                                            continue;
                                        }
                                        let tx = &block_response.block.transactions[tx_idx];
                                        let mut output_udts: Vec<crate::parser::ParsedUdtCell> =
                                            Vec::new();
                                        for (output_index, udt_cell) in self
                                        .parse_udt_cells_with_store_fallback(tx)
                                        .unwrap_or_else(|e| {
                                            panic!(
                                                "UDT prefetch parse failed: tx_hash=0x{}, block={}, error={}",
                                                hex::encode(tx_data.hash),
                                                parsed.number,
                                                e
                                            )
                                        })
                                    {
                                        batch_udt_cells.insert(
                                            (tx_data.hash.to_vec(), output_index),
                                            udt_cell.clone(),
                                        );
                                        udt_cache.insert(
                                            (tx_data.hash, output_index),
                                            CachedUdtCellInfo {
                                                type_script_hash: udt_cell.type_script_hash.clone(),
                                                type_code_hash: udt_cell.type_code_hash.clone(),
                                                type_hash_type: udt_cell.type_hash_type,
                                                type_args: udt_cell.type_args.clone(),
                                                lock_script_hash: udt_cell.lock_script_hash.clone(),
                                                amount: udt_cell.amount,
                                                standard: udt_cell.standard.as_str().to_string(),
                                            },
                                        );
                                        output_udts.push(udt_cell);
                                    }
                                        let input_outpoints: Vec<(Vec<u8>, i16)> = tx_data
                                        .inputs
                                        .iter()
                                        .map(|i| {
                                            let previous_output_index = i16::try_from(
                                                i.previous_output_index,
                                            )
                                            .unwrap_or_else(|_| {
                                                panic!(
                                                    "UDT prefetch input previous_output_index exceeds i16 range: tx_hash=0x{}, block={}, previous_output_index={}",
                                                    hex::encode(tx_data.hash),
                                                    parsed.number,
                                                    i.previous_output_index
                                                )
                                            });
                                            (i.previous_tx_hash.to_vec(), previous_output_index)
                                        })
                                        .collect();
                                        all_input_outpoints_udt
                                            .extend(input_outpoints.iter().cloned());
                                        all_tx_infos_for_udt.push(TxInfoForUdt {
                                            tx_hash: tx_data.hash.to_vec(),
                                            block_number: parsed.number,
                                            timestamp: parsed.timestamp,
                                            output_udts,
                                            input_outpoints,
                                        });
                                    }
                                }

                                // Resolve UDT inputs from live_cells only.
                                let mut input_udt_info: HashMap<
                                    (Vec<u8>, i16),
                                    (Vec<u8>, Vec<u8>, i16, Vec<u8>, Vec<u8>, u128, String),
                                > = HashMap::new();
                                if !skip_token && !all_input_outpoints_udt.is_empty() {
                                    let db_results = resolve_input_udt_info_from_live_cells(
                                    writer,
                                    udt_cache,
                                    &all_input_outpoints_udt,
                                )
                                .unwrap_or_else(|e| {
                                    panic!(
                                        "failed to resolve UDT input info from live cells during bulk prefetch: inputs={}, error={}",
                                        all_input_outpoints_udt.len(),
                                        e
                                    )
                                });
                                    input_udt_info.extend(db_results);
                                }

                                // Build tx contexts for UDT processing
                                struct UdtTxInfo {
                                    tx_hash: Vec<u8>,
                                    block_number: i64,
                                    #[allow(dead_code)]
                                    timestamp: chrono::DateTime<Utc>,
                                    output_udts: Vec<crate::parser::ParsedUdtCell>,
                                    input_outpoints: Vec<(Vec<u8>, i16)>,
                                }
                                let mut udt_tx_contexts: Vec<UdtTxInfo> = Vec::new();
                                for tx_info in all_tx_infos_for_udt {
                                    let has_udt_outputs = !tx_info.output_udts.is_empty();
                                    let has_udt_inputs =
                                        tx_info.input_outpoints.iter().any(|(tx_hash, idx)| {
                                            input_udt_info.contains_key(&(tx_hash.clone(), *idx))
                                                || batch_udt_cells
                                                    .contains_key(&(tx_hash.clone(), *idx))
                                        });
                                    if has_udt_outputs || has_udt_inputs {
                                        udt_tx_contexts.push(UdtTxInfo {
                                            tx_hash: tx_info.tx_hash,
                                            block_number: tx_info.block_number,
                                            timestamp: tx_info.timestamp,
                                            output_udts: tx_info.output_udts,
                                            input_outpoints: tx_info.input_outpoints,
                                        });
                                    }
                                }

                                (input_udt_info, batch_udt_cells, udt_tx_contexts)
                            },
                        )
                    },
                    || {
                        rayon::join(
                            || {
                                if !lock_hash_keys.is_empty() {
                                    writer
                                    .read_address_balances(&lock_hash_keys)
                                    .unwrap_or_else(|e| {
                                        panic!(
                                            "failed to prefetch address balances: lock_hashes={}, error={}",
                                            lock_hash_keys.len(),
                                            e
                                        )
                                    })
                                } else {
                                    HashMap::new()
                                }
                            },
                            || {
                                if !code_hash_refs.is_empty() {
                                    writer
                                        .read_script_info(&code_hash_refs)
                                        .unwrap_or_else(|e| {
                                            panic!(
                                        "failed to prefetch script info: code_hashes={}, error={}",
                                        code_hash_refs.len(),
                                        e
                                    )
                                        })
                                } else {
                                    HashMap::new()
                                }
                            },
                        )
                    },
                )
            }));
            prefetch_result.map_err(|panic_payload| {
                anyhow!(
                    "bulk prefetch worker panicked for blocks {}-{}: {}",
                    first_block,
                    last_block,
                    panic_payload_to_string(panic_payload.as_ref())
                )
            })?
        } else {
            (
                (HashMap::new(), (HashMap::new(), HashMap::new(), Vec::new())),
                (HashMap::new(), HashMap::new()),
            )
        };
        let prefetch_ms = t_prefetch.elapsed().as_secs_f64() * 1000.0;
        let mut batch_new_addresses = 0i64;
        if bulk_sync_mode && !skip_address_balances && !changes_ref.is_empty() {
            batch_new_addresses = count_new_addresses(&changes_ref, &prefetched_addr_balances);
        }

        let t_write = Instant::now();
        let mut write_commit_ms = 0.0_f64;
        let mut batch_stats;
        let mut thread_times: Option<[f64; 8]> = None;
        if bulk_sync_mode {
            // Parallel write path: each thread writes to its own StoreBatch and commits independently.
            // DAO/UDT/addr/script DB reads are pre-fetched above via rayon::join, so threads only do writes.
            // Independent batches let all threads run fully in parallel; the RocksDB write
            // group overhead (~2ms) is negligible.
            let store = self.writer.store();
            let append_only_store = &self.append_only_store;
            let writer = &self.writer;

            let tt;
            (batch_stats, tt, write_commit_ms) = std::thread::scope(
                |s| -> Result<(BatchStats, [f64; 8], f64)> {
                    // T1: Cells + Consumption + Cell Indexes (merged T1a+T1b)
                    // CFs: LIVE_CELLS, CONSUMED_CELLS, CELL_BY_LOCK, CELL_BY_TYPE,
                    //       CELL_BY_LOCK_CODE, CELL_BY_TYPE_CODE
                    // Single batch + single commit reduces atomic flush trigger frequency.
                    let h1 = s.spawn(|| -> Result<(f64, f64)> {
                        let t = Instant::now();
                        let mut batch = StoreBatch::new(store);
                        if !all_cells.is_empty() {
                            writer.insert_cells_batch(&all_cells, &mut batch, true)?;
                        }
                        if !all_consumptions.is_empty() {
                            writer.consume_cells_batch_preloaded(
                                &all_consumptions,
                                &input_cell_info,
                                &batch_cell_infos,
                                &mut batch,
                                true,
                            )?;
                        }
                        for op in &cell_index_puts {
                            batch.put_cell_by_lock_raw(&op.lock_hash_key);
                            batch.put_cell_by_lock_code_raw(&op.lock_code_hash_key);
                            if let Some(ref k) = op.type_hash_key {
                                batch.put_cell_by_type_raw(k);
                            }
                            if let Some(ref k) = op.type_code_hash_key {
                                batch.put_cell_by_type_code_raw(k);
                            }
                        }
                        for op in &cell_index_deletes {
                            batch.delete_cell_by_lock_raw(&op.lock_hash_key);
                            batch.delete_cell_by_lock_code_raw(&op.lock_code_hash_key);
                            if let Some(ref k) = op.type_hash_key {
                                batch.delete_cell_by_type_raw(k);
                            }
                            if let Some(ref k) = op.type_code_hash_key {
                                batch.delete_cell_by_type_code_raw(k);
                            }
                        }
                        let commit_ms =
                            commit_phase_no_wal("T1_cells", first_block, last_block, batch)?;
                        Ok((t.elapsed().as_secs_f64() * 1000.0, commit_ms))
                    });

                    // T2: Transactions + Address Balances + Script Usage + Addr TX index
                    // CFs: TX_INDEX, TX_HASH_MAP, ADDR_BALANCE, SCRIPT_INFO, REORG_UNDO_LOG_BY_BLOCK, ADDR_TX
                    let h2 = s.spawn(|| -> Result<(f64, f64)> {
                        let t = Instant::now();
                        let mut batch = StoreBatch::new(store);
                        let mut domain_analytics_batch = StoreBatch::new(store);
                        let mut append_history_batch = StoreBatch::new(append_only_store);
                        let mut append_undo_seq_by_block: HashMap<i64, u64> = HashMap::new();
                        if !all_tx_data.is_empty() {
                            put_tx_context_undo_entries(
                                &mut batch,
                                &mut append_undo_seq_by_block,
                                &all_tx_data,
                            )?;
                        }
                        if !txs_for_batch.is_empty() {
                            writer.insert_transactions_batch(&txs_for_batch, &mut batch)?;
                        }
                        if !skip_address_balances && !changes_ref.is_empty() {
                            writer.apply_address_balance_deltas(
                                &prefetched_addr_balances,
                                &changes_ref,
                                &mut batch,
                            )?;
                        }
                        if !script_usage_changes.is_empty() {
                            writer.apply_script_usage_deltas(
                                &prefetched_script_info,
                                &script_usage_changes,
                                &mut domain_analytics_batch,
                            )?;
                        }
                        if !script_daily_changes.is_empty() {
                            writer.update_script_daily_deltas_batch(
                                &script_daily_changes,
                                &mut domain_analytics_batch,
                            )?;
                        }
                        if !token_daily_changes.is_empty() {
                            writer.update_token_daily_deltas_batch(
                                &token_daily_changes,
                                &mut domain_analytics_batch,
                            )?;
                        }
                        if !spore_type_index_changes.is_empty() {
                            writer.update_spore_type_index_batch(
                                &spore_type_index_changes,
                                &mut batch,
                            )?;
                        }
                        if !spore_daily_changes.is_empty() {
                            writer.update_spore_daily_deltas_batch(
                                &spore_daily_changes,
                                &mut domain_analytics_batch,
                            )?;
                        }
                        if !nft_type_index_changes.is_empty() {
                            writer
                                .update_nft_type_index_batch(&nft_type_index_changes, &mut batch)?;
                        }
                        if !nft_daily_changes.is_empty() {
                            writer.update_nft_daily_deltas_batch(
                                &nft_daily_changes,
                                &mut domain_analytics_batch,
                            )?;
                        }
                        if !cluster_daily_changes.is_empty() {
                            writer.update_cluster_daily_deltas_batch(
                                &cluster_daily_changes,
                                &mut domain_analytics_batch,
                            )?;
                        }
                        for (lock_hash, block_num, tx_idx, tx_hash) in &addr_tx_entries {
                            put_addr_tx_with_undo_log(
                                &mut batch,
                                &mut append_history_batch,
                                &mut append_undo_seq_by_block,
                                lock_hash,
                                *block_num,
                                *tx_idx,
                                tx_hash,
                            );
                        }
                        let mut commit_ms =
                            commit_phase_no_wal("T2_txs_addr", first_block, last_block, batch)?;
                        if !domain_analytics_batch.is_empty() {
                            commit_ms += commit_phase_no_wal(
                                "T2_domain_analytics",
                                first_block,
                                last_block,
                                domain_analytics_batch,
                            )?;
                        }
                        if !append_history_batch.is_empty() {
                            commit_ms += commit_phase_no_wal(
                                "T2_append_history",
                                first_block,
                                last_block,
                                append_history_batch,
                            )?;
                        }
                        Ok((t.elapsed().as_secs_f64() * 1000.0, commit_ms))
                    });

                    // T4: DAO (writes only — DB reads pre-fetched above)
                    // CFs: DAO_DEPOSITS, DAO_BY_WITHDRAW_TX
                    let h4 = s.spawn(|| -> Result<(f64, f64)> {
                    let t = Instant::now();
                    let mut batch = StoreBatch::new(store);

                    // DAO deposits
                    let mut all_dao_deposits: Vec<(
                        crate::parser::ParsedDaoDeposit,
                        i64,
                        DateTime<Utc>,
                        i64,
                    )> = Vec::new();
                    let mut block_tx_idx = 0usize;
                    for parsed in all_parsed_blocks {
                        let tx_count_for_block = parsed.transactions_count as usize;
                        let tx_slice =
                            &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                        block_tx_idx += tx_count_for_block;
                        let ar = extract_ar_i64_from_dao(&parsed.dao, parsed.number)?;
                        for tx_data in tx_slice {
                            let dao_deposits =
                                DaoParser::parse_deposits_from_cells(&tx_data.hash, &tx_data.cells);
                            for deposit in dao_deposits {
                                all_dao_deposits.push((
                                    deposit,
                                    parsed.number,
                                    parsed.timestamp,
                                    ar,
                                ));
                            }
                        }
                    }
                    if !all_dao_deposits.is_empty() {
                        writer.insert_dao_deposits_batch(&all_dao_deposits, &mut batch)?;
                    }

                    // Build a same-batch deposit map so that deposits created
                    // and consumed within the same batch can be found.
                    // consumed_dao_map was pre-fetched from DB before the batch,
                    // so it misses deposits written above in this batch.
                    let mut same_batch_dao_deposits: HashMap<
                        (Vec<u8>, i16),
                        (i64, Vec<u8>, i16, String, i64, i16),
                    > = HashMap::new();
                    // Also build a pending entries map keyed by outpoint for
                    // process_dao_withdrawals_batch to update same-batch deposits.
                    let mut pending_dao_entries: HashMap<
                        [u8; 34],
                        ckbadger_store::types::DaoDepositCacheEntry,
                    > = HashMap::new();
                    for (deposit, block_number, _ts, ar) in &all_dao_deposits {
                        let deposit_output_index = checked_i32_to_i16(
                            deposit.output_index,
                            "DAO deposit output index while building same-batch map",
                        )
                        .map_err(|e| {
                            anyhow!(
                                "{}: deposit_tx_hash=0x{}, block={}",
                                e,
                                hex::encode(&deposit.tx_hash),
                                block_number
                            )
                        })?;
                        same_batch_dao_deposits.insert(
                            (deposit.tx_hash.clone(), deposit_output_index),
                            (
                                0,
                                deposit.tx_hash.clone(),
                                deposit_output_index,
                                deposit.capacity.to_string(),
                                *block_number,
                                0i16, // status = 0 (active)
                            ),
                        );
                        let outpoint_key = ckbadger_store::keys::encode_outpoint(
                            &deposit.tx_hash,
                            deposit_output_index,
                        );
                        pending_dao_entries.insert(
                            outpoint_key,
                            ckbadger_store::types::DaoDepositCacheEntry {
                                capacity: deposit.capacity,
                                deposit_block_number: *block_number,
                                lock_script_hash: deposit.lock_script_hash.clone(),
                                deposit_ar: *ar,
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
                    }

                    if !consumed_dao_map.is_empty() || !same_batch_dao_deposits.is_empty() {
                        let mut withdrawal_contexts: Vec<DaoWithdrawalContext> = Vec::new();
                        let mut block_tx_idx = 0usize;
                        for parsed in all_parsed_blocks {
                            let tx_count_for_block = parsed.transactions_count as usize;
                            let tx_slice =
                                &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                            block_tx_idx += tx_count_for_block;
                            for tx_data in tx_slice {
                                if tx_data.is_cellbase || tx_data.inputs.is_empty() {
                                    continue;
                                }
                                let mut consumed_deposits: Vec<(
                                    i64,
                                    Vec<u8>,
                                    i16,
                                    String,
                                    i64,
                                    i16,
                                )> = Vec::new();
                                for input in &tx_data.inputs {
                                    let key = (
                                        input.previous_tx_hash.to_vec(),
                                        parsed_input_outpoint_index_i16(input.previous_output_index, "sync_indexer"),
                                    );
                                    if let Some(deposit_info) = consumed_dao_map
                                        .get(&key)
                                        .or_else(|| same_batch_dao_deposits.get(&key))
                                    {
                                        consumed_deposits.push(deposit_info.clone());
                                    }
                                }
                                if consumed_deposits.is_empty() {
                                    continue;
                                }
                                let tx_inputs: Vec<(Vec<u8>, i16)> = tx_data
                                    .inputs
                                    .iter()
                                    .map(|input| {
                                        let output_index =
                                            i16::try_from(input.previous_output_index).map_err(
                                                |_| {
                                                    anyhow!(
                                                        "DAO processing input index exceeds i16 range: tx_hash=0x{}, previous_output_index={}",
                                                        hex::encode(tx_data.hash),
                                                        input.previous_output_index
                                                    )
                                                },
                                            )?;
                                        Ok((input.previous_tx_hash.to_vec(), output_index))
                                    })
                                    .collect::<Result<_>>()?;
                                let mut new_dao_outputs: Vec<(Vec<u8>, i16, Vec<u8>, i64, u64)> =
                                    Vec::new();
                                let mut candidate_withdraw_to_outputs: Vec<(i16, Vec<u8>)> =
                                    Vec::new();
                                for (idx, cell) in tx_data.cells.iter().enumerate() {
                                    let output_index = checked_usize_to_i16(
                                        idx,
                                        "DAO output index while building withdrawal contexts",
                                    )
                                    .map_err(|e| {
                                        anyhow!(
                                            "{}: tx_hash=0x{}, block={}",
                                            e,
                                            hex::encode(tx_data.hash),
                                            parsed.number
                                        )
                                    })?;
                                    if let Some(ref type_code_hash) = cell.type_code_hash {
                                        if type_code_hash == &dao_code_hash && cell.data_size == 8 {
                                            if let Some(data) = tx_data.outputs_data.get(idx) {
                                                let data_bytes =
                                                    crate::rpc::parse_hex_to_bytes(data);
                                                if let Some(deposit_block) =
                                                    DaoParser::parse_deposit_block_number(
                                                        &data_bytes,
                                                    )
                                                {
                                                    new_dao_outputs.push((
                                                        tx_data.hash.to_vec(),
                                                        output_index,
                                                        cell.lock_script_hash.clone(),
                                                        cell.capacity,
                                                        deposit_block,
                                                    ));
                                                }
                                            }
                                        } else {
                                            candidate_withdraw_to_outputs
                                                .push((output_index, cell.lock_script_hash.clone()));
                                        }
                                    } else {
                                        candidate_withdraw_to_outputs
                                            .push((output_index, cell.lock_script_hash.clone()));
                                    }
                                }
                                withdrawal_contexts.push(DaoWithdrawalContext {
                                    consumed_deposits,
                                    new_dao_outputs,
                                    tx_inputs,
                                    candidate_withdraw_to_outputs,
                                    block_number: parsed.number,
                                    consuming_tx_hash: tx_data.hash.to_vec(),
                                    timestamp: parsed.timestamp,
                                });
                            }
                        }
                        if !withdrawal_contexts.is_empty() {
                            writer.process_dao_withdrawals_batch(
                                &withdrawal_contexts,
                                &mut batch,
                                &pending_dao_entries,
                            )?;
                        }
                    }

                    let commit_ms = commit_phase_no_wal("T4_dao", first_block, last_block, batch)?;
                    Ok((t.elapsed().as_secs_f64() * 1000.0, commit_ms))
                });

                    // T5: UDT (writes only — DB reads + parsing pre-fetched above)
                    // CFs: TOKENS, TOKEN_HOLDERS
                    let input_udt_info = &prefetched_input_udt_info;
                    let batch_udt_cells = &prefetched_batch_udt_cells;
                    let max_supply_observations =
                        collect_token_max_supply_observations(&all_tx_data);
                    let h5 =
                        s.spawn(move || -> Result<(f64, f64)> {
                            let t = Instant::now();
                            let mut batch = StoreBatch::new(store);

                            if !skip_token && !prefetched_udt_tx_infos.is_empty() {
                                let mut all_transfers: Vec<(
                                    crate::parser::ParsedUdtTransfer,
                                    Vec<u8>,
                                    i64,
                                )> = Vec::new();
                                for ctx in &prefetched_udt_tx_infos {
                                    let mut input_udts: Vec<crate::parser::ParsedUdtCell> =
                                        Vec::new();
                                    for (tx_hash, idx) in &ctx.input_outpoints {
                                        if let Some((tsh, tch, tht, ta, lsh, am, std)) =
                                            input_udt_info.get(&(tx_hash.clone(), *idx))
                                        {
                                            input_udts.push(crate::parser::ParsedUdtCell {
                                                type_script_hash: tsh.clone(),
                                                type_code_hash: tch.clone(),
                                                type_hash_type: *tht,
                                                type_args: ta.clone(),
                                                lock_script_hash: lsh.clone(),
                                                amount: *am,
                                                standard: crate::parser::UdtStandard::parse(std),
                                            });
                                        } else if let Some(udt_cell) =
                                            batch_udt_cells.get(&(tx_hash.clone(), *idx))
                                        {
                                            input_udts.push(udt_cell.clone());
                                        }
                                    }
                                    for transfer in
                                        crate::parser::UdtParser::build_transfers_from_cells(
                                            &input_udts,
                                            &ctx.output_udts,
                                        )
                                    {
                                        all_transfers.push((
                                            transfer,
                                            ctx.tx_hash.clone(),
                                            ctx.block_number,
                                        ));
                                    }
                                }

                                if !all_transfers.is_empty() {
                                    let transfer_refs: Vec<_> = all_transfers
                                        .iter()
                                        .map(|(t, h, b)| (t, h.as_slice(), *b))
                                        .collect();
                                    let block_timestamps: HashMap<i64, i64> = all_parsed_blocks
                                        .iter()
                                        .map(|p| (p.number, p.timestamp.timestamp_millis()))
                                        .collect();
                                    let mut udt_state = writer.new_udt_batch_state();
                                    writer.process_udt_transfers_batch_with_state(
                                        &transfer_refs,
                                        &max_supply_observations,
                                        &block_timestamps,
                                        &mut batch,
                                        &mut udt_state,
                                    )?;
                                }
                            }

                            let commit_ms =
                                commit_phase_no_wal("T5_udt", first_block, last_block, batch)?;
                            Ok((t.elapsed().as_secs_f64() * 1000.0, commit_ms))
                        });

                    // T6a: Spore writes (clusters + cells + content).
                    // CFs: SPORE_DATA, SPORE_BY_CLUSTER, CLUSTER_AGG, STATS (spore keys only)
                    // Runs in parallel with T6b — writes to independent CFs.
                    let h6a = s.spawn(|| -> Result<(f64, f64)> {
                        if skip_spore {
                            return Ok((0.0, 0.0));
                        }
                        let t = Instant::now();
                        let mut batch = StoreBatch::new(store);
                        let mut activity_batch = StoreBatch::new(append_only_store);
                        let mut spore_state = writer.new_spore_batch_state();
                        let mut spore_activity_acc = NftCollectionActivityAccumulator::new();
                        let mut block_tx_idx = 0usize;
                        for (block_idx, _block_response) in blocks.iter().enumerate() {
                            let parsed = &all_parsed_blocks[block_idx];
                            let tx_count_for_block = parsed.transactions_count as usize;
                            let tx_slice =
                                &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                            let ts_ms = parsed.timestamp.timestamp_millis();
                            for (tx_idx, tx_data) in tx_slice.iter().enumerate() {
                                let tx_global_index = block_tx_idx + tx_idx;
                                let (ref pre_spores, ref pre_clusters) =
                                    pre_parsed_spore_data[tx_global_index];
                                for cluster in pre_clusters {
                                    writer.insert_spore_cluster(
                                        cluster,
                                        parsed.number,
                                        &tx_data.hash,
                                        &mut batch,
                                        &mut spore_state,
                                    )?;
                                }
                                for (output_index, spore) in pre_spores.iter().enumerate() {
                                    let output_index_i16 = checked_usize_to_i16(
                                        output_index,
                                        "spore output index while flushing pre-parsed spores",
                                    )
                                    .map_err(|e| {
                                        anyhow!(
                                            "{}: block={}, tx_hash=0x{}",
                                            e,
                                            parsed.number,
                                            hex::encode(tx_data.hash)
                                        )
                                    })?;
                                    writer.insert_spore_cell(
                                        spore,
                                        &tx_data.hash,
                                        output_index_i16,
                                        parsed.number,
                                        ts_ms,
                                        &mut batch,
                                        &mut spore_state,
                                    )?;
                                    let coll_id = if spore.is_did {
                                        &DID_CKB_SENTINEL_COLLECTION[..]
                                    } else if let Some(ref cid) = spore.cluster_id {
                                        cid.as_slice()
                                    } else {
                                        continue;
                                    };
                                    spore_activity_acc.record(
                                        coll_id,
                                        &tx_data.hash,
                                        &spore.spore_id,
                                        parsed.number,
                                        checked_usize_to_i32(tx_idx, "tx_idx"),
                                        ts_ms,
                                        true,
                                    );
                                }
                            }
                            block_tx_idx += tx_count_for_block;
                        }
                        spore_activity_acc.flush(&mut activity_batch);
                        let mut commit_ms =
                            commit_phase_no_wal("T6a_spore", first_block, last_block, batch)?;
                        if !activity_batch.is_empty() {
                            commit_ms += commit_phase_no_wal(
                                "T6a_spore_activity",
                                first_block,
                                last_block,
                                activity_batch,
                            )?;
                        }
                        Ok((t.elapsed().as_secs_f64() * 1000.0, commit_ms))
                    });

                    // T6b: mNFT + DotBit writes and DotBit consumption.
                    // CFs: NFT_DATA, NFT_COLLECTION_AGG, NFT_BY_COLLECTION, STATS (nft keys)
                    // Runs in parallel with T6a — mNFT/DotBit use independent CFs from Spore.
                    // All parsing is done in the parser stage (pre_parsed_nft_data);
                    // this thread only does DB writes.
                    let h6b = s.spawn(|| -> Result<(f64, f64)> {
                    let t = Instant::now();
                    let mut batch = StoreBatch::new(store);
                    let mut activity_batch = StoreBatch::new(append_only_store);
                    let mut append_undo_seq_by_block: HashMap<i64, u64> = HashMap::new();
                    let mut dotbit_state = writer.new_dotbit_batch_state();
                    let mut mnft_state = writer.new_mnft_batch_state();
                    let mut nft_activity_acc = NftCollectionActivityAccumulator::new();

                    // Build tx_global_index → (tx_idx_in_block, block_number, ts_ms) lookup.
                    let mut tx_lookup: Vec<(usize, i64, i64)> = Vec::with_capacity(all_tx_data.len());
                    for parsed in all_parsed_blocks.iter().take(blocks.len()) {
                        let tx_count_for_block = parsed.transactions_count as usize;
                        let ts_ms = parsed.timestamp.timestamp_millis();
                        for tx_idx in 0..tx_count_for_block {
                            tx_lookup.push((tx_idx, parsed.number, ts_ms));
                        }
                    }

                    // Phase 1: Insert mNFT issuers/classes/tokens from pre-parsed data.
                    for &(tx_gi, ref issuer) in &pre_parsed_nft_data.mnft_issuers {
                        let (_, block_number, _) = tx_lookup[tx_gi];
                        writer.insert_mnft_issuer(
                            issuer,
                            &all_tx_data[tx_gi].hash,
                            0,
                            block_number,
                            &mut batch,
                        )?;
                    }
                    for &(tx_gi, output_index, ref class) in &pre_parsed_nft_data.mnft_classes {
                        let (_, block_number, _) = tx_lookup[tx_gi];
                        let output_index = i16::try_from(output_index).map_err(|_| {
                            anyhow!(
                                "mNFT class output index exceeds i16 range: block={}, tx_hash=0x{}, output_index={}",
                                block_number,
                                hex::encode(all_tx_data[tx_gi].hash),
                                output_index
                            )
                        })?;
                        writer.insert_mnft_class_with_state(
                            class,
                            &all_tx_data[tx_gi].hash,
                            output_index,
                            block_number,
                            &mut batch,
                            &mut mnft_state,
                        )?;
                    }
                    for &(tx_gi, output_index, ref token) in &pre_parsed_nft_data.mnft_tokens {
                        let (tx_idx, block_number, ts_ms) = tx_lookup[tx_gi];
                        let output_index = i16::try_from(output_index).map_err(|_| {
                            anyhow!(
                                "mNFT token output index exceeds i16 range: block={}, tx_hash=0x{}, output_index={}",
                                block_number,
                                hex::encode(all_tx_data[tx_gi].hash),
                                output_index
                            )
                        })?;
                        writer.insert_mnft_token_with_state(
                            token,
                            &all_tx_data[tx_gi].hash,
                            output_index,
                            block_number,
                            ts_ms,
                            &mut batch,
                            &mut mnft_state,
                        )?;
                        nft_activity_acc.record(
                            &token.class_id,
                            &all_tx_data[tx_gi].hash,
                            &token.token_id,
                            block_number,
                            checked_usize_to_i32(tx_idx, "tx_idx"),
                            ts_ms,
                            true,
                        );
                    }

                    // Phase 1b: Insert DotBit accounts from pre-parsed data
                    // and collect per-tx activity data for direct writes.
                    let mut dotbit_pipeline_activity: HashMap<[u8; 32], DotbitTxActivityData> =
                        HashMap::new();
                    for &(tx_gi, ref account) in &pre_parsed_nft_data.dotbit_accounts {
                        let (tx_idx, block_number, ts_ms) = tx_lookup[tx_gi];
                        writer.insert_dotbit_account_with_state(
                            account,
                            &all_tx_data[tx_gi].hash,
                            block_number,
                            ts_ms,
                            &mut batch,
                            &mut dotbit_state,
                        )?;
                        let activity = dotbit_pipeline_activity
                            .entry(all_tx_data[tx_gi].hash)
                            .or_insert_with(|| DotbitTxActivityData {
                                das_action: pre_parsed_nft_data
                                    .dotbit_tx_actions
                                    .get(&tx_gi)
                                    .cloned(),
                                created_account_ids: HashSet::new(),
                                consumed_account_ids: HashSet::new(),
                                block_number,
                                tx_idx: checked_usize_to_i32(tx_idx, "tx_idx"),
                                timestamp_ms: ts_ms,
                            });
                        activity
                            .created_account_ids
                            .insert(account.account.account_id.clone());
                    }

                    // Phase 2: Consume DotBit accounts from pre-identified events
                    // (zero DB reads — all identification done in parser via
                    // input_cell_info type_code_hash + type_args).
                    for event in &pre_parsed_nft_data.consumed_dotbit {
                        if writer.consume_dotbit_account_with_state(
                            &event.account_id,
                            event.block_number,
                            &event.consuming_tx_hash,
                            &mut batch,
                            &mut dotbit_state,
                        )?.is_some() {
                            let activity = dotbit_pipeline_activity
                                .entry(event.consuming_tx_hash)
                                .or_insert_with(|| {
                                    // Find the tx_global_index for this consume event
                                    let tx_gi = tx_lookup.iter().position(|&(tx_idx, bn, ts)| {
                                        bn == event.block_number
                                            && checked_usize_to_i32(tx_idx, "tx_idx") == event.tx_idx
                                            && ts == event.ts_ms
                                    });
                                    let das_action = tx_gi.and_then(|gi| {
                                        pre_parsed_nft_data.dotbit_tx_actions.get(&gi).cloned()
                                    });
                                    DotbitTxActivityData {
                                        das_action,
                                        created_account_ids: HashSet::new(),
                                        consumed_account_ids: HashSet::new(),
                                        block_number: event.block_number,
                                        tx_idx: event.tx_idx,
                                        timestamp_ms: event.ts_ms,
                                    }
                                });
                            activity.consumed_account_ids.insert(event.account_id.clone());
                        }
                    }

                    // Write .bit collection activities directly (bypassing accumulator)
                    for (tx_hash, activity) in &dotbit_pipeline_activity {
                        let inserted = resolve_dotbit_tx_activity(
                            activity.das_action.as_deref(),
                            &activity.created_account_ids,
                            &activity.consumed_account_ids,
                            tx_hash,
                            activity.block_number,
                            activity.tx_idx,
                            activity.timestamp_ms,
                            &mut activity_batch,
                        );
                        if inserted {
                            let append_key = keys::encode_nft_collection_activity_key(
                                &DOTBIT_SENTINEL_COLLECTION,
                                activity.block_number,
                                activity.tx_idx,
                            );
                            put_append_delete_undo_entry(
                                &mut batch,
                                &mut append_undo_seq_by_block,
                                UndoSeqScope::AppendNftCollectionActivity,
                                activity.block_number,
                                ckbadger_store::CF_NFT_COLLECTION_ACTIVITIES,
                                &append_key,
                            );
                        }
                    }
                    for (collection_id, block_number, tx_idx) in
                        nft_activity_acc.flush(&mut activity_batch)
                    {
                        let append_key = keys::encode_nft_collection_activity_key(
                            &collection_id,
                            block_number,
                            tx_idx,
                        );
                        put_append_delete_undo_entry(
                            &mut batch,
                            &mut append_undo_seq_by_block,
                            UndoSeqScope::AppendNftCollectionActivity,
                            block_number,
                            ckbadger_store::CF_NFT_COLLECTION_ACTIVITIES,
                            &append_key,
                        );
                    }
                    let mut commit_ms =
                        commit_phase_no_wal("T6b_mnft_dotbit", first_block, last_block, batch)?;
                    if !activity_batch.is_empty() {
                        commit_ms += commit_phase_no_wal(
                            "T6b_nft_activity",
                            first_block,
                            last_block,
                            activity_batch,
                        )?;
                    }
                    Ok((t.elapsed().as_secs_f64() * 1000.0, commit_ms))
                });

                    // T7: Stats accumulation (overlaps with T1-T6 IO)
                    // Safe: reads CF_BLOCK_HEADERS which is NOT written by T1-T6.
                    // RocksDB supports concurrent reads. All other stats computation is
                    // purely CPU-bound on immutable all_parsed_blocks + all_tx_data.
                    let h7 = s.spawn(|| -> Result<(BatchStats, f64)> {
                    let t = Instant::now();
                    let mut stats = BatchStats::default();
                    let mut prev_timestamp: Option<chrono::DateTime<Utc>> =
                        if let Some(first_block) = all_parsed_blocks.first() {
                            writer.get_previous_block_timestamp(first_block.number)?
                        } else {
                            None
                        };
                    let mut prev_epoch: Option<(i64, chrono::DateTime<Utc>, f64)> =
                        if let Some(first_block) = all_parsed_blocks.first() {
                            writer
                                .get_last_epoch_start(first_block.number)?
                                .map(|(epoch_num, ts)| (epoch_num, ts, 0.0))
                        } else {
                            None
                        };
                    let mut prev_dao_cs: Option<(i128, i128)> =
                        if let Some(first_block) = all_parsed_blocks.first() {
                            if first_block.number > 0 {
                                writer
                                    .store()
                                    .get_block_header(first_block.number - 1)?
                                    .and_then(|h| extract_dao_csu(&h.dao).map(|(c, s, _)| (c, s)))
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                    let mut same_batch_dao_deposits: HashMap<(Vec<u8>, i16), i64> = HashMap::new();

                    let mut block_tx_idx = 0usize;
                    for parsed in all_parsed_blocks {
                        let block_date = ckbadger_common::block_date(parsed.timestamp);
                        accumulate_secondary_issuance_deltas(
                            &mut stats,
                            parsed,
                            block_date,
                            &mut prev_dao_cs,
                        )?;
                        let tx_count_for_block = parsed.transactions_count as usize;
                        let tx_slice =
                            &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                        block_tx_idx += tx_count_for_block;

                        // Exact DAO per-day deltas for snapshot accumulation in bulk mode.
                        accumulate_dao_snapshot_deltas_for_txs(
                            tx_slice,
                            block_date,
                            &dao_code_hash,
                            &consumed_dao_map,
                            &mut same_batch_dao_deposits,
                            &mut stats.dao_daily_active_delta,
                            &mut stats.dao_daily_gross_deposit_delta,
                            &mut stats.dao_daily_new_deposits_delta,
                            &mut stats.dao_daily_withdrawals_delta,
                        )?;

                        let cells_created: i32 =
                            tx_slice.iter().map(|tx| tx.cells.len() as i32).sum();
                        let cells_consumed: i32 = tx_slice
                            .iter()
                            .filter(|tx| !tx.is_cellbase)
                            .map(|tx| tx.inputs.len() as i32)
                            .sum();
                        let capacity_transferred: i128 = tx_slice
                            .iter()
                            .filter(|tx| !tx.is_cellbase)
                            .map(|tx| i128::from(tx.total_output_capacity))
                            .sum();
                        let data_size_added: i64 = tx_slice
                            .iter()
                            .flat_map(|tx| tx.cells.iter())
                            .map(|cell| cell.data_size as i64)
                            .sum();
                        let occupied_capacity_created: i128 = tx_slice
                            .iter()
                            .flat_map(|tx| tx.cells.iter())
                            .map(|cell| {
                                occupied_capacity_shannons_i128(
                                    cell.lock_args.len(),
                                    cell.type_args.as_ref().map(|args| args.len()),
                                    cell.data_size,
                                )
                            })
                            .sum();
                        let data_size_consumed: i64 = tx_slice
                            .iter()
                            .filter(|tx| !tx.is_cellbase)
                            .flat_map(|tx| tx.inputs.iter())
                            .filter_map(|input| {
                                let key = (
                                    input.previous_tx_hash.to_vec(),
                                    parsed_input_outpoint_index_i16(input.previous_output_index, "sync_indexer"),
                                );
                                input_cell_info
                                    .get(&key)
                                    .map(|info| info.data_size as i64)
                                    .or_else(|| {
                                        batch_cell_infos.get(&key).map(|info| info.data_size as i64)
                                    })
                            })
                            .sum();
                        let occupied_capacity_consumed: i128 = tx_slice
                            .iter()
                            .filter(|tx| !tx.is_cellbase)
                            .flat_map(|tx| tx.inputs.iter())
                            .filter_map(|input| {
                                let key = (
                                    input.previous_tx_hash.to_vec(),
                                    parsed_input_outpoint_index_i16(input.previous_output_index, "sync_indexer"),
                                );
                                input_cell_info
                                    .get(&key)
                                    .map(|info| i128::from(info.occupied_capacity))
                                    .or_else(|| {
                                        batch_cell_infos
                                            .get(&key)
                                            .map(|info| i128::from(info.occupied_capacity))
                                    })
                            })
                            .sum();

                        stats.sync_totals.0 += parsed.transactions_count as i64;
                        stats.sync_totals.1 += cells_created as i64;
                        stats.sync_totals.2 += cells_consumed as i64;
                        stats.last_block = Some((parsed.number, parsed.hash.clone()));

                        {
                            let entry = stats.daily_stats.entry(block_date).or_default();
                            entry.0 += 1;
                            entry.1 += parsed.transactions_count;
                            entry.2 += cells_created;
                            entry.3 += cells_consumed;
                            entry.4 = entry.4.checked_add(capacity_transferred).ok_or_else(|| {
                                anyhow!(
                                    "daily capacity_transferred overflow: date={} block={}",
                                    block_date,
                                    parsed.number
                                )
                            })?;
                            entry.5 = entry
                                .5
                                .checked_add(occupied_capacity_created)
                                .ok_or_else(|| {
                                    anyhow!(
                                        "daily occupied_capacity_created overflow: date={} block={}",
                                        block_date,
                                        parsed.number
                                    )
                                })?;
                            entry.6 = entry
                                .6
                                .checked_add(occupied_capacity_consumed)
                                .ok_or_else(|| {
                                    anyhow!(
                                        "daily occupied_capacity_consumed overflow: date={} block={}",
                                        block_date,
                                        parsed.number
                                    )
                                })?;
                            entry.7 += data_size_added;
                            entry.8 += data_size_consumed;
                        }
                        stats
                            .daily_dao_fields
                            .insert(block_date, parsed.dao.clone());
                        {
                            let block_hour = truncate_to_hour(parsed.timestamp);
                            let entry = stats.hourly_stats.entry(block_hour).or_default();
                            entry.0 += 1;
                            entry.1 += parsed.transactions_count;
                            entry.2 += cells_created;
                            entry.3 += cells_consumed;
                            entry.4 = entry.4.checked_add(capacity_transferred).ok_or_else(|| {
                                anyhow!(
                                    "hourly capacity_transferred overflow: hour={} block={}",
                                    block_hour,
                                    parsed.number
                                )
                            })?;
                        }
                        {
                            let entry = stats.daily_block_stats.entry(block_date).or_default();
                            entry.0 += parsed.compact_target as i128;
                            entry.1 += 1;
                            entry.2 += parsed.uncles_count;
                        }
                        if let Some(first_tx) = tx_slice.first() {
                            if first_tx.is_cellbase {
                                if let Some(first_cell) = first_tx.cells.first() {
                                    let key = (block_date, first_cell.lock_script_hash.clone());
                                    let entry = stats.miner_stats.entry(key).or_insert((0, 0));
                                    entry.0 += 1;
                                    entry.1 = parsed.number;
                                }
                            }
                        }
                        {
                            let entry = stats
                                .epoch_stats
                                .entry(parsed.epoch_number)
                                .or_insert_with(|| EpochAccum {
                                    start_block: parsed.number,
                                    end_block: parsed.number,
                                    length: parsed.epoch_length,
                                    start_ts: parsed.timestamp,
                                    end_ts: parsed.timestamp,
                                    tx_count: 0,
                                    is_new: parsed.epoch_index == 0,
                                });
                            entry.end_block = parsed.number;
                            entry.end_ts = parsed.timestamp;
                            entry.tx_count += parsed.transactions_count;
                        }

                        if let Some(prev_ts) = prev_timestamp {
                            let block_time_seconds = (parsed.timestamp - prev_ts).num_seconds();
                            if block_time_seconds >= 0 {
                                *stats
                                    .block_time_dist
                                    .entry(block_time_to_bucket(block_time_seconds))
                                    .or_default() += 1;
                                let block_time_ms = block_time_seconds * 1000;
                                let entry =
                                    stats.daily_block_times.entry(block_date).or_insert((0, 0));
                                entry.0 += block_time_ms;
                                entry.1 += 1;
                            }
                        }
                        prev_timestamp = Some(parsed.timestamp);

                        if parsed.epoch_index == 0 && parsed.epoch_number > 0 {
                            if let Some((prev_epoch_num, prev_start_ts, _)) = prev_epoch {
                                if prev_epoch_num == parsed.epoch_number - 1 {
                                    let epoch_duration_minutes =
                                        (parsed.timestamp - prev_start_ts).num_seconds() as f64
                                            / 60.0;
                                    let bucket_minutes = epoch_duration_minutes.round() as i32;
                                    *stats.epoch_time_dist.entry(bucket_minutes).or_default() += 1;
                                }
                            }
                        }
                        if parsed.epoch_index == 0 {
                            prev_epoch = Some((parsed.epoch_number, parsed.timestamp, 0.0));
                        }
                        stats.dao_snapshot_dates.insert(block_date);
                    }
                    stats.dao_deltas_computed = true;
                    Ok((stats, t.elapsed().as_secs_f64() * 1000.0))
                });

                    // T_ACT: Activity builder
                    // CFs: REORG_UNDO_LOG_BY_BLOCK, ACTIVITIES
                    let h_act = if !skip_activities {
                        Some(s.spawn(|| -> Result<(f64, f64)> {
                            let t = Instant::now();
                            let mut domain_batch = StoreBatch::new(store);
                            let mut append_batch = StoreBatch::new(append_only_store);
                            let mut append_undo_seq_by_block: HashMap<i64, u64> = HashMap::new();
                            let token_info_cache = load_activity_token_info_cache(
                                store,
                                &all_tx_data,
                                &input_cell_info,
                                &batch_cell_infos,
                            )?;
                            let mut block_tx_idx = 0usize;
                            for parsed in all_parsed_blocks {
                                let tx_count = parsed.transactions_count as usize;
                                let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count];
                                block_tx_idx += tx_count;

                                let tx_views: Vec<crate::db::writer::activities::TxView<'_>> =
                                    tx_slice
                                        .iter()
                                        .map(
                                            |td| -> Result<
                                                crate::db::writer::activities::TxView<'_>,
                                            > {
                                                let inputs = build_activity_input_views(
                                                    store,
                                                    td,
                                                    parsed.number,
                                                    &input_cell_info,
                                                    &batch_cell_infos,
                                                )?;
                                                Ok(crate::db::writer::activities::TxView {
                                                    tx_hash: &td.hash,
                                                    tx_index: td.tx_index,
                                                    block_number: parsed.number,
                                                    timestamp: parsed.timestamp.timestamp_millis(),
                                                    is_cellbase: td.is_cellbase,
                                                    inputs,
                                                    outputs: &td.cells,
                                                    outputs_data: &td.outputs_data,
                                                })
                                            },
                                        )
                                        .collect::<Result<Vec<_>>>()?;

                                let activities =
                                    crate::db::writer::activities::build_activities_for_block(
                                        &tx_views,
                                        &token_info_cache,
                                    );
                                for (lock_hash, entry) in activities {
                                    put_activity_with_undo_log(
                                        &mut domain_batch,
                                        &mut append_batch,
                                        &mut append_undo_seq_by_block,
                                        &lock_hash,
                                        entry.block_number,
                                        entry.tx_index,
                                        &entry,
                                    );
                                }
                            }
                            let mut commit_ms = 0.0;
                            if !domain_batch.is_empty() {
                                commit_ms += commit_phase_no_wal(
                                    "T_ACT_reorg_history",
                                    first_block,
                                    last_block,
                                    domain_batch,
                                )?;
                            }
                            if !append_batch.is_empty() {
                                commit_ms += commit_phase_no_wal(
                                    "T_ACT_activities",
                                    first_block,
                                    last_block,
                                    append_batch,
                                )?;
                            }
                            Ok((t.elapsed().as_secs_f64() * 1000.0, commit_ms))
                        }))
                    } else {
                        None
                    };

                    let (t1_ms, t1_commit_ms) = h1.join().expect("T1 panicked")?;
                    let (t2_ms, t2_commit_ms) = h2.join().expect("T2 panicked")?;
                    let (t4_ms, t4_commit_ms) = h4.join().expect("T4 panicked")?;
                    let (t5_ms, t5_commit_ms) = h5.join().expect("T5 panicked")?;
                    let (t6a_ms, t6a_commit_ms) = h6a.join().expect("T6a panicked")?;
                    let (t6b_ms, t6b_commit_ms) = h6b.join().expect("T6b panicked")?;
                    let (stats, t7_ms) = h7.join().expect("T7 panicked")?;
                    let (t_act_ms, t_act_commit_ms) = match h_act {
                        Some(h) => h.join().expect("T_ACT panicked")?,
                        None => (0.0, 0.0),
                    };
                    let commit_total_ms = t1_commit_ms
                        + t2_commit_ms
                        + t4_commit_ms
                        + t5_commit_ms
                        + t6a_commit_ms
                        + t6b_commit_ms
                        + t_act_commit_ms;
                    Ok((
                        stats,
                        [t1_ms, t2_ms, t4_ms, t5_ms, t6a_ms, t6b_ms, t7_ms, t_act_ms],
                        commit_total_ms,
                    ))
                },
            )?;
            thread_times = Some(tt);
        } else {
            // Live sync: serial writes in a single batch
            let mut data_batch = StoreBatch::new(self.writer.store());
            let mut append_undo_seq_by_block: HashMap<i64, u64> = HashMap::new();
            if !all_tx_data.is_empty() {
                put_tx_context_undo_entries(
                    &mut data_batch,
                    &mut append_undo_seq_by_block,
                    &all_tx_data,
                )?;
            }
            if !txs_for_batch.is_empty() {
                self.writer
                    .insert_transactions_batch(&txs_for_batch, &mut data_batch)?;
            }
            if !all_cells.is_empty() {
                self.writer
                    .insert_cells_batch(&all_cells, &mut data_batch, false)?;
            }
            if !all_consumptions.is_empty() {
                self.writer.consume_cells_batch_preloaded(
                    &all_consumptions,
                    &input_cell_info,
                    &batch_cell_infos,
                    &mut data_batch,
                    false,
                )?;
            }

            // Parallel DB reads for address balances and script usage
            let lock_hash_keys: Vec<&Vec<u8>> = if !skip_address_balances && !changes_ref.is_empty()
            {
                changes_ref.keys().collect()
            } else {
                vec![]
            };
            let unique_code_hashes: Vec<Vec<u8>> = if !script_usage_changes.is_empty() {
                let mut seen = std::collections::HashSet::new();
                script_usage_changes
                    .keys()
                    .filter_map(|(code_hash, _)| {
                        if seen.insert(code_hash.clone()) {
                            Some(code_hash.clone())
                        } else {
                            None
                        }
                    })
                    .collect()
            } else {
                vec![]
            };
            let code_hash_refs: Vec<&Vec<u8>> = unique_code_hashes.iter().collect();

            let need_balances = !lock_hash_keys.is_empty();
            let need_scripts = !code_hash_refs.is_empty();
            let mut domain_analytics_batch = StoreBatch::new(self.writer.store());
            let mut append_history_batch = StoreBatch::new(&self.append_only_store);

            if need_balances || need_scripts {
                let writer = &self.writer;
                let (existing_balances, existing_scripts) = std::thread::scope(|s| {
                    let bal = if need_balances {
                        Some(s.spawn(|| writer.read_address_balances(&lock_hash_keys)))
                    } else {
                        None
                    };
                    let scr = if need_scripts {
                        Some(s.spawn(|| writer.read_script_info(&code_hash_refs)))
                    } else {
                        None
                    };
                    (
                        bal.map(|h| h.join().unwrap()),
                        scr.map(|h| h.join().unwrap()),
                    )
                });
                if let Some(existing) = existing_balances {
                    let existing = existing?;
                    batch_new_addresses = count_new_addresses(&changes_ref, &existing);
                    self.writer.apply_address_balance_deltas(
                        &existing,
                        &changes_ref,
                        &mut data_batch,
                    )?;
                }
                if let Some(existing) = existing_scripts {
                    self.writer.apply_script_usage_deltas(
                        &existing?,
                        &script_usage_changes,
                        &mut domain_analytics_batch,
                    )?;
                }
            }
            if !script_daily_changes.is_empty() {
                self.writer.update_script_daily_deltas_batch(
                    &script_daily_changes,
                    &mut domain_analytics_batch,
                )?;
            }
            if !token_daily_changes.is_empty() {
                self.writer.update_token_daily_deltas_batch(
                    &token_daily_changes,
                    &mut domain_analytics_batch,
                )?;
            }
            if !spore_type_index_changes.is_empty() {
                self.writer
                    .update_spore_type_index_batch(&spore_type_index_changes, &mut data_batch)?;
            }
            if !spore_daily_changes.is_empty() {
                self.writer.update_spore_daily_deltas_batch(
                    &spore_daily_changes,
                    &mut domain_analytics_batch,
                )?;
            }
            if !nft_type_index_changes.is_empty() {
                self.writer
                    .update_nft_type_index_batch(&nft_type_index_changes, &mut data_batch)?;
            }
            if !nft_daily_changes.is_empty() {
                self.writer.update_nft_daily_deltas_batch(
                    &nft_daily_changes,
                    &mut domain_analytics_batch,
                )?;
            }
            if !cluster_daily_changes.is_empty() {
                self.writer.update_cluster_daily_deltas_batch(
                    &cluster_daily_changes,
                    &mut domain_analytics_batch,
                )?;
            }

            // Write addr_txs entries
            for (lock_hash, block_num, tx_idx, tx_hash) in &addr_tx_entries {
                put_addr_tx_with_undo_log(
                    &mut data_batch,
                    &mut append_history_batch,
                    &mut append_undo_seq_by_block,
                    lock_hash,
                    *block_num,
                    *tx_idx,
                    tx_hash,
                );
            }

            // Group A: DAO processing
            {
                let mut all_dao_deposits: Vec<(
                    crate::parser::ParsedDaoDeposit,
                    i64,
                    DateTime<Utc>,
                    i64,
                )> = Vec::new();
                let mut block_tx_idx = 0usize;
                for parsed in all_parsed_blocks {
                    let tx_count_for_block = parsed.transactions_count as usize;
                    let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                    block_tx_idx += tx_count_for_block;
                    let ar = extract_ar_i64_from_dao(&parsed.dao, parsed.number)?;
                    for tx_data in tx_slice {
                        let dao_deposits =
                            DaoParser::parse_deposits_from_cells(&tx_data.hash, &tx_data.cells);
                        for deposit in dao_deposits {
                            all_dao_deposits.push((deposit, parsed.number, parsed.timestamp, ar));
                        }
                    }
                }
                if !all_dao_deposits.is_empty() {
                    self.writer
                        .insert_dao_deposits_batch(&all_dao_deposits, &mut data_batch)?;
                }

                // Batch query consumed DAO deposits
                let mut all_input_outpoints_dao: Vec<(Vec<u8>, i16)> = Vec::new();
                let mut block_tx_idx = 0usize;
                for parsed in all_parsed_blocks {
                    let tx_count_for_block = parsed.transactions_count as usize;
                    let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                    block_tx_idx += tx_count_for_block;
                    for tx_data in tx_slice {
                        if tx_data.is_cellbase || tx_data.inputs.is_empty() {
                            continue;
                        }
                        for input in &tx_data.inputs {
                            all_input_outpoints_dao.push((
                                input.previous_tx_hash.to_vec(),
                                parsed_input_outpoint_index_i16(
                                    input.previous_output_index,
                                    "sync_indexer",
                                ),
                            ));
                        }
                    }
                }

                let consumed_dao_map = if !all_input_outpoints_dao.is_empty() {
                    let unique_outpoints: Vec<(Vec<u8>, i16)> = {
                        let mut seen = HashSet::new();
                        all_input_outpoints_dao
                            .into_iter()
                            .filter(|x| seen.insert(x.clone()))
                            .collect()
                    };
                    let outpoint_refs: Vec<(&[u8], i16)> = unique_outpoints
                        .iter()
                        .map(|(h, i)| (h.as_slice(), *i))
                        .collect();
                    self.writer
                        .find_consumed_dao_deposits_batch(&outpoint_refs)?
                } else {
                    HashMap::new()
                };

                // Build a same-batch deposit map for deposits created in this
                // batch that may also be consumed within the same batch.
                let mut same_batch_dao_deposits: HashMap<
                    (Vec<u8>, i16),
                    (i64, Vec<u8>, i16, String, i64, i16),
                > = HashMap::new();
                // Also build pending entries map for process_dao_withdrawals_batch
                let mut pending_dao_entries: HashMap<
                    [u8; 34],
                    ckbadger_store::types::DaoDepositCacheEntry,
                > = HashMap::new();
                for (deposit, block_number, _ts, ar) in &all_dao_deposits {
                    let deposit_output_index = checked_i32_to_i16(
                        deposit.output_index,
                        "DAO deposit output index while building same-batch map",
                    )
                    .map_err(|e| {
                        anyhow!(
                            "{}: deposit_tx_hash=0x{}, block={}",
                            e,
                            hex::encode(&deposit.tx_hash),
                            block_number
                        )
                    })?;
                    same_batch_dao_deposits.insert(
                        (deposit.tx_hash.clone(), deposit_output_index),
                        (
                            0,
                            deposit.tx_hash.clone(),
                            deposit_output_index,
                            deposit.capacity.to_string(),
                            *block_number,
                            0i16, // status = 0 (active)
                        ),
                    );
                    let outpoint_key = ckbadger_store::keys::encode_outpoint(
                        &deposit.tx_hash,
                        deposit_output_index,
                    );
                    pending_dao_entries.insert(
                        outpoint_key,
                        ckbadger_store::types::DaoDepositCacheEntry {
                            capacity: deposit.capacity,
                            deposit_block_number: *block_number,
                            lock_script_hash: deposit.lock_script_hash.clone(),
                            deposit_ar: *ar,
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
                }

                if !consumed_dao_map.is_empty() || !same_batch_dao_deposits.is_empty() {
                    let mut withdrawal_contexts: Vec<DaoWithdrawalContext> = Vec::new();
                    let mut block_tx_idx = 0usize;
                    for parsed in all_parsed_blocks {
                        let tx_count_for_block = parsed.transactions_count as usize;
                        let tx_slice =
                            &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                        block_tx_idx += tx_count_for_block;
                        for tx_data in tx_slice {
                            if tx_data.is_cellbase || tx_data.inputs.is_empty() {
                                continue;
                            }
                            let mut consumed_deposits: Vec<(i64, Vec<u8>, i16, String, i64, i16)> =
                                Vec::new();
                            for input in &tx_data.inputs {
                                let key = (
                                    input.previous_tx_hash.to_vec(),
                                    parsed_input_outpoint_index_i16(
                                        input.previous_output_index,
                                        "sync_indexer",
                                    ),
                                );
                                if let Some(deposit_info) = consumed_dao_map
                                    .get(&key)
                                    .or_else(|| same_batch_dao_deposits.get(&key))
                                {
                                    consumed_deposits.push(deposit_info.clone());
                                }
                            }
                            if consumed_deposits.is_empty() {
                                continue;
                            }
                            let tx_inputs: Vec<(Vec<u8>, i16)> = tx_data
                                .inputs
                                .iter()
                                .map(|input| {
                                    let output_index =
                                        i16::try_from(input.previous_output_index).map_err(|_| {
                                            anyhow!(
                                                "DAO processing input index exceeds i16 range: tx_hash=0x{}, previous_output_index={}",
                                                hex::encode(tx_data.hash),
                                                input.previous_output_index
                                            )
                                        })?;
                                    Ok((input.previous_tx_hash.to_vec(), output_index))
                                })
                                .collect::<Result<_>>()?;
                            let mut new_dao_outputs: Vec<(Vec<u8>, i16, Vec<u8>, i64, u64)> =
                                Vec::new();
                            let mut candidate_withdraw_to_outputs: Vec<(i16, Vec<u8>)> = Vec::new();
                            for (idx, cell) in tx_data.cells.iter().enumerate() {
                                let output_index = checked_usize_to_i16(
                                    idx,
                                    "DAO output index while processing grouped withdrawals",
                                )
                                .map_err(|e| {
                                    anyhow!(
                                        "{}: tx_hash=0x{}, block={}",
                                        e,
                                        hex::encode(tx_data.hash),
                                        parsed.number
                                    )
                                })?;
                                if let Some(ref type_code_hash) = cell.type_code_hash {
                                    if type_code_hash == &dao_code_hash && cell.data_size == 8 {
                                        if let Some(data) = tx_data.outputs_data.get(idx) {
                                            let data_bytes = crate::rpc::parse_hex_to_bytes(data);
                                            if let Some(deposit_block) =
                                                DaoParser::parse_deposit_block_number(&data_bytes)
                                            {
                                                new_dao_outputs.push((
                                                    tx_data.hash.to_vec(),
                                                    output_index,
                                                    cell.lock_script_hash.clone(),
                                                    cell.capacity,
                                                    deposit_block,
                                                ));
                                            }
                                        }
                                    } else {
                                        candidate_withdraw_to_outputs
                                            .push((output_index, cell.lock_script_hash.clone()));
                                    }
                                } else {
                                    candidate_withdraw_to_outputs
                                        .push((output_index, cell.lock_script_hash.clone()));
                                }
                            }
                            withdrawal_contexts.push(DaoWithdrawalContext {
                                consumed_deposits,
                                new_dao_outputs,
                                tx_inputs,
                                candidate_withdraw_to_outputs,
                                block_number: parsed.number,
                                consuming_tx_hash: tx_data.hash.to_vec(),
                                timestamp: parsed.timestamp,
                            });
                        }
                    }
                    if !withdrawal_contexts.is_empty() {
                        self.writer.process_dao_withdrawals_batch(
                            &withdrawal_contexts,
                            &mut data_batch,
                            &pending_dao_entries,
                        )?;
                    }
                }
            }

            // Group B: UDT processing
            {
                struct UdtTxContext {
                    tx_hash: Vec<u8>,
                    block_number: i64,
                    #[allow(dead_code)]
                    timestamp: chrono::DateTime<Utc>,
                    output_udts: Vec<crate::parser::ParsedUdtCell>,
                    input_outpoints: Vec<(Vec<u8>, i16)>,
                }
                let mut udt_tx_contexts: Vec<UdtTxContext> = Vec::new();
                let mut all_input_outpoints_udt: Vec<(Vec<u8>, i16)> = Vec::new();
                let mut batch_udt_cells: HashMap<(Vec<u8>, i16), crate::parser::ParsedUdtCell> =
                    HashMap::new();
                struct TxInfoForUdt {
                    tx_hash: Vec<u8>,
                    block_number: i64,
                    timestamp: chrono::DateTime<Utc>,
                    output_udts: Vec<crate::parser::ParsedUdtCell>,
                    input_outpoints: Vec<(Vec<u8>, i16)>,
                }
                let mut all_tx_infos_for_udt: Vec<TxInfoForUdt> = Vec::new();

                let mut block_tx_idx = 0usize;
                for (block_idx, block_response) in blocks.iter().enumerate() {
                    let parsed = &all_parsed_blocks[block_idx];
                    let tx_count_for_block = parsed.transactions_count as usize;
                    let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                    block_tx_idx += tx_count_for_block;
                    for (tx_idx, tx_data) in tx_slice.iter().enumerate() {
                        if tx_data.is_cellbase {
                            continue;
                        }
                        let tx = &block_response.block.transactions[tx_idx];
                        let mut output_udts: Vec<crate::parser::ParsedUdtCell> = Vec::new();
                        for (output_index, udt_cell) in
                            self.parse_udt_cells_with_store_fallback(tx)?
                        {
                            batch_udt_cells
                                .insert((tx_data.hash.to_vec(), output_index), udt_cell.clone());
                            self.udt_cell_cache.insert(
                                (tx_data.hash, output_index),
                                CachedUdtCellInfo {
                                    type_script_hash: udt_cell.type_script_hash.clone(),
                                    type_code_hash: udt_cell.type_code_hash.clone(),
                                    type_hash_type: udt_cell.type_hash_type,
                                    type_args: udt_cell.type_args.clone(),
                                    lock_script_hash: udt_cell.lock_script_hash.clone(),
                                    amount: udt_cell.amount,
                                    standard: udt_cell.standard.as_str().to_string(),
                                },
                            );
                            output_udts.push(udt_cell);
                        }
                        let input_outpoints: Vec<(Vec<u8>, i16)> = tx_data
                            .inputs
                            .iter()
                            .map(|i| {
                                let previous_output_index =
                                    i16::try_from(i.previous_output_index).map_err(|_| {
                                        anyhow!(
                                            "UDT input previous_output_index exceeds i16 range: tx_hash=0x{}, block={}, previous_output_index={}",
                                            hex::encode(tx_data.hash),
                                            parsed.number,
                                            i.previous_output_index
                                        )
                                    })?;
                                Ok((i.previous_tx_hash.to_vec(), previous_output_index))
                            })
                            .collect::<Result<_>>()?;
                        all_input_outpoints_udt.extend(input_outpoints.iter().cloned());
                        all_tx_infos_for_udt.push(TxInfoForUdt {
                            tx_hash: tx_data.hash.to_vec(),
                            block_number: parsed.number,
                            timestamp: parsed.timestamp,
                            output_udts,
                            input_outpoints,
                        });
                    }
                }

                let mut input_udt_info: HashMap<
                    (Vec<u8>, i16),
                    (Vec<u8>, Vec<u8>, i16, Vec<u8>, Vec<u8>, u128, String),
                > = HashMap::new();
                if !skip_token && !all_input_outpoints_udt.is_empty() {
                    input_udt_info = resolve_input_udt_info_from_live_cells(
                        &self.writer,
                        &self.udt_cell_cache,
                        &all_input_outpoints_udt,
                    )?;
                }

                for tx_info in all_tx_infos_for_udt {
                    let has_udt_outputs = !tx_info.output_udts.is_empty();
                    let has_udt_inputs = tx_info.input_outpoints.iter().any(|(tx_hash, idx)| {
                        input_udt_info.contains_key(&(tx_hash.clone(), *idx))
                            || batch_udt_cells.contains_key(&(tx_hash.clone(), *idx))
                    });
                    if has_udt_outputs || has_udt_inputs {
                        udt_tx_contexts.push(UdtTxContext {
                            tx_hash: tx_info.tx_hash,
                            block_number: tx_info.block_number,
                            timestamp: tx_info.timestamp,
                            output_udts: tx_info.output_udts,
                            input_outpoints: tx_info.input_outpoints,
                        });
                    }
                }

                if !skip_token && !udt_tx_contexts.is_empty() {
                    let max_supply_observations =
                        collect_token_max_supply_observations(&all_tx_data);
                    let mut all_transfers: Vec<(crate::parser::ParsedUdtTransfer, Vec<u8>, i64)> =
                        Vec::new();
                    for ctx in &udt_tx_contexts {
                        let mut input_udts: Vec<crate::parser::ParsedUdtCell> = Vec::new();
                        for (tx_hash, idx) in &ctx.input_outpoints {
                            if let Some((tsh, tch, tht, ta, lsh, am, std)) =
                                input_udt_info.get(&(tx_hash.clone(), *idx))
                            {
                                input_udts.push(crate::parser::ParsedUdtCell {
                                    type_script_hash: tsh.clone(),
                                    type_code_hash: tch.clone(),
                                    type_hash_type: *tht,
                                    type_args: ta.clone(),
                                    lock_script_hash: lsh.clone(),
                                    amount: *am,
                                    standard: crate::parser::UdtStandard::parse(std),
                                });
                            } else if let Some(udt_cell) =
                                batch_udt_cells.get(&(tx_hash.clone(), *idx))
                            {
                                input_udts.push(udt_cell.clone());
                            }
                        }
                        for transfer in crate::parser::UdtParser::build_transfers_from_cells(
                            &input_udts,
                            &ctx.output_udts,
                        ) {
                            all_transfers.push((transfer, ctx.tx_hash.clone(), ctx.block_number));
                        }
                    }

                    if !all_transfers.is_empty() {
                        let transfer_refs: Vec<_> = all_transfers
                            .iter()
                            .map(|(t, h, b)| (t, h.as_slice(), *b))
                            .collect();
                        let block_timestamps: HashMap<i64, i64> = all_parsed_blocks
                            .iter()
                            .map(|p| (p.number, p.timestamp.timestamp_millis()))
                            .collect();
                        let mut udt_state = self.writer.new_udt_batch_state();
                        self.writer.process_udt_transfers_batch_with_state(
                            &transfer_refs,
                            &max_supply_observations,
                            &block_timestamps,
                            &mut data_batch,
                            &mut udt_state,
                        )?;
                    }
                }
            }

            let mut nft_activity_batch = StoreBatch::new(&self.append_only_store);

            // Group C: NFT/Spore processing
            {
                let mut batch_spore_ids: HashSet<Vec<u8>> = HashSet::new();
                let mut batch_mnft_token_outpoints: HashMap<(Vec<u8>, i16), Vec<u8>> =
                    HashMap::new();
                let mut batch_dotbit_outpoints: HashMap<(Vec<u8>, i16), Vec<u8>> = HashMap::new();
                let mut batch_dotbit_latest_create_order: HashMap<Vec<u8>, u64> = HashMap::new();
                let mut spore_state = self.writer.new_spore_batch_state();
                let mut dotbit_state = self.writer.new_dotbit_batch_state();
                let mut mnft_state = self.writer.new_mnft_batch_state();
                let mut nft_activity_acc = NftCollectionActivityAccumulator::new();
                let mut dotbit_tx_activity_data: HashMap<[u8; 32], DotbitTxActivityData> =
                    HashMap::new();
                let mut block_tx_idx = 0usize;
                for (block_idx, block_response) in blocks.iter().enumerate() {
                    let parsed = &all_parsed_blocks[block_idx];
                    let tx_count_for_block = parsed.transactions_count as usize;
                    let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                    let ts_ms = parsed.timestamp.timestamp_millis();
                    for (tx_idx, tx_data) in tx_slice.iter().enumerate() {
                        let tx_global_index = block_tx_idx + tx_idx;
                        let dotbit_create_order = dotbit_create_event_order(tx_global_index)?;
                        let tx = &block_response.block.transactions[tx_idx];
                        if !skip_spore {
                            for cluster in SporeParser::parse_clusters(tx) {
                                self.writer.insert_spore_cluster(
                                    &cluster,
                                    parsed.number,
                                    &tx_data.hash,
                                    &mut data_batch,
                                    &mut spore_state,
                                )?;
                            }
                            for (output_index, spore) in
                                SporeParser::parse_spores(tx).iter().enumerate()
                            {
                                let output_index_i16 = checked_usize_to_i16(
                                    output_index,
                                    "spore output index while processing grouped blocks",
                                )
                                .map_err(|e| {
                                    anyhow!(
                                        "{}: block={}, tx_hash=0x{}",
                                        e,
                                        parsed.number,
                                        hex::encode(tx_data.hash)
                                    )
                                })?;
                                batch_spore_ids.insert(spore.spore_id.clone());
                                self.writer.insert_spore_cell(
                                    spore,
                                    &tx_data.hash,
                                    output_index_i16,
                                    parsed.number,
                                    ts_ms,
                                    &mut data_batch,
                                    &mut spore_state,
                                )?;
                                let coll_id = if spore.is_did {
                                    &DID_CKB_SENTINEL_COLLECTION[..]
                                } else if let Some(ref cid) = spore.cluster_id {
                                    cid.as_slice()
                                } else {
                                    continue;
                                };
                                nft_activity_acc.record(
                                    coll_id,
                                    &tx_data.hash,
                                    &spore.spore_id,
                                    parsed.number,
                                    checked_usize_to_i32(tx_idx, "tx_idx"),
                                    ts_ms,
                                    true,
                                );
                            }
                        }
                        for issuer in MnftParser::parse_issuers(tx) {
                            self.writer.insert_mnft_issuer(
                                &issuer,
                                &tx_data.hash,
                                0,
                                parsed.number,
                                &mut data_batch,
                            )?;
                        }
                        for (output_index, class) in
                            MnftParser::parse_classes_with_output_indices(tx)
                        {
                            let output_index = i16::try_from(output_index).map_err(|_| {
                                anyhow!(
                                    "mNFT class output index exceeds i16 range: block={}, tx_hash=0x{}, output_index={}",
                                    parsed.number,
                                    hex::encode(tx_data.hash),
                                    output_index
                                )
                            })?;
                            self.writer.insert_mnft_class_with_state(
                                &class,
                                &tx_data.hash,
                                output_index,
                                parsed.number,
                                &mut data_batch,
                                &mut mnft_state,
                            )?;
                        }
                        for (output_index, token) in
                            MnftParser::parse_tokens_with_output_indices(tx)
                        {
                            let output_index = i16::try_from(output_index).map_err(|_| {
                                anyhow!(
                                    "mNFT token output index exceeds i16 range: block={}, tx_hash=0x{}, output_index={}",
                                    parsed.number,
                                    hex::encode(tx_data.hash),
                                    output_index
                                )
                            })?;
                            self.writer.insert_mnft_token_with_state(
                                &token,
                                &tx_data.hash,
                                output_index,
                                parsed.number,
                                ts_ms,
                                &mut data_batch,
                                &mut mnft_state,
                            )?;
                            batch_mnft_token_outpoints.insert(
                                (tx_data.hash.to_vec(), output_index),
                                token.token_id.clone(),
                            );
                            nft_activity_acc.record(
                                &token.class_id,
                                &tx_data.hash,
                                &token.token_id,
                                parsed.number,
                                checked_usize_to_i32(tx_idx, "tx_idx"),
                                ts_ms,
                                true,
                            );
                        }
                        let dotbit_accounts = DotbitParser::parse_accounts(tx)?;
                        if !dotbit_accounts.is_empty() {
                            let das_action = DotbitParser::parse_das_action(&tx.witnesses);
                            let mut created_ids = HashSet::new();
                            for account in &dotbit_accounts {
                                self.writer.insert_dotbit_account_with_state(
                                    account,
                                    &tx_data.hash,
                                    parsed.number,
                                    ts_ms,
                                    &mut data_batch,
                                    &mut dotbit_state,
                                )?;
                                batch_dotbit_outpoints.insert(
                                    (tx_data.hash.to_vec(), account.output_index),
                                    account.account.account_id.clone(),
                                );
                                let account_id = account.account.account_id.clone();
                                batch_dotbit_latest_create_order
                                    .entry(account_id.clone())
                                    .and_modify(|current| {
                                        if dotbit_create_order > *current {
                                            *current = dotbit_create_order;
                                        }
                                    })
                                    .or_insert(dotbit_create_order);
                                created_ids.insert(account_id);
                            }
                            dotbit_tx_activity_data.insert(
                                tx_data.hash,
                                DotbitTxActivityData {
                                    das_action,
                                    created_account_ids: created_ids,
                                    consumed_account_ids: HashSet::new(),
                                    block_number: parsed.number,
                                    tx_idx: checked_usize_to_i32(tx_idx, "tx_idx"),
                                    timestamp_ms: ts_ms,
                                },
                            );
                        }
                    }
                    block_tx_idx += tx_count_for_block;
                }

                // Spore/mNFT consumption runs in live sync mode only, DotBit consumption runs in all sync modes.
                let bulk_sync_active = self.is_bulk_sync_active();
                let mut all_prev_tx_hashes: Vec<Vec<u8>> = Vec::new();
                let mut all_prev_indices: Vec<i16> = Vec::new();
                // (block_number, consuming_tx_hash, dotbit_consume_order, tx_idx, ts_ms)
                let mut outpoint_context: Vec<(i64, Vec<u8>, u64, i32, i64)> = Vec::new();
                let mut block_tx_idx = 0usize;
                for (block_idx, block_response) in blocks.iter().enumerate() {
                    let parsed = &all_parsed_blocks[block_idx];
                    let tx_count_for_block = parsed.transactions_count as usize;
                    let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                    for (tx_idx, tx_data) in tx_slice.iter().enumerate() {
                        if tx_data.is_cellbase || tx_data.inputs.is_empty() {
                            continue;
                        }
                        let tx_global_index = block_tx_idx + tx_idx;
                        let dotbit_consume_order = dotbit_consume_event_order(tx_global_index)?;
                        let tx = &block_response.block.transactions[tx_idx];
                        for input in &tx.inputs {
                            let prev_tx_hash =
                                crate::rpc::parse_hex_to_bytes(&input.previous_output.tx_hash);
                            let prev_index = parse_outpoint_index_i16(
                                &input.previous_output.index,
                                "input.previous_output.index",
                            )
                            .map_err(|e| {
                                anyhow!(
                                    "invalid input index while prefetching outpoints at block {}, tx 0x{}: {}",
                                    parsed.number,
                                    hex::encode(tx_data.hash),
                                    e
                                )
                            })?;
                            all_prev_tx_hashes.push(prev_tx_hash);
                            all_prev_indices.push(prev_index);
                            outpoint_context.push((
                                parsed.number,
                                tx_data.hash.to_vec(),
                                dotbit_consume_order,
                                checked_usize_to_i32(tx_idx, "tx_idx"),
                                parsed.timestamp.timestamp_millis(),
                            ));
                        }
                    }
                    block_tx_idx += tx_count_for_block;
                }
                if !all_prev_tx_hashes.is_empty() {
                    let dotbit_results = self.writer.get_dotbit_account_ids_by_outpoints_batch(
                        &all_prev_tx_hashes,
                        &all_prev_indices,
                    )?;
                    let spore_results = if bulk_sync_active {
                        Vec::new()
                    } else {
                        self.writer.get_spore_ids_by_outpoints_batch(
                            &all_prev_tx_hashes,
                            &all_prev_indices,
                        )?
                    };
                    let mnft_results = if bulk_sync_active {
                        Vec::new()
                    } else {
                        self.writer.get_mnft_token_ids_by_outpoints_batch(
                            &all_prev_tx_hashes,
                            &all_prev_indices,
                        )?
                    };

                    let spore_map: HashMap<(Vec<u8>, i16), Vec<u8>> = spore_results
                        .into_iter()
                        .map(|(h, i, id)| ((h, i), id))
                        .collect();
                    let mut spore_map = spore_map;
                    if !bulk_sync_active {
                        for (idx, tx_hash) in all_prev_tx_hashes.iter().enumerate() {
                            let key = (tx_hash.clone(), all_prev_indices[idx]);
                            if spore_map.contains_key(&key) {
                                continue;
                            }
                            if let Some(spore_id) = spore_state
                                .get_cached_spore_id_by_outpoint(tx_hash, all_prev_indices[idx])
                            {
                                spore_map.insert(key, spore_id);
                            }
                        }
                    }

                    let mnft_map: HashMap<(Vec<u8>, i16), Vec<u8>> = mnft_results
                        .into_iter()
                        .map(|(h, i, id)| ((h, i), id))
                        .collect();
                    let mut mnft_map = mnft_map;
                    if !bulk_sync_active {
                        for (key, token_id) in &batch_mnft_token_outpoints {
                            mnft_map
                                .entry(key.clone())
                                .or_insert_with(|| token_id.clone());
                        }
                    }

                    let dotbit_map: HashMap<(Vec<u8>, i16), Vec<u8>> = dotbit_results
                        .into_iter()
                        .map(|(h, i, id)| ((h, i), id))
                        .collect();
                    let mut dotbit_map = dotbit_map;
                    for (key, account_id) in &batch_dotbit_outpoints {
                        dotbit_map
                            .entry(key.clone())
                            .or_insert_with(|| account_id.clone());
                    }

                    for (
                        i,
                        (
                            block_number,
                            consuming_tx_hash,
                            dotbit_consume_order,
                            ctx_tx_idx,
                            ctx_ts_ms,
                        ),
                    ) in outpoint_context.iter().enumerate()
                    {
                        let key = (all_prev_tx_hashes[i].clone(), all_prev_indices[i]);
                        if !bulk_sync_active {
                            if let Some(spore_id) = spore_map.get(&key) {
                                if !batch_spore_ids.contains(spore_id) {
                                    if let Some(coll_id) = self.writer.consume_spore(
                                        spore_id,
                                        *block_number,
                                        consuming_tx_hash,
                                        &mut data_batch,
                                        &mut spore_state,
                                    )? {
                                        nft_activity_acc.record(
                                            &coll_id,
                                            consuming_tx_hash,
                                            spore_id,
                                            *block_number,
                                            *ctx_tx_idx,
                                            *ctx_ts_ms,
                                            false,
                                        );
                                    }
                                }
                            }
                            if let Some(token_id) = mnft_map.get(&key) {
                                if let Some(coll_id) = self.writer.consume_mnft_token_with_state(
                                    token_id,
                                    *block_number,
                                    consuming_tx_hash,
                                    &mut data_batch,
                                    &mut mnft_state,
                                )? {
                                    nft_activity_acc.record(
                                        &coll_id,
                                        consuming_tx_hash,
                                        token_id,
                                        *block_number,
                                        *ctx_tx_idx,
                                        *ctx_ts_ms,
                                        false,
                                    );
                                }
                            }
                        }
                        if let Some(account_id) = dotbit_map.get(&key) {
                            let latest_create_order =
                                batch_dotbit_latest_create_order.get(account_id).copied();
                            if should_consume_dotbit_account(
                                latest_create_order,
                                *dotbit_consume_order,
                            ) && self
                                .writer
                                .consume_dotbit_account_with_state(
                                    account_id,
                                    *block_number,
                                    consuming_tx_hash,
                                    &mut data_batch,
                                    &mut dotbit_state,
                                )?
                                .is_some()
                            {
                                let tx_key: [u8; 32] = consuming_tx_hash
                                    .as_slice()
                                    .try_into()
                                    .expect("consuming_tx_hash must be 32 bytes");
                                let activity = dotbit_tx_activity_data
                                    .entry(tx_key)
                                    .or_insert_with(|| DotbitTxActivityData {
                                        das_action: None,
                                        created_account_ids: HashSet::new(),
                                        consumed_account_ids: HashSet::new(),
                                        block_number: *block_number,
                                        tx_idx: *ctx_tx_idx,
                                        timestamp_ms: *ctx_ts_ms,
                                    });
                                activity.consumed_account_ids.insert(account_id.clone());
                            }
                        }
                    }
                }
                // Write .bit collection activities directly (bypassing accumulator)
                for (tx_hash, activity) in &dotbit_tx_activity_data {
                    let inserted = resolve_dotbit_tx_activity(
                        activity.das_action.as_deref(),
                        &activity.created_account_ids,
                        &activity.consumed_account_ids,
                        tx_hash,
                        activity.block_number,
                        activity.tx_idx,
                        activity.timestamp_ms,
                        &mut nft_activity_batch,
                    );
                    if inserted {
                        let append_key = keys::encode_nft_collection_activity_key(
                            &DOTBIT_SENTINEL_COLLECTION,
                            activity.block_number,
                            activity.tx_idx,
                        );
                        put_append_delete_undo_entry(
                            &mut data_batch,
                            &mut append_undo_seq_by_block,
                            UndoSeqScope::AppendNftCollectionActivity,
                            activity.block_number,
                            ckbadger_store::CF_NFT_COLLECTION_ACTIVITIES,
                            &append_key,
                        );
                    }
                }
                for (collection_id, block_number, tx_idx) in
                    nft_activity_acc.flush(&mut nft_activity_batch)
                {
                    let append_key = keys::encode_nft_collection_activity_key(
                        &collection_id,
                        block_number,
                        tx_idx,
                    );
                    put_append_delete_undo_entry(
                        &mut data_batch,
                        &mut append_undo_seq_by_block,
                        UndoSeqScope::AppendNftCollectionActivity,
                        block_number,
                        ckbadger_store::CF_NFT_COLLECTION_ACTIVITIES,
                        &append_key,
                    );
                }
            }

            // Activity writes (live sync)
            let mut activity_batch = StoreBatch::new(&self.append_only_store);
            {
                let token_info_cache = load_activity_token_info_cache(
                    self.writer.store(),
                    &all_tx_data,
                    &input_cell_info,
                    &batch_cell_infos,
                )?;
                let mut block_tx_idx = 0usize;
                for parsed in all_parsed_blocks {
                    let tx_count = parsed.transactions_count as usize;
                    let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count];
                    block_tx_idx += tx_count;

                    let tx_views: Vec<crate::db::writer::activities::TxView<'_>> = tx_slice
                        .iter()
                        .map(|td| -> Result<crate::db::writer::activities::TxView<'_>> {
                            let inputs = build_activity_input_views(
                                self.writer.store(),
                                td,
                                parsed.number,
                                &input_cell_info,
                                &batch_cell_infos,
                            )?;
                            Ok(crate::db::writer::activities::TxView {
                                tx_hash: &td.hash,
                                tx_index: td.tx_index,
                                block_number: parsed.number,
                                timestamp: parsed.timestamp.timestamp_millis(),
                                is_cellbase: td.is_cellbase,
                                inputs,
                                outputs: &td.cells,
                                outputs_data: &td.outputs_data,
                            })
                        })
                        .collect::<Result<Vec<_>>>()?;

                    let activities = crate::db::writer::activities::build_activities_for_block(
                        &tx_views,
                        &token_info_cache,
                    );
                    for (lock_hash, entry) in activities {
                        put_activity_with_undo_log(
                            &mut data_batch,
                            &mut activity_batch,
                            &mut append_undo_seq_by_block,
                            &lock_hash,
                            entry.block_number,
                            entry.tx_index,
                            &entry,
                        );
                    }
                }
            }

            // Commit all data writes in a single batch
            let data_commit_started = Instant::now();
            data_batch.commit()?;
            write_commit_ms += data_commit_started.elapsed().as_secs_f64() * 1000.0;
            if !domain_analytics_batch.is_empty() {
                let script_commit_started = Instant::now();
                domain_analytics_batch.commit()?;
                write_commit_ms += script_commit_started.elapsed().as_secs_f64() * 1000.0;
            }
            if !nft_activity_batch.is_empty() {
                let nft_activity_commit_started = Instant::now();
                nft_activity_batch.commit()?;
                write_commit_ms += nft_activity_commit_started.elapsed().as_secs_f64() * 1000.0;
            }
            if !append_history_batch.is_empty() {
                let append_commit_started = Instant::now();
                append_history_batch.commit()?;
                write_commit_ms += append_commit_started.elapsed().as_secs_f64() * 1000.0;
            }
            if !activity_batch.is_empty() {
                let activity_commit_started = Instant::now();
                activity_batch.commit()?;
                write_commit_ms += activity_commit_started.elapsed().as_secs_f64() * 1000.0;
            }

            // Stats accumulation for live sync (serial — before finalize)
            batch_stats = BatchStats::default();
            let mut prev_timestamp: Option<chrono::DateTime<Utc>> =
                if let Some(first_block) = all_parsed_blocks.first() {
                    self.writer
                        .get_previous_block_timestamp(first_block.number)?
                } else {
                    None
                };
            let mut prev_epoch: Option<(i64, chrono::DateTime<Utc>, f64)> =
                if let Some(first_block) = all_parsed_blocks.first() {
                    self.writer
                        .get_last_epoch_start(first_block.number)?
                        .map(|(epoch_num, ts)| (epoch_num, ts, 0.0))
                } else {
                    None
                };
            let mut prev_dao_cs: Option<(i128, i128)> =
                if let Some(first_block) = all_parsed_blocks.first() {
                    if first_block.number > 0 {
                        self.writer
                            .store()
                            .get_block_header(first_block.number - 1)?
                            .and_then(|h| extract_dao_csu(&h.dao).map(|(c, s, _)| (c, s)))
                    } else {
                        None
                    }
                } else {
                    None
                };

            // Pre-build consumed DAO deposit map for delta computation
            let dao_code_hash_for_stats =
                crate::rpc::parse_hex_to_bytes(crate::parser::dao::DAO_CODE_HASH);
            let all_input_outpoints_for_dao: Vec<(Vec<u8>, i16)> = all_tx_data
                .iter()
                .filter(|tx| !tx.is_cellbase)
                .flat_map(|tx| {
                    tx.inputs.iter().map(|input| {
                        (
                            input.previous_tx_hash.to_vec(),
                            parsed_input_outpoint_index_i16(
                                input.previous_output_index,
                                "sync_indexer",
                            ),
                        )
                    })
                })
                .collect();
            let consumed_dao_for_stats = if !all_input_outpoints_for_dao.is_empty() {
                let unique: Vec<(Vec<u8>, i16)> = {
                    let mut seen = HashSet::new();
                    all_input_outpoints_for_dao
                        .into_iter()
                        .filter(|x| seen.insert(x.clone()))
                        .collect()
                };
                let refs: Vec<(&[u8], i16)> =
                    unique.iter().map(|(h, i)| (h.as_slice(), *i)).collect();
                self.writer.find_consumed_dao_deposits_batch(&refs)?
            } else {
                HashMap::new()
            };
            let mut same_batch_dao_for_stats: HashMap<(Vec<u8>, i16), i64> = HashMap::new();

            let mut block_tx_idx = 0usize;
            for parsed in all_parsed_blocks {
                let block_date = ckbadger_common::block_date(parsed.timestamp);
                accumulate_secondary_issuance_deltas(
                    &mut batch_stats,
                    parsed,
                    block_date,
                    &mut prev_dao_cs,
                )?;
                let tx_count_for_block = parsed.transactions_count as usize;
                let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                block_tx_idx += tx_count_for_block;

                let cells_created: i32 = tx_slice.iter().map(|tx| tx.cells.len() as i32).sum();
                let cells_consumed: i32 = tx_slice
                    .iter()
                    .filter(|tx| !tx.is_cellbase)
                    .map(|tx| tx.inputs.len() as i32)
                    .sum();
                let capacity_transferred: i128 = tx_slice
                    .iter()
                    .filter(|tx| !tx.is_cellbase)
                    .map(|tx| i128::from(tx.total_output_capacity))
                    .sum();
                let data_size_added: i64 = tx_slice
                    .iter()
                    .flat_map(|tx| tx.cells.iter())
                    .map(|cell| cell.data_size as i64)
                    .sum();
                let occupied_capacity_created: i128 = tx_slice
                    .iter()
                    .flat_map(|tx| tx.cells.iter())
                    .map(|cell| {
                        occupied_capacity_shannons_i128(
                            cell.lock_args.len(),
                            cell.type_args.as_ref().map(|args| args.len()),
                            cell.data_size,
                        )
                    })
                    .sum();
                let data_size_consumed: i64 = tx_slice
                    .iter()
                    .filter(|tx| !tx.is_cellbase)
                    .flat_map(|tx| tx.inputs.iter())
                    .filter_map(|input| {
                        let key = (
                            input.previous_tx_hash.to_vec(),
                            parsed_input_outpoint_index_i16(
                                input.previous_output_index,
                                "sync_indexer",
                            ),
                        );
                        input_cell_info
                            .get(&key)
                            .map(|info| info.data_size as i64)
                            .or_else(|| {
                                batch_cell_infos.get(&key).map(|info| info.data_size as i64)
                            })
                    })
                    .sum();
                let occupied_capacity_consumed: i128 = tx_slice
                    .iter()
                    .filter(|tx| !tx.is_cellbase)
                    .flat_map(|tx| tx.inputs.iter())
                    .filter_map(|input| {
                        let key = (
                            input.previous_tx_hash.to_vec(),
                            parsed_input_outpoint_index_i16(
                                input.previous_output_index,
                                "sync_indexer",
                            ),
                        );
                        input_cell_info
                            .get(&key)
                            .map(|info| i128::from(info.occupied_capacity))
                            .or_else(|| {
                                batch_cell_infos
                                    .get(&key)
                                    .map(|info| i128::from(info.occupied_capacity))
                            })
                    })
                    .sum();

                batch_stats.sync_totals.0 += parsed.transactions_count as i64;
                batch_stats.sync_totals.1 += cells_created as i64;
                batch_stats.sync_totals.2 += cells_consumed as i64;
                batch_stats.last_block = Some((parsed.number, parsed.hash.clone()));

                {
                    let entry = batch_stats.daily_stats.entry(block_date).or_default();
                    entry.0 += 1;
                    entry.1 += parsed.transactions_count;
                    entry.2 += cells_created;
                    entry.3 += cells_consumed;
                    entry.4 = entry.4.checked_add(capacity_transferred).ok_or_else(|| {
                        anyhow!(
                            "daily capacity_transferred overflow: date={} block={}",
                            block_date,
                            parsed.number
                        )
                    })?;
                    entry.5 = entry
                        .5
                        .checked_add(occupied_capacity_created)
                        .ok_or_else(|| {
                            anyhow!(
                                "daily occupied_capacity_created overflow: date={} block={}",
                                block_date,
                                parsed.number
                            )
                        })?;
                    entry.6 = entry
                        .6
                        .checked_add(occupied_capacity_consumed)
                        .ok_or_else(|| {
                            anyhow!(
                                "daily occupied_capacity_consumed overflow: date={} block={}",
                                block_date,
                                parsed.number
                            )
                        })?;
                    entry.7 += data_size_added;
                    entry.8 += data_size_consumed;
                }
                batch_stats
                    .daily_dao_fields
                    .insert(block_date, parsed.dao.clone());
                {
                    let block_hour = truncate_to_hour(parsed.timestamp);
                    let entry = batch_stats.hourly_stats.entry(block_hour).or_default();
                    entry.0 += 1;
                    entry.1 += parsed.transactions_count;
                    entry.2 += cells_created;
                    entry.3 += cells_consumed;
                    entry.4 = entry.4.checked_add(capacity_transferred).ok_or_else(|| {
                        anyhow!(
                            "hourly capacity_transferred overflow: hour={} block={}",
                            block_hour,
                            parsed.number
                        )
                    })?;
                }
                {
                    let entry = batch_stats.daily_block_stats.entry(block_date).or_default();
                    entry.0 += parsed.compact_target as i128;
                    entry.1 += 1;
                    entry.2 += parsed.uncles_count;
                }
                if let Some(first_tx) = tx_slice.first() {
                    if first_tx.is_cellbase {
                        if let Some(first_cell) = first_tx.cells.first() {
                            let key = (block_date, first_cell.lock_script_hash.clone());
                            let entry = batch_stats.miner_stats.entry(key).or_insert((0, 0));
                            entry.0 += 1;
                            entry.1 = parsed.number;
                        }
                    }
                }
                {
                    let entry = batch_stats
                        .epoch_stats
                        .entry(parsed.epoch_number)
                        .or_insert_with(|| EpochAccum {
                            start_block: parsed.number,
                            end_block: parsed.number,
                            length: parsed.epoch_length,
                            start_ts: parsed.timestamp,
                            end_ts: parsed.timestamp,
                            tx_count: 0,
                            is_new: parsed.epoch_index == 0,
                        });
                    entry.end_block = parsed.number;
                    entry.end_ts = parsed.timestamp;
                    entry.tx_count += parsed.transactions_count;
                }

                if let Some(prev_ts) = prev_timestamp {
                    let block_time_seconds = (parsed.timestamp - prev_ts).num_seconds();
                    if block_time_seconds >= 0 {
                        *batch_stats
                            .block_time_dist
                            .entry(block_time_to_bucket(block_time_seconds))
                            .or_default() += 1;
                        let block_time_ms = block_time_seconds * 1000;
                        let entry = batch_stats
                            .daily_block_times
                            .entry(block_date)
                            .or_insert((0, 0));
                        entry.0 += block_time_ms;
                        entry.1 += 1;
                    }
                }
                prev_timestamp = Some(parsed.timestamp);

                if parsed.epoch_index == 0 && parsed.epoch_number > 0 {
                    if let Some((prev_epoch_num, prev_start_ts, _)) = prev_epoch {
                        if prev_epoch_num == parsed.epoch_number - 1 {
                            let epoch_duration_minutes =
                                (parsed.timestamp - prev_start_ts).num_seconds() as f64 / 60.0;
                            let bucket_minutes = epoch_duration_minutes.round() as i32;
                            *batch_stats
                                .epoch_time_dist
                                .entry(bucket_minutes)
                                .or_default() += 1;
                        }
                    }
                }
                if parsed.epoch_index == 0 {
                    prev_epoch = Some((parsed.epoch_number, parsed.timestamp, 0.0));
                }

                // DAO per-day deltas for snapshot accumulation (mirrors T7 bulk path)
                accumulate_dao_snapshot_deltas_for_txs(
                    tx_slice,
                    block_date,
                    &dao_code_hash_for_stats,
                    &consumed_dao_for_stats,
                    &mut same_batch_dao_for_stats,
                    &mut batch_stats.dao_daily_active_delta,
                    &mut batch_stats.dao_daily_gross_deposit_delta,
                    &mut batch_stats.dao_daily_new_deposits_delta,
                    &mut batch_stats.dao_daily_withdrawals_delta,
                )?;

                batch_stats.dao_snapshot_dates.insert(block_date);
            }
            batch_stats.dao_deltas_computed = true;
        }
        let write_ms = t_write.elapsed().as_secs_f64() * 1000.0;

        // Finalization: block headers + stats commit
        let t_finalize = Instant::now();
        {
            let mut core_batch = StoreBatch::new(self.writer.store());
            self.writer
                .insert_blocks_batch(&block_refs, &mut core_batch)?;
            let mut stats_batch = StoreBatch::new(self.writer.store());
            self.write_batch_stats_to_batch(&batch_stats, &mut stats_batch)?;
            debug!(
                phase = "finalize_headers_stats",
                batch_start = first_block,
                batch_end = last_block,
                bulk_sync_mode,
                "Batch finalize commit start"
            );
            let finalize_commit_started = Instant::now();
            if bulk_sync_mode {
                core_batch.commit_no_wal().with_context(|| {
                    format!(
                        "core finalize commit_no_wal failed for blocks {}-{}",
                        first_block, last_block
                    )
                })?;
                stats_batch.commit_no_wal().with_context(|| {
                    format!(
                        "stats finalize commit_no_wal failed for blocks {}-{}",
                        first_block, last_block
                    )
                })?;
            } else {
                core_batch.commit().with_context(|| {
                    format!(
                        "core finalize commit failed for blocks {}-{}",
                        first_block, last_block
                    )
                })?;
                stats_batch.commit().with_context(|| {
                    format!(
                        "stats finalize commit failed for blocks {}-{}",
                        first_block, last_block
                    )
                })?;
            }
            let finalize_commit_ms = finalize_commit_started.elapsed().as_secs_f64() * 1000.0;
            write_commit_ms += finalize_commit_ms;
            if finalize_commit_ms >= BULK_PHASE_COMMIT_SLOW_WARN_MS {
                warn!(
                    phase = "finalize_headers_stats",
                    batch_start = first_block,
                    batch_end = last_block,
                    commit_ms = format!("{:.1}", finalize_commit_ms),
                    bulk_sync_mode,
                    "Batch finalize commit slow"
                );
            } else {
                debug!(
                    phase = "finalize_headers_stats",
                    batch_start = first_block,
                    batch_end = last_block,
                    commit_ms = format!("{:.1}", finalize_commit_ms),
                    bulk_sync_mode,
                    "Batch finalize commit done"
                );
            }
        }

        // HODL wave tracker update
        self.update_hodl_wave(
            all_parsed_blocks,
            &all_tx_data,
            &input_cell_info,
            &batch_cell_infos,
            &address_balance_changes,
        )?;

        // Lightweight async cache update (no DB write)
        if let Some((block_number, ref block_hash)) = batch_stats.last_block {
            let ema_rate = self.progress.ema_blocks_per_second();
            let ema_rate_opt = if ema_rate > 0.0 { Some(ema_rate) } else { None };
            self.writer
                .update_sync_status(
                    block_number,
                    block_hash,
                    batch_stats.sync_totals.0,
                    batch_stats.sync_totals.1,
                    batch_stats.sync_totals.2,
                    batch_new_addresses,
                    ema_rate_opt,
                )
                .await?;
        }

        if !bulk_sync_mode {
            let committed_proposal_ids = collect_committed_proposal_ids(&all_tx_data);
            if !committed_proposal_ids.is_empty() {
                self.cache_invalidator
                    .remove_committed_proposals(&committed_proposal_ids)
                    .await;
            }
        }
        let finalize_ms = t_finalize.elapsed().as_secs_f64() * 1000.0;

        let batch_tx_count = all_tx_data.len();
        let batch_cell_count: usize = all_tx_data.iter().map(|t| t.cells.len()).sum();
        let batch_input_count: usize = all_tx_data
            .iter()
            .filter(|t| !t.is_cellbase)
            .map(|t| t.inputs.len())
            .sum();
        if let Some([t1, t2, t4, t5, t6a, t6b, t7, t_act]) = thread_times {
            info!(
                precompute_ms = format!("{:.1}", precompute_ms),
                prefetch_ms = format!("{:.1}", prefetch_ms),
                write_ms = format!("{:.1}", write_ms),
                write_commit_ms = format!("{:.1}", write_commit_ms),
                finalize_ms = format!("{:.1}", finalize_ms),
                t1_ms = format!("{:.1}", t1),
                t2_ms = format!("{:.1}", t2),
                t4_ms = format!("{:.1}", t4),
                t5_ms = format!("{:.1}", t5),
                t6a_ms = format!("{:.1}", t6a),
                t6b_ms = format!("{:.1}", t6b),
                t7_ms = format!("{:.1}", t7),
                t_act_ms = format!("{:.1}", t_act),
                txs = batch_tx_count,
                cells = batch_cell_count,
                inputs = batch_input_count,
                "Batch write breakdown"
            );
        } else {
            info!(
                precompute_ms = format!("{:.1}", precompute_ms),
                write_ms = format!("{:.1}", write_ms),
                write_commit_ms = format!("{:.1}", write_commit_ms),
                finalize_ms = format!("{:.1}", finalize_ms),
                txs = batch_tx_count,
                cells = batch_cell_count,
                inputs = batch_input_count,
                "Batch write breakdown"
            );
        }
        Ok(BatchWriteMetrics {
            commit_ms: write_commit_ms,
        })
    }
    // === write_batch_stats_to_batch ===

    fn write_batch_stats_to_batch(&self, stats: &BatchStats, batch: &mut StoreBatch) -> Result<()> {
        // Epoch statistics
        for (epoch_number, accum) in &stats.epoch_stats {
            self.writer.upsert_epoch_statistics_batch(
                *epoch_number,
                accum.start_block,
                accum.end_block,
                accum.length,
                accum.start_ts,
                accum.end_ts,
                accum.tx_count,
                accum.is_new,
                batch,
            )?;
        }

        // Daily statistics (with block time data folded in).
        // Sort dates so cumulative totals are threaded forward correctly
        // when a batch spans multiple calendar days.
        let mut sorted_dates: Vec<_> = stats.daily_stats.keys().copied().collect();
        sorted_dates.sort();
        let mut prev_day_stats: Option<ckbadger_store::types::DailyStats> = None;
        for date in &sorted_dates {
            let (
                blocks,
                txs,
                created,
                consumed,
                capacity,
                occupied_created,
                occupied_consumed,
                data_size_added,
                data_size_consumed,
            ) = stats.daily_stats[date];
            let dao_field = stats.daily_dao_fields.get(date);
            let block_time = stats.daily_block_times.get(date).copied();
            let result = self.writer.update_daily_statistics(
                *date,
                blocks,
                txs,
                created,
                consumed,
                capacity,
                occupied_created,
                occupied_consumed,
                data_size_added,
                data_size_consumed,
                dao_field.map(|v| v.as_slice()),
                block_time,
                prev_day_stats.as_ref(),
                batch,
            )?;
            prev_day_stats = Some(result);
        }

        // Daily block stats
        for (date, (sum_target, count, uncles)) in &stats.daily_block_stats {
            let avg_target = if *count > 0 {
                i64::try_from(*sum_target / *count as i128).map_err(|_| {
                    anyhow!(
                        "daily avg compact target exceeds i64: date={} sum_target={} count={}",
                        date,
                        sum_target,
                        count
                    )
                })?
            } else {
                0
            };
            self.writer
                .update_daily_block_stats_batch(*date, avg_target, *count, *uncles, batch)?;
        }

        // Hourly statistics
        for (hour, (blocks, txs, created, consumed, capacity)) in &stats.hourly_stats {
            self.writer.update_hourly_statistics(
                *hour, *blocks, *txs, *created, *consumed, *capacity, batch,
            )?;
        }

        // Miner statistics
        for ((date, miner_hash), (blocks_count, last_block)) in &stats.miner_stats {
            self.writer.update_miner_statistics_batch(
                miner_hash,
                *last_block,
                *date,
                *blocks_count,
                batch,
            )?;
        }

        // Block time distribution
        for (bucket, count) in &stats.block_time_dist {
            self.writer
                .update_block_time_distribution_batch(*bucket, *count, batch)?;
        }

        // Epoch time distribution
        for (bucket, count) in &stats.epoch_time_dist {
            self.writer
                .update_epoch_time_distribution_batch(*bucket, *count, batch)?;
        }

        // DAO daily snapshots
        {
            let mut snapshot_dates: Vec<_> = stats.dao_snapshot_dates.iter().collect();
            snapshot_dates.sort();
            if !snapshot_dates.is_empty() {
                // Continue from the latest snapshot and apply exact per-day deltas from
                // this batch (deposits and phase-1 withdrawals) in date order.
                // When dao_deltas_computed is false (e.g. live sync path), deposit
                // deltas default to 0 via unwrap_or(0), carrying forward previous
                // totals while still updating DAO fields and secondary issuance.
                let latest_snapshot = load_latest_dao_daily_snapshot(self.writer.store())?;

                let mut running_total_deposited = latest_snapshot
                    .as_ref()
                    .map(|s| s.total_deposited)
                    .unwrap_or(0);
                let mut running_total_deposit_count = latest_snapshot
                    .as_ref()
                    .map(|s| s.new_deposits)
                    .unwrap_or(0);
                let mut running_total_withdrawal_count =
                    latest_snapshot.as_ref().map(|s| s.withdrawals).unwrap_or(0);
                let mut running_cumulative_deposit_amount = latest_snapshot
                    .as_ref()
                    .map(|s| s.cumulative_deposit_amount)
                    .unwrap_or(0);
                let mut running_cum_miner = latest_snapshot
                    .as_ref()
                    .map(|s| s.cum_miner_secondary)
                    .unwrap_or(0);
                let mut running_cum_dao = latest_snapshot
                    .as_ref()
                    .map(|s| s.cum_dao_compensation)
                    .unwrap_or(0);
                let mut running_cum_treasury = latest_snapshot
                    .as_ref()
                    .map(|s| s.cum_treasury)
                    .unwrap_or(0);
                let mut prev_secondary_pool = latest_snapshot
                    .as_ref()
                    .map(|s| s.secondary_pool)
                    .unwrap_or(0);

                for date in snapshot_dates {
                    running_total_deposited +=
                        stats.dao_daily_active_delta.get(date).copied().unwrap_or(0);
                    running_cumulative_deposit_amount += stats
                        .dao_daily_gross_deposit_delta
                        .get(date)
                        .copied()
                        .unwrap_or(0);
                    running_total_deposit_count += stats
                        .dao_daily_new_deposits_delta
                        .get(date)
                        .copied()
                        .unwrap_or(0);
                    running_total_withdrawal_count += stats
                        .dao_daily_withdrawals_delta
                        .get(date)
                        .copied()
                        .unwrap_or(0);

                    // Extract C, S, U from the DAO header field for this date.
                    let (total_issuance, secondary_pool, occupied_capacity) =
                        dao_csu_for_snapshot_date(stats, *date)?;
                    let non_miner_secondary = resolve_non_miner_secondary_delta_for_snapshot(
                        *date,
                        stats.daily_secondary_non_miner_delta.get(date).copied(),
                        secondary_pool,
                        prev_secondary_pool,
                    )?;
                    let (daily_miner, daily_dao_share, daily_treasury_share) =
                        if total_issuance > 0 && non_miner_secondary > 0 {
                            split_secondary_issuance(
                                total_issuance,
                                occupied_capacity,
                                running_total_deposited,
                                non_miner_secondary,
                            )?
                        } else {
                            (0, 0, 0)
                        };
                    running_cum_miner += daily_miner;
                    running_cum_dao += daily_dao_share;
                    running_cum_treasury += daily_treasury_share;
                    prev_secondary_pool = secondary_pool;

                    let running_depositors = derive_running_depositors(
                        running_total_deposit_count,
                        running_total_withdrawal_count,
                        *date,
                    )?;
                    let running_total_compensation = running_cum_dao;

                    let dao_snapshot = crate::db::writer::DaoSnapshotInput {
                        total_deposited: running_total_deposited,
                        depositors_count: running_depositors,
                        total_deposit_count: running_total_deposit_count,
                        total_withdrawal_count: running_total_withdrawal_count,
                        total_compensation: running_total_compensation,
                        cumulative_deposit_amount: running_cumulative_deposit_amount,
                        total_issuance,
                        secondary_pool,
                        occupied_capacity,
                        cum_miner_secondary: running_cum_miner,
                        cum_dao_compensation: running_cum_dao,
                        cum_treasury: running_cum_treasury,
                    };
                    self.writer
                        .update_dao_daily_snapshot(*date, &dao_snapshot, batch)?;
                }
            }
        }

        Ok(())
    }

    // === update_hodl_wave ===

    fn reconcile_hodl_tracker_with_tip(&self, tip_block: i64) -> Result<()> {
        let state = self.writer.store().get_hodl_tracker_state()?;
        let rebuilt = rebuild_hodl_tracker_from_state(state, tip_block)?;

        let mut tracker = self.hodl_tracker.lock().unwrap();
        *tracker = rebuilt;
        Ok(())
    }

    /// Feed parsed block data into the HODL wave tracker and write snapshots at day boundaries.
    fn update_hodl_wave(
        &self,
        all_parsed_blocks: &[crate::parser::block::ParsedBlock],
        all_tx_data: &[TxData],
        input_cell_info: &HashMap<(Vec<u8>, i16), LiveCellInfo>,
        batch_cell_infos: &HashMap<(Vec<u8>, i16), LiveCellInfo>,
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
        // Each entry: (balance_delta, live_delta, total_delta, tx_delta, block_num, tx_hash, occupied_delta)
        for (
            lock_hash,
            (
                _balance_delta,
                live_delta,
                _total_delta,
                _tx_delta,
                _block_num,
                _tx_hash,
                _occupied_delta,
            ),
        ) in address_balance_changes
        {
            let current_balance = store.get_addr_balance(lock_hash)?;
            let post_live = current_balance
                .as_ref()
                .map(|b| b.live_cells_count)
                .unwrap_or(0);
            let old_live = derive_pre_batch_live_cells(post_live, *live_delta)?;
            tracker.update_holder_count(old_live, post_live)?;
        }

        // Phase 3: Persist tracker state
        store.put_hodl_tracker_state(&tracker.to_state())?;

        Ok(())
    }

    // === get_chain_block_hash, get_chain_tip ===

    /// Get the block hash for a given block number, using direct RocksDB reads when available.
    async fn get_chain_block_hash(&self, number: u64) -> Result<Vec<u8>> {
        if let Some(ref store) = self.ckb_store {
            store.refresh()?;
            store
                .get_block_hash(number)
                .map(|h| h.to_vec())
                .ok_or_else(|| anyhow::anyhow!("Block {} not found in CKB RocksDB", number))
        } else {
            let hash_hex = self
                .rpc
                .get_block_hash(number)
                .await?
                .ok_or_else(|| anyhow::anyhow!("Block {} not found on chain", number))?;
            Ok(crate::rpc::parse_hex_to_bytes(&hash_hex))
        }
    }

    /// Get the chain tip block number, using direct RocksDB reads when available.
    async fn get_chain_tip(&self) -> Result<u64> {
        if let Some(ref store) = self.ckb_store {
            store.refresh()?;
            store
                .tip_number()
                .ok_or_else(|| anyhow::anyhow!("Failed to get chain tip from CKB RocksDB"))
        } else {
            self.rpc.get_tip_block_number().await
        }
    }

    // === check_and_handle_reorg, find_fork_point ===

    async fn check_and_handle_reorg(
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

    async fn find_fork_point(&self, db_tip: u64, min_height: u64) -> Result<(u64, Vec<u8>)> {
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
    async fn run_proposal_cache_batch(
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

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_live_cell_info() -> LiveCellInfo {
        LiveCellInfo {
            capacity: 1,
            created_at_block: 1,
            lock_script_hash: vec![1u8; 32],
            lock_code_hash: vec![2u8; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 1,
            udt_amount: None,
        }
    }

    fn dummy_tx_index_entry() -> ckbadger_store::types::TxIndexEntry {
        ckbadger_store::types::TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_000_000,
            inputs_count: 1,
            outputs_count: 1,
            fee: 1,
            tx_size: 1,
            cycles: None,
        }
    }

    #[test]
    fn test_collect_missing_input_outpoints_dedups_and_skips_resolved() {
        let input_outpoints = vec![
            (vec![0xAA; 32], 0),
            (vec![0xAA; 32], 0),
            (vec![0xBB; 32], 1),
            (vec![0xCC; 32], 2),
        ];
        let mut resolved = HashMap::new();
        resolved.insert((vec![0xBB; 32], 1), dummy_live_cell_info());
        let mut same_batch = HashMap::new();
        same_batch.insert((vec![0xCC; 32], 2), ());

        let missing = collect_missing_input_outpoints(&input_outpoints, &resolved, &same_batch);
        assert_eq!(missing, vec![(vec![0xAA; 32], 0)]);
    }

    #[test]
    fn test_build_activity_input_views_errors_when_input_cell_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let previous_tx_hash = [0x44; 32];
        let tx = dummy_tx_data(
            [0x11; 32],
            false,
            vec![crate::parser::transaction::ParsedInput {
                previous_tx_hash,
                previous_output_index: 2,
                since: 0,
            }],
            vec![],
            vec![],
            vec![],
        );

        let err =
            match build_activity_input_views(&store, &tx, 99, &HashMap::new(), &HashMap::new()) {
                Ok(_) => panic!("missing input cell info should fail fast"),
                Err(err) => err,
            };
        assert!(err
            .to_string()
            .contains("missing input cell info while building activities"));
        assert!(err.to_string().contains("block=99"));
    }

    #[test]
    fn test_build_activity_input_views_uses_batch_fallback_cell_info() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let previous_tx_hash = [0x55; 32];
        let tx = dummy_tx_data(
            [0x22; 32],
            false,
            vec![crate::parser::transaction::ParsedInput {
                previous_tx_hash,
                previous_output_index: 3,
                since: 0,
            }],
            vec![],
            vec![],
            vec![],
        );

        let mut info = dummy_live_cell_info();
        info.capacity = 123;
        info.occupied_capacity = 456;
        info.data_size = 16;
        info.lock_script_hash = vec![0xAB; 32];

        let mut batch_cell_infos = HashMap::new();
        batch_cell_infos.insert((previous_tx_hash.to_vec(), 3), info.clone());

        let inputs =
            build_activity_input_views(&store, &tx, 100, &HashMap::new(), &batch_cell_infos)
                .expect("input lookup should fall back to same-batch cell cache");
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].capacity, info.capacity);
        assert_eq!(inputs[0].occupied_capacity, info.occupied_capacity);
        assert_eq!(inputs[0].lock_script_hash, info.lock_script_hash);
    }

    #[test]
    fn test_build_activity_input_views_marks_dao_withdraw_request_inputs() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let previous_tx_hash = [0x66; 32];
        let tx = dummy_tx_data(
            [0x33; 32],
            false,
            vec![crate::parser::transaction::ParsedInput {
                previous_tx_hash,
                previous_output_index: 1,
                since: 0,
            }],
            vec![],
            vec![],
            vec![],
        );

        let mut info = dummy_live_cell_info();
        info.type_code_hash = Some(crate::rpc::parse_hex_to_bytes(
            crate::parser::dao::DAO_CODE_HASH,
        ));
        let mut input_cell_info = HashMap::new();
        input_cell_info.insert((previous_tx_hash.to_vec(), 1), info);

        let mut seed = StoreBatch::new(&store);
        let deposit_outpoint_key = keys::encode_outpoint(&[0x77; 32], 0);
        seed.put_dao_by_withdraw_tx(&previous_tx_hash, 1, &deposit_outpoint_key);
        seed.commit().unwrap();

        let inputs =
            build_activity_input_views(&store, &tx, 200, &input_cell_info, &HashMap::new())
                .unwrap();
        assert_eq!(inputs.len(), 1);
        assert!(inputs[0].is_dao_withdraw_request);
    }

    #[test]
    fn test_require_chain_tip_number_errors_on_missing_tip() {
        let err = require_chain_tip_number(None, "CKB RocksDB").unwrap_err();
        assert!(err
            .to_string()
            .contains("Failed to get chain tip from CKB RocksDB"));
    }

    #[test]
    fn test_load_optional_index_from_store_propagates_error() {
        let mut cache: HashMap<Vec<u8>, Option<i32>> = HashMap::new();
        let err = load_optional_index_from_store(&mut cache, &[0xAA; 32], "test_index", || {
            Err(anyhow!("synthetic index read failure"))
        })
        .unwrap_err();
        assert!(err.to_string().contains("failed to load test_index index"));
        assert!(err
            .chain()
            .any(|cause| cause.to_string().contains("synthetic index read failure")));
    }

    #[test]
    fn test_load_optional_index_from_store_caches_loaded_value() {
        let mut cache: HashMap<Vec<u8>, Option<i32>> = HashMap::new();
        let mut load_calls = 0usize;

        let first = load_optional_index_from_store(&mut cache, &[0xAB; 32], "test_index", || {
            load_calls += 1;
            Ok(Some(7))
        })
        .unwrap();
        let second = load_optional_index_from_store(&mut cache, &[0xAB; 32], "test_index", || {
            load_calls += 1;
            Ok(Some(9))
        })
        .unwrap();

        assert_eq!(first, Some(7));
        assert_eq!(second, Some(7));
        assert_eq!(load_calls, 1);
    }

    #[test]
    fn test_next_start_block_from_db_tip_rejects_negative_tip() {
        let err = next_start_block_from_db_tip(-1, &Some(vec![0x11; 32]), "unit-test").unwrap_err();
        assert!(err.to_string().contains("negative block number"));
    }

    #[test]
    fn test_blocks_behind_tip_rejects_inverted_tip_order() {
        let err = blocks_behind_tip(100, 101, "unit-test").unwrap_err();
        assert!(err.to_string().contains("exceeds chain_tip"));
    }

    #[test]
    fn test_mempool_short_tx_id_validates_shape() {
        assert_eq!(
            mempool_short_tx_id("0x1234567890abcdef123456").unwrap(),
            "1234567890abcdef1234"
        );
        assert!(mempool_short_tx_id("1234").is_err());
        assert!(mempool_short_tx_id("0x1234").is_err());
    }

    #[test]
    fn test_load_latest_dao_daily_snapshot_propagates_deserialize_error() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let key = keys::encode_stats_key(keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT, b"20260101");
        store.put_cf(store.cf_stats_dao(), &key, b"broken").unwrap();

        let err = load_latest_dao_daily_snapshot(&store).unwrap_err();
        assert!(err
            .to_string()
            .contains("failed to list dao daily snapshots while building cumulative snapshot"));
    }

    #[test]
    fn test_parse_udt_cells_with_store_fallback_preserves_output_index() {
        let lock = crate::rpc::Script {
            code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8"
                .to_string(),
            hash_type: "type".to_string(),
            args: "0x0102030405060708090a0b0c0d0e0f1011121314".to_string(),
        };
        let sudt_type = crate::rpc::Script {
            code_hash: crate::parser::udt::SUDT_CODE_HASH.to_string(),
            hash_type: "type".to_string(),
            args: "0xa92deeb134132d493d340f2cc4e7b62f930bcd037f0fb7f06b48f931f36f9fc2".to_string(),
        };
        let tx = crate::rpc::TransactionView {
            hash: format!("0x{}", "11".repeat(32)),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![],
            outputs: vec![
                crate::rpc::CellOutput {
                    capacity: "0x0".to_string(),
                    lock: lock.clone(),
                    type_: None,
                },
                crate::rpc::CellOutput {
                    capacity: "0x0".to_string(),
                    lock,
                    type_: Some(sudt_type),
                },
            ],
            outputs_data: vec![
                "0x".to_string(),
                "0x01000000000000000000000000000000".to_string(),
            ],
            witnesses: vec![],
        };

        let parsed = parse_udt_cells_with_store_fallback_inner(&tx, |_| Ok(None)).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].0, 1);
        assert_eq!(parsed[0].1.amount, 1);
    }

    #[test]
    fn test_parse_udt_cells_with_store_fallback_propagates_lookup_error() {
        let lock = crate::rpc::Script {
            code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8"
                .to_string(),
            hash_type: "type".to_string(),
            args: "0x0102030405060708090a0b0c0d0e0f1011121314".to_string(),
        };
        let non_udt_type = crate::rpc::Script {
            code_hash: "0x1234".to_string(),
            hash_type: "type".to_string(),
            args: "0x56".to_string(),
        };
        let tx = crate::rpc::TransactionView {
            hash: format!("0x{}", "aa".repeat(32)),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![],
            outputs: vec![crate::rpc::CellOutput {
                capacity: "0x0".to_string(),
                lock,
                type_: Some(non_udt_type),
            }],
            outputs_data: vec!["0x".to_string()],
            witnesses: vec![],
        };

        let err = parse_udt_cells_with_store_fallback_inner(&tx, |_| {
            Err(anyhow!("synthetic token metadata lookup failure"))
        })
        .unwrap_err();
        assert!(err
            .to_string()
            .contains("synthetic token metadata lookup failure"));
    }

    #[test]
    fn test_parse_udt_cells_with_store_fallback_errors_on_outputs_data_length_mismatch() {
        let tx = crate::rpc::TransactionView {
            hash: format!("0x{}", "bb".repeat(32)),
            version: "0x0".to_string(),
            cell_deps: vec![],
            header_deps: vec![],
            inputs: vec![],
            outputs: vec![crate::rpc::CellOutput {
                capacity: "0x0".to_string(),
                lock: crate::rpc::Script {
                    code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8"
                        .to_string(),
                    hash_type: "type".to_string(),
                    args: "0x0102030405060708090a0b0c0d0e0f1011121314".to_string(),
                },
                type_: None,
            }],
            outputs_data: vec![],
            witnesses: vec![],
        };

        let err = parse_udt_cells_with_store_fallback_inner(&tx, |_| Ok(None)).unwrap_err();
        assert!(err.to_string().contains(
            "transaction outputs mismatch while parsing UDT outputs with store fallback"
        ));
    }

    #[test]
    fn test_resolve_input_udt_info_ignores_stale_cache_entry_for_non_live_outpoint() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store);
        let cache = DashMap::new();

        let tx_hash = vec![0xAA; 32];
        let output_index = 0i16;
        cache.insert(
            ([0xAA; 32], output_index),
            CachedUdtCellInfo {
                type_script_hash: vec![0x10; 32],
                type_code_hash: vec![0x20; 32],
                type_hash_type: 1,
                type_args: vec![0x30; 32],
                lock_script_hash: vec![0x40; 32],
                amount: 145_203,
                standard: "xudt_compatible".to_string(),
            },
        );

        let resolved = resolve_input_udt_info_from_live_cells(
            &writer,
            &cache,
            &[(tx_hash.clone(), output_index)],
        )
        .unwrap();

        assert!(resolved.is_empty());
    }

    #[test]
    fn test_resolve_input_udt_info_reads_live_cells_and_refreshes_cache() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());
        let cache = DashMap::new();

        let type_hash = vec![0x11; 32];
        let type_code_hash = vec![0x22; 32];
        let tx_hash = vec![0x33; 32];
        let output_index = 0i16;

        let token = ckbadger_store::types::TokenInfo {
            type_code_hash: type_code_hash.clone(),
            hash_type: 1,
            type_args: vec![0x44; 32],
            standard: "xudt_compatible".to_string(),
            name: None,
            symbol: None,
            decimals: None,
            total_supply: Some(0),
            max_supply: None,
            holders_count: 0,
            first_seen_block: 0,
            icon_url: None,
            description: None,
            transfers_count: 0,
        };
        store.put_token_direct(&type_hash, &token).unwrap();

        let live_cell = LiveCellInfo {
            capacity: 100_000_000,
            created_at_block: 1,
            lock_script_hash: vec![0x55; 32],
            lock_code_hash: vec![0x66; 32],
            lock_hash_type: 1,
            lock_args: vec![0x77; 20],
            type_script_hash: Some(type_hash.clone()),
            type_code_hash: Some(type_code_hash),
            type_args: Some(vec![0x44; 32]),
            data_size: 16,
            occupied_capacity: 0,
            udt_amount: Some(145_203),
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_cell(&tx_hash, output_index, &live_cell);
        batch.commit().unwrap();

        let resolved = resolve_input_udt_info_from_live_cells(
            &writer,
            &cache,
            &[(tx_hash.clone(), output_index)],
        )
        .unwrap();

        let entry = resolved
            .get(&(tx_hash.clone(), output_index))
            .expect("expected live UDT input to be resolved");
        assert_eq!(entry.0, type_hash);
        assert_eq!(entry.5, 145_203);
        assert_eq!(entry.6, "xudt");
        assert!(cache.get(&([0x33; 32], output_index)).is_some());
    }

    #[test]
    fn test_collect_committed_proposal_ids_uses_first_10_bytes_and_skips_cellbase() {
        let tx1 = dummy_tx_data([0x11; 32], false, vec![], vec![], vec![], vec![]);
        let tx2 = dummy_tx_data([0x22; 32], false, vec![], vec![], vec![], vec![]);
        let tx3_cellbase = dummy_tx_data([0x33; 32], true, vec![], vec![], vec![], vec![]);

        let ids = collect_committed_proposal_ids(&[tx1, tx2, tx3_cellbase]);

        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0], "11111111111111111111");
        assert_eq!(ids[1], "22222222222222222222");
    }

    #[test]
    fn test_collect_committed_proposal_ids_deduplicates_identical_hashes() {
        let tx_a = dummy_tx_data([0x44; 32], false, vec![], vec![], vec![], vec![]);
        let tx_b = dummy_tx_data([0x44; 32], false, vec![], vec![], vec![], vec![]);

        let ids = collect_committed_proposal_ids(&[tx_a, tx_b]);

        assert_eq!(ids, vec!["44444444444444444444".to_string()]);
    }

    #[test]
    fn test_classify_unresolved_local_probe_marks_missing_everywhere() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store);

        let unresolved = vec![(vec![0x12; 32], 0i16)];
        let summary = classify_unresolved_local_probe(&writer, &unresolved, 5);

        assert_eq!(summary.sampled, 1);
        assert_eq!(summary.missing_everywhere, 1);
        assert_eq!(summary.tx_location_hits, 0);
        assert_eq!(summary.live_hits, 0);
        assert_eq!(summary.consumed_hits, 0);
        assert_eq!(summary.store_errors, 0);
    }

    #[test]
    fn test_classify_unresolved_local_probe_marks_tx_location_exists_without_cell() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone());
        let tx_hash = vec![0x34; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_tx_hash_map(&tx_hash, 42, 0);
        batch.put_tx_index(42, 0, &dummy_tx_index_entry());
        batch.commit().unwrap();

        let unresolved = vec![(tx_hash, 1i16)];
        let summary = classify_unresolved_local_probe(&writer, &unresolved, 5);

        assert_eq!(summary.sampled, 1);
        assert_eq!(summary.tx_location_hits, 1);
        assert_eq!(summary.missing_everywhere, 0);
        assert_eq!(summary.live_hits, 0);
        assert_eq!(summary.consumed_hits, 0);
        assert_eq!(summary.store_errors, 0);
    }

    #[test]
    fn test_parser_unresolved_retry_defaults() {
        assert_eq!(PARSER_UNRESOLVED_RETRY_DELAY_MS, 500);
        assert_eq!(PARSER_UNRESOLVED_MAX_RETRIES, 240);
    }

    #[test]
    fn test_should_abort_unresolved_retry_on_epoch_change() {
        assert!(!should_abort_unresolved_retry_on_epoch_change(10, 10));
        assert!(should_abort_unresolved_retry_on_epoch_change(10, 11));
    }

    #[test]
    fn test_startup_header_gap_fail_fast_message_requires_rebuild() {
        let msg = startup_header_gap_fail_fast_message(123, 500, Some(600), Some(590));
        assert!(msg.contains("gap at block 123"));
        assert!(msg.contains("delete RocksDB and re-sync from genesis"));
        assert!(msg.contains("automatic gap replay is disabled"));
    }

    fn dummy_tx_data(
        hash: [u8; 32],
        is_cellbase: bool,
        inputs: Vec<crate::parser::transaction::ParsedInput>,
        cells: Vec<crate::parser::cell::ParsedCell>,
        witnesses: Vec<String>,
        outputs_data: Vec<String>,
    ) -> TxData {
        let inputs_count =
            i16::try_from(inputs.len()).expect("test helper inputs_count exceeds i16 range");
        let outputs_count =
            i16::try_from(cells.len()).expect("test helper outputs_count exceeds i16 range");
        let witnesses_count =
            i16::try_from(witnesses.len()).expect("test helper witnesses_count exceeds i16 range");
        TxData {
            hash,
            block_number: 0,
            block_hash: vec![],
            tx_index: 0,
            version: 0,
            inputs_count,
            outputs_count,
            witnesses_count,
            cell_deps_count: 0,
            header_deps_count: 0,
            is_cellbase,
            inputs,
            cells,
            witnesses,
            outputs_data,
            total_input_capacity: 0,
            total_output_capacity: 0,
            fee: 0,
            tx_size: 0,
            cycles: None,
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn test_bump_pipeline_reset_epoch_is_monotonic() {
        let epoch = AtomicU64::new(0);
        assert_eq!(bump_pipeline_reset_epoch(&epoch), 1);
        assert_eq!(bump_pipeline_reset_epoch(&epoch), 2);
        assert_eq!(epoch.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_ensure_hodl_tracker_state_consistency_rules() {
        assert!(ensure_hodl_tracker_state_consistent(None, 0).is_ok());
        let missing = ensure_hodl_tracker_state_consistent(None, 1).unwrap_err();
        assert!(missing.to_string().contains("missing HODL tracker state"));

        let empty = HodlTrackerState {
            capacity_by_date: vec![],
            date_transitions: vec![],
            holder_count: 0,
            last_snapshot_date: None,
        };
        let empty_err = ensure_hodl_tracker_state_consistent(Some(&empty), 100).unwrap_err();
        assert!(empty_err.to_string().contains("empty date_transitions"));

        let aligned = HodlTrackerState {
            capacity_by_date: vec![("20240101".to_string(), 1)],
            date_transitions: vec![(0, "20240101".to_string()), (100, "20240102".to_string())],
            holder_count: 1,
            last_snapshot_date: Some("20240102".to_string()),
        };
        assert!(ensure_hodl_tracker_state_consistent(Some(&aligned), 100).is_ok());

        let ahead = HodlTrackerState {
            capacity_by_date: vec![("20240101".to_string(), 1)],
            date_transitions: vec![(0, "20240101".to_string()), (101, "20240102".to_string())],
            holder_count: 1,
            last_snapshot_date: Some("20240102".to_string()),
        };
        let ahead_err = ensure_hodl_tracker_state_consistent(Some(&ahead), 100).unwrap_err();
        assert!(ahead_err.to_string().contains("ahead of sync tip"));
    }

    #[test]
    fn test_rebuild_hodl_tracker_from_state_resets_when_tip_is_zero() {
        let stale = HodlTrackerState {
            capacity_by_date: vec![("20240101".to_string(), 10)],
            date_transitions: vec![(200, "20240101".to_string())],
            holder_count: 7,
            last_snapshot_date: Some("20240101".to_string()),
        };

        let tracker = rebuild_hodl_tracker_from_state(Some(stale), 0).unwrap();
        let state = tracker.to_state();
        assert!(state.capacity_by_date.is_empty());
        assert!(state.date_transitions.is_empty());
        assert_eq!(state.holder_count, 0);
        assert!(state.last_snapshot_date.is_none());
    }

    #[test]
    fn test_rebuild_hodl_tracker_from_state_restores_when_tip_is_positive() {
        let persisted = HodlTrackerState {
            capacity_by_date: vec![("20240101".to_string(), 10)],
            date_transitions: vec![(1, "20240101".to_string())],
            holder_count: 3,
            last_snapshot_date: Some("20240101".to_string()),
        };

        let tracker = rebuild_hodl_tracker_from_state(Some(persisted), 1).unwrap();
        let state = tracker.to_state();
        assert_eq!(state.capacity_by_date.len(), 1);
        assert_eq!(state.date_transitions.len(), 1);
        assert_eq!(state.holder_count, 3);
        assert_eq!(state.last_snapshot_date, Some("20240101".to_string()));
    }

    #[test]
    fn test_ensure_compaction_mode_drain_guard_defers_when_pressure_high() {
        // Simulates the drain guard logic: when store is in bulk mode but should transition
        // to normal, if compaction_pressure reports high L0 files, we should NOT restore
        // normal mode yet.
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());

        // Enter bulk mode
        store.set_bulk_sync_compaction_options();
        assert!(store.is_bulk_sync_mode());

        // Check compaction_pressure on an empty store (should be 0/0/0 → drain OK)
        let (l0_files_max, compaction_pending_bytes, _imm) = store.compaction_pressure();
        // Empty store has no L0 files and no pending compaction
        assert!(l0_files_max < 10);
        assert!(compaction_pending_bytes < 2 * 1024 * 1024 * 1024);

        // Restore should succeed on empty store (drain condition met)
        store.restore_normal_compaction_options();
        assert!(!store.is_bulk_sync_mode());
    }

    #[test]
    fn test_ensure_compaction_mode_reentry() {
        // Verifies that after restoring normal mode, re-entering bulk mode works
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());

        // Enter bulk
        store.set_bulk_sync_compaction_options();
        assert!(store.is_bulk_sync_mode());

        // Exit bulk
        store.restore_normal_compaction_options();
        assert!(!store.is_bulk_sync_mode());

        // Re-enter bulk
        store.set_bulk_sync_compaction_options();
        assert!(store.is_bulk_sync_mode());

        // Exit again
        store.restore_normal_compaction_options();
        assert!(!store.is_bulk_sync_mode());
    }
}
