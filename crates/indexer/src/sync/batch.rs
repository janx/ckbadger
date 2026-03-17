#![allow(clippy::type_complexity)]

use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Timelike, Utc};
use ckbadger_common::LabelImportConfig;
use dashmap::DashMap;
use rayon::prelude::*;
use tracing::{debug, info, warn};

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::keys;
use ckbadger_store::types::{
    DailyActivityStats, DaoDailySnapshot, IdentityCollectionAggregate, ObjectTypeIndex,
    PositionedCellInfo, SporeTypeIndex, SOLE_SPORES_SENTINEL_COLLECTION,
};
use ckbadger_store::CkbadgerStore;

use crate::db::writer::dotbit::resolve_dotbit_tx_activity;
use crate::db::writer::nft_activity_acc::ObjectCollectionActivityAccumulator;
use crate::db::{BatchWriter, DaoWithdrawalContext};
use crate::parser::{
    BlockParser, CellParser, DaoParser, DotbitParser, MnftParser, ParsedClusterCell,
    ParsedSporeCell, ScriptParser, SporeParser, TransactionParser, UdtParser,
};

use crate::rpc::{BlockResponseWithCycles, CkbRpcClient};

use super::adaptive::*;
use super::checked_tx_count;
use super::dao_helpers::*;
use super::diagnostics::*;
use super::helpers::*;
use super::indexer::{Indexer, CACHE_INVALIDATION_INTERVAL};
use super::nft_helpers::*;
use super::sync_mode::*;
use super::token_helpers::*;
use super::types::{
    BatchWriteMetrics, CachedUdtCellInfo, DotbitTxActivityData, PreParsedNftData, TxData,
    UnresolvedLocalProbeSummary, UnresolvedRpcProbeSummary,
};
use super::undo::*;

pub(super) fn collect_missing_input_outpoints<T>(
    all_input_outpoints: &[(Vec<u8>, i16)],
    input_cell_info: &HashMap<(Vec<u8>, i16), PositionedCellInfo>,
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

fn wait_for_bulk_cells_commit_signal(cells_ready_rx: std::sync::mpsc::Receiver<()>) -> Result<()> {
    cells_ready_rx.recv().map_err(|_| {
        anyhow!("cells commit phase ended before T6b commit could observe cells readiness")
    })
}

fn resolve_live_dotbit_account_id_for_consume(
    key: &(Vec<u8>, i16),
    input_cell_info: &HashMap<(Vec<u8>, i16), PositionedCellInfo>,
    batch_cell_infos: &HashMap<(Vec<u8>, i16), PositionedCellInfo>,
    dotbit_map: &HashMap<(Vec<u8>, i16), Vec<u8>>,
) -> Option<Vec<u8>> {
    match input_cell_info
        .get(key)
        .or_else(|| batch_cell_infos.get(key))
    {
        Some(info) => info
            .type_code_hash
            .as_deref()
            .filter(|tc| DotbitParser::is_account_cell_type_script(tc))
            .and_then(|_| {
                resolve_dotbit_account_id_from_type_args_or_fallback(
                    info.type_args.as_deref(),
                    dotbit_map.get(key).cloned(),
                )
            }),
        None => dotbit_map.get(key).cloned(),
    }
}

/// Resolve consumed data_size and used_capacity from input cell info maps.
/// Fails fast if any input is unresolved — at this point all inputs should be resolved.
fn resolve_consumed_stats(
    tx_slice: &[TxData],
    input_cell_info: &HashMap<(Vec<u8>, i16), PositionedCellInfo>,
    batch_cell_infos: &HashMap<(Vec<u8>, i16), PositionedCellInfo>,
    block_number: i64,
) -> Result<(i64, i128)> {
    let mut data_size_consumed: i64 = 0;
    let mut used_capacity_consumed: i128 = 0;
    for tx in tx_slice.iter().filter(|tx| !tx.is_cellbase) {
        for input in &tx.inputs {
            let key = (
                input.previous_tx_hash.to_vec(),
                parsed_input_outpoint_index_i16(input.previous_output_index, "stats"),
            );
            let info = input_cell_info
                .get(&key)
                .or_else(|| batch_cell_infos.get(&key))
                .ok_or_else(|| {
                    anyhow!(
                        "unresolved input cell info for stats accumulation: block={}, tx_hash=0x{}, idx={}",
                        block_number,
                        short_tx_hash(&key.0),
                        key.1
                    )
                })?;
            data_size_consumed += info.data_size as i64;
            used_capacity_consumed += i128::from(info.occupied_capacity);
        }
    }
    Ok((data_size_consumed, used_capacity_consumed))
}

fn build_activity_input_views(
    tx_data: &TxData,
    block_number: i64,
    input_cell_info: &HashMap<(Vec<u8>, i16), PositionedCellInfo>,
    batch_cell_infos: &HashMap<(Vec<u8>, i16), PositionedCellInfo>,
    dao_withdraw_outpoints: &HashSet<(Vec<u8>, i16)>,
    dao_compensations: &HashMap<(Vec<u8>, i16), i64>,
) -> Result<Vec<crate::db::writer::activities::InputCellView>> {
    if tx_data.is_cellbase {
        return Ok(Vec::new());
    }

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
            let is_dao_withdraw_request =
                dao_withdraw_outpoints.contains(&key);
            let dao_compensation = if is_dao_withdraw_request {
                dao_compensations.get(&key).copied()
            } else {
                None
            };

            Ok(crate::db::writer::activities::InputCellView {
                lock_script_hash: info.lock_script_hash.clone(),
                lock_code_hash: info.lock_code_hash.clone(),
                lock_hash_type: info.lock_hash_type,
                lock_args: info.lock_args.clone(),
                capacity: info.capacity,
                occupied_capacity: info.occupied_capacity,
                type_code_hash: info.type_code_hash.clone(),
                type_hash_type: info.type_hash_type,
                type_script_hash: info.type_script_hash.clone(),
                type_args: info.type_args.clone(),
                udt_amount: info.udt_amount,
                data: Vec::new(),
                is_dao_withdraw_request,
                dao_compensation,
            })
        })
        .collect()
}

/// Extract outpoints whose DAO status == 1 (withdraw request) from consumed_dao_map.
/// The returned set lets T_ACT classify inputs without per-input RocksDB reads.
fn dao_withdraw_outpoints_from_map(
    consumed_dao_map: &HashMap<(Vec<u8>, i16), (Vec<u8>, i16, String, i64, i16)>,
) -> HashSet<(Vec<u8>, i16)> {
    consumed_dao_map
        .iter()
        .filter(|(_, row)| row.4 == 1) // status == 1 means withdraw request
        .map(|(k, _)| k.clone())
        .collect()
}

/// Pre-compute DAO compensation for each withdraw-complete outpoint.
/// This allows the activity builder to include compensation in activities
/// without duplicating the DAO processing logic.
///
/// The `consumed_dao_map` key is the withdraw-request outpoint (tx_hash, output_index)
/// being consumed in Phase 2. The value tuple is
/// (original_deposit_tx_hash, original_deposit_output_index, capacity_str, deposit_block, status).
/// Only status==1 entries are withdraw completes.
fn pre_compute_dao_compensations(
    store: &CkbadgerStore,
    consumed_dao_map: &HashMap<(Vec<u8>, i16), (Vec<u8>, i16, String, i64, i16)>,
) -> Result<HashMap<(Vec<u8>, i16), i64>> {
    use crate::db::writer::dao::{calculate_dao_compensation_from_ar, extract_ar_from_dao};

    // Filter to status==1 entries only
    let withdraw_entries: Vec<_> = consumed_dao_map
        .iter()
        .filter(|(_, row)| row.4 == 1)
        .collect();

    if withdraw_entries.is_empty() {
        return Ok(HashMap::new());
    }

    // For each withdraw-complete entry, look up the DaoDepositCacheEntry
    // to get withdraw_request_block (needed for AR lookup).
    // The deposit is keyed by (original_tx_hash, original_output_index).
    let mut blocks_needed: HashSet<i64> = HashSet::new();
    let mut entries_with_request_block: Vec<(
        &(Vec<u8>, i16), // withdraw outpoint key (for result map)
        i64,             // capacity
        i64,             // deposit_block
        i64,             // withdraw_request_block
    )> = Vec::new();

    for (withdraw_key, (orig_tx_hash, orig_output_index, capacity_str, deposit_block, _status)) in
        &withdraw_entries
    {
        let capacity: i64 = capacity_str.parse().map_err(|e| {
            anyhow!(
                "invalid DAO capacity string in compensation pre-compute: value='{}', error={}",
                capacity_str,
                e
            )
        })?;

        // Look up the deposit entry to get withdraw_request_block
        let outpoint_key = keys::encode_outpoint(orig_tx_hash, *orig_output_index);
        let request_block = if let Some(value) =
            store.get_cf(store.cf_dao_deposits(), &outpoint_key)?
        {
            let entry: ckbadger_store::types::DaoDepositCacheEntry =
                bincode::deserialize(&value).map_err(|e| {
                    anyhow!(
                        "failed to deserialize DAO deposit for compensation pre-compute: outpoint=0x{}:{}, error={}",
                        hex::encode(orig_tx_hash),
                        orig_output_index,
                        e
                    )
                })?;
            match entry.withdraw_request_block {
                Some(b) => b,
                None => continue, // shouldn't happen for status==1, skip
            }
        } else {
            continue; // deposit not found in store, skip
        };

        blocks_needed.insert(*deposit_block);
        blocks_needed.insert(request_block);
        entries_with_request_block.push((withdraw_key, capacity, *deposit_block, request_block));
    }

    // Batch-fetch DAO header fields for all needed blocks
    let blocks_vec: Vec<i64> = blocks_needed.into_iter().collect();
    let dao_fields = store.get_dao_fields_batch(&blocks_vec)?;

    // Compute compensations
    let mut compensations = HashMap::new();
    for (withdraw_key, capacity, deposit_block, request_block) in entries_with_request_block {
        let deposit_dao = match dao_fields.get(&deposit_block) {
            Some(d) => d,
            None => continue,
        };
        let withdraw_dao = match dao_fields.get(&request_block) {
            Some(d) => d,
            None => continue,
        };
        let ar_deposit = match extract_ar_from_dao(deposit_dao) {
            Some(ar) => ar,
            None => continue,
        };
        let ar_withdraw = match extract_ar_from_dao(withdraw_dao) {
            Some(ar) => ar,
            None => continue,
        };
        match calculate_dao_compensation_from_ar(capacity, ar_deposit, ar_withdraw) {
            Ok(compensation) => {
                compensations.insert(withdraw_key.clone(), compensation);
            }
            Err(_) => continue,
        }
    }

    Ok(compensations)
}

fn apply_object_collection_activity_count_deltas_with_pending(
    store: &CkbadgerStore,
    batch: &mut StoreBatch,
    deltas: HashMap<Vec<u8>, i64>,
    pending_aggregates: &HashMap<Vec<u8>, ckbadger_store::types::ObjectCollectionAggregate>,
    pending_cluster_ids: &HashSet<Vec<u8>>,
) -> Result<()> {
    if deltas.is_empty() {
        return Ok(());
    }

    let mut aggregates: HashMap<Vec<u8>, ckbadger_store::types::ObjectCollectionAggregate> =
        HashMap::new();

    for (collection_id, delta) in deltas {
        if delta == 0 {
            continue;
        }
        let agg = if let Some(cached) = aggregates.get_mut(&collection_id) {
            cached
        } else {
            let loaded = if let Some(pending) = pending_aggregates.get(&collection_id) {
                Some(pending.clone())
            } else {
                store.get_object_collection_aggregate(&collection_id)?
            };
            match loaded {
                Some(loaded) => aggregates.entry(collection_id.clone()).or_insert(loaded),
                None => {
                    if store.get_cluster_aggregate(&collection_id)?.is_some()
                        || pending_cluster_ids.contains(&collection_id)
                    {
                        // Spore cluster activities share the same append-only CF but do not
                        // belong to nft_collection_agg.
                        continue;
                    }
                    bail!(
                        "missing collection aggregate while applying activity_count delta: collection_id=0x{}",
                        hex::encode(&collection_id)
                    );
                }
            }
        };

        let next = agg.activities_count.checked_add(delta).ok_or_else(|| {
            anyhow!(
                "nft collection activities_count overflow: collection_id=0x{}, current={}, delta={}",
                hex::encode(&collection_id),
                agg.activities_count,
                delta
            )
        })?;
        if next < 0 {
            bail!(
                "nft collection activities_count underflow: collection_id=0x{}, current={}, delta={}",
                hex::encode(&collection_id),
                agg.activities_count,
                delta
            );
        }
        agg.activities_count = next;
    }

    for (collection_id, agg) in aggregates {
        batch.put_object_collection_aggregate(&collection_id, &agg);
    }
    Ok(())
}

fn apply_identity_collection_activity_count_deltas(
    store: &CkbadgerStore,
    batch: &mut StoreBatch,
    deltas: HashMap<Vec<u8>, i64>,
    pending_identity_aggs: &HashMap<Vec<u8>, IdentityCollectionAggregate>,
) -> Result<()> {
    if deltas.is_empty() {
        return Ok(());
    }
    for (collection_id, delta) in deltas {
        if delta == 0 {
            continue;
        }
        let mut agg = if let Some(pending) = pending_identity_aggs.get(&collection_id) {
            pending.clone()
        } else {
            store
                .get_identity_collection_aggregate(&collection_id)?
                .ok_or_else(|| {
                    anyhow!(
                        "missing identity collection aggregate while applying activity_count delta: collection_id=0x{}",
                        hex::encode(&collection_id)
                    )
                })?
        };
        let next = agg.activities_count.checked_add(delta).ok_or_else(|| {
            anyhow!(
                "identity collection activities_count overflow: collection_id=0x{}, current={}, delta={}",
                hex::encode(&collection_id),
                agg.activities_count,
                delta
            )
        })?;
        if next < 0 {
            bail!(
                "identity collection activities_count underflow: collection_id=0x{}, current={}, delta={}",
                hex::encode(&collection_id),
                agg.activities_count,
                delta
            );
        }
        agg.activities_count = next;
        batch.put_identity_collection_aggregate(&collection_id, &agg);
    }
    Ok(())
}

fn should_consume_grouped_mnft_token(
    last_batch_output_tx_index: Option<usize>,
    consume_tx_index: usize,
) -> bool {
    match last_batch_output_tx_index {
        Some(last_output_tx_index) => last_output_tx_index < consume_tx_index,
        None => true,
    }
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

        let type_script_hash = ScriptParser::compute_script_hash(type_script);
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

fn cached_to_udt_input_info(cached: &CachedUdtCellInfo) -> UdtInputInfo {
    (
        cached.type_script_hash.clone(),
        cached.type_code_hash.clone(),
        cached.type_hash_type,
        cached.type_args.clone(),
        cached.lock_script_hash.clone(),
        cached.amount,
        cached.standard.clone(),
    )
}

/// Resolve input UDT cells from cache first, then DB for misses.
/// Cache hits are safe during bulk sync: cells are consumed sequentially and same-batch outputs
/// are guaranteed valid. Cache is populated from both output parsing and DB results.
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

    // Check cache first, collect misses for DB lookup
    let mut result = HashMap::with_capacity(unique_outpoints.len());
    let mut cache_misses = Vec::new();

    for (tx_hash, idx) in &unique_outpoints {
        let key = tx_hash_key32(tx_hash, "resolve_input_udt cache lookup")?;
        if let Some(cached) = udt_cache.get(&(key, *idx)) {
            result.insert((tx_hash.clone(), *idx), cached_to_udt_input_info(&cached));
        } else {
            cache_misses.push((tx_hash.clone(), *idx));
        }
    }

    // Only send cache misses to DB
    if !cache_misses.is_empty() {
        let outpoint_refs: Vec<(&[u8], i16)> = cache_misses
            .iter()
            .map(|(h, i)| (h.as_slice(), *i))
            .collect();
        let db_results = writer.get_udt_cells_info_batch(&outpoint_refs)?;

        for ((tx_hash, idx), info) in &db_results {
            let key = tx_hash_key32(
                tx_hash,
                "resolve_input_udt_info_from_live_cells cache insert",
            )?;
            udt_cache.insert(
                (key, *idx),
                CachedUdtCellInfo {
                    type_script_hash: info.0.clone(),
                    type_code_hash: info.1.clone(),
                    type_hash_type: info.2,
                    type_args: info.3.clone(),
                    lock_script_hash: info.4.clone(),
                    amount: info.5,
                    standard: info.6.clone(),
                },
            );
        }

        result.extend(db_results);
    }

    if udt_cache.len() > UDT_CELL_CACHE_CAPACITY * 2 {
        udt_cache.clear();
    }

    Ok(result)
}

pub(super) fn classify_unresolved_local_probe(
    writer: &BatchWriter,
    unresolved_outpoints: &[(Vec<u8>, i16)],
    sample_limit: usize,
) -> UnresolvedLocalProbeSummary {
    let mut summary = UnresolvedLocalProbeSummary::default();
    let sampled = unresolved_outpoints.iter().take(sample_limit);
    let store = writer.store();
    let cells_store = writer.append_only_store();

    for (tx_hash, output_index) in sampled {
        summary.sampled += 1;
        let outpoint_label = format!("0x{}:{}", short_tx_hash(tx_hash), output_index);

        let live_exists = match store.get_cell(tx_hash, *output_index, cells_store) {
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

        let consumed_exists = match store.get_consumed_cell(tx_hash, *output_index, cells_store) {
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

pub(super) async fn collect_unresolved_rpc_probe(
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

pub(super) fn should_abort_unresolved_retry_on_epoch_change(
    batch_epoch: u64,
    current_epoch: u64,
) -> bool {
    batch_epoch != current_epoch
}

pub(super) fn load_optional_index_from_store<T, F>(
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

pub(super) fn load_latest_dao_daily_snapshot(
    store: &CkbadgerStore,
) -> Result<Option<DaoDailySnapshot>> {
    store
        .get_latest_dao_daily_snapshot()
        .context("failed to get latest dao daily snapshot while building cumulative snapshot")
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

pub(super) type ScriptUsageChanges = HashMap<(Vec<u8>, bool), (i64, i64, i128, i128, i128, i128)>;
pub(super) fn parse_blocks_parallel(
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
                let parsed = BlockParser::parse(block)?;
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
                            tx_index: tx_index as i32,
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

impl Indexer {
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

    pub(crate) async fn maybe_invalidate_chart_caches(&self, current_block: u64) {
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

    // === check_bulk_sync_completion, task submission ===

    pub(crate) async fn check_bulk_sync_completion(&self) {
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

            if let Err(e) = self.writer.store().update_sync_status(|status| {
                status.mark_bulk_sync_completed(chain_tip_i64);
            }) {
                warn!(
                    error = %e,
                    chain_tip = chain_tip_i64,
                    "Failed to persist bulk sync completion marker in sync status"
                );
            }

            let elapsed = self
                .writer
                .store()
                .get_sync_status()
                .ok()
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
            self.finalize_bulk_sync_perf_completed();

            self.cache_invalidator.invalidate_chart_caches().await;

            // Compaction mode transition is now handled by ensure_compaction_mode()
            // which runs after every batch and includes a drain guard.
        }

        self.was_bulk_sync_active
            .store(currently_bulk, Ordering::SeqCst);
    }

    pub(crate) fn maybe_start_label_import(&self) {
        if self.label_import_started.swap(true, Ordering::SeqCst) {
            debug!("Label import already started in this process, skipping");
            return;
        }

        let token_labels_path = self.config.token_labels_path.clone();
        let has_fs_labels = !token_labels_path.is_empty()
            && std::path::Path::new(&token_labels_path)
                .join("information")
                .exists();
        let network = self.config.network.clone();

        let core_store = Arc::clone(self.writer.store());

        if has_fs_labels {
            let config = LabelImportConfig {
                token_labels_path,
                ..Default::default()
            };
            tokio::spawn(async move {
                let result = tokio::task::spawn_blocking(move || {
                    crate::label_import::run_label_import_staged(core_store.as_ref(), &config)
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
        } else {
            info!("No filesystem token-labels found, using bundled label data");
            tokio::spawn(async move {
                let result = tokio::task::spawn_blocking(move || {
                    crate::label_import::run_label_import_bundled(core_store.as_ref(), &network)
                })
                .await;

                match result {
                    Ok(Ok(summary)) => info!(
                        "Background bundled label import finished: {} UDT, {} scripts, {} errors",
                        summary.udt_labels_imported,
                        summary.script_labels_imported,
                        summary.errors.len()
                    ),
                    Ok(Err(e)) => warn!("Background bundled label import failed: {}", e),
                    Err(e) => warn!("Background bundled label import task panicked: {}", e),
                }
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn write_parsed_batch(
        &self,
        blocks: &[BlockResponseWithCycles],
        all_parsed_blocks: &[crate::parser::block::ParsedBlock],
        all_tx_data: Vec<TxData>,
        input_cell_info: HashMap<(Vec<u8>, i16), PositionedCellInfo>,
        batch_cell_infos: HashMap<(Vec<u8>, i16), PositionedCellInfo>,
        address_balance_changes: HashMap<Vec<u8>, (i128, i32, i32, i64, i64, Vec<u8>, i128)>,
        script_usage_changes: ScriptUsageChanges,
        script_daily_changes: HashMap<(Vec<u8>, bool, u32), (i128, i128)>,
        token_daily_changes: HashMap<(Vec<u8>, u32), (i128, i128)>,
        spore_type_index_changes: HashMap<Vec<u8>, SporeTypeIndex>,
        spore_daily_changes: HashMap<(Vec<u8>, u32), (i128, i128)>,
        cluster_daily_changes: HashMap<(Vec<u8>, u32), (i128, i128)>,
        object_type_index_changes: HashMap<Vec<u8>, ObjectTypeIndex>,
        object_daily_changes: HashMap<(Vec<u8>, u32), (i128, i128)>,
        pre_parsed_spore_data: Vec<(Vec<(usize, ParsedSporeCell)>, Vec<ParsedClusterCell>)>,
        pre_parsed_nft_data: PreParsedNftData,
        chain_tip: u64,
    ) -> Result<BatchWriteMetrics> {
        if all_parsed_blocks.is_empty() {
            return Ok(BatchWriteMetrics::default());
        }

        let first_block = all_parsed_blocks.first().map(|b| b.number).unwrap_or(0);
        let last_block = all_parsed_blocks.last().map(|b| b.number).unwrap_or(0);
        let end_block = last_block as u64;
        let bulk_sync_mode = is_effective_bulk_sync_batch(
            chain_tip,
            end_block,
            self.config.bulk_sync_threshold,
            self.bulk_sync_allowed.load(Ordering::SeqCst),
        );

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
        let txs_for_batch: Vec<&TxData> = all_tx_data.iter().collect();

        for tx_data in &all_tx_data {
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

        // Collect unique code_hashes for batch-level detector pre-filtering.
        // This allows entire detectors to be skipped when none of their
        // protocol scripts appear anywhere in the batch.
        let mut batch_lock_code_hashes: HashSet<[u8; 32]> = HashSet::new();
        let mut batch_type_code_hashes: HashSet<[u8; 32]> = HashSet::new();

        // Collect from output cells
        for tx_data in &all_tx_data {
            for cell in &tx_data.cells {
                if cell.lock_code_hash.len() == 32 {
                    let mut h = [0u8; 32];
                    h.copy_from_slice(&cell.lock_code_hash);
                    batch_lock_code_hashes.insert(h);
                }
                if let Some(ref tc) = cell.type_code_hash {
                    if tc.len() == 32 {
                        let mut h = [0u8; 32];
                        h.copy_from_slice(tc);
                        batch_type_code_hashes.insert(h);
                    }
                }
            }
        }

        // Collect from resolved input cells (DB-fetched and same-batch)
        for info in input_cell_info.values().chain(batch_cell_infos.values()) {
            if info.lock_code_hash.len() == 32 {
                let mut h = [0u8; 32];
                h.copy_from_slice(&info.lock_code_hash);
                batch_lock_code_hashes.insert(h);
            }
            if let Some(ref tc) = info.type_code_hash {
                if tc.len() == 32 {
                    let mut h = [0u8; 32];
                    h.copy_from_slice(tc);
                    batch_type_code_hashes.insert(h);
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
                (
                    prefetched_input_udt_info,
                    prefetched_batch_udt_cells,
                    prefetched_udt_tx_infos,
                    prefetched_activity_token_cache,
                ),
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
                                    let tx_count_for_block =
                                        checked_tx_count(parsed.transactions_count, parsed.number)
                                            .expect("transactions_count must be non-negative");
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

                                // Pre-collect unique type hashes across all blocks for batch lookup.
                                // Includes both outputs (for UDT parsing) and consumed inputs
                                // (for activity token enrichment) so a single get_tokens_batch
                                // call serves both consumers.
                                let mut unique_type_hashes: HashSet<Vec<u8>> = HashSet::new();
                                for block_response in blocks.iter() {
                                    for tx in &block_response.block.transactions {
                                        for output in &tx.outputs {
                                            if let Some(type_script) = output.type_.as_ref() {
                                                let hash =
                                                    ScriptParser::compute_script_hash(type_script);
                                                unique_type_hashes.insert(hash);
                                            }
                                        }
                                    }
                                }
                                for tx_data in all_tx_data.iter() {
                                    if tx_data.is_cellbase {
                                        continue;
                                    }
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
                                            if let Some(tsh) = &info.type_script_hash {
                                                unique_type_hashes.insert(tsh.clone());
                                            }
                                        }
                                    }
                                }
                                let type_hash_vec: Vec<Vec<u8>> =
                                    unique_type_hashes.into_iter().collect();
                                let token_batch_result = writer
                                    .store()
                                    .get_tokens_batch(&type_hash_vec)
                                    .unwrap_or_else(|e| {
                                        panic!("UDT batch token prefetch failed: {}", e)
                                    });
                                // Build standard-only map for UDT parsing
                                let token_standard_map: HashMap<Vec<u8>, Option<String>> =
                                    token_batch_result
                                        .iter()
                                        .map(|(hash, info)| {
                                            (
                                                hash.clone(),
                                                info.as_ref().map(|t| t.standard.clone()),
                                            )
                                        })
                                        .collect();
                                // Build symbol+decimals cache for activity enrichment
                                let prefetched_activity_token_cache: HashMap<
                                    Vec<u8>,
                                    (Option<String>, Option<u8>),
                                > = token_batch_result
                                    .into_iter()
                                    .filter_map(|(hash, info)| {
                                        let info = info?;
                                        let decimals =
                                            info.decimals.and_then(|d| u8::try_from(d).ok());
                                        let symbol = info.symbol.or(info.name);
                                        Some((hash, (symbol, decimals)))
                                    })
                                    .collect();

                                let mut block_tx_idx = 0usize;
                                for (block_idx, block_response) in blocks.iter().enumerate() {
                                    let parsed = &all_parsed_blocks[block_idx];
                                    let tx_count_for_block =
                                        checked_tx_count(parsed.transactions_count, parsed.number)
                                            .expect("transactions_count must be non-negative");
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
                                        for (output_index, udt_cell) in parse_udt_cells_with_store_fallback_inner(tx, |hash| {
                                            Ok(token_standard_map.get(hash).and_then(|o| o.clone()))
                                        })
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

                                (
                                    input_udt_info,
                                    batch_udt_cells,
                                    udt_tx_contexts,
                                    prefetched_activity_token_cache,
                                )
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
                (
                    HashMap::new(),
                    (HashMap::new(), HashMap::new(), Vec::new(), HashMap::new()),
                ),
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
        let mut thread_times: Option<[f64; 10]> = None;
        let mut daily_activity_accum: HashMap<String, DailyActivityStats> = HashMap::new();
        let mut daily_activity_addrs: HashMap<String, HashSet<[u8; 32]>> = HashMap::new();
        let mut hourly_activity_accum: HashMap<String, DailyActivityStats> = HashMap::new();
        let mut hourly_activity_addrs: HashMap<String, HashSet<[u8; 32]>> = HashMap::new();
        let mut deferred_identity_deltas: HashMap<Vec<u8>, i64> = HashMap::new();
        let mut deferred_object_deltas: HashMap<Vec<u8>, i64> = HashMap::new();
        let mut data_batch = StoreBatch::new(self.writer.store());
        if bulk_sync_mode {
            // Parallel write path: each thread writes to its own StoreBatch and commits independently.
            // DAO/UDT/addr/script DB reads are pre-fetched above via rayon::join, so threads only do writes.
            // Independent batches let all threads run fully in parallel; the RocksDB write
            // group overhead (~2ms) is negligible.
            let store = self.writer.store();
            let append_only_store = &self.append_only_store;
            let writer = &self.writer;
            let dao_withdraw_outpoints = dao_withdraw_outpoints_from_map(&consumed_dao_map);
            let dao_compensations = pre_compute_dao_compensations(store, &consumed_dao_map)?;

            let protocol_detectors: Vec<Box<dyn crate::db::writer::activities::ProtocolDetector>> =
                vec![
                    Box::new(crate::db::writer::rgbpp_detector::RgbppDetector::new(
                        self.config.is_mainnet(),
                    ))
                        as Box<dyn crate::db::writer::activities::ProtocolDetector>,
                    Box::new(crate::db::writer::fiber_detector::FiberDetector::new(
                        self.config.is_mainnet(),
                    )),
                    Box::new(crate::db::writer::stablepp_detector::StableppDetector::new(
                        self.config.is_mainnet(),
                    )),
                    Box::new(crate::db::writer::utxoswap_detector::UtxoSwapDetector::new(
                        self.config.is_mainnet(),
                    )),
                ]
                .into_iter()
                .filter(|d| d.might_apply_batch(&batch_lock_code_hashes, &batch_type_code_hashes))
                .collect();

            // Write a batch-in-progress marker BEFORE worker threads start.
            // If a crash occurs after workers commit but before finalize,
            // startup will detect this marker and trigger cleanup.
            {
                let marker_value = format!("{}-{}", first_block, last_block);
                store.put_cf(
                    store.cf_sync_meta(),
                    ckbadger_store::keys::sync_meta_keys::BULK_BATCH_IN_PROGRESS,
                    marker_value.as_bytes(),
                )?;
            }
            let (cells_ready_tx, cells_ready_rx) = std::sync::mpsc::channel();

            let tt;
            (
                batch_stats,
                tt,
                write_commit_ms,
                daily_activity_accum,
                daily_activity_addrs,
                hourly_activity_accum,
                hourly_activity_addrs,
                deferred_identity_deltas,
                deferred_object_deltas,
            ) = std::thread::scope(
                |s| -> Result<(
                    BatchStats,
                    [f64; 10],
                    f64,
                    HashMap<String, DailyActivityStats>,
                    HashMap<String, HashSet<[u8; 32]>>,
                    HashMap<String, DailyActivityStats>,
                    HashMap<String, HashSet<[u8; 32]>>,
                    HashMap<Vec<u8>, i64>,
                    HashMap<Vec<u8>, i64>,
                )> {
                    // T1: Cells + Consumption (cell data only)
                    // CFs: CELLS (append-only), LIVE_CELLS, CONSUMED_CELLS (domain)
                    let h1 = s.spawn(|| -> Result<(f64, f64)> {
                        let t = Instant::now();
                        let cells_ready_tx = cells_ready_tx;
                        let mut domain_batch = StoreBatch::new(store);
                        let mut cells_batch = StoreBatch::new(append_only_store);
                        if !all_cells.is_empty() {
                            writer.insert_cells_batch(
                                &all_cells,
                                &batch_cell_infos,
                                &mut domain_batch,
                                &mut cells_batch,
                                true,
                            )?;
                        }
                        if !all_consumptions.is_empty() {
                            writer.consume_cells_batch_preloaded(
                                &all_consumptions,
                                &input_cell_info,
                                &batch_cell_infos,
                                &mut domain_batch,
                                true,
                            )?;
                        }
                        // Commit append-only (cell payloads) BEFORE domain (live markers).
                        // If crash between these two commits, orphaned payloads in append-only
                        // are harmless (content-addressed, idempotent), whereas dangling live
                        // markers without payloads would violate the split-store invariant.
                        let cells_commit_ms = commit_phase_no_wal(
                            "T1_cells_append",
                            first_block,
                            last_block,
                            cells_batch,
                        )?;
                        let commit_ms = commit_phase_no_wal(
                            "T1_cells_domain",
                            first_block,
                            last_block,
                            domain_batch,
                        )?;
                        // If the receiver is already gone, T6b has already failed.
                        // Preserve that original error instead of replacing it here.
                        let _ = cells_ready_tx.send(());
                        Ok((
                            t.elapsed().as_secs_f64() * 1000.0,
                            commit_ms + cells_commit_ms,
                        ))
                    });

                    // T1b: Cell Indexes (lock + type, puts + deletes)
                    // CFs: CELL_BY_LOCK, CELL_BY_LOCK_CODE, CELL_BY_TYPE, CELL_BY_TYPE_CODE
                    let h1b = s.spawn(|| -> Result<(f64, f64)> {
                        let t = Instant::now();
                        let mut batch = StoreBatch::new(store);
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
                            commit_phase_no_wal("T1b_cell_idx", first_block, last_block, batch)?;
                        Ok((t.elapsed().as_secs_f64() * 1000.0, commit_ms))
                    });

                    // T2: Transactions + Address Balances + Script Usage + Analytics + Addr TX index
                    // CFs: TX_INDEX, TX_HASH_MAP, ADDR_BALANCE, SCRIPT_INFO, STATS_SCRIPT, STATS_TOKEN, STATS_SPORE, STATS_NFT, ADDR_TX
                    let h2 = s.spawn(|| -> Result<(f64, f64)> {
                        let t = Instant::now();
                        let mut batch = StoreBatch::new(store);
                        let mut append_history_batch = StoreBatch::new(store);
                        // Bulk sync: skip undo journal (BULK_SYNC.md rules 5-7)
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
                                &mut batch,
                            )?;
                        }
                        if !script_daily_changes.is_empty() {
                            writer.update_script_daily_deltas_batch(
                                &script_daily_changes,
                                &mut batch,
                            )?;
                        }
                        if !token_daily_changes.is_empty() {
                            writer.update_token_daily_deltas_batch(
                                &token_daily_changes,
                                &mut batch,
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
                                &mut batch,
                            )?;
                        }
                        if !object_type_index_changes.is_empty() {
                            writer.update_object_type_index_batch(
                                &object_type_index_changes,
                                &mut batch,
                            )?;
                        }
                        if !object_daily_changes.is_empty() {
                            writer.update_object_daily_deltas_batch(
                                &object_daily_changes,
                                &mut batch,
                            )?;
                        }
                        if !cluster_daily_changes.is_empty() {
                            writer.update_cluster_daily_deltas_batch(
                                &cluster_daily_changes,
                                &mut batch,
                            )?;
                        }
                        // Bulk sync: skip undo journal (BULK_SYNC.md rules 5-7)
                        for (lock_hash, block_num, tx_idx, tx_hash) in &addr_tx_entries {
                            append_history_batch
                                .put_addr_tx(lock_hash, *block_num, *tx_idx, tx_hash);
                        }
                        let mut commit_ms =
                            commit_phase_no_wal("T2_txs_addr", first_block, last_block, batch)?;
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
                        let tx_count_for_block = checked_tx_count(parsed.transactions_count, parsed.number)?;
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
                        (Vec<u8>, i16, String, i64, i16),
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
                            let tx_count_for_block = checked_tx_count(parsed.transactions_count, parsed.number)?;
                            let tx_slice =
                                &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                            block_tx_idx += tx_count_for_block;
                            for tx_data in tx_slice {
                                if tx_data.is_cellbase || tx_data.inputs.is_empty() {
                                    continue;
                                }
                                let mut consumed_deposits: Vec<(
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
                    let h6a = s.spawn(|| -> Result<(f64, f64, HashMap<Vec<u8>, i64>)> {
                        if skip_spore {
                            return Ok((0.0, 0.0, HashMap::new()));
                        }
                        let t = Instant::now();
                        let mut batch = StoreBatch::new(store);
                        let mut activity_batch = StoreBatch::new(store);
                        let mut spore_state = writer.new_spore_batch_state();
                        let mut spore_activity_acc = ObjectCollectionActivityAccumulator::new();
                        let mut identity_activity_acc = ObjectCollectionActivityAccumulator::new();
                        let mut identity_activity_batch = StoreBatch::new(store);
                        let mut batch_spore_latest_create: HashMap<Vec<u8>, usize> = HashMap::new();
                        let mut block_tx_idx = 0usize;
                        for (block_idx, _block_response) in blocks.iter().enumerate() {
                            let parsed = &all_parsed_blocks[block_idx];
                            let tx_count_for_block =
                                checked_tx_count(parsed.transactions_count, parsed.number)?;
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
                                for &(output_index, ref spore) in pre_spores.iter() {
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
                                    batch_spore_latest_create
                                        .entry(spore.spore_id.clone())
                                        .and_modify(|idx| *idx = (*idx).max(tx_global_index))
                                        .or_insert(tx_global_index);
                                    if spore.is_did {
                                        identity_activity_acc.record(
                                            &DID_CKB_SENTINEL_COLLECTION,
                                            &tx_data.hash,
                                            &spore.spore_id,
                                            &parsed.hash,
                                            parsed.number,
                                            checked_usize_to_i32(tx_idx, "tx_idx"),
                                            ts_ms,
                                            true,
                                        );
                                    } else {
                                        let cid = spore
                                            .cluster_id
                                            .as_deref()
                                            .unwrap_or(&SOLE_SPORES_SENTINEL_COLLECTION);
                                        spore_activity_acc.record(
                                            cid,
                                            &tx_data.hash,
                                            &spore.spore_id,
                                            &parsed.hash,
                                            parsed.number,
                                            checked_usize_to_i32(tx_idx, "tx_idx"),
                                            ts_ms,
                                            true,
                                        );
                                    }
                                }
                            }
                            block_tx_idx += tx_count_for_block;
                        }

                        // Build tx metadata lookup for consumption processing
                        let mut tx_lookup: Vec<(usize, i64, i64, Vec<u8>)> =
                            Vec::with_capacity(all_tx_data.len());
                        for parsed in all_parsed_blocks.iter().take(blocks.len()) {
                            let tx_count_for_block = parsed.transactions_count as usize;
                            let ts_ms = parsed.timestamp.timestamp_millis();
                            for tx_idx in 0..tx_count_for_block {
                                tx_lookup.push((tx_idx, parsed.number, ts_ms, parsed.hash.clone()));
                            }
                        }

                        // Phase 2: Spore consumption from pre-identified events
                        for event in &pre_parsed_nft_data.consumed_spore {
                            let should_consume = should_consume_spore(
                                batch_spore_latest_create.get(&event.spore_id).copied(),
                                event.tx_global_index,
                            );
                            if should_consume {
                                if let Some(collection_id) = writer.consume_spore(
                                    &event.spore_id,
                                    event.block_number,
                                    &event.consuming_tx_hash,
                                    &mut batch,
                                    &mut spore_state,
                                )? {
                                    let (tx_idx, _, ts_ms, ref block_hash) =
                                        tx_lookup[event.tx_global_index];
                                    spore_activity_acc.record(
                                        &collection_id,
                                        &event.consuming_tx_hash,
                                        &event.spore_id,
                                        block_hash,
                                        event.block_number,
                                        checked_usize_to_i32(tx_idx, "tx_idx"),
                                        ts_ms,
                                        false, // is_creation = false for consumption
                                    );
                                }
                                // Note: consume_spore returns None for did:ckb identity spores
                                // (consumption is handled internally, no activity via accumulator — matches live sync behavior)
                            }
                        }

                        spore_activity_acc.flush(&mut activity_batch);
                        let identity_inserted =
                            identity_activity_acc.flush_identity(&mut identity_activity_batch);
                        let mut identity_activity_count_deltas: HashMap<Vec<u8>, i64> =
                            HashMap::new();
                        for (collection_id, _, _, _, _) in identity_inserted {
                            let delta = identity_activity_count_deltas
                                .entry(collection_id)
                                .or_insert(0);
                            *delta = delta.checked_add(1).ok_or_else(|| {
                                anyhow!("identity activity delta overflow in T6a pipeline")
                            })?;
                        }
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
                        if !identity_activity_batch.is_empty() {
                            commit_ms += commit_phase_no_wal(
                                "T6a_identity_activity",
                                first_block,
                                last_block,
                                identity_activity_batch,
                            )?;
                        }
                        Ok((
                            t.elapsed().as_secs_f64() * 1000.0,
                            commit_ms,
                            identity_activity_count_deltas,
                        ))
                    });

                    // T6b: mNFT + DotBit writes and DotBit consumption.
                    // CFs: NFT_DATA, NFT_COLLECTION_AGG, NFT_BY_COLLECTION, STATS (nft keys)
                    // Runs in parallel with T6a — mNFT/DotBit use independent CFs from Spore.
                    // All parsing is done in the parser stage (pre_parsed_nft_data);
                    // this thread only does DB writes.
                    let h6b = s.spawn(|| -> Result<(f64, f64, HashMap<Vec<u8>, i64>, HashMap<Vec<u8>, i64>)> {
                    let t = Instant::now();
                    let mut batch = StoreBatch::new(store);
                    let mut activity_batch = StoreBatch::new(store);
                    // Bulk sync: skip undo journal (BULK_SYNC.md rules 5-7)
                    let mut dotbit_state = writer.new_dotbit_batch_state();
                    let mut mnft_state = writer.new_mnft_batch_state();
                    let mut object_activity_acc = ObjectCollectionActivityAccumulator::new();

                    // Build tx_global_index → (tx_idx_in_block, block_number, ts_ms) lookup.
                    let mut tx_lookup: Vec<(usize, i64, i64, Vec<u8>)> =
                        Vec::with_capacity(all_tx_data.len());
                    for parsed in all_parsed_blocks.iter().take(blocks.len()) {
                        let tx_count_for_block = checked_tx_count(parsed.transactions_count, parsed.number)?;
                        let ts_ms = parsed.timestamp.timestamp_millis();
                        for tx_idx in 0..tx_count_for_block {
                            tx_lookup.push((tx_idx, parsed.number, ts_ms, parsed.hash.clone()));
                        }
                    }

                    // Phase 1: Insert mNFT issuers/classes/tokens from pre-parsed data.
                    for &(tx_gi, output_index, ref issuer) in &pre_parsed_nft_data.mnft_issuers {
                        let (_, block_number, _, _) = tx_lookup[tx_gi];
                        let output_index_i16 = i16::try_from(output_index).map_err(|_| {
                            anyhow!(
                                "mNFT issuer output index exceeds i16 range: block={}, tx_hash=0x{}, output_index={}",
                                block_number,
                                hex::encode(all_tx_data[tx_gi].hash),
                                output_index
                            )
                        })?;
                        writer.insert_mnft_issuer(
                            issuer,
                            &all_tx_data[tx_gi].hash,
                            output_index_i16,
                            block_number,
                            &mut batch,
                        )?;
                    }
                    for &(tx_gi, output_index, ref class) in &pre_parsed_nft_data.mnft_classes {
                        let (_, block_number, _, _) = tx_lookup[tx_gi];
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
                        let (tx_idx, block_number, ts_ms, ref block_hash) = tx_lookup[tx_gi];
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
                        object_activity_acc.record(
                            &token.class_id,
                            &all_tx_data[tx_gi].hash,
                            &token.token_id,
                            block_hash,
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
                        let (tx_idx, block_number, ts_ms, ref block_hash) = tx_lookup[tx_gi];
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
                                block_hash: block_hash.clone(),
                                tx_idx: checked_usize_to_i32(tx_idx, "tx_idx"),
                                timestamp_ms: ts_ms,
                            });
                        activity
                            .created_account_ids
                            .insert(account.account.account_id.clone());
                    }

                    // Phase 2: Consume DotBit accounts from pre-identified events
                    // (identification done in parser via input_cell_info
                    // type_code_hash + type_args, with DB outpoint fallback for
                    // historical cells with empty type_args).
                    for event in &pre_parsed_nft_data.consumed_dotbit {
                        if writer.consume_dotbit_account_with_state(
                            &event.account_id,
                            event.block_number,
                            &event.consuming_tx_hash,
                            &mut batch,
                            &mut dotbit_state,
                        )?.is_some() {
                            let tx_gi = event.tx_global_index;
                            let activity = dotbit_pipeline_activity
                                .entry(event.consuming_tx_hash)
                                .or_insert_with(|| {
                                    let (tx_idx, block_number, ts_ms, ref block_hash) =
                                        tx_lookup[tx_gi];
                                    let das_action = pre_parsed_nft_data
                                        .dotbit_tx_actions
                                        .get(&tx_gi)
                                        .cloned();
                                    DotbitTxActivityData {
                                        das_action,
                                        created_account_ids: HashSet::new(),
                                        consumed_account_ids: HashSet::new(),
                                        block_number,
                                        block_hash: block_hash.clone(),
                                        tx_idx: checked_usize_to_i32(tx_idx, "tx_idx"),
                                        timestamp_ms: ts_ms,
                                    }
                                });
                            activity.consumed_account_ids.insert(event.account_id.clone());
                        }
                    }

                    // Phase 2b: mNFT token consumption from pre-identified events
                    let mut batch_mnft_latest_create: HashMap<Vec<u8>, usize> = HashMap::new();
                    for &(tx_gi, _, ref token) in &pre_parsed_nft_data.mnft_tokens {
                        batch_mnft_latest_create
                            .entry(token.token_id.clone())
                            .and_modify(|idx| *idx = (*idx).max(tx_gi))
                            .or_insert(tx_gi);
                    }

                    for event in &pre_parsed_nft_data.consumed_mnft {
                        let should_consume = should_consume_mnft_token(
                            batch_mnft_latest_create.get(&event.token_id).copied(),
                            event.tx_global_index,
                        );
                        if should_consume {
                            if let Some(collection_id) = writer.consume_mnft_token_with_state(
                                &event.token_id,
                                event.block_number,
                                &event.consuming_tx_hash,
                                &mut batch,
                                &mut mnft_state,
                            )? {
                                let (tx_idx, _, ts_ms, ref block_hash) = tx_lookup[event.tx_global_index];
                                object_activity_acc.record(
                                    &collection_id,
                                    &event.consuming_tx_hash,
                                    &event.token_id,
                                    block_hash,
                                    event.block_number,
                                    checked_usize_to_i32(tx_idx, "tx_idx"),
                                    ts_ms,
                                    false, // is_creation = false for consumption
                                );
                            }
                        }
                    }

                    // Write .bit collection activities directly (bypassing accumulator)
                    let mut identity_activity_count_deltas: HashMap<Vec<u8>, i64> = HashMap::new();
                    for (tx_hash, activity) in &dotbit_pipeline_activity {
                        let inserted = resolve_dotbit_tx_activity(
                            activity.das_action.as_deref(),
                            &activity.created_account_ids,
                            &activity.consumed_account_ids,
                            tx_hash,
                            &activity.block_hash,
                            activity.block_number,
                            activity.tx_idx,
                            activity.timestamp_ms,
                            &mut activity_batch,
                        );
                        if inserted {
                            let delta = identity_activity_count_deltas
                                .entry(DOTBIT_SENTINEL_COLLECTION.to_vec())
                                .or_insert(0);
                            *delta = delta.checked_add(1).ok_or_else(|| {
                                anyhow!("dotbit identity activity delta overflow in T6b pipeline")
                            })?;
                        }
                    }
                    // Bulk sync: skip undo journal (BULK_SYNC.md rules 5-7)
                    let object_inserted = object_activity_acc.flush(&mut activity_batch);
                    let mut object_activity_count_deltas: HashMap<Vec<u8>, i64> = HashMap::new();
                    for (collection_id, _, _, _, _) in object_inserted {
                        let delta = object_activity_count_deltas
                            .entry(collection_id)
                            .or_insert(0);
                        *delta = delta.checked_add(1).ok_or_else(|| {
                            anyhow!("nft activity delta overflow in T6b pipeline")
                        })?;
                    }
                    // DotBit identities become user-visible only after T1 has committed
                    // append-only payloads and live markers, otherwise API can observe
                    // live `.bit` entries whose backing cells are not readable yet.
                    wait_for_bulk_cells_commit_signal(cells_ready_rx)?;
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
                    Ok((t.elapsed().as_secs_f64() * 1000.0, commit_ms, identity_activity_count_deltas, object_activity_count_deltas))
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
                        let mut prev_dao_cs: Option<(i128, i128)> = if let Some(first_block) =
                            all_parsed_blocks.first()
                        {
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
                        let mut same_batch_dao_deposits: HashMap<(Vec<u8>, i16), i64> =
                            HashMap::new();

                        let mut block_tx_idx = 0usize;
                        for parsed in all_parsed_blocks {
                            let block_date = ckbadger_common::block_date(parsed.timestamp);
                            accumulate_secondary_issuance_deltas(
                                &mut stats,
                                parsed,
                                block_date,
                                &mut prev_dao_cs,
                            )?;
                            let tx_count_for_block =
                                checked_tx_count(parsed.transactions_count, parsed.number)?;
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

                            let cells_created: i32 = tx_slice
                                .iter()
                                .map(|tx| {
                                    i32::try_from(tx.cells.len()).expect("output count exceeds i32")
                                })
                                .sum();
                            let cells_consumed: i32 = tx_slice
                                .iter()
                                .filter(|tx| !tx.is_cellbase)
                                .map(|tx| {
                                    i32::try_from(tx.inputs.len()).expect("input count exceeds i32")
                                })
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
                            let used_capacity_created: i128 = tx_slice
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
                            let (data_size_consumed, used_capacity_consumed) =
                                resolve_consumed_stats(
                                    tx_slice,
                                    &input_cell_info,
                                    &batch_cell_infos,
                                    parsed.number,
                                )?;

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
                                entry.4 =
                                    entry.4.checked_add(capacity_transferred).ok_or_else(|| {
                                        anyhow!(
                                            "daily capacity_transferred overflow: date={} block={}",
                                            block_date,
                                            parsed.number
                                        )
                                    })?;
                                entry.5 = entry.5.checked_add(used_capacity_created).ok_or_else(
                                    || {
                                        anyhow!(
                                        "daily used_capacity_created overflow: date={} block={}",
                                        block_date,
                                        parsed.number
                                    )
                                    },
                                )?;
                                entry.6 = entry.6.checked_add(used_capacity_consumed).ok_or_else(
                                    || {
                                        anyhow!(
                                        "daily used_capacity_consumed overflow: date={} block={}",
                                        block_date,
                                        parsed.number
                                    )
                                    },
                                )?;
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
                                entry.4 =
                                    entry.4.checked_add(capacity_transferred).ok_or_else(|| {
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
                                        *stats
                                            .epoch_time_dist
                                            .entry(bucket_minutes)
                                            .or_default() += 1;
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

                    // T_ACT: Activity builder + daily activity stats accumulation
                    // CFs: ACTIVITIES
                    type ActResult = (
                        f64,
                        f64,
                        HashMap<String, DailyActivityStats>,
                        HashMap<String, HashSet<[u8; 32]>>,
                        HashMap<String, DailyActivityStats>,
                        HashMap<String, HashSet<[u8; 32]>>,
                    );
                    let h_act = if !skip_activities {
                        Some(s.spawn(|| -> Result<ActResult> {
                            let t = Instant::now();
                            // Bulk sync: skip undo journal (BULK_SYNC.md rules 5-7)
                            let mut activity_batch = StoreBatch::new(store);
                            // Use pre-fetched token cache from bulk prefetch (eliminates
                            // a separate get_tokens_batch call that duplicated the UDT prefetch).
                            let token_info_cache = &prefetched_activity_token_cache;
                            let mut act_stats_accum: HashMap<String, DailyActivityStats> =
                                HashMap::new();
                            let mut act_stats_addrs: HashMap<String, HashSet<[u8; 32]>> =
                                HashMap::new();
                            let mut hourly_accum: HashMap<String, DailyActivityStats> =
                                HashMap::new();
                            let mut hourly_addrs: HashMap<String, HashSet<[u8; 32]>> =
                                HashMap::new();
                            let mut block_tx_idx = 0usize;
                            for parsed in all_parsed_blocks {
                                let tx_count =
                                    checked_tx_count(parsed.transactions_count, parsed.number)?;
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
                                                    td,
                                                    parsed.number,
                                                    &input_cell_info,
                                                    &batch_cell_infos,
                                                    &dao_withdraw_outpoints,
                                                    &dao_compensations,
                                                )?;
                                                Ok(crate::db::writer::activities::TxView {
                                                    tx_hash: &td.hash,
                                                    block_hash: &parsed.hash,
                                                    tx_index: td.tx_index,
                                                    block_number: parsed.number,
                                                    timestamp: parsed.timestamp.timestamp_millis(),
                                                    is_cellbase: td.is_cellbase,
                                                    inputs,
                                                    outputs: &td.cells,
                                                    witnesses: &td.witnesses,
                                                })
                                            },
                                        )
                                        .collect::<Result<Vec<_>>>()?;

                                let bundles =
                                    crate::db::writer::activities::build_activity_bundles_for_block_with_detectors(
                                        &tx_views,
                                        token_info_cache,
                                        &protocol_detectors,
                                    )?;

                                for bundle in bundles {
                                    for owner in &bundle.owners {
                                        // Accumulate daily activity stats
                                        let date =
                                            ckbadger_common::block_date_from_ms(bundle.timestamp)
                                                .format("%Y%m%d")
                                                .to_string();
                                        let day_stats =
                                            act_stats_accum.entry(date.clone()).or_default();
                                        BatchWriter::accumulate_owner_activity_stats(
                                            bundle.is_cellbase,
                                            owner,
                                            day_stats,
                                        );
                                        // Exclude coinbase from unique address count
                                        if !bundle.is_cellbase && owner.lock_hash.len() == 32 {
                                            let mut hash = [0u8; 32];
                                            hash.copy_from_slice(&owner.lock_hash);
                                            act_stats_addrs.entry(date).or_default().insert(hash);
                                        }

                                        // Accumulate hourly activity stats
                                        let hour = ckbadger_common::block_datetime_from_ms(
                                            bundle.timestamp,
                                        )
                                        .format("%Y%m%d%H")
                                        .to_string();
                                        let hour_stats =
                                            hourly_accum.entry(hour.clone()).or_default();
                                        BatchWriter::accumulate_owner_activity_stats(
                                            bundle.is_cellbase,
                                            owner,
                                            hour_stats,
                                        );
                                        if !bundle.is_cellbase && owner.lock_hash.len() == 32 {
                                            let mut hash = [0u8; 32];
                                            hash.copy_from_slice(&owner.lock_hash);
                                            hourly_addrs.entry(hour).or_default().insert(hash);
                                        }
                                    }
                                    activity_batch.put_tx_activity_bundle(&bundle);

                                    // Process Fiber channel lifecycle events
                                    crate::db::writer::fiber::process_fiber_channel_events(
                                        &mut activity_batch,
                                        store,
                                        &bundle,
                                    )?;
                                }
                            }
                            let mut commit_ms = 0.0;
                            if !activity_batch.is_empty() {
                                commit_ms += commit_phase_no_wal(
                                    "T_ACT_activities",
                                    first_block,
                                    last_block,
                                    activity_batch,
                                )?;
                            }
                            Ok((
                                t.elapsed().as_secs_f64() * 1000.0,
                                commit_ms,
                                act_stats_accum,
                                act_stats_addrs,
                                hourly_accum,
                                hourly_addrs,
                            ))
                        }))
                    } else {
                        None
                    };

                    // T_TRACK: Merged HODL wave + cell distribution trackers
                    // CFs: CF_SYNC_META (tracker states), CF_STATS (cell_distribution, address_cohort, hodl_wave)
                    let h_track = s.spawn(|| -> Result<f64> {
                        let t = Instant::now();
                        self.update_trackers(
                            all_parsed_blocks,
                            &all_tx_data,
                            &input_cell_info,
                            &batch_cell_infos,
                            &address_balance_changes,
                            &prefetched_addr_balances,
                        )?;
                        Ok(t.elapsed().as_secs_f64() * 1000.0)
                    });

                    let (t1_ms, t1_commit_ms) = h1.join().expect("T1 panicked")?;
                    let (t1b_ms, t1b_commit_ms) = h1b.join().expect("T1b panicked")?;
                    let (t2_ms, t2_commit_ms) = h2.join().expect("T2 panicked")?;
                    let (t4_ms, t4_commit_ms) = h4.join().expect("T4 panicked")?;
                    let (t5_ms, t5_commit_ms) = h5.join().expect("T5 panicked")?;
                    let (t6a_ms, t6a_commit_ms, t6a_identity_deltas) =
                        h6a.join().expect("T6a panicked")?;
                    let (t6b_ms, t6b_commit_ms, t6b_identity_deltas, t6b_object_deltas) =
                        h6b.join().expect("T6b panicked")?;
                    let (stats, t7_ms) = h7.join().expect("T7 panicked")?;
                    let (t_act_ms, t_act_commit_ms, act_accum, act_addrs, h_accum, h_addrs) =
                        match h_act {
                            Some(h) => {
                                let (ms, commit_ms, accum, addrs, h_accum, h_addrs) =
                                    h.join().expect("T_ACT panicked")?;
                                (ms, commit_ms, accum, addrs, h_accum, h_addrs)
                            }
                            None => (
                                0.0,
                                0.0,
                                HashMap::new(),
                                HashMap::new(),
                                HashMap::new(),
                                HashMap::new(),
                            ),
                        };
                    let t_track_ms = h_track.join().expect("T_TRACK panicked")?;

                    // Combine activity count deltas from T6a/T6b. These are NOT
                    // committed here — they are deferred and merged into the
                    // finalize batch so that a crash between worker commits and
                    // finalize cannot leave aggregates incremented without the
                    // corresponding block headers committed.
                    let combined_identity_deltas = {
                        let mut m = t6a_identity_deltas;
                        for (k, v) in t6b_identity_deltas {
                            let entry = m.entry(k).or_insert(0);
                            *entry = entry.checked_add(v).ok_or_else(|| {
                                anyhow!("combined identity activity delta overflow in pipeline")
                            })?;
                        }
                        m
                    };

                    let commit_total_ms = t1_commit_ms
                        + t1b_commit_ms
                        + t2_commit_ms
                        + t4_commit_ms
                        + t5_commit_ms
                        + t6a_commit_ms
                        + t6b_commit_ms
                        + t_act_commit_ms;
                    Ok((
                        stats,
                        [
                            t1_ms, t1b_ms, t2_ms, t4_ms, t5_ms, t6a_ms, t6b_ms, t7_ms, t_act_ms,
                            t_track_ms,
                        ],
                        commit_total_ms,
                        act_accum,
                        act_addrs,
                        h_accum,
                        h_addrs,
                        combined_identity_deltas,
                        t6b_object_deltas,
                    ))
                },
            )?;
            thread_times = Some(tt);
        } else {
            // Live sync: serial writes in a single batch
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
                let mut cells_batch = StoreBatch::new(&self.append_only_store);
                self.writer.insert_cells_batch(
                    &all_cells,
                    &batch_cell_infos,
                    &mut data_batch,
                    &mut cells_batch,
                    false,
                )?;
                if !cells_batch.is_empty() {
                    cells_batch.commit()?;
                }
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
            let mut append_history_batch = StoreBatch::new(self.writer.store());

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
            if !object_type_index_changes.is_empty() {
                self.writer
                    .update_object_type_index_batch(&object_type_index_changes, &mut data_batch)?;
            }
            if !object_daily_changes.is_empty() {
                self.writer.update_object_daily_deltas_batch(
                    &object_daily_changes,
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
                put_addr_tx(
                    &mut append_history_batch,
                    &mut append_undo_seq_by_block,
                    lock_hash,
                    *block_num,
                    *tx_idx,
                    tx_hash,
                );
            }

            // DAO withdraw-request outpoints for activity classification (no per-input DB reads).
            let dao_withdraw_outpoints: HashSet<(Vec<u8>, i16)>;
            // Pre-computed DAO compensations for activity entries.
            let dao_compensations: HashMap<(Vec<u8>, i16), i64>;

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
                    let tx_count_for_block =
                        checked_tx_count(parsed.transactions_count, parsed.number)?;
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
                    let tx_count_for_block =
                        checked_tx_count(parsed.transactions_count, parsed.number)?;
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
                dao_withdraw_outpoints = dao_withdraw_outpoints_from_map(&consumed_dao_map);
                dao_compensations =
                    pre_compute_dao_compensations(self.writer.store(), &consumed_dao_map)?;

                // Build a same-batch deposit map for deposits created in this
                // batch that may also be consumed within the same batch.
                let mut same_batch_dao_deposits: HashMap<
                    (Vec<u8>, i16),
                    (Vec<u8>, i16, String, i64, i16),
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
                        let tx_count_for_block =
                            checked_tx_count(parsed.transactions_count, parsed.number)?;
                        let tx_slice =
                            &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                        block_tx_idx += tx_count_for_block;
                        for tx_data in tx_slice {
                            if tx_data.is_cellbase || tx_data.inputs.is_empty() {
                                continue;
                            }
                            let mut consumed_deposits: Vec<(Vec<u8>, i16, String, i64, i16)> =
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
                    let tx_count_for_block =
                        checked_tx_count(parsed.transactions_count, parsed.number)?;
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

            let mut object_activity_batch = StoreBatch::new(self.writer.store());
            let mut object_activity_count_deltas: HashMap<Vec<u8>, i64> = HashMap::new();
            let mut identity_activity_batch = StoreBatch::new(self.writer.store());
            let mut identity_activity_count_deltas: HashMap<Vec<u8>, i64> = HashMap::new();

            let mut pending_object_collection_aggs = HashMap::new();
            let mut pending_cluster_ids = HashSet::new();
            let mut pending_identity_aggs: HashMap<Vec<u8>, IdentityCollectionAggregate> =
                HashMap::new();

            // Group C: NFT/Spore processing
            {
                let mut batch_spore_ids: HashSet<Vec<u8>> = HashSet::new();
                let mut batch_mnft_token_outpoints: HashMap<(Vec<u8>, i16), Vec<u8>> =
                    HashMap::new();
                let mut batch_mnft_last_output_tx_index: HashMap<Vec<u8>, usize> = HashMap::new();
                let mut batch_dotbit_outpoints: HashMap<(Vec<u8>, i16), Vec<u8>> = HashMap::new();
                let mut batch_dotbit_latest_create_order: HashMap<Vec<u8>, u64> = HashMap::new();
                let mut spore_state = self.writer.new_spore_batch_state();
                let mut dotbit_state = self.writer.new_dotbit_batch_state();
                let mut mnft_state = self.writer.new_mnft_batch_state();
                let mut object_activity_acc = ObjectCollectionActivityAccumulator::new();
                let mut identity_activity_acc = ObjectCollectionActivityAccumulator::new();
                let mut dotbit_tx_activity_data: HashMap<[u8; 32], DotbitTxActivityData> =
                    HashMap::new();
                // Cache DAS actions for all non-cellbase txs so consumption-only
                // txs can look up their action (not just txs with .bit outputs).
                let mut das_action_cache: HashMap<[u8; 32], String> = HashMap::new();
                let mut block_tx_idx = 0usize;
                for (block_idx, block_response) in blocks.iter().enumerate() {
                    let parsed = &all_parsed_blocks[block_idx];
                    let tx_count_for_block =
                        checked_tx_count(parsed.transactions_count, parsed.number)?;
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
                            for (output_index, ref spore) in
                                SporeParser::parse_spores_with_output_indices(tx)
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
                                if spore.is_did {
                                    identity_activity_acc.record(
                                        &DID_CKB_SENTINEL_COLLECTION,
                                        &tx_data.hash,
                                        &spore.spore_id,
                                        &parsed.hash,
                                        parsed.number,
                                        checked_usize_to_i32(tx_idx, "tx_idx"),
                                        ts_ms,
                                        true,
                                    );
                                } else {
                                    let cid = spore
                                        .cluster_id
                                        .as_deref()
                                        .unwrap_or(&SOLE_SPORES_SENTINEL_COLLECTION);
                                    object_activity_acc.record(
                                        cid,
                                        &tx_data.hash,
                                        &spore.spore_id,
                                        &parsed.hash,
                                        parsed.number,
                                        checked_usize_to_i32(tx_idx, "tx_idx"),
                                        ts_ms,
                                        true,
                                    );
                                }
                            }
                        }
                        for (output_index, issuer) in
                            MnftParser::parse_issuers_with_output_indices(tx)
                        {
                            let output_index_i16 = i16::try_from(output_index).map_err(|_| {
                                anyhow!(
                                    "mNFT issuer output index exceeds i16 range: block={}, tx_hash=0x{}, output_index={}",
                                    parsed.number,
                                    hex::encode(tx_data.hash),
                                    output_index
                                )
                            })?;
                            self.writer.insert_mnft_issuer(
                                &issuer,
                                &tx_data.hash,
                                output_index_i16,
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
                            batch_mnft_last_output_tx_index
                                .insert(token.token_id.clone(), tx_global_index);
                            object_activity_acc.record(
                                &token.class_id,
                                &tx_data.hash,
                                &token.token_id,
                                &parsed.hash,
                                parsed.number,
                                checked_usize_to_i32(tx_idx, "tx_idx"),
                                ts_ms,
                                true,
                            );
                        }
                        // Parse DAS action for all non-cellbase txs so
                        // consumption-only .bit txs can look up their action.
                        if !tx_data.is_cellbase {
                            if let Some(action) = DotbitParser::parse_das_action(&tx.witnesses) {
                                das_action_cache.insert(tx_data.hash, action);
                            }
                        }
                        let dotbit_accounts = DotbitParser::parse_accounts(tx)?;
                        if !dotbit_accounts.is_empty() {
                            let das_action = das_action_cache.get(&tx_data.hash).cloned();
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
                                    block_hash: parsed.hash.clone(),
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
                // (block_number, block_hash, consuming_tx_hash, dotbit_consume_order, tx_idx, ts_ms, tx_global_index)
                let mut outpoint_context: Vec<(i64, Vec<u8>, Vec<u8>, u64, i32, i64, usize)> =
                    Vec::new();
                let mut block_tx_idx = 0usize;
                for (block_idx, block_response) in blocks.iter().enumerate() {
                    let parsed = &all_parsed_blocks[block_idx];
                    let tx_count_for_block =
                        checked_tx_count(parsed.transactions_count, parsed.number)?;
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
                                parsed.hash.clone(),
                                tx_data.hash.to_vec(),
                                dotbit_consume_order,
                                checked_usize_to_i32(tx_idx, "tx_idx"),
                                parsed.timestamp.timestamp_millis(),
                                tx_global_index,
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
                            block_hash,
                            consuming_tx_hash,
                            dotbit_consume_order,
                            ctx_tx_idx,
                            ctx_ts_ms,
                            consume_tx_global_index,
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
                                        object_activity_acc.record(
                                            &coll_id,
                                            consuming_tx_hash,
                                            spore_id,
                                            block_hash,
                                            *block_number,
                                            *ctx_tx_idx,
                                            *ctx_ts_ms,
                                            false,
                                        );
                                    }
                                }
                            }
                            if let Some(token_id) = mnft_map.get(&key) {
                                let should_consume = should_consume_grouped_mnft_token(
                                    batch_mnft_last_output_tx_index.get(token_id).copied(),
                                    *consume_tx_global_index,
                                );
                                if should_consume {
                                    if let Some(coll_id) =
                                        self.writer.consume_mnft_token_with_state(
                                            token_id,
                                            *block_number,
                                            consuming_tx_hash,
                                            &mut data_batch,
                                            &mut mnft_state,
                                        )?
                                    {
                                        object_activity_acc.record(
                                            &coll_id,
                                            consuming_tx_hash,
                                            token_id,
                                            block_hash,
                                            *block_number,
                                            *ctx_tx_idx,
                                            *ctx_ts_ms,
                                            false,
                                        );
                                    }
                                }
                            }
                        }
                        // Match bulk precompute semantics: if resolved input metadata exists,
                        // only treat it as dotbit when the input cell itself is dotbit.
                        let dotbit_account_id = resolve_live_dotbit_account_id_for_consume(
                            &key,
                            &input_cell_info,
                            &batch_cell_infos,
                            &dotbit_map,
                        );
                        if let Some(account_id) = dotbit_account_id.as_ref() {
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
                                        das_action: das_action_cache.get(&tx_key).cloned(),
                                        created_account_ids: HashSet::new(),
                                        consumed_account_ids: HashSet::new(),
                                        block_number: *block_number,
                                        block_hash: block_hash.clone(),
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
                        &activity.block_hash,
                        activity.block_number,
                        activity.tx_idx,
                        activity.timestamp_ms,
                        &mut identity_activity_batch,
                    );
                    // identity collection activities are now in domain store;
                    // rollback deletes entries directly (no undo log needed)
                    if inserted {
                        let delta = identity_activity_count_deltas
                            .entry(DOTBIT_SENTINEL_COLLECTION.to_vec())
                            .or_insert(0);
                        *delta = delta.checked_add(1).ok_or_else(|| {
                            anyhow!(
                                "dotbit identity activity delta overflow while writing grouped batch"
                            )
                        })?;
                    }
                }
                // Object/identity collection activities are now in domain store;
                // rollback deletes entries directly (no undo log needed)
                for (collection_id, _block_number, _tx_idx, _block_hash, _tx_hash) in
                    object_activity_acc.flush(&mut object_activity_batch)
                {
                    let delta = object_activity_count_deltas
                        .entry(collection_id)
                        .or_insert(0);
                    *delta = delta.checked_add(1).ok_or_else(|| {
                        anyhow!("nft activity delta overflow while writing grouped batch")
                    })?;
                }
                for (collection_id, _block_number, _tx_idx, _block_hash, _tx_hash) in
                    identity_activity_acc.flush_identity(&mut identity_activity_batch)
                {
                    let delta = identity_activity_count_deltas
                        .entry(collection_id)
                        .or_insert(0);
                    *delta = delta.checked_add(1).ok_or_else(|| {
                        anyhow!("identity activity delta overflow while writing grouped batch")
                    })?;
                }

                mnft_state
                    .extend_pending_collection_aggregates(&mut pending_object_collection_aggs);
                spore_state.extend_pending_cluster_ids(&mut pending_cluster_ids);

                pending_identity_aggs.extend(
                    spore_state
                        .pending_identity_aggs()
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone())),
                );
                pending_identity_aggs.extend(
                    dotbit_state
                        .pending_identity_aggs()
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone())),
                );
            }
            apply_object_collection_activity_count_deltas_with_pending(
                self.writer.store(),
                &mut data_batch,
                object_activity_count_deltas,
                &pending_object_collection_aggs,
                &pending_cluster_ids,
            )?;
            apply_identity_collection_activity_count_deltas(
                self.writer.store(),
                &mut data_batch,
                identity_activity_count_deltas,
                &pending_identity_aggs,
            )?;

            // Activity writes (live sync)
            let protocol_detectors: Vec<Box<dyn crate::db::writer::activities::ProtocolDetector>> =
                vec![
                    Box::new(crate::db::writer::rgbpp_detector::RgbppDetector::new(
                        self.config.is_mainnet(),
                    ))
                        as Box<dyn crate::db::writer::activities::ProtocolDetector>,
                    Box::new(crate::db::writer::fiber_detector::FiberDetector::new(
                        self.config.is_mainnet(),
                    )),
                    Box::new(crate::db::writer::stablepp_detector::StableppDetector::new(
                        self.config.is_mainnet(),
                    )),
                    Box::new(crate::db::writer::utxoswap_detector::UtxoSwapDetector::new(
                        self.config.is_mainnet(),
                    )),
                ]
                .into_iter()
                .filter(|d| d.might_apply_batch(&batch_lock_code_hashes, &batch_type_code_hashes))
                .collect();
            let mut activity_batch = StoreBatch::new(self.writer.store());
            {
                let token_info_cache = load_activity_token_info_cache(
                    self.writer.store(),
                    &all_tx_data,
                    &input_cell_info,
                    &batch_cell_infos,
                )?;
                let mut block_tx_idx = 0usize;
                for parsed in all_parsed_blocks {
                    let tx_count = checked_tx_count(parsed.transactions_count, parsed.number)?;
                    let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count];
                    block_tx_idx += tx_count;

                    let tx_views: Vec<crate::db::writer::activities::TxView<'_>> = tx_slice
                        .iter()
                        .map(|td| -> Result<crate::db::writer::activities::TxView<'_>> {
                            let inputs = build_activity_input_views(
                                td,
                                parsed.number,
                                &input_cell_info,
                                &batch_cell_infos,
                                &dao_withdraw_outpoints,
                                &dao_compensations,
                            )?;
                            Ok(crate::db::writer::activities::TxView {
                                tx_hash: &td.hash,
                                block_hash: &parsed.hash,
                                tx_index: td.tx_index,
                                block_number: parsed.number,
                                timestamp: parsed.timestamp.timestamp_millis(),
                                is_cellbase: td.is_cellbase,
                                inputs,
                                outputs: &td.cells,
                                witnesses: &td.witnesses,
                            })
                        })
                        .collect::<Result<Vec<_>>>()?;

                    let bundles = crate::db::writer::activities::build_activity_bundles_for_block_with_detectors(
                        &tx_views,
                        &token_info_cache,
                        &protocol_detectors,
                    )?;

                    for bundle in bundles {
                        for owner in &bundle.owners {
                            // Accumulate daily activity stats
                            let date = ckbadger_common::block_date_from_ms(bundle.timestamp)
                                .format("%Y%m%d")
                                .to_string();
                            let day_stats = daily_activity_accum.entry(date.clone()).or_default();
                            BatchWriter::accumulate_owner_activity_stats(
                                bundle.is_cellbase,
                                owner,
                                day_stats,
                            );
                            // Exclude coinbase from unique address count
                            if !bundle.is_cellbase && owner.lock_hash.len() == 32 {
                                let mut hash = [0u8; 32];
                                hash.copy_from_slice(&owner.lock_hash);
                                daily_activity_addrs.entry(date).or_default().insert(hash);
                            }

                            // Accumulate hourly activity stats
                            let hour = ckbadger_common::block_datetime_from_ms(bundle.timestamp)
                                .format("%Y%m%d%H")
                                .to_string();
                            let hour_stats = hourly_activity_accum.entry(hour.clone()).or_default();
                            BatchWriter::accumulate_owner_activity_stats(
                                bundle.is_cellbase,
                                owner,
                                hour_stats,
                            );
                            if !bundle.is_cellbase && owner.lock_hash.len() == 32 {
                                let mut hash = [0u8; 32];
                                hash.copy_from_slice(&owner.lock_hash);
                                hourly_activity_addrs.entry(hour).or_default().insert(hash);
                            }
                        }
                        put_tx_activity_bundle(
                            &mut activity_batch,
                            &mut append_undo_seq_by_block,
                            bundle.block_number,
                            &bundle,
                        );

                        // Process Fiber channel lifecycle events
                        crate::db::writer::fiber::process_fiber_channel_events(
                            &mut activity_batch,
                            self.writer.store(),
                            &bundle,
                        )?;
                    }
                }
            }

            // Merge all secondary domain batches into data_batch for atomic commit
            data_batch.merge_from(domain_analytics_batch);
            data_batch.merge_from(object_activity_batch);
            data_batch.merge_from(identity_activity_batch);
            data_batch.merge_from(append_history_batch);
            data_batch.merge_from(activity_batch);

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
                let tx_count_for_block =
                    checked_tx_count(parsed.transactions_count, parsed.number)?;
                let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                block_tx_idx += tx_count_for_block;

                let cells_created: i32 = tx_slice
                    .iter()
                    .map(|tx| i32::try_from(tx.cells.len()).expect("output count exceeds i32"))
                    .sum();
                let cells_consumed: i32 = tx_slice
                    .iter()
                    .filter(|tx| !tx.is_cellbase)
                    .map(|tx| i32::try_from(tx.inputs.len()).expect("input count exceeds i32"))
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
                let used_capacity_created: i128 = tx_slice
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
                let (data_size_consumed, used_capacity_consumed) = resolve_consumed_stats(
                    tx_slice,
                    &input_cell_info,
                    &batch_cell_infos,
                    parsed.number,
                )?;

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
                    entry.5 = entry.5.checked_add(used_capacity_created).ok_or_else(|| {
                        anyhow!(
                            "daily used_capacity_created overflow: date={} block={}",
                            block_date,
                            parsed.number
                        )
                    })?;
                    entry.6 = entry.6.checked_add(used_capacity_consumed).ok_or_else(|| {
                        anyhow!(
                            "daily used_capacity_consumed overflow: date={} block={}",
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

        // Finalization: block headers + stats
        let t_finalize = Instant::now();
        {
            let mut core_batch = StoreBatch::new(self.writer.store());
            self.writer
                .insert_blocks_batch(&block_refs, &mut core_batch)?;
            let mut stats_batch = StoreBatch::new(self.writer.store());
            self.write_batch_stats_to_batch(&batch_stats, &mut stats_batch)?;
            // Write accumulated daily activity stats
            for (date, stats) in &daily_activity_accum {
                let unique_count = daily_activity_addrs.get(date).map_or(0, |s| s.len() as u32);
                self.writer.update_daily_activity_stats(
                    date,
                    stats,
                    unique_count,
                    &mut stats_batch,
                )?;
            }
            // Write accumulated hourly activity stats
            for (hour, stats) in &hourly_activity_accum {
                let unique_count = hourly_activity_addrs
                    .get(hour)
                    .map_or(0, |s| s.len() as u32);
                self.writer.update_hourly_activity_stats(
                    hour,
                    stats,
                    unique_count,
                    &mut stats_batch,
                )?;
            }
            // Inject sync_status update into the finalize batch so it is
            // committed atomically with block headers and domain data.
            // Previously sync_status was written via a separate put_cf after
            // commit, creating a crash window where totals could drift.
            if let Some((block_number, ref block_hash)) = batch_stats.last_block {
                let ema_rate = self.progress.ema_blocks_per_second();
                let mut status = self.writer.store().get_sync_status()?;
                status.tip_block_number = block_number;
                status.tip_block_hash = block_hash.clone();
                status.total_transactions += batch_stats.sync_totals.0;
                status.total_cells_created += batch_stats.sync_totals.1;
                status.total_cells_consumed += batch_stats.sync_totals.2;
                status.last_synced_at = chrono::Utc::now().timestamp();
                if ema_rate > 0.0 {
                    status.sync_ema_rate = Some(ema_rate);
                }
                let status_bytes = bincode::serialize(&status)
                    .with_context(|| "failed to serialize sync_status for atomic batch commit")?;
                stats_batch.put_sync_meta(
                    ckbadger_store::keys::sync_meta_keys::SYNC_STATUS,
                    &status_bytes,
                );
            }

            // Apply deferred activity count deltas atomically with finalize.
            // In bulk mode, T6a/T6b worker threads commit aggregates independently
            // before finalize. Applying deltas here (instead of as a separate commit)
            // ensures a crash between worker commits and finalize cannot leave
            // aggregate counters incremented without corresponding block headers.
            if !deferred_identity_deltas.is_empty() || !deferred_object_deltas.is_empty() {
                let empty_identity_aggs = HashMap::new();
                apply_identity_collection_activity_count_deltas(
                    self.writer.store(),
                    &mut core_batch,
                    deferred_identity_deltas,
                    &empty_identity_aggs,
                )?;
                let empty_object_aggs = HashMap::new();
                apply_object_collection_activity_count_deltas_with_pending(
                    self.writer.store(),
                    &mut core_batch,
                    deferred_object_deltas,
                    &empty_object_aggs,
                    &HashSet::new(),
                )?;
            }

            let commit_started = Instant::now();
            if bulk_sync_mode {
                // Merge headers + stats into single batch to halve finalize commit cost.
                core_batch.merge_from(stats_batch);
                // Delete the batch-in-progress marker atomically with finalize.
                // If crash occurs before this commit, startup detects the marker.
                core_batch
                    .delete_sync_meta(ckbadger_store::keys::sync_meta_keys::BULK_BATCH_IN_PROGRESS);
                debug!(
                    phase = "finalize_headers_stats",
                    batch_start = first_block,
                    batch_end = last_block,
                    bulk_sync_mode,
                    "Batch finalize commit start (merged)"
                );
                core_batch.commit_no_wal().with_context(|| {
                    format!(
                        "finalize commit_no_wal failed for blocks {}-{}",
                        first_block, last_block
                    )
                })?;
            } else {
                // Live sync: merge headers and stats into the single data_batch
                // that already holds all domain writes, then commit atomically.
                data_batch.merge_from(core_batch);
                data_batch.merge_from(stats_batch);
                debug!(
                    phase = "domain_atomic_commit",
                    batch_start = first_block,
                    batch_end = last_block,
                    bulk_sync_mode,
                    "Atomic domain batch commit start"
                );
                data_batch.commit().with_context(|| {
                    format!(
                        "atomic domain commit failed for blocks {}-{}",
                        first_block, last_block
                    )
                })?;
            }
            let commit_ms = commit_started.elapsed().as_secs_f64() * 1000.0;
            write_commit_ms += commit_ms;
            if commit_ms >= BULK_PHASE_COMMIT_SLOW_WARN_MS {
                warn!(
                    phase = "finalize_commit",
                    batch_start = first_block,
                    batch_end = last_block,
                    commit_ms = format!("{:.1}", commit_ms),
                    bulk_sync_mode,
                    "Finalize commit slow"
                );
            } else {
                debug!(
                    phase = "finalize_commit",
                    batch_start = first_block,
                    batch_end = last_block,
                    commit_ms = format!("{:.1}", commit_ms),
                    bulk_sync_mode,
                    "Finalize commit done"
                );
            }
        }

        // Tracker updates: in bulk sync these run inside T_TRACK thread;
        // in live sync they run serially here.
        if !bulk_sync_mode {
            self.update_hodl_wave(
                all_parsed_blocks,
                &all_tx_data,
                &input_cell_info,
                &batch_cell_infos,
                &address_balance_changes,
            )?;
            self.update_cell_distribution(
                all_parsed_blocks,
                &all_tx_data,
                &input_cell_info,
                &batch_cell_infos,
                &address_balance_changes,
            )?;
        }

        // In-memory cache notification only — the DB sync_status update was
        // already committed atomically in the finalize batch above.
        if let Some((block_number, ref block_hash)) = batch_stats.last_block {
            let ema_rate = self.progress.ema_blocks_per_second();
            let ema_rate_opt = if ema_rate > 0.0 { Some(ema_rate) } else { None };
            if !bulk_sync_mode {
                self.writer.refresh_latest_dao_statistics()?;
            }
            if let Some(cache) = self.writer.cache_invalidator() {
                let hash_hex = format!("0x{}", hex::encode(block_hash));
                cache
                    .update_sync_status(|status| {
                        status.update_batch(
                            block_number,
                            &hash_hex,
                            batch_stats.sync_totals.0,
                            batch_stats.sync_totals.1,
                            batch_stats.sync_totals.2,
                            batch_new_addresses,
                            ema_rate_opt,
                        );
                    })
                    .await;
            }
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
        let thread_ms =
            if let Some([t1, t1b, t2, t4, t5, t6a, t6b, t7, t_act, t_track]) = thread_times {
                info!(
                    precompute_ms = format!("{:.1}", precompute_ms),
                    prefetch_ms = format!("{:.1}", prefetch_ms),
                    write_ms = format!("{:.1}", write_ms),
                    write_commit_ms = format!("{:.1}", write_commit_ms),
                    finalize_ms = format!("{:.1}", finalize_ms),
                    t1_ms = format!("{:.1}", t1),
                    t1b_ms = format!("{:.1}", t1b),
                    t2_ms = format!("{:.1}", t2),
                    t4_ms = format!("{:.1}", t4),
                    t5_ms = format!("{:.1}", t5),
                    t6a_ms = format!("{:.1}", t6a),
                    t6b_ms = format!("{:.1}", t6b),
                    t7_ms = format!("{:.1}", t7),
                    t_act_ms = format!("{:.1}", t_act),
                    t_track_ms = format!("{:.1}", t_track),
                    txs = batch_tx_count,
                    cells = batch_cell_count,
                    inputs = batch_input_count,
                    "Batch write breakdown"
                );
                [t1, t1b, t2, t4, t5, t6a, t6b, t7, t_act, t_track]
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
                [0.0; 10]
            };
        Ok(BatchWriteMetrics {
            commit_ms: write_commit_ms,
            write_ms,
            prefetch_ms,
            finalize_ms,
            txs: u64::try_from(batch_tx_count).expect("parsed batch tx count exceeds u64"),
            cells: u64::try_from(batch_cell_count).expect("parsed batch cell count exceeds u64"),
            inputs: u64::try_from(batch_input_count).expect("parsed batch input count exceeds u64"),
            t1_ms: thread_ms[0],
            t1b_ms: thread_ms[1],
            t2_ms: thread_ms[2],
            t4_ms: thread_ms[3],
            t5_ms: thread_ms[4],
            t6a_ms: thread_ms[5],
            t6b_ms: thread_ms[6],
            t7_ms: thread_ms[7],
            t_act_ms: thread_ms[8],
            t_track_ms: thread_ms[9],
        })
    }

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
                        unclaimed_compensation: 0,
                    };
                    self.writer
                        .update_dao_daily_snapshot(*date, &dao_snapshot, batch)?;
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_store::LiveCellInfo;
    use std::sync::mpsc;
    use std::time::Duration;

    fn dummy_live_cell_info() -> LiveCellInfo {
        LiveCellInfo {
            capacity: 1,
            lock_script_hash: vec![1u8; 32],
            lock_code_hash: vec![2u8; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 0,
            occupied_capacity: 1,
            udt_amount: None,
            data_hash: None,
        }
    }

    fn dummy_positioned_cell_info() -> PositionedCellInfo {
        PositionedCellInfo::new(dummy_live_cell_info(), 1)
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
    fn test_wait_for_bulk_cells_commit_signal_blocks_until_cells_are_ready() {
        let (tx, rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();

        std::thread::spawn(move || {
            wait_for_bulk_cells_commit_signal(rx).expect("cells readiness wait should succeed");
            done_tx.send(()).expect("done signal");
        });

        assert!(
            done_rx.recv_timeout(Duration::from_millis(50)).is_err(),
            "dotbit-visible bulk commit must stay blocked until cells are committed"
        );

        tx.send(()).expect("cells ready signal");

        done_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("wait should unblock after cells are committed");
    }

    #[test]
    fn test_wait_for_bulk_cells_commit_signal_errors_when_cells_phase_exits_early() {
        let (_tx, rx) = mpsc::channel::<()>();
        drop(_tx);

        let err = wait_for_bulk_cells_commit_signal(rx).unwrap_err();
        assert!(err
            .to_string()
            .contains("cells commit phase ended before T6b commit could observe cells readiness"));
    }

    #[test]
    fn test_resolve_live_dotbit_account_id_for_consume_skips_outpoint_fallback_when_metadata_is_not_dotbit(
    ) {
        let key = (vec![0x11; 32], 0i16);
        let mut live_info = dummy_live_cell_info();
        live_info.type_code_hash = Some(crate::rpc::parse_hex_to_bytes(
            crate::parser::dao::DAO_CODE_HASH,
        ));
        let info = PositionedCellInfo::new(live_info, 1);

        let mut input_cell_info = HashMap::new();
        input_cell_info.insert(key.clone(), info);

        let mut dotbit_map = HashMap::new();
        dotbit_map.insert(key.clone(), vec![0x22; 20]);

        let resolved = resolve_live_dotbit_account_id_for_consume(
            &key,
            &input_cell_info,
            &HashMap::new(),
            &dotbit_map,
        );

        assert!(
            resolved.is_none(),
            "live sync must not consume dotbit from stale outpoint index when metadata resolves to a non-dotbit cell"
        );
    }

    #[test]
    fn test_resolve_live_dotbit_account_id_for_consume_uses_outpoint_fallback_when_metadata_is_missing(
    ) {
        let key = (vec![0x33; 32], 1i16);
        let expected_account_id = vec![0x44; 20];
        let mut dotbit_map = HashMap::new();
        dotbit_map.insert(key.clone(), expected_account_id.clone());

        let resolved = resolve_live_dotbit_account_id_for_consume(
            &key,
            &HashMap::new(),
            &HashMap::new(),
            &dotbit_map,
        );

        assert_eq!(resolved, Some(expected_account_id));
    }

    #[test]
    fn test_apply_nft_collection_activity_count_deltas_updates_only_nft_collections() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let object_collection_id = vec![0x11; 32];
        let cluster_id = vec![0x22; 32];

        let mut seed = StoreBatch::new(&store);
        seed.put_object_collection_aggregate(
            &object_collection_id,
            &ckbadger_store::types::ObjectCollectionAggregate {
                standard: ckbadger_store::types::ObjectStandard::MnftClass,
                activities_count: 3,
                ..Default::default()
            },
        );
        seed.put_cluster_aggregate(
            &cluster_id,
            &ckbadger_store::types::ClusterAggregate {
                name: Some("cluster".to_string()),
                ..Default::default()
            },
        );
        seed.commit().unwrap();

        let mut deltas = HashMap::new();
        deltas.insert(object_collection_id.clone(), 2);
        deltas.insert(cluster_id, 4);

        let mut batch = StoreBatch::new(&store);
        apply_object_collection_activity_count_deltas_with_pending(
            &store,
            &mut batch,
            deltas,
            &HashMap::new(),
            &HashSet::new(),
        )
        .unwrap();
        batch.commit().unwrap();

        let agg = store
            .get_object_collection_aggregate(&object_collection_id)
            .unwrap()
            .unwrap();
        assert_eq!(agg.activities_count, 5);
    }

    #[test]
    fn test_apply_nft_collection_activity_count_deltas_uses_pending_batch_aggregate() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let collection_id = vec![0x33; 32];
        let pending_agg = ckbadger_store::types::ObjectCollectionAggregate {
            standard: ckbadger_store::types::ObjectStandard::MnftClass,
            name: Some("fresh collection".to_string()),
            ..Default::default()
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_object_collection_aggregate(&collection_id, &pending_agg);

        let mut pending = HashMap::new();
        pending.insert(collection_id.clone(), pending_agg);

        let mut deltas = HashMap::new();
        deltas.insert(collection_id.clone(), 1);

        apply_object_collection_activity_count_deltas_with_pending(
            &store,
            &mut batch,
            deltas,
            &pending,
            &HashSet::new(),
        )
        .unwrap();
        batch.commit().unwrap();

        let agg = store
            .get_object_collection_aggregate(&collection_id)
            .unwrap()
            .unwrap();
        assert_eq!(agg.activities_count, 1);
        assert_eq!(agg.name.as_deref(), Some("fresh collection"));
    }

    #[test]
    fn test_apply_nft_collection_activity_count_deltas_uses_pending_cluster_ids() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let cluster_id = vec![0x44; 32];

        let mut batch = StoreBatch::new(&store);
        batch.put_cluster_aggregate(
            &cluster_id,
            &ckbadger_store::types::ClusterAggregate {
                name: Some("fresh cluster".to_string()),
                ..Default::default()
            },
        );

        let mut pending_cluster_ids = HashSet::new();
        pending_cluster_ids.insert(cluster_id.clone());

        let mut deltas = HashMap::new();
        deltas.insert(cluster_id.clone(), 1);

        apply_object_collection_activity_count_deltas_with_pending(
            &store,
            &mut batch,
            deltas,
            &HashMap::new(),
            &pending_cluster_ids,
        )
        .unwrap();
        batch.commit().unwrap();

        let cluster = store.get_cluster_aggregate(&cluster_id).unwrap().unwrap();
        assert_eq!(cluster.name.as_deref(), Some("fresh cluster"));
        assert!(store
            .get_object_collection_aggregate(&cluster_id)
            .unwrap()
            .is_none());
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
        resolved.insert((vec![0xBB; 32], 1), dummy_positioned_cell_info());
        let mut same_batch = HashMap::new();
        same_batch.insert((vec![0xCC; 32], 2), ());

        let missing = collect_missing_input_outpoints(&input_outpoints, &resolved, &same_batch);
        assert_eq!(missing, vec![(vec![0xAA; 32], 0)]);
    }

    #[test]
    fn test_build_activity_input_views_errors_when_input_cell_is_missing() {
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

        let err = match build_activity_input_views(
            &tx,
            99,
            &HashMap::new(),
            &HashMap::new(),
            &HashSet::new(),
            &HashMap::new(),
        ) {
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
        batch_cell_infos.insert(
            (previous_tx_hash.to_vec(), 3),
            PositionedCellInfo::new(info.clone(), 1),
        );

        let inputs = build_activity_input_views(
            &tx,
            100,
            &HashMap::new(),
            &batch_cell_infos,
            &HashSet::new(),
            &HashMap::new(),
        )
        .expect("input lookup should fall back to same-batch cell cache");
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].capacity, info.capacity);
        assert_eq!(inputs[0].occupied_capacity, info.occupied_capacity);
        assert_eq!(inputs[0].lock_script_hash, info.lock_script_hash);
    }

    #[test]
    fn test_build_activity_input_views_marks_dao_withdraw_request_inputs() {
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
        input_cell_info.insert(
            (previous_tx_hash.to_vec(), 1),
            PositionedCellInfo::new(info, 1),
        );

        // Populate dao_withdraw_outpoints with the withdraw request outpoint
        let mut dao_withdraw_outpoints = HashSet::new();
        dao_withdraw_outpoints.insert((previous_tx_hash.to_vec(), 1i16));

        // Populate dao_compensations with a test compensation value
        let mut dao_compensations = HashMap::new();
        dao_compensations.insert((previous_tx_hash.to_vec(), 1i16), 5_00000000i64);

        let inputs = build_activity_input_views(
            &tx,
            200,
            &input_cell_info,
            &HashMap::new(),
            &dao_withdraw_outpoints,
            &dao_compensations,
        )
        .unwrap();
        assert_eq!(inputs.len(), 1);
        assert!(inputs[0].is_dao_withdraw_request);
        assert_eq!(inputs[0].dao_compensation, Some(5_00000000));
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
    fn test_resolve_input_udt_info_returns_cache_hit_without_db_lookup() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_domain(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store);
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

        // Cache entries are trusted — no DB round-trip needed.
        // During bulk sync, cached outputs from the same batch are guaranteed valid.
        let resolved = resolve_input_udt_info_from_live_cells(
            &writer,
            &cache,
            &[(tx_hash.clone(), output_index)],
        )
        .unwrap();

        let entry = resolved
            .get(&(tx_hash, output_index))
            .expect("expected cache hit to be returned");
        assert_eq!(entry.0, vec![0x10; 32]); // type_script_hash
        assert_eq!(entry.5, 145_203); // amount
        assert_eq!(entry.6, "xudt_compatible"); // standard
    }

    #[test]
    fn test_resolve_input_udt_info_reads_live_cells_and_refreshes_cache() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
        let writer = BatchWriter::new(store.clone(), store.clone());
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
            lock_script_hash: vec![0x55; 32],
            lock_code_hash: vec![0x66; 32],
            lock_hash_type: 1,
            lock_args: vec![0x77; 20],
            type_script_hash: Some(type_hash.clone()),
            type_code_hash: Some(type_code_hash),
            type_hash_type: Some(1),
            type_args: Some(vec![0x44; 32]),
            data_size: 16,
            occupied_capacity: 0,
            udt_amount: Some(145_203),
            data_hash: None,
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_cell(&tx_hash, output_index, &live_cell, 1);
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
        let writer = BatchWriter::new(store.clone(), store);

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
        let writer = BatchWriter::new(store.clone(), store.clone());
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
        TxData {
            hash,
            block_number: 0,
            tx_index: 0,
            inputs_count,
            outputs_count,
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
    fn test_load_latest_dao_daily_snapshot_propagates_deserialize_error() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let key = keys::encode_stats_key(keys::STATS_PREFIX_DAO_DAILY_SNAPSHOT, b"20260101");
        store.put_cf(store.cf_stats_dao(), &key, b"broken").unwrap();

        let err = load_latest_dao_daily_snapshot(&store).unwrap_err();
        assert!(err.to_string().contains(
            "failed to get latest dao daily snapshot while building cumulative snapshot"
        ));
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
    fn test_grouped_mnft_consume_skips_intermediate_transfer_but_keeps_final_burn() {
        // Same-tx or later-tx re-creations are already reflected by batched output inserts.
        assert!(!should_consume_grouped_mnft_token(Some(5), 4));
        assert!(!should_consume_grouped_mnft_token(Some(5), 5));

        // Once the last batch output for the token is behind us, a later consume is real burn.
        assert!(should_consume_grouped_mnft_token(Some(5), 6));
        assert!(should_consume_grouped_mnft_token(None, 6));
    }
}
