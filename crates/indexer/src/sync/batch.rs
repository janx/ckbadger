#![allow(clippy::type_complexity)]

use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::time::Instant;

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, NaiveDate, Timelike, Utc};
use ckb_types::utilities::compact_to_difficulty as ckb_compact_to_difficulty;
use dashmap::DashMap;
use rayon::prelude::*;
use tracing::{debug, info, warn};

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::keys;
use ckbadger_store::types::{
    DailyActivityStats, DaoDailySnapshot, IdentityCollectionAggregate, MnftTypeIndex,
    PositionedCellInfo, ScriptReferenceInfo, SporeTypeIndex, SOLE_SPORES_SENTINEL_COLLECTION,
};
use ckbadger_store::CkbadgerStore;

use crate::db::writer::dotbit::resolve_dotbit_tx_activity;
use crate::db::writer::object_activity_acc::ObjectCollectionActivityAccumulator;
use crate::db::writer::DaoSnapshotBoundary;
use crate::db::{BatchWriter, DaoWithdrawalContext};
use crate::parser::{
    BitCellParser, BlockParser, CellParser, DaoParser, DotbitParser, MnftParser, ScriptParser,
    SporeParser, TransactionParser, UdtParser,
};

use crate::rpc::{BlockResponseWithCycles, CkbRpcClient};

use super::adaptive::*;
use super::checked_tx_count;
use super::dao_helpers::*;
use super::diagnostics::*;
use super::helpers::*;
use super::indexer::{
    persist_bulk_sync_completion_status, take_bulk_sync_completion_transition, Indexer,
    CACHE_INVALIDATION_INTERVAL,
};
use super::nft_helpers::*;
use super::sync_mode::*;
use super::token_helpers::*;
use super::types::{
    AddressBalanceDelta, BatchWriteMetrics, CachedUdtCellInfo, DotbitTxActivityData, TxData,
    UnresolvedLocalProbeSummary, UnresolvedRpcProbeSummary,
};
use super::undo::*;

/// Marks an invariant failure that occurred while constructing the atomic
/// domain batch, before either store commit can run. Retrying or rolling back
/// cannot change this deterministic result.
#[derive(Debug, thiserror::Error)]
#[error("pre-commit invariant failed in {component}: {source}")]
pub(crate) struct PreCommitInvariantError {
    component: &'static str,
    #[source]
    source: anyhow::Error,
}

impl PreCommitInvariantError {
    pub(crate) fn new(component: &'static str, source: anyhow::Error) -> Self {
        Self { component, source }
    }
}

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

fn ensure_pipeline_bulk_path_disabled(
    bulk_sync_mode: bool,
    first_block: i64,
    last_block: i64,
    chain_tip: u64,
) -> Result<()> {
    if bulk_sync_mode {
        bail!(
            "pipeline bulk path is disabled after bulk-build cutover: range {}-{} chain_tip={} \
             bulk-only writer/prefetch logic must not run; startup bulk sync must run through bulk build engine first",
            first_block,
            last_block,
            chain_tip
        );
    }
    Ok(())
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
                parsed_input_outpoint_index_i16(input.previous_output_index, "stats")?,
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
            data_size_consumed += i64::from(info.data_size);
            used_capacity_consumed += i128::from(info.occupied_capacity);
        }
    }
    Ok((data_size_consumed, used_capacity_consumed))
}

struct ActivityInputIndexes<'a> {
    cell_info: &'a HashMap<(Vec<u8>, i16), PositionedCellInfo>,
    batch_cell_info: &'a HashMap<(Vec<u8>, i16), PositionedCellInfo>,
    dao_withdraw_outpoints: &'a HashSet<(Vec<u8>, i16)>,
    dao_compensations: &'a HashMap<(Vec<u8>, i16), i64>,
    dotbit_ids: &'a HashMap<(Vec<u8>, i16), Vec<u8>>,
    bit_cell_identity_ids: &'a HashMap<(Vec<u8>, i16), Vec<u8>>,
}

fn build_activity_input_views<'a>(
    tx_data: &'a TxData,
    block_number: i64,
    indexes: ActivityInputIndexes<'a>,
) -> Result<Vec<crate::db::writer::activities::InputCellView<'a>>> {
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
            let info = indexes
                .cell_info
                .get(&key)
                .or_else(|| indexes.batch_cell_info.get(&key))
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
            let is_dao_withdraw_request = indexes.dao_withdraw_outpoints.contains(&key);
            let dao_compensation = if is_dao_withdraw_request {
                indexes.dao_compensations.get(&key).copied()
            } else {
                None
            };
            // For consumed .bit cells, pass the pre-resolved account_id as
            // data so resolve_dotbit_account_id can use it as fallback when
            // type_args are empty (old .bit layout).  Raw cell data is
            // unavailable for inputs; the account_id comes from
            // CF_DOTBIT_OUTPOINT via the dotbit_map built earlier.
            let data: &[u8] = indexes
                .dotbit_ids
                .get(&key)
                .map(|v| v.as_slice())
                .unwrap_or(&[]);

            Ok(crate::db::writer::activities::InputCellView {
                previous_tx_hash: input.previous_tx_hash.as_slice(),
                previous_output_index: u32::try_from(input.previous_output_index).map_err(
                    |_| {
                        anyhow!(
                            "negative input previous output index while building activities: block={}, tx_hash=0x{}, tx_index={}, input_index={}, previous_output_index={}",
                            block_number,
                            hex::encode(tx_data.hash),
                            tx_data.tx_index,
                            input_index,
                            input.previous_output_index
                        )
                    },
                )?,
                lock_script_hash: &info.lock_script_hash,
                lock_code_hash: &info.lock_code_hash,
                lock_hash_type: info.lock_hash_type,
                lock_args: &info.lock_args,
                capacity: info.capacity,
                occupied_capacity: info.occupied_capacity,
                type_code_hash: info.type_code_hash.as_deref(),
                type_hash_type: info.type_hash_type,
                type_script_hash: info.type_script_hash.as_deref(),
                type_args: info.type_args.as_deref(),
                udt_amount: info.udt_amount,
                bit_cell_identity_id: indexes
                    .bit_cell_identity_ids
                    .get(&key)
                    .map(Vec::as_slice),
                data,
                is_dao_withdraw_request,
                dao_compensation,
            })
        })
        .collect()
}

/// Extract outpoints whose DAO status == 1 (withdraw request) from consumed_dao_map.
/// The returned set lets T_ACT classify inputs without per-input RocksDB reads.
fn dao_withdraw_outpoints_from_map(
    consumed_dao_map: &crate::sync::dao_helpers::DaoConsumedMap,
) -> HashSet<(Vec<u8>, i16)> {
    consumed_dao_map
        .iter()
        .filter(|(_, row)| row.status == 1) // status == 1 means withdraw request
        .map(|(k, _)| k.clone())
        .collect()
}

fn tx_slice_claimed_dao_compensation(
    tx_slice: &[TxData],
    dao_compensations: &HashMap<(Vec<u8>, i16), i64>,
) -> Result<i128> {
    let mut claimed = 0i128;
    for tx in tx_slice.iter().filter(|tx| !tx.is_cellbase) {
        for input in &tx.inputs {
            let key = (
                input.previous_tx_hash.to_vec(),
                parsed_input_outpoint_index_i16(input.previous_output_index, "sync_indexer")?,
            );
            if let Some(compensation) = dao_compensations.get(&key) {
                claimed = claimed
                    .checked_add(i128::from(*compensation))
                    .ok_or_else(|| {
                        anyhow!(
                            "claimed DAO compensation overflow while accumulating block totals: tx_hash=0x{}, prev_outpoint=0x{}:{}",
                            hex::encode(tx.hash),
                            hex::encode(input.previous_tx_hash),
                            input.previous_output_index
                        )
                    })?;
            }
        }
    }
    Ok(claimed)
}

/// Correct fees for DAO withdrawal-completion (phase-2) txs on the live-sync
/// write path, BEFORE `insert_transactions_batch` serializes `TxIndexEntry`.
///
/// The parser stage cannot compute the real miner fee for these txs because
/// DAO compensation (interest) is unknown at parse time: it writes a fee=0
/// placeholder when outputs exceed raw inputs, or an undercounted fee when
/// extra plain inputs keep raw inputs >= outputs. The correction criterion is
/// therefore "does this tx consume a withdraw-request outpoint" — never the
/// current fee value. For every such tx the fee is unconditionally recomputed
/// as `(raw inputs + compensation) - outputs`, mirroring the bulk path
/// (`bulk_build::resolved_tx_fee`). Phase-1 (withdraw request) txs consume
/// deposit outpoints, not withdraw-request outpoints, so their parser fee
/// stands untouched.
fn correct_dao_withdrawal_fees(
    all_tx_data: &mut [TxData],
    dao_withdraw_outpoints: &HashSet<(Vec<u8>, i16)>,
    dao_compensations: &HashMap<(Vec<u8>, i16), i64>,
) -> Result<()> {
    if dao_withdraw_outpoints.is_empty() {
        return Ok(());
    }
    for tx_data in all_tx_data.iter_mut() {
        if tx_data.is_cellbase {
            continue;
        }
        let mut consumes_withdraw_request = false;
        let mut total_compensation: i64 = 0;
        for input in &tx_data.inputs {
            let key = (
                input.previous_tx_hash.to_vec(),
                parsed_input_outpoint_index_i16(input.previous_output_index, "dao_fee_correct")?,
            );
            if !dao_withdraw_outpoints.contains(&key) {
                continue;
            }
            consumes_withdraw_request = true;
            let comp = dao_compensations.get(&key).ok_or_else(|| {
                anyhow!(
                    "missing pre-computed DAO compensation for consumed withdraw-request outpoint: tx=0x{} block={} outpoint=0x{}:{}",
                    hex::encode(tx_data.hash),
                    tx_data.block_number,
                    hex::encode(input.previous_tx_hash),
                    input.previous_output_index
                )
            })?;
            total_compensation = total_compensation.checked_add(*comp).ok_or_else(|| {
                anyhow!(
                    "DAO compensation overflow in fee correction: tx=0x{} block={}",
                    hex::encode(tx_data.hash),
                    tx_data.block_number
                )
            })?;
        }
        if !consumes_withdraw_request {
            continue;
        }
        let effective_input = tx_data
            .total_input_capacity
            .checked_add(total_compensation)
            .ok_or_else(|| {
                anyhow!(
                    "effective input overflow in DAO fee correction: tx=0x{} block={}",
                    hex::encode(tx_data.hash),
                    tx_data.block_number
                )
            })?;
        let fee = effective_input
            .checked_sub(tx_data.total_output_capacity)
            .ok_or_else(|| {
                anyhow!(
                    "fee subtraction overflow in DAO fee correction: tx=0x{} block={} effective_input={} outputs={}",
                    hex::encode(tx_data.hash),
                    tx_data.block_number,
                    effective_input,
                    tx_data.total_output_capacity
                )
            })?;
        if fee < 0 {
            bail!(
                "negative fee after DAO compensation in fee correction: tx=0x{} block={} raw_inputs={} compensation={} outputs={}",
                hex::encode(tx_data.hash),
                tx_data.block_number,
                tx_data.total_input_capacity,
                total_compensation,
                tx_data.total_output_capacity
            );
        }
        tx_data.fee = fee;
    }
    Ok(())
}

/// Pre-compute DAO compensation for each withdraw-complete outpoint.
/// This allows the activity builder to include compensation in activities
/// without duplicating the DAO processing logic.
///
/// The `consumed_dao_map` key is the withdraw-request outpoint (tx_hash, output_index)
/// being consumed in Phase 2. The value struct has fields for the original deposit outpoint,
/// capacity_str, deposit_block, and status.
/// Only status==1 entries are withdraw completes.
fn pre_compute_dao_compensations(
    store: &CkbadgerStore,
    consumed_dao_map: &crate::sync::dao_helpers::DaoConsumedMap,
) -> Result<HashMap<(Vec<u8>, i16), i64>> {
    use crate::db::writer::dao::{calculate_dao_compensation_from_ar, extract_ar_from_dao};

    // Filter to status==1 entries only
    let withdraw_entries: Vec<_> = consumed_dao_map
        .iter()
        .filter(|(_, row)| row.status == 1)
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
        i64,             // occupied capacity
        i64,             // deposit_block
        i64,             // withdraw_request_block
    )> = Vec::new();

    for (withdraw_key, row) in &withdraw_entries {
        let orig_tx_hash = &row.tx_hash;
        let orig_output_index = row.output_index;
        let capacity_str = &row.capacity_str;
        let deposit_block = row.deposit_block;
        let capacity: i64 = capacity_str.parse().map_err(|e| {
            anyhow!(
                "invalid DAO capacity string in compensation pre-compute: value='{}', error={}",
                capacity_str,
                e
            )
        })?;

        // Look up the deposit entry to get withdraw_request_block
        let outpoint_key = keys::encode_outpoint(orig_tx_hash, orig_output_index);
        let (request_block, occupied_capacity) = if let Some(value) =
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
                Some(b) => (b, entry.occupied_capacity),
                None => bail!(
                    "withdraw_request_block missing for status=1 deposit in compensation pre-compute: outpoint=0x{}:{}",
                    hex::encode(orig_tx_hash),
                    orig_output_index,
                ),
            }
        } else {
            bail!(
                "DAO deposit entry not found in store during compensation pre-compute: outpoint=0x{}:{}",
                hex::encode(orig_tx_hash),
                orig_output_index,
            );
        };

        blocks_needed.insert(deposit_block);
        blocks_needed.insert(request_block);
        entries_with_request_block.push((
            withdraw_key,
            capacity,
            occupied_capacity,
            deposit_block,
            request_block,
        ));
    }

    // Batch-fetch DAO header fields for all needed blocks
    let blocks_vec: Vec<i64> = blocks_needed.into_iter().collect();
    let dao_fields = store.get_dao_fields_batch(&blocks_vec)?;

    // Compute compensations
    let mut compensations = HashMap::new();
    for (withdraw_key, capacity, occupied_capacity, deposit_block, request_block) in
        entries_with_request_block
    {
        let deposit_dao = dao_fields.get(&deposit_block).ok_or_else(|| {
            anyhow!(
                "DAO field missing for deposit block in compensation pre-compute: block={}, outpoint=0x{}:{}",
                deposit_block,
                hex::encode(&withdraw_key.0),
                withdraw_key.1,
            )
        })?;
        let withdraw_dao = dao_fields.get(&request_block).ok_or_else(|| {
            anyhow!(
                "DAO field missing for request block in compensation pre-compute: block={}, outpoint=0x{}:{}",
                request_block,
                hex::encode(&withdraw_key.0),
                withdraw_key.1,
            )
        })?;
        let ar_deposit = extract_ar_from_dao(deposit_dao).ok_or_else(|| {
            anyhow!(
                "failed to extract AR from deposit block DAO field in compensation pre-compute: block={}, outpoint=0x{}:{}",
                deposit_block,
                hex::encode(&withdraw_key.0),
                withdraw_key.1,
            )
        })?;
        let ar_withdraw = extract_ar_from_dao(withdraw_dao).ok_or_else(|| {
            anyhow!(
                "failed to extract AR from request block DAO field in compensation pre-compute: block={}, outpoint=0x{}:{}",
                request_block,
                hex::encode(&withdraw_key.0),
                withdraw_key.1,
            )
        })?;
        let compensation = calculate_dao_compensation_from_ar(
            capacity,
            occupied_capacity,
            ar_deposit,
            ar_withdraw,
        )
        .map_err(|e| {
                anyhow!(
                    "DAO compensation calculation failed in pre-compute: outpoint=0x{}:{}, capacity={}, ar_deposit={}, ar_withdraw={}, error={}",
                    hex::encode(&withdraw_key.0),
                    withdraw_key.1,
                    capacity,
                    ar_deposit,
                    ar_withdraw,
                    e,
                )
            })?;
        compensations.insert(withdraw_key.clone(), compensation);
    }

    Ok(compensations)
}

fn apply_object_collection_activity_count_deltas_with_pending(
    store: &CkbadgerStore,
    batch: &mut StoreBatch,
    deltas: HashMap<Vec<u8>, i64>,
    pending_aggregates: &HashMap<Vec<u8>, ckbadger_store::types::MnftCollectionAggregate>,
    pending_cluster_ids: &HashSet<Vec<u8>>,
) -> Result<()> {
    if deltas.is_empty() {
        return Ok(());
    }

    let mut aggregates: HashMap<Vec<u8>, ckbadger_store::types::MnftCollectionAggregate> =
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
                store.get_mnft_collection_aggregate(&collection_id)?
            };
            match loaded {
                Some(loaded) => aggregates.entry(collection_id.clone()).or_insert(loaded),
                None => {
                    if store.get_cluster_aggregate(&collection_id)?.is_some()
                        || pending_cluster_ids.contains(&collection_id)
                    {
                        // Spore cluster activities share the same append-only CF but do not
                        // belong to object_collection_agg.
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
                "object collection activities_count overflow: collection_id=0x{}, current={}, delta={}",
                hex::encode(&collection_id),
                agg.activities_count,
                delta
            )
        })?;
        if next < 0 {
            bail!(
                "object collection activities_count underflow: collection_id=0x{}, current={}, delta={}",
                hex::encode(&collection_id),
                agg.activities_count,
                delta
            );
        }
        agg.activities_count = next;
    }

    for (collection_id, agg) in aggregates {
        batch.put_mnft_collection_aggregate(&collection_id, &agg);
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

fn completed_dao_snapshot_boundaries(stats: &BatchStats) -> Result<Vec<DaoSnapshotBoundary>> {
    let mut dates = stats.dao_snapshot_dates.iter().copied().collect::<Vec<_>>();
    dates.sort_unstable();
    dates.pop();

    dates
        .into_iter()
        .map(|date| {
            let end_block = stats
                .dao_block_numbers_by_date
                .get(&date)
                .and_then(|blocks| blocks.iter().max())
                .copied()
                .ok_or_else(|| {
                    anyhow!("missing end block for completed DAO snapshot date {}", date)
                })?;
            Ok(DaoSnapshotBoundary { date, end_block })
        })
        .collect()
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

fn truncate_to_hour(dt: DateTime<Utc>) -> DateTime<Utc> {
    dt.with_minute(0)
        .and_then(|d| d.with_second(0))
        .and_then(|d| d.with_nanosecond(0))
        .unwrap_or(dt)
}

pub(super) type ScriptUsageChanges = HashMap<(Vec<u8>, bool), (i64, i64, i128, i128, i128, i128)>;
pub(super) type ScriptReferenceUsageChanges =
    HashMap<(Vec<u8>, u8, bool), (i64, i64, i128, i128, i128, i128)>;
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
                        // Compute output-side semantic tags; input-side tags are
                        // OR-ed in later by write_parsed_batch after prefetch.
                        let mut output_semantic_tags: u16 = 0;
                        for cell in &cells {
                            output_semantic_tags |=
                                super::pipeline::classify_bulk_cell_semantic_tag(cell).to_bit();
                        }
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
                        let tx_index_i32 = i32::try_from(tx_index).map_err(|_| {
                            anyhow!(
                                "tx index exceeds i32 range in parse_blocks_parallel: tx_hash=0x{}, block={}, tx_index={}",
                                hex::encode(parsed_tx.hash),
                                parsed.number,
                                tx_index
                            )
                        })?;
                        Ok(TxData {
                            hash: parsed_tx.hash,
                            block_number: parsed.number,
                            tx_index: tx_index_i32,
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
                            semantic_tags: output_semantic_tags,
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

        if take_bulk_sync_completion_transition(&self.was_bulk_sync_active, currently_bulk) {
            let stats = self.writer.store().memory_stats();
            let current = self.progress.current();
            let chain_tip = self.progress.target();
            let sst_gb = stats.sst_files_size as f64 / (1024.0 * 1024.0 * 1024.0);

            if let Err(e) =
                persist_bulk_sync_completion_status(self.writer.store().as_ref(), chain_tip)
            {
                warn!(
                    error = %e,
                    chain_tip,
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
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn write_parsed_batch(
        &self,
        blocks: &[BlockResponseWithCycles],
        all_parsed_blocks: &[crate::parser::block::ParsedBlock],
        mut all_tx_data: Vec<TxData>,
        input_cell_info: HashMap<(Vec<u8>, i16), PositionedCellInfo>,
        batch_cell_infos: HashMap<(Vec<u8>, i16), PositionedCellInfo>,
        address_balance_changes: HashMap<Vec<u8>, AddressBalanceDelta>,
        script_usage_changes: ScriptUsageChanges,
        script_reference_usage_changes: ScriptReferenceUsageChanges,
        script_daily_changes: HashMap<(Vec<u8>, u8, bool, u32), (i128, i128)>,
        token_daily_changes: HashMap<(Vec<u8>, u32), (i128, i128)>,
        spore_type_index_changes: HashMap<Vec<u8>, SporeTypeIndex>,
        spore_daily_changes: HashMap<(Vec<u8>, u32), (i128, i128)>,
        cluster_daily_changes: HashMap<(Vec<u8>, u32), (i128, i128)>,
        object_type_index_changes: HashMap<Vec<u8>, MnftTypeIndex>,
        object_daily_changes: HashMap<(Vec<u8>, u32), (i128, i128)>,
        chain_tip: u64,
    ) -> Result<BatchWriteMetrics> {
        if all_parsed_blocks.is_empty() {
            return Ok(BatchWriteMetrics::default());
        }

        // Safe: early return on empty guarantees .first()/.last() are Some.
        let first_block = all_parsed_blocks.first().unwrap().number;
        let last_block = all_parsed_blocks.last().unwrap().number;
        let end_block = u64::try_from(last_block).map_err(|_| {
            anyhow!(
                "negative last_block in write_parsed_batch: last_block={}",
                last_block
            )
        })?;
        let bulk_sync_mode = is_effective_bulk_sync_batch(
            chain_tip,
            end_block,
            self.config.bulk_sync_threshold,
            self.bulk_sync_allowed.load(Ordering::SeqCst),
        );
        ensure_pipeline_bulk_path_disabled(bulk_sync_mode, first_block, last_block, chain_tip)?;

        let mut all_input_outpoints: Vec<(Vec<u8>, i16)> = Vec::new();
        for tx in all_tx_data.iter().filter(|tx| !tx.is_cellbase) {
            for input in &tx.inputs {
                all_input_outpoints.push((
                    input.previous_tx_hash.to_vec(),
                    parsed_input_outpoint_index_i16(input.previous_output_index, "sync_indexer")?,
                ));
            }
        }
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
        // OR-in input-side semantic tags now that input cells are resolved.
        for tx_data in all_tx_data.iter_mut() {
            if tx_data.is_cellbase {
                continue;
            }
            for input in &tx_data.inputs {
                let key = (
                    input.previous_tx_hash.to_vec(),
                    parsed_input_outpoint_index_i16(
                        input.previous_output_index,
                        "semantic_tags_input",
                    )?,
                );
                if let Some(info) = input_cell_info
                    .get(&key)
                    .or_else(|| batch_cell_infos.get(&key))
                {
                    tx_data.semantic_tags |=
                        super::pipeline::classify_live_cell_semantic_tag(&info.cell).to_bit();
                }
            }
        }

        // DAO context resolution + phase-2 fee correction.
        //
        // This MUST run before `insert_transactions_batch` below stages
        // `TxIndexEntry`: the parser writes a placeholder fee for txs that
        // consume a withdraw-request outpoint (DAO compensation is unknown at
        // parse time), and this is the single point on the live path where
        // the real fee is computed (mirroring bulk_build::resolved_tx_fee).
        // All reads go to committed store state: a consumed withdraw-request
        // outpoint always references a request committed in an earlier batch
        // (a same-block request+completion is impossible — the completion
        // must reference the request block header as a header dep).
        let consumed_dao_map = {
            let unique_outpoints: Vec<(Vec<u8>, i16)> = {
                let mut seen = HashSet::new();
                all_input_outpoints
                    .into_iter()
                    .filter(|x| seen.insert(x.clone()))
                    .collect()
            };
            if unique_outpoints.is_empty() {
                HashMap::new()
            } else {
                let outpoint_refs: Vec<(&[u8], i16)> = unique_outpoints
                    .iter()
                    .map(|(h, i)| (h.as_slice(), *i))
                    .collect();
                self.writer
                    .find_consumed_dao_deposits_batch(&outpoint_refs)?
            }
        };
        // DAO withdraw-request outpoints for fee correction and activity
        // classification (no per-input DB reads).
        let dao_withdraw_outpoints = dao_withdraw_outpoints_from_map(&consumed_dao_map);
        // Pre-computed DAO compensations for fee correction and activities.
        let dao_compensations =
            pre_compute_dao_compensations(self.writer.store(), &consumed_dao_map)?;
        correct_dao_withdrawal_fees(
            &mut all_tx_data,
            &dao_withdraw_outpoints,
            &dao_compensations,
        )?;

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
                        )?,
                    );
                    let info = input_cell_info
                        .get(&key)
                        .or_else(|| batch_cell_infos.get(&key));
                    let info = info.ok_or_else(|| {
                        anyhow::anyhow!(
                            "missing input cell info for consumption: tx=0x{}, input_prev_tx=0x{}, output_index={}, block={}",
                            hex::encode(tx_data.hash),
                            hex::encode(input.previous_tx_hash),
                            input.previous_output_index,
                            tx_data.block_number
                        )
                    })?;
                    all_consumptions.push((
                        input.previous_tx_hash.as_slice(),
                        parsed_input_outpoint_index_i16(
                            input.previous_output_index,
                            "sync_indexer",
                        )?,
                        info.created_at_block,
                        tx_data.hash.as_slice(),
                        tx_data.block_number,
                        input_index_i16,
                    ));
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

        // Compute per-tx address entries for addr_txs index.
        // Defer AddrTxValue construction until participant tags are known: the
        // actual write is performed after TxActions is built below, where
        // `tags_by_addr_tx` provides the tag bitmask for `AddrTxValue.tags`.
        // per_addr: lock_hash -> (output_cap_sum, input_cap_sum, has_outputs, has_inputs)
        struct PendingAddrTx {
            lock_hash: Vec<u8>,
            block_number: i64,
            tx_index: i32,
            tx_hash: Vec<u8>,
            capacity_change: i64,
            has_in: bool,
            has_out: bool,
        }
        let mut addr_tx_entries: Vec<PendingAddrTx> = Vec::new();
        for tx_data in &all_tx_data {
            let mut per_addr: HashMap<Vec<u8>, (i64, i64, bool, bool)> = HashMap::new();
            for cell in &tx_data.cells {
                let e = per_addr.entry(cell.lock_script_hash.clone()).or_default();
                e.0 = e.0.checked_add(cell.capacity).ok_or_else(|| {
                    anyhow::anyhow!(
                        "output capacity sum overflow for addr in tx block={}",
                        tx_data.block_number
                    )
                })?;
                e.2 = true;
            }
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
                        .or_else(|| batch_cell_infos.get(&key));
                    let info = info.ok_or_else(|| {
                        anyhow::anyhow!(
                            "missing input cell info for addr_txs: tx=0x{}, input_prev_tx=0x{}, output_index={}, block={}",
                            hex::encode(tx_data.hash),
                            hex::encode(input.previous_tx_hash),
                            input.previous_output_index,
                            tx_data.block_number
                        )
                    })?;
                    let e = per_addr.entry(info.lock_script_hash.clone()).or_default();
                    e.1 = e.1.checked_add(info.capacity).ok_or_else(|| {
                        anyhow::anyhow!(
                            "input capacity sum overflow for addr in tx block={}",
                            tx_data.block_number
                        )
                    })?;
                    e.3 = true;
                }
            }
            for (lock_hash, (out_cap, in_cap, has_out, has_in)) in per_addr {
                let capacity_change = out_cap.checked_sub(in_cap).ok_or_else(|| {
                    anyhow::anyhow!(
                        "capacity_change overflow: out={} in={} block={}",
                        out_cap,
                        in_cap,
                        tx_data.block_number
                    )
                })?;
                addr_tx_entries.push(PendingAddrTx {
                    lock_hash,
                    block_number: tx_data.block_number,
                    tx_index: tx_data.tx_index,
                    tx_hash: tx_data.hash.to_vec(),
                    capacity_change,
                    has_in,
                    has_out,
                });
            }
        }

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
        let precompute_ms = t_precompute.elapsed().as_secs_f64() * 1000.0;

        let mut batch_new_addresses = 0i64;

        let t_write = Instant::now();
        let mut write_commit_ms = 0.0_f64;
        let mut batch_stats;
        // Post-batch DAO lifecycle view of everything this batch stages, used to
        // materialize completed-day snapshots exactly before the atomic commit.
        let staged_dao_entries: HashMap<Vec<u8>, ckbadger_store::types::DaoDepositCacheEntry>;
        let mut staged_dao_completions: HashMap<Vec<u8>, (i64, Vec<u8>)> = HashMap::new();
        let mut daily_activity_accum: HashMap<String, DailyActivityStats> = HashMap::new();
        let mut daily_activity_addrs: HashMap<String, HashSet<[u8; 32]>> = HashMap::new();
        let mut hourly_activity_accum: HashMap<String, DailyActivityStats> = HashMap::new();
        let mut hourly_activity_addrs: HashMap<String, HashSet<[u8; 32]>> = HashMap::new();
        let mut data_batch = StoreBatch::new(self.writer.store());
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
        // Cell payloads go into the append-only store. Build the batch now
        // but defer its commit until just before the domain batch commit so
        // that a crash between the two commits is as narrow as possible.
        let mut cells_batch = StoreBatch::new(&self.append_only_store);
        if !all_cells.is_empty() {
            self.writer.insert_cells_batch(
                &all_cells,
                &batch_cell_infos,
                &mut data_batch,
                &mut cells_batch,
                false,
            )?;
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
        let lock_hash_keys: Vec<&Vec<u8>> = if !address_balance_changes.is_empty() {
            address_balance_changes.keys().collect()
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
        let reference_keys: Vec<(Vec<u8>, u8)> = if !script_reference_usage_changes.is_empty() {
            let mut seen = std::collections::HashSet::new();
            script_reference_usage_changes
                .keys()
                .filter_map(|(reference_hash, hash_type, _is_type)| {
                    let key = (reference_hash.clone(), *hash_type);
                    seen.insert(key.clone()).then_some(key)
                })
                .collect()
        } else {
            vec![]
        };
        let code_hash_refs: Vec<&Vec<u8>> = unique_code_hashes.iter().collect();

        let need_balances = !lock_hash_keys.is_empty();
        let need_scripts = !code_hash_refs.is_empty();
        let need_script_references = !reference_keys.is_empty();
        let mut updated_script_references: HashMap<(Vec<u8>, u8), ScriptReferenceInfo> =
            HashMap::new();
        let mut domain_analytics_batch = StoreBatch::new(self.writer.store());
        let mut append_history_batch = StoreBatch::new(self.writer.store());

        if need_balances || need_scripts || need_script_references {
            let writer = &self.writer;
            let (existing_balances, existing_scripts, existing_script_references) =
                std::thread::scope(|s| {
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
                    let refs = if need_script_references {
                        Some(s.spawn(|| writer.read_script_reference_info(&reference_keys)))
                    } else {
                        None
                    };
                    (
                        bal.map(|h| h.join().unwrap()),
                        scr.map(|h| h.join().unwrap()),
                        refs.map(|h| h.join().unwrap()),
                    )
                });
            if let Some(existing) = existing_balances {
                let existing = existing?;
                batch_new_addresses = count_new_addresses(&address_balance_changes, &existing);
                self.writer.apply_address_balance_deltas(
                    &existing,
                    &address_balance_changes,
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
            if let Some(existing) = existing_script_references {
                updated_script_references = self.writer.apply_script_reference_usage_deltas(
                    &existing?,
                    &script_reference_usage_changes,
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

        // addr_tx writes are deferred until participant tags are known (see
        // tags_by_addr_tx construction during TxActions processing below).

        // Group A: DAO processing
        {
            // Build DAO field map from parsed blocks so that
            // process_dao_withdrawals_batch can read AR for blocks in this
            // batch (whose headers haven't been committed to the store yet).
            let batch_dao_fields: HashMap<i64, Vec<u8>> = all_parsed_blocks
                .iter()
                .map(|p| (p.number, p.dao.to_vec()))
                .collect();

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
                        DaoParser::parse_deposits_from_cells(&tx_data.hash, &tx_data.cells)?;
                    for deposit in dao_deposits {
                        all_dao_deposits.push((deposit, parsed.number, parsed.timestamp, ar));
                    }
                }
            }
            if !all_dao_deposits.is_empty() {
                self.writer
                    .insert_dao_deposits_batch(&all_dao_deposits, &mut data_batch)?;
            }

            // `consumed_dao_map`, `dao_withdraw_outpoints`, and
            // `dao_compensations` were resolved before the tx index was
            // staged (see the phase-2 fee correction above).

            // Build a same-batch deposit map for deposits created in this
            // batch that may also be consumed within the same batch.
            let mut same_batch_dao_deposits: crate::sync::dao_helpers::DaoConsumedMap =
                HashMap::new();
            // Post-batch DAO entries for deposits this batch creates or moves to
            // phase 1. `process_dao_withdrawals_batch` keeps this in sync with
            // what it stages into `data_batch`, so it doubles as the read-your-
            // writes overlay for pre-commit daily snapshot materialization.
            let mut pending_dao_entries: HashMap<
                [u8; 34],
                ckbadger_store::types::DaoDepositCacheEntry,
            > = HashMap::new();
            for (deposit, block_number, ts, ar) in &all_dao_deposits {
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
                    crate::sync::dao_helpers::DaoConsumedRow {
                        tx_hash: deposit.tx_hash.clone(),
                        output_index: deposit_output_index,
                        capacity_str: deposit.capacity.to_string(),
                        deposit_block: *block_number,
                        status: 0, // active
                        lock_script_hash: deposit.lock_script_hash.clone(),
                    },
                );
                let outpoint_key =
                    ckbadger_store::keys::encode_outpoint(&deposit.tx_hash, deposit_output_index);
                pending_dao_entries.insert(
                    outpoint_key,
                    ckbadger_store::types::DaoDepositCacheEntry {
                        capacity: deposit.capacity,
                        occupied_capacity: deposit.occupied_capacity,
                        deposit_block_number: *block_number,
                        deposit_timestamp: ts.timestamp_millis(),
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
                    let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                    block_tx_idx += tx_count_for_block;
                    for tx_data in tx_slice {
                        if tx_data.is_cellbase || tx_data.inputs.is_empty() {
                            continue;
                        }
                        let mut consumed_deposits: Vec<crate::sync::dao_helpers::DaoConsumedRow> =
                            Vec::new();
                        for input in &tx_data.inputs {
                            let key = (
                                input.previous_tx_hash.to_vec(),
                                parsed_input_outpoint_index_i16(
                                    input.previous_output_index,
                                    "sync_indexer",
                                )?,
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
                                if DaoParser::is_dao_code_hash(type_code_hash)
                                    && cell.data_size == 8
                                {
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
                        &mut pending_dao_entries,
                        &batch_dao_fields,
                    )?;
                }

                // Phase-2 completions are staged into `data_batch` but not into
                // `pending_dao_entries`, so record them separately as
                // (deposit outpoint -> completion block + tx). The store derives
                // the completed entry — and therefore the claimed compensation —
                // from the committed phase-1 entry through its own single
                // frozen-request-AR path, so nothing is recomputed here.
                for ctx in &withdrawal_contexts {
                    for row in &ctx.consumed_deposits {
                        if row.status != 1 {
                            continue;
                        }
                        let outpoint_key = keys::encode_outpoint(&row.tx_hash, row.output_index);
                        staged_dao_completions.insert(
                            outpoint_key.to_vec(),
                            (ctx.block_number, ctx.consuming_tx_hash.clone()),
                        );
                    }
                }
            }

            staged_dao_entries = pending_dao_entries
                .into_iter()
                .map(|(outpoint_key, entry)| (outpoint_key.to_vec(), entry))
                .collect();
        }

        // Group B: UDT processing
        {
            struct UdtTxContext {
                tx_hash: Vec<u8>,
                block_number: i64,
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
                    for (output_index, udt_cell) in self.parse_udt_cells_with_store_fallback(tx)? {
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
                        output_udts,
                        input_outpoints,
                    });
                }
            }

            let mut input_udt_info: HashMap<
                (Vec<u8>, i16),
                (Vec<u8>, Vec<u8>, i16, Vec<u8>, Vec<u8>, u128, String),
            > = HashMap::new();
            if !all_input_outpoints_udt.is_empty() {
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
                        output_udts: tx_info.output_udts,
                        input_outpoints: tx_info.input_outpoints,
                    });
                }
            }

            if !udt_tx_contexts.is_empty() {
                let max_supply_observations = collect_token_max_supply_observations(&all_tx_data);
                // Types carried by inputs created before this batch, projected
                // from the prefetched input view this write already resolved —
                // no second resolution path. Every input of every non-cellbase
                // tx is guaranteed to be in `input_cell_info` or in this
                // batch's own outputs (the unresolved-input check above
                // fail-fasts otherwise), so the union the co-occurrence rule
                // sees is exactly the bulk reducer's resolved-input set. Using
                // `input_udt_info` here would be narrower than bulk: it drops
                // owner-mode (amount-less) and not-yet-registered typed inputs,
                // which must still veto a "mint" classification.
                let persisted_input_types: OutpointTypeHashes<'_> = input_cell_info
                    .iter()
                    .filter_map(|((tx_hash, output_index), info)| {
                        info.type_script_hash.as_deref().map(|type_script_hash| {
                            (
                                (tx_hash.as_slice(), i32::from(*output_index)),
                                type_script_hash,
                            )
                        })
                    })
                    .collect();
                let onchain_token_info =
                    collect_token_onchain_info(&all_tx_data, &persisted_input_types);
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
                        } else if let Some(udt_cell) = batch_udt_cells.get(&(tx_hash.clone(), *idx))
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
                        &onchain_token_info,
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

        // Dotbit account IDs resolved during consume, needed later by activity builder.
        let mut resolved_dotbit_ids: HashMap<(Vec<u8>, i16), Vec<u8>> = HashMap::new();
        // `.bit Cell` identity IDs resolved from the outpoint index. Legacy
        // testnet cells have empty type args, so the activity builder must use
        // the parser result retained by the identity write path.
        let mut resolved_bit_cell_ids: HashMap<(Vec<u8>, i16), Vec<u8>> = HashMap::new();

        // Group C: Object/Spore processing
        //
        // IMPORTANT: Within each transaction, inputs (consumes) MUST be
        // processed BEFORE outputs (inserts).  This matches the bulk-build
        // engine ordering (apply_input → apply_output) and ensures that
        // spore/mNFT transfers leave is_live=true after the output re-creates
        // the entity.  The previous outputs-first ordering caused transferred
        // entities to be left as is_live=false (the consume ran after the
        // insert), which then triggered "already consumed" errors when the
        // entity was next consumed in a later batch.
        {
            let mut batch_mnft_token_outpoints: HashMap<(Vec<u8>, i16), Vec<u8>> = HashMap::new();
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

            // --- Pre-pass: collect all input outpoints for batch DB lookup ---
            let bulk_sync_active = self.is_bulk_sync_active();
            let mut all_prev_tx_hashes: Vec<Vec<u8>> = Vec::new();
            let mut all_prev_indices: Vec<i16> = Vec::new();
            // For each flat input index, record (block_idx, tx_idx_in_block, dotbit_consume_order, tx_global_index).
            let mut input_meta: Vec<(usize, usize, u64, usize)> = Vec::new();
            {
                let mut pre_block_tx_idx = 0usize;
                for (block_idx, block_response) in blocks.iter().enumerate() {
                    let parsed = &all_parsed_blocks[block_idx];
                    let tx_count_for_block =
                        checked_tx_count(parsed.transactions_count, parsed.number)?;
                    let tx_slice =
                        &all_tx_data[pre_block_tx_idx..pre_block_tx_idx + tx_count_for_block];
                    for (tx_idx, tx_data) in tx_slice.iter().enumerate() {
                        if tx_data.is_cellbase || tx_data.inputs.is_empty() {
                            continue;
                        }
                        let tx_global_index = pre_block_tx_idx + tx_idx;
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
                            input_meta.push((
                                block_idx,
                                tx_idx,
                                dotbit_consume_order,
                                tx_global_index,
                            ));
                        }
                    }
                    pre_block_tx_idx += tx_count_for_block;
                }
            }

            // --- Batch DB lookups (efficient: one query per entity type) ---
            let dotbit_results = if !all_prev_tx_hashes.is_empty() {
                self.writer.get_dotbit_account_ids_by_outpoints_batch(
                    &all_prev_tx_hashes,
                    &all_prev_indices,
                )?
            } else {
                Vec::new()
            };
            let spore_db_results = if !all_prev_tx_hashes.is_empty() && !bulk_sync_active {
                self.writer
                    .get_spore_ids_by_outpoints_batch(&all_prev_tx_hashes, &all_prev_indices)?
            } else {
                Vec::new()
            };
            let mnft_db_results = if !all_prev_tx_hashes.is_empty() && !bulk_sync_active {
                self.writer
                    .get_mnft_token_ids_by_outpoints_batch(&all_prev_tx_hashes, &all_prev_indices)?
            } else {
                Vec::new()
            };

            // Build base maps from DB results (immutable for the batch).
            let mut spore_map: HashMap<(Vec<u8>, i16), Vec<u8>> = spore_db_results
                .into_iter()
                .map(|(h, i, id)| ((h, i), id))
                .collect();
            let mut mnft_map: HashMap<(Vec<u8>, i16), Vec<u8>> = mnft_db_results
                .into_iter()
                .map(|(h, i, id)| ((h, i), id))
                .collect();
            let mut dotbit_map: HashMap<(Vec<u8>, i16), Vec<u8>> = dotbit_results
                .into_iter()
                .map(|(h, i, id)| ((h, i), id))
                .collect();

            // Group flat input indices by (block_idx, tx_idx) for per-tx access.
            let mut inputs_by_tx: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
            for (flat_idx, &(block_idx, tx_idx, _, _)) in input_meta.iter().enumerate() {
                inputs_by_tx
                    .entry((block_idx, tx_idx))
                    .or_default()
                    .push(flat_idx);
            }

            // --- Main pass: per-tx with inputs-before-outputs ---
            let mut block_tx_idx = 0usize;
            for (block_idx, block_response) in blocks.iter().enumerate() {
                let parsed = &all_parsed_blocks[block_idx];
                let tx_count_for_block =
                    checked_tx_count(parsed.transactions_count, parsed.number)?;
                let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                let ts_ms = parsed.timestamp.timestamp_millis();
                for (tx_idx, tx_data) in tx_slice.iter().enumerate() {
                    let tx_global_index = block_tx_idx + tx_idx;
                    let tx = &block_response.block.transactions[tx_idx];

                    // Parse DAS action early — needed by both input and output processing.
                    if !tx_data.is_cellbase {
                        if let Some(action) = DotbitParser::parse_das_action(&tx.witnesses) {
                            das_action_cache.insert(tx_data.hash, action);
                        }
                    }

                    // ===== INPUTS FIRST (consumes) =====
                    if let Some(flat_indices) = inputs_by_tx.get(&(block_idx, tx_idx)) {
                        for &flat_idx in flat_indices {
                            let key = (
                                all_prev_tx_hashes[flat_idx].clone(),
                                all_prev_indices[flat_idx],
                            );
                            let (_, _, dotbit_consume_order, consume_tx_global_index) =
                                input_meta[flat_idx];

                            // Augment maps with in-batch cache (outpoints from earlier txs).
                            if !bulk_sync_active && !spore_map.contains_key(&key) {
                                if let Some(spore_id) =
                                    spore_state.get_cached_spore_id_by_outpoint(&key.0, key.1)
                                {
                                    spore_map.insert(key.clone(), spore_id);
                                }
                            }
                            if !bulk_sync_active && !mnft_map.contains_key(&key) {
                                if let Some(token_id) = batch_mnft_token_outpoints.get(&key) {
                                    mnft_map.insert(key.clone(), token_id.clone());
                                }
                            }
                            if !dotbit_map.contains_key(&key) {
                                if let Some(account_id) = batch_dotbit_outpoints.get(&key) {
                                    dotbit_map.insert(key.clone(), account_id.clone());
                                }
                            }

                            if !bulk_sync_active {
                                // Spore consumption
                                if let Some(spore_id) = spore_map.get(&key) {
                                    let identity_collection_id = input_cell_info
                                        .get(&key)
                                        .or_else(|| batch_cell_infos.get(&key))
                                        .and_then(|info| info.type_code_hash.as_deref())
                                        .and_then(|type_code_hash| {
                                            if BitCellParser::is_type_script(type_code_hash) {
                                                Some(BIT_CELL_SENTINEL_COLLECTION.as_slice())
                                            } else if SporeParser::is_did_type_script(
                                                type_code_hash,
                                            ) {
                                                Some(DID_CKB_SENTINEL_COLLECTION.as_slice())
                                            } else {
                                                None
                                            }
                                        });
                                    let object_collection_id = self.writer.consume_spore(
                                        spore_id,
                                        parsed.number,
                                        &tx_data.hash,
                                        &mut data_batch,
                                        &mut spore_state,
                                    )?;
                                    if let Some(collection_id) = identity_collection_id {
                                        if collection_id == BIT_CELL_SENTINEL_COLLECTION {
                                            resolved_bit_cell_ids
                                                .insert(key.clone(), spore_id.clone());
                                        }
                                        identity_activity_acc.record(
                                            collection_id,
                                            &tx_data.hash,
                                            spore_id,
                                            &parsed.hash,
                                            parsed.number,
                                            checked_usize_to_i32(tx_idx, "tx_idx")?,
                                            ts_ms,
                                            false,
                                        );
                                    } else if let Some(coll_id) = object_collection_id {
                                        object_activity_acc.record(
                                            &coll_id,
                                            &tx_data.hash,
                                            spore_id,
                                            &parsed.hash,
                                            parsed.number,
                                            checked_usize_to_i32(tx_idx, "tx_idx")?,
                                            ts_ms,
                                            false,
                                        );
                                    }
                                } else if let Some(info) = input_cell_info
                                    .get(&key)
                                    .or_else(|| batch_cell_infos.get(&key))
                                {
                                    if let Some(tch) = info.type_code_hash.as_ref() {
                                        if SporeParser::is_spore_type_script(tch)
                                            || BitCellParser::is_type_script(tch)
                                        {
                                            bail!(
                                                "spore outpoint-id mapping missing for consumed spore cell: block={}, tx=0x{}, prev_outpoint=0x{}:{}",
                                                parsed.number,
                                                hex::encode(tx_data.hash),
                                                hex::encode(&key.0),
                                                key.1
                                            );
                                        }
                                    }
                                }

                                // mNFT consumption
                                if let Some(token_id) = mnft_map.get(&key) {
                                    let should_consume = should_consume_grouped_mnft_token(
                                        batch_mnft_last_output_tx_index.get(token_id).copied(),
                                        consume_tx_global_index,
                                    );
                                    if should_consume {
                                        if let Some(coll_id) =
                                            self.writer.consume_mnft_token_with_state(
                                                token_id,
                                                parsed.number,
                                                &tx_data.hash,
                                                &mut data_batch,
                                                &mut mnft_state,
                                            )?
                                        {
                                            object_activity_acc.record(
                                                &coll_id,
                                                &tx_data.hash,
                                                token_id,
                                                &parsed.hash,
                                                parsed.number,
                                                checked_usize_to_i32(tx_idx, "tx_idx")?,
                                                ts_ms,
                                                false,
                                            );
                                        }
                                    }
                                } else if let Some(info) = input_cell_info
                                    .get(&key)
                                    .or_else(|| batch_cell_infos.get(&key))
                                {
                                    if let Some(tch) = info.type_code_hash.as_ref() {
                                        if MnftParser::is_token_type_script(tch) {
                                            bail!(
                                                "mNFT outpoint-id mapping missing for consumed mNFT cell: block={}, tx=0x{}, prev_outpoint=0x{}:{}",
                                                parsed.number,
                                                hex::encode(tx_data.hash),
                                                hex::encode(&key.0),
                                                key.1
                                            );
                                        }
                                    }
                                }
                            }

                            // DotBit consumption (runs in all sync modes)
                            let dotbit_account_id = resolve_live_dotbit_account_id_for_consume(
                                &key,
                                &input_cell_info,
                                &batch_cell_infos,
                                &dotbit_map,
                            );
                            if let Some(ref id) = dotbit_account_id {
                                resolved_dotbit_ids.insert(key.clone(), id.clone());
                            }
                            if let Some(account_id) = dotbit_account_id.as_ref() {
                                let latest_create_order =
                                    batch_dotbit_latest_create_order.get(account_id).copied();
                                if should_consume_dotbit_account(
                                    latest_create_order,
                                    dotbit_consume_order,
                                ) && self
                                    .writer
                                    .consume_dotbit_account_with_state(
                                        account_id,
                                        parsed.number,
                                        &tx_data.hash,
                                        &mut data_batch,
                                        &mut dotbit_state,
                                    )?
                                    .is_some()
                                {
                                    let tx_key: [u8; 32] = tx_data.hash.as_slice()[..32]
                                        .try_into()
                                        .expect("tx_hash must be 32 bytes");
                                    let activity = dotbit_tx_activity_data
                                        .entry(tx_key)
                                        .or_insert_with(|| DotbitTxActivityData {
                                            das_action: das_action_cache.get(&tx_key).cloned(),
                                            created_account_ids: HashSet::new(),
                                            consumed_account_ids: HashSet::new(),
                                            block_number: parsed.number,
                                            block_hash: parsed.hash.clone(),
                                            tx_idx: checked_usize_to_i32(tx_idx, "tx_idx")
                                                .expect("tx_idx overflow"),
                                            timestamp_ms: ts_ms,
                                        });
                                    activity.consumed_account_ids.insert(account_id.clone());
                                }
                            }
                        }
                    }

                    // ===== OUTPUTS SECOND (inserts) =====
                    let dotbit_create_order = dotbit_create_event_order(tx_global_index)?;

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
                                checked_usize_to_i32(tx_idx, "tx_idx")?,
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
                                checked_usize_to_i32(tx_idx, "tx_idx")?,
                                ts_ms,
                                true,
                            );
                        }
                    }
                    for bit_cell_output in BitCellParser::parse_cells(tx)? {
                        self.writer.insert_bit_cell(
                            &bit_cell_output.cell,
                            &tx_data.hash,
                            bit_cell_output.output_index,
                            parsed.number,
                            &mut data_batch,
                            &mut spore_state,
                        )?;
                        identity_activity_acc.record(
                            &BIT_CELL_SENTINEL_COLLECTION,
                            &tx_data.hash,
                            &bit_cell_output.cell.identity_id,
                            &parsed.hash,
                            parsed.number,
                            checked_usize_to_i32(tx_idx, "tx_idx")?,
                            ts_ms,
                            true,
                        );
                    }
                    for (output_index, issuer) in MnftParser::parse_issuers_with_output_indices(tx)
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
                            &mut mnft_state,
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
                            &mut data_batch,
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
                            checked_usize_to_i32(tx_idx, "tx_idx")?,
                            ts_ms,
                            true,
                        );
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
                                tx_idx: checked_usize_to_i32(tx_idx, "tx_idx")?,
                                timestamp_ms: ts_ms,
                            },
                        );
                    }
                }
                block_tx_idx += tx_count_for_block;
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
                    anyhow!("object activity delta overflow while writing grouped batch")
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

            mnft_state.extend_pending_collection_aggregates(&mut pending_object_collection_aggs);
            spore_state.extend_pending_cluster_ids(&mut pending_cluster_ids);

            // Apply cumulative capacity deltas to cluster aggregates
            if !cluster_daily_changes.is_empty() {
                self.writer.apply_cluster_capacity_deltas(
                    &cluster_daily_changes,
                    &mut data_batch,
                    &mut spore_state,
                )?;
            }

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
                Box::new(crate::db::writer::rgbpp_detector::RgbppDetector::new())
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
        // Per-(block, tx_idx, lock_hash) participant tag bitmap. Populated as
        // TxActions are built below, then consumed by the deferred addr_tx
        // write loop so each `AddrTxValue.tags` matches its TxActions sibling.
        let mut tags_by_addr_tx: HashMap<(i64, i32, Vec<u8>), u16> = HashMap::new();
        {
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
                            ActivityInputIndexes {
                                cell_info: &input_cell_info,
                                batch_cell_info: &batch_cell_infos,
                                dao_withdraw_outpoints: &dao_withdraw_outpoints,
                                dao_compensations: &dao_compensations,
                                dotbit_ids: &resolved_dotbit_ids,
                                bit_cell_identity_ids: &resolved_bit_cell_ids,
                            },
                        )?;
                        let outputs: Vec<crate::db::writer::activities::OutputCellView<'_>> = td
                            .cells
                            .iter()
                            .map(|cell| crate::db::writer::activities::OutputCellView {
                                capacity: cell.capacity,
                                lock_code_hash: &cell.lock_code_hash,
                                lock_hash_type: cell.lock_hash_type,
                                lock_args: &cell.lock_args,
                                lock_script_hash: &cell.lock_script_hash,
                                type_code_hash: cell.type_code_hash.as_deref(),
                                type_hash_type: cell.type_hash_type,
                                type_args: cell.type_args.as_deref(),
                                type_script_hash: cell.type_script_hash.as_deref(),
                                data_hash: &cell.data_hash,
                                data_size: cell.data_size,
                                data: &cell.data,
                            })
                            .collect();
                        Ok(crate::db::writer::activities::TxView {
                            tx_hash: &td.hash,
                            block_hash: &parsed.hash,
                            tx_index: td.tx_index,
                            block_number: parsed.number,
                            timestamp: parsed.timestamp.timestamp_millis(),
                            is_cellbase: td.is_cellbase,
                            inputs,
                            outputs,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;

                let tx_actions_list = crate::db::writer::activities::build_tx_actions_for_block(
                    &tx_views,
                    &protocol_detectors,
                )?;

                for tx_actions in &tx_actions_list {
                    // Capture participant tags so the deferred addr_tx writes
                    // populate AddrTxValue.tags consistently with TxActions.
                    for participant in &tx_actions.participants {
                        tags_by_addr_tx.insert(
                            (
                                tx_actions.block_number,
                                tx_actions.tx_index,
                                participant.lock_hash.clone(),
                            ),
                            participant.tags,
                        );
                    }

                    // Accumulate daily activity stats
                    let date = ckbadger_common::block_date_from_ms(tx_actions.timestamp)
                        .format("%Y%m%d")
                        .to_string();
                    let day_stats = daily_activity_accum.entry(date.clone()).or_default();
                    BatchWriter::accumulate_tx_activity_stats(tx_actions, day_stats);

                    // Accumulate hourly activity stats. Activity hour keys
                    // are UTC+8 (`block_datetime_from_ms`) — unlike the
                    // chain-level hourly stats, whose keys are UTC.
                    let hour = ckbadger_common::block_datetime_from_ms(tx_actions.timestamp)
                        .format("%Y%m%d%H")
                        .to_string();
                    let hour_stats = hourly_activity_accum.entry(hour.clone()).or_default();
                    BatchWriter::accumulate_tx_activity_stats(tx_actions, hour_stats);

                    // Unique address counts (exclude coinbase)
                    if !tx_actions.is_cellbase {
                        for participant in &tx_actions.participants {
                            if participant.lock_hash.len() == 32 {
                                let mut hash = [0u8; 32];
                                hash.copy_from_slice(&participant.lock_hash);
                                daily_activity_addrs
                                    .entry(date.clone())
                                    .or_default()
                                    .insert(hash);
                                hourly_activity_addrs
                                    .entry(hour.clone())
                                    .or_default()
                                    .insert(hash);
                            }
                        }
                    }

                    // Skip cellbase from CF_TX_ACTIONS — matches bulk build behavior.
                    // API filters them at read time (activity_ops.rs), and activity stats
                    // accumulation uses the in-memory tx_actions_list above.
                    if tx_actions.is_cellbase {
                        continue;
                    }

                    put_tx_actions(
                        &mut activity_batch,
                        &mut append_undo_seq_by_block,
                        tx_actions.block_number,
                        tx_actions,
                    );

                    // Process Fiber channel lifecycle events
                    crate::db::writer::fiber::process_fiber_channel_events(
                        &mut activity_batch,
                        tx_actions,
                    )
                    .map_err(|source| PreCommitInvariantError::new("fiber lifecycle", source))?;
                }
            }
        }

        // Deferred addr_tx writes. Each entry pairs with a TxActions participant
        // populated above; the participant's tag bitmap drives `AddrTxValue.tags`.
        for entry in &addr_tx_entries {
            let tags = *tags_by_addr_tx
                .get(&(entry.block_number, entry.tx_index, entry.lock_hash.clone()))
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "missing participant tags for addr_tx: block={}, tx_idx={}, lock_hash=0x{}",
                        entry.block_number,
                        entry.tx_index,
                        hex::encode(&entry.lock_hash)
                    )
                })?;
            let addr_tx_value = ckbadger_store::types::AddrTxValue::new(
                entry.capacity_change,
                entry.has_in,
                entry.has_out,
                tags,
            );
            put_addr_tx(
                &mut append_history_batch,
                &mut append_undo_seq_by_block,
                &entry.lock_hash,
                entry.block_number,
                entry.tx_index,
                &entry.tx_hash,
                &addr_tx_value,
            );
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
        let mut prev_dao_csu: Option<(i128, i128, i128)> =
            if let Some(first_block) = all_parsed_blocks.first() {
                if first_block.number > 0 {
                    self.writer
                        .store()
                        .get_block_header(first_block.number - 1)?
                        .and_then(|h| extract_dao_csu(&h.dao))
                } else {
                    None
                }
            } else {
                None
            };

        // Pre-build consumed DAO deposit map for delta computation
        let dao_code_hash_for_stats =
            crate::rpc::parse_hex_to_bytes(crate::parser::dao::DAO_CODE_HASH);
        let mut all_input_outpoints_for_dao: Vec<(Vec<u8>, i16)> = Vec::new();
        for tx in all_tx_data.iter().filter(|tx| !tx.is_cellbase) {
            for input in &tx.inputs {
                all_input_outpoints_for_dao.push((
                    input.previous_tx_hash.to_vec(),
                    parsed_input_outpoint_index_i16(input.previous_output_index, "sync_indexer")?,
                ));
            }
        }
        let consumed_dao_for_stats = if !all_input_outpoints_for_dao.is_empty() {
            let unique: Vec<(Vec<u8>, i16)> = {
                let mut seen = HashSet::new();
                all_input_outpoints_for_dao
                    .into_iter()
                    .filter(|x| seen.insert(x.clone()))
                    .collect()
            };
            let refs: Vec<(&[u8], i16)> = unique.iter().map(|(h, i)| (h.as_slice(), *i)).collect();
            self.writer.find_consumed_dao_deposits_batch(&refs)?
        } else {
            HashMap::new()
        };
        let mut same_batch_dao_for_stats: HashMap<(Vec<u8>, i16), i64> = HashMap::new();
        let mut active_dao_deposit_counts_by_lock: HashMap<Vec<u8>, i64> = HashMap::new();
        let mut ever_deposited_by_lock: HashMap<Vec<u8>, bool> = HashMap::new();
        {
            let mut touched_lock_hashes: HashSet<Vec<u8>> = HashSet::new();
            for tx_data in &all_tx_data {
                for cell in &tx_data.cells {
                    if cell
                        .type_code_hash
                        .as_deref()
                        .is_some_and(DaoParser::is_dao_code_hash)
                    {
                        touched_lock_hashes.insert(cell.lock_script_hash.clone());
                    }
                }
                if tx_data.is_cellbase {
                    continue;
                }
                for input in &tx_data.inputs {
                    let key = (
                        input.previous_tx_hash.to_vec(),
                        parsed_input_outpoint_index_i16(
                            input.previous_output_index,
                            "sync_indexer",
                        )?,
                    );
                    if let Some(info) = input_cell_info
                        .get(&key)
                        .or_else(|| batch_cell_infos.get(&key))
                        .filter(|info| {
                            info.type_code_hash
                                .as_deref()
                                .is_some_and(DaoParser::is_dao_code_hash)
                        })
                    {
                        touched_lock_hashes.insert(info.lock_script_hash.clone());
                    }
                }
            }

            for lock_hash in touched_lock_hashes {
                let mut active_count = 0i64;
                let mut has_any_deposit = false;
                self.writer
                    .store()
                    .scan_dao_deposits_by_lock(&lock_hash, |_, entry| {
                        has_any_deposit = true;
                        // Only status=0 (deposit) counts as active.  The runtime
                        // decrements at phase-1 (status 0→1 withdraw request),
                        // not at phase-2, so status=1 deposits have already been
                        // decremented and must NOT be included in the seed count.
                        if entry.status == 0 {
                            active_count = active_count.checked_add(1).ok_or_else(|| {
                                anyhow!(
                                    "active DAO deposit count overflow while seeding unique depositor tracking: lock_hash=0x{}",
                                    hex::encode(&lock_hash)
                                )
                            })?;
                        }
                        Ok(())
                    })?;
                active_dao_deposit_counts_by_lock.insert(lock_hash.clone(), active_count);
                ever_deposited_by_lock.insert(lock_hash, has_any_deposit);
            }
        }

        let mut block_tx_idx = 0usize;
        for parsed in all_parsed_blocks {
            let block_date = ckbadger_common::block_date(parsed.timestamp);
            let tx_count_for_block = checked_tx_count(parsed.transactions_count, parsed.number)?;
            let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
            let claimed_compensation_in_block =
                tx_slice_claimed_dao_compensation(tx_slice, &dao_compensations)?;
            accumulate_secondary_issuance_deltas(
                &mut batch_stats,
                parsed,
                block_date,
                claimed_compensation_in_block,
                &mut prev_dao_csu,
            )?;
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
                .map(|cell| i64::from(cell.data_size))
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

            batch_stats.sync_totals.0 = batch_stats
                .sync_totals
                .0
                .checked_add(i64::from(parsed.transactions_count))
                .ok_or_else(|| {
                    anyhow!(
                        "sync_totals transaction count overflow at block {}",
                        parsed.number
                    )
                })?;
            batch_stats.sync_totals.1 = batch_stats
                .sync_totals
                .1
                .checked_add(i64::from(cells_created))
                .ok_or_else(|| {
                    anyhow!(
                        "sync_totals cells_created overflow at block {}",
                        parsed.number
                    )
                })?;
            batch_stats.sync_totals.2 = batch_stats
                .sync_totals
                .2
                .checked_add(i64::from(cells_consumed))
                .ok_or_else(|| {
                    anyhow!(
                        "sync_totals cells_consumed overflow at block {}",
                        parsed.number
                    )
                })?;
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
                .insert(block_date, parsed.dao.to_vec());
            {
                // Chain-level hourly bucket: keyed by the UTC-truncated block
                // hour (`STATS_PREFIX_HOURLY` keys are UTC `%Y%m%d%H`; the
                // activity hourly family below is UTC+8 instead). The tx
                // count includes the cellbase. Reorg rollback and the bulk
                // ChainStatsAccumulator must mirror these semantics exactly.
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
                let difficulty_u256 = ckb_compact_to_difficulty(parsed.compact_target as u32);
                let difficulty_u64: u64 = difficulty_u256.to_string().parse().map_err(|_| {
                    anyhow!(
                        "difficulty exceeds u64 range: block={}, date={}, compact_target={:#x}, difficulty={}",
                        parsed.number, block_date, parsed.compact_target, difficulty_u256
                    )
                })?;
                entry.0 += difficulty_u64 as i128;
                entry.1 += 1;
                entry.2 += parsed.uncles_count;
            }
            // Miner attribution uses the cellbase WITNESS lock (the block's
            // own miner, RFC-0022) — the cellbase output lock instead pays the
            // reward of the block 11 confirmations back.
            if let Some(miner_lock_hash) = parsed.miner_lock_hash.as_ref() {
                let key = (block_date, miner_lock_hash.clone());
                let entry = batch_stats.miner_stats.entry(key).or_insert((0, 0));
                entry.0 += 1;
                entry.1 = parsed.number;
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
                batch_stats.accumulate_block_time(prev_ts, parsed.timestamp, block_date);
            }
            prev_timestamp = Some(parsed.timestamp);

            if parsed.epoch_index == 0 && parsed.epoch_number > 0 {
                if let Some((prev_epoch_num, prev_start_ts, _)) = prev_epoch {
                    if prev_epoch_num == parsed.epoch_number - 1 {
                        let duration_secs = (parsed.timestamp - prev_start_ts).num_seconds();
                        let epoch_duration_minutes = duration_secs as f64 / 60.0;
                        let bucket_minutes = i32::try_from(epoch_duration_minutes.round() as i64)
                            .unwrap_or(i32::MAX);
                        if bucket_minutes <= 0 {
                            anyhow::bail!(
                                "epoch time distribution: invalid bucket_minutes={} \
                                 for epoch {} (prev_epoch={}, duration_secs={}, \
                                 block={})",
                                bucket_minutes,
                                parsed.epoch_number,
                                prev_epoch_num,
                                duration_secs,
                                parsed.number,
                            );
                        }
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
                &input_cell_info,
                &batch_cell_infos,
                &mut active_dao_deposit_counts_by_lock,
                &mut batch_stats.dao_daily_unique_depositors_delta,
                &mut batch_stats.dao_daily_active_delta,
                &mut batch_stats.dao_daily_protocol_delta,
                &mut batch_stats.dao_daily_gross_deposit_delta,
                &mut batch_stats.dao_daily_new_deposits_delta,
                &mut batch_stats.dao_daily_withdrawals_delta,
                &mut ever_deposited_by_lock,
                &mut batch_stats.dao_daily_cumulative_depositors_delta,
                &mut batch_stats.dao_daily_depositing_addresses,
            )?;

            batch_stats.dao_snapshot_dates.insert(block_date);
            batch_stats
                .dao_block_numbers_by_date
                .entry(block_date)
                .or_default()
                .push(parsed.number);
        }
        batch_stats.dao_deltas_computed = true;
        let write_ms = t_write.elapsed().as_secs_f64() * 1000.0;

        // Finalization: block headers + stats
        let t_finalize = Instant::now();
        {
            let mut core_batch = StoreBatch::new(self.writer.store());
            self.writer
                .insert_blocks_batch(&block_refs, &mut core_batch)?;
            let mut stats_batch = StoreBatch::new(self.writer.store());
            self.write_batch_stats_to_batch(
                &batch_stats,
                &staged_dao_entries,
                &staged_dao_completions,
                &mut stats_batch,
            )?;
            // Write accumulated daily activity stats
            let empty_addr_set = HashSet::new();
            for (date, stats) in &daily_activity_accum {
                let addrs = daily_activity_addrs.get(date).unwrap_or(&empty_addr_set);
                self.writer
                    .update_daily_activity_stats(date, stats, addrs, &mut stats_batch)?;
            }
            // Write accumulated hourly activity stats
            for (hour, stats) in &hourly_activity_accum {
                let addrs = hourly_activity_addrs.get(hour).unwrap_or(&empty_addr_set);
                self.writer
                    .update_hourly_activity_stats(hour, stats, addrs, &mut stats_batch)?;
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

            let commit_started = Instant::now();
            // Live sync: merge headers and stats into the single data_batch
            // that already holds all domain writes, then commit atomically.
            data_batch.merge_from(core_batch);
            data_batch.merge_from(stats_batch);
            let lock_hash_refs: Vec<&Vec<u8>> = address_balance_changes.keys().collect();
            let prefetched_address_balances = if lock_hash_refs.is_empty() {
                HashMap::new()
            } else {
                self.writer.read_address_balances(&lock_hash_refs)?
            };
            let prepared_hodl_tracker = self.prepare_hodl_wave_batch(
                all_parsed_blocks,
                &all_tx_data,
                &input_cell_info,
                &batch_cell_infos,
                &prefetched_address_balances,
                &mut data_batch,
            )?;
            let prepared_cell_dist_tracker = self.prepare_cell_distribution_batch(
                all_parsed_blocks,
                &all_tx_data,
                &input_cell_info,
                &batch_cell_infos,
                &prefetched_address_balances,
                &mut data_batch,
            )?;
            // Merge script reference rollup writes into the same atomic batch,
            // eliminating the crash window between data_batch.commit() and a
            // separate post-commit refresh.
            self.writer
                .materialize_script_versions_and_families(
                    &updated_script_references,
                    &mut data_batch,
                )
                .with_context(|| {
                    format!(
                        "script reference rollup materialize failed for blocks {}-{}",
                        first_block, last_block
                    )
                })?;

            debug!(
                phase = "domain_atomic_commit",
                batch_start = first_block,
                batch_end = last_block,
                bulk_sync_mode,
                "Atomic domain batch commit start"
            );
            // Commit append-only cell payloads immediately before the domain
            // batch to minimise the non-atomic window between the two stores.
            //
            // SAFETY: append-only commits first because orphan cell payloads (crash
            // after CF_CELLS commit, before domain commit) are inert — content-addressed
            // by outpoint, never referenced until the domain batch also lands, and
            // overwritten with identical data on re-sync.  The reverse order (domain
            // first) would leave live_cell_markers pointing at missing payloads,
            // breaking cross-store reads.  Startup cleanup only inspects domain state;
            // orphan payloads in CF_CELLS are harmless and require no rollback.
            if !cells_batch.is_empty() {
                cells_batch.commit().with_context(|| {
                    format!(
                        "append-only cell payload commit failed for blocks {}-{}",
                        first_block, last_block
                    )
                })?;
            }
            data_batch.commit().with_context(|| {
                format!(
                    "atomic domain commit failed for blocks {}-{}",
                    first_block, last_block
                )
            })?;
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
            {
                let mut tracker = self.hodl_tracker.lock().unwrap();
                *tracker = prepared_hodl_tracker;
            }
            {
                let mut tracker = self.cell_dist_tracker.lock().unwrap();
                *tracker = prepared_cell_dist_tracker;
            }
        }

        // Spawn proposal cache AFTER batch commit to avoid writing proposals
        // for a batch that failed to commit.
        if !batch_proposals.is_empty() {
            tokio::spawn(Self::run_proposal_cache_batch(
                self.rpc.clone(),
                self.cache_invalidator.clone(),
                batch_proposals,
                last_proposal_block,
            ));
        }

        // In-memory cache notification only — the DB sync_status update was
        // already committed atomically in the finalize batch above.
        if let Some((block_number, ref block_hash)) = batch_stats.last_block {
            let ema_rate = self.progress.ema_blocks_per_second();
            let ema_rate_opt = if ema_rate > 0.0 { Some(ema_rate) } else { None };
            // Completed days were materialized exact inside the atomic commit
            // above. This only re-derives the tip-scoped DAO singletons and the
            // still-incomplete tip day.
            self.writer.refresh_latest_dao_statistics()?;
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

        let committed_proposal_ids = collect_committed_proposal_ids(&all_tx_data);
        if !committed_proposal_ids.is_empty() {
            self.cache_invalidator
                .remove_committed_proposals(&committed_proposal_ids)
                .await;
        }
        let finalize_ms = t_finalize.elapsed().as_secs_f64() * 1000.0;

        let batch_tx_count = all_tx_data.len();
        let batch_cell_count: usize = all_tx_data.iter().map(|t| t.cells.len()).sum();
        let batch_input_count: usize = all_tx_data
            .iter()
            .filter(|t| !t.is_cellbase)
            .map(|t| t.inputs.len())
            .sum();
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
        Ok(BatchWriteMetrics {
            commit_ms: write_commit_ms,
            write_ms,
            prefetch_ms: 0.0,
            finalize_ms,
            txs: u64::try_from(batch_tx_count).expect("parsed batch tx count exceeds u64"),
            cells: u64::try_from(batch_cell_count).expect("parsed batch cell count exceeds u64"),
            inputs: u64::try_from(batch_input_count).expect("parsed batch input count exceeds u64"),
        })
    }

    fn write_batch_stats_to_batch(
        &self,
        stats: &BatchStats,
        staged_dao_entries: &HashMap<Vec<u8>, ckbadger_store::types::DaoDepositCacheEntry>,
        staged_dao_completions: &HashMap<Vec<u8>, (i64, Vec<u8>)>,
        batch: &mut StoreBatch,
    ) -> Result<()> {
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
        for (date, (sum_difficulty, count, uncles)) in &stats.daily_block_stats {
            let avg_difficulty = if *count > 0 {
                (*sum_difficulty / *count as i128) as f64
            } else {
                0.0
            };
            self.writer.update_daily_block_stats_batch(
                *date,
                avg_difficulty,
                *count,
                *uncles,
                batch,
            )?;
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
                let mut running_protocol_deposited = latest_snapshot
                    .as_ref()
                    .map(|s| s.protocol_deposited.unwrap_or(s.total_deposited))
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
                // Carried forward for the still-incomplete tip day only; every
                // completed day below replaces these with its own exact
                // end-of-day lifecycle values before this batch commits.
                let mut staged_cum_dao = latest_snapshot
                    .as_ref()
                    .map(|s| s.cum_dao_compensation)
                    .unwrap_or(0);
                let mut staged_cum_treasury = latest_snapshot
                    .as_ref()
                    .map(|s| s.cum_treasury)
                    .unwrap_or(0);
                // Every date this batch wrote except the last is complete: no
                // later block can land on it.
                let completed_boundaries: HashMap<NaiveDate, i64> =
                    completed_dao_snapshot_boundaries(stats)?
                        .into_iter()
                        .map(|boundary| (boundary.date, boundary.end_block))
                        .collect();
                let mut running_total_depositors = latest_snapshot
                    .as_ref()
                    .map(|s| s.depositors_count)
                    .unwrap_or(0);
                let mut running_cumulative_depositors = latest_snapshot
                    .as_ref()
                    .map(|s| s.cumulative_depositors)
                    .unwrap_or(0);

                for date in snapshot_dates {
                    running_total_deposited +=
                        stats.dao_daily_active_delta.get(date).copied().unwrap_or(0);
                    running_protocol_deposited += stats
                        .dao_daily_protocol_delta
                        .get(date)
                        .copied()
                        .unwrap_or(0);
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

                    let daily_miner = stats
                        .daily_secondary_miner_delta
                        .get(date)
                        .copied()
                        .unwrap_or(0);
                    running_cum_miner += daily_miner;
                    running_total_depositors = derive_running_depositors(
                        running_total_depositors,
                        stats
                            .dao_daily_unique_depositors_delta
                            .get(date)
                            .copied()
                            .unwrap_or(0),
                        *date,
                    )?;
                    running_cumulative_depositors += stats
                        .dao_daily_cumulative_depositors_delta
                        .get(date)
                        .copied()
                        .unwrap_or(0);

                    // Compute unique depositor addresses for this day.
                    // Merge depositors from previous batches (already in store) with
                    // current batch depositors to get the full daily unique count.
                    let daily_depositor_addresses = {
                        let batch_blocks = stats.dao_block_numbers_by_date.get(date);
                        // Find earlier blocks on the same date from the store.
                        let mut all_blocks_on_date: Vec<i64> = Vec::new();
                        if let Some(blocks) = batch_blocks {
                            if let Some(&first_batch_block) = blocks.first() {
                                // Scan backwards for blocks on the same date before this batch
                                let mut check_block = first_batch_block - 1;
                                while check_block >= 0 {
                                    if let Ok(Some(hdr)) =
                                        self.writer.store().get_block_header(check_block)
                                    {
                                        let hdr_date =
                                            ckbadger_common::block_date_from_ms(hdr.timestamp);
                                        if hdr_date == *date {
                                            all_blocks_on_date.push(check_block);
                                            check_block -= 1;
                                        } else {
                                            break;
                                        }
                                    } else {
                                        break;
                                    }
                                }
                            }
                        }
                        // Get depositors from earlier blocks via store scan
                        let mut depositors = if all_blocks_on_date.is_empty() {
                            std::collections::HashSet::new()
                        } else {
                            self.writer
                                .store()
                                .collect_depositor_lock_hashes_for_blocks(&all_blocks_on_date)?
                        };
                        // Merge with current batch depositors
                        if let Some(batch_set) = stats.dao_daily_depositing_addresses.get(date) {
                            depositors.extend(batch_set.iter().cloned());
                        }
                        depositors.len() as i64
                    };

                    let dao_bytes = stats
                        .daily_dao_fields
                        .get(date)
                        .ok_or_else(|| anyhow!("missing DAO field for snapshot date {}", date))?;
                    let ar = DaoParser::extract_ar_from_dao_field(dao_bytes).ok_or_else(|| {
                        anyhow!(
                            "invalid DAO AR field for snapshot date {}: dao_len={}",
                            date,
                            dao_bytes.len()
                        )
                    })?;
                    let end_block = stats
                        .dao_block_numbers_by_date
                        .get(date)
                        .and_then(|blocks| blocks.iter().max())
                        .copied()
                        .ok_or_else(|| {
                            anyhow!("missing DAO end block for snapshot date {}", date)
                        })?;
                    let completed_end_block = completed_boundaries.get(date).copied();
                    let unmade_dao_interests = if completed_end_block.is_some() {
                        // Replaced below by the exact end-of-day lifecycle value,
                        // which already includes this batch's staged deposits.
                        0
                    } else {
                        self.writer
                            .store()
                            .compute_unmade_dao_interests(end_block, ar)?
                    };

                    let mut dao_snapshot = crate::db::writer::DaoSnapshotInput {
                        total_deposited: running_total_deposited,
                        depositors_count: running_total_depositors,
                        total_deposit_count: running_total_deposit_count,
                        total_withdrawal_count: running_total_withdrawal_count,
                        // Carried forward from the previous snapshot. This is
                        // only ever persisted for the still-incomplete tip day,
                        // which the post-commit `refresh_latest_dao_statistics`
                        // re-evaluates at the committed tip.
                        total_compensation: staged_cum_dao,
                        cumulative_deposit_amount: running_cumulative_deposit_amount,
                        total_issuance,
                        secondary_pool,
                        occupied_capacity,
                        cum_miner_secondary: running_cum_miner,
                        cum_dao_compensation: staged_cum_dao,
                        cum_treasury: staged_cum_treasury,
                        unmade_dao_interests,
                        unclaimed_compensation: 0,
                        cumulative_depositors: running_cumulative_depositors,
                        daily_depositor_addresses,
                        protocol_deposited: Some(running_protocol_deposited),
                    };

                    // A completed day never changes again, so it must not be
                    // persisted with carried-forward placeholders and corrected
                    // afterwards: a crash in that window froze the day at the
                    // preceding batch's cumulative values (DAO-026). Evaluate it
                    // here — at its own final block and AR, against this batch's
                    // staged DAO lifecycle — and let the same atomic commit
                    // carry both the values and the sync tip that certifies them.
                    if let Some(boundary_end_block) = completed_end_block {
                        if boundary_end_block != end_block {
                            bail!(
                                "completed DAO snapshot boundary disagrees with the batch end block: date={}, boundary_block={}, end_block={}",
                                date,
                                boundary_end_block,
                                end_block
                            );
                        }
                        self.writer.apply_exact_completed_dao_snapshot(
                            DaoSnapshotBoundary {
                                date: *date,
                                end_block,
                            },
                            ar,
                            secondary_pool,
                            staged_dao_entries,
                            staged_dao_completions,
                            &mut dao_snapshot,
                        )?;
                        // Thread the exact end-of-day totals forward so a later
                        // day in this same batch continues from them.
                        staged_cum_dao = dao_snapshot.cum_dao_compensation;
                        staged_cum_treasury = dao_snapshot.cum_treasury;
                    }

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
    use std::sync::Arc;

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
            semantic_tags: 0,
        }
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
    fn test_apply_object_collection_activity_count_deltas_updates_only_object_collections() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let object_collection_id = vec![0x11; 32];
        let cluster_id = vec![0x22; 32];

        let mut seed = StoreBatch::new(&store);
        seed.put_mnft_collection_aggregate(
            &object_collection_id,
            &ckbadger_store::types::MnftCollectionAggregate {
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
            .get_mnft_collection_aggregate(&object_collection_id)
            .unwrap()
            .unwrap();
        assert_eq!(agg.activities_count, 5);
    }

    #[test]
    fn test_apply_object_collection_activity_count_deltas_uses_pending_batch_aggregate() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let collection_id = vec![0x33; 32];
        let pending_agg = ckbadger_store::types::MnftCollectionAggregate {
            standard: ckbadger_store::types::ObjectStandard::MnftClass,
            name: Some("fresh collection".to_string()),
            ..Default::default()
        };

        let mut batch = StoreBatch::new(&store);
        batch.put_mnft_collection_aggregate(&collection_id, &pending_agg);

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
            .get_mnft_collection_aggregate(&collection_id)
            .unwrap()
            .unwrap();
        assert_eq!(agg.activities_count, 1);
        assert_eq!(agg.name.as_deref(), Some("fresh collection"));
    }

    #[test]
    fn test_apply_object_collection_activity_count_deltas_uses_pending_cluster_ids() {
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
            .get_mnft_collection_aggregate(&cluster_id)
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

        let empty_cell_info = HashMap::new();
        let empty_batch_info = HashMap::new();
        let empty_dao_outpoints = HashSet::new();
        let empty_dao_comp = HashMap::new();
        let empty_dotbit = HashMap::new();
        let err = match build_activity_input_views(
            &tx,
            99,
            ActivityInputIndexes {
                cell_info: &empty_cell_info,
                batch_cell_info: &empty_batch_info,
                dao_withdraw_outpoints: &empty_dao_outpoints,
                dao_compensations: &empty_dao_comp,
                dotbit_ids: &empty_dotbit,
                bit_cell_identity_ids: &empty_dotbit,
            },
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

        let empty_cell_info = HashMap::new();
        let empty_dao_outpoints = HashSet::new();
        let empty_dao_comp = HashMap::new();
        let empty_dotbit = HashMap::new();
        let inputs = build_activity_input_views(
            &tx,
            100,
            ActivityInputIndexes {
                cell_info: &empty_cell_info,
                batch_cell_info: &batch_cell_infos,
                dao_withdraw_outpoints: &empty_dao_outpoints,
                dao_compensations: &empty_dao_comp,
                dotbit_ids: &empty_dotbit,
                bit_cell_identity_ids: &empty_dotbit,
            },
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

        let empty_batch_info = HashMap::new();
        let empty_dotbit = HashMap::new();
        let inputs = build_activity_input_views(
            &tx,
            200,
            ActivityInputIndexes {
                cell_info: &input_cell_info,
                batch_cell_info: &empty_batch_info,
                dao_withdraw_outpoints: &dao_withdraw_outpoints,
                dao_compensations: &dao_compensations,
                dotbit_ids: &empty_dotbit,
                bit_cell_identity_ids: &empty_dotbit,
            },
        )
        .unwrap();
        assert_eq!(inputs.len(), 1);
        assert!(inputs[0].is_dao_withdraw_request);
        assert_eq!(inputs[0].dao_compensation, Some(5_00000000));
    }

    #[test]
    fn test_build_activity_input_views_carries_legacy_bit_cell_identity_id() {
        let previous_tx_hash = [0x67; 32];
        let outpoint = (previous_tx_hash.to_vec(), 1_i16);
        let tx = dummy_tx_data(
            [0x34; 32],
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
            crate::parser::bit_cell::BIT_CELL_CODE_HASH_TESTNET,
        ));
        info.type_args = Some(Vec::new());
        let mut input_cell_info = HashMap::new();
        input_cell_info.insert(outpoint.clone(), PositionedCellInfo::new(info, 1));
        let identity_id = vec![0x81; 32];
        let bit_cell_ids = HashMap::from([(outpoint, identity_id.clone())]);
        let empty_batch_info = HashMap::new();
        let empty_dao_outpoints = HashSet::new();
        let empty_dao_compensations = HashMap::new();
        let empty_dotbit_ids = HashMap::new();

        let inputs = build_activity_input_views(
            &tx,
            201,
            ActivityInputIndexes {
                cell_info: &input_cell_info,
                batch_cell_info: &empty_batch_info,
                dao_withdraw_outpoints: &empty_dao_outpoints,
                dao_compensations: &empty_dao_compensations,
                dotbit_ids: &empty_dotbit_ids,
                bit_cell_identity_ids: &bit_cell_ids,
            },
        )
        .unwrap();

        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].bit_cell_identity_id, Some(identity_id.as_slice()));
        assert!(inputs[0].data.is_empty());
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
            max_supply: None,
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
            semantic_tags: 0,
        }
    }

    fn withdraw_consuming_tx(
        request_outpoint: ([u8; 32], i32),
        extra_inputs: Vec<crate::parser::transaction::ParsedInput>,
        total_input_capacity: i64,
        total_output_capacity: i64,
        parser_fee: i64,
    ) -> TxData {
        let mut inputs = vec![crate::parser::transaction::ParsedInput {
            previous_tx_hash: request_outpoint.0,
            previous_output_index: request_outpoint.1,
            since: 0,
        }];
        inputs.extend(extra_inputs);
        let mut tx = dummy_tx_data([0xd3; 32], false, inputs, vec![], vec![], vec![]);
        tx.total_input_capacity = total_input_capacity;
        tx.total_output_capacity = total_output_capacity;
        tx.fee = parser_fee;
        tx
    }

    #[test]
    fn test_correct_dao_withdrawal_fees_replaces_placeholder_zero() {
        let request = ([0xd2u8; 32], 0i32);
        let mut txs = vec![withdraw_consuming_tx(
            request,
            vec![],
            100_000_000_000,
            100_897_999_473,
            0,
        )];
        let outpoints: HashSet<(Vec<u8>, i16)> = [(request.0.to_vec(), 0i16)].into();
        let compensations: HashMap<(Vec<u8>, i16), i64> =
            [((request.0.to_vec(), 0i16), 8_98000000i64)].into();

        correct_dao_withdrawal_fees(&mut txs, &outpoints, &compensations).unwrap();
        assert_eq!(txs[0].fee, 527);
    }

    #[test]
    fn test_correct_dao_withdrawal_fees_recomputes_nonzero_parser_fee() {
        // Extra plain input keeps raw inputs >= outputs, so the parser
        // computed a NON-zero fee that undercounts compensation. The
        // criterion is the consumed withdraw-request outpoint, so the fee
        // must still be recomputed.
        let request = ([0xd2u8; 32], 0i32);
        let mut txs = vec![withdraw_consuming_tx(
            request,
            vec![crate::parser::transaction::ParsedInput {
                previous_tx_hash: [0xd1; 32],
                previous_output_index: 1,
                since: 0,
            }],
            280_000_000_000,
            279_898_000_000,
            1_02000000,
        )];
        let outpoints: HashSet<(Vec<u8>, i16)> = [(request.0.to_vec(), 0i16)].into();
        let compensations: HashMap<(Vec<u8>, i16), i64> =
            [((request.0.to_vec(), 0i16), 8_98000000i64)].into();

        correct_dao_withdrawal_fees(&mut txs, &outpoints, &compensations).unwrap();
        assert_eq!(txs[0].fee, 10_00000000);
    }

    #[test]
    fn test_correct_dao_withdrawal_fees_leaves_non_withdraw_txs_untouched() {
        // Phase-1 (withdraw request) and plain txs consume no
        // withdraw-request outpoint: the parser fee stands.
        let mut txs = vec![withdraw_consuming_tx(
            ([0xaa; 32], 0),
            vec![],
            1_000,
            900,
            100,
        )];
        let outpoints: HashSet<(Vec<u8>, i16)> = [(vec![0xd2u8; 32], 0i16)].into();
        let compensations: HashMap<(Vec<u8>, i16), i64> =
            [((vec![0xd2u8; 32], 0i16), 8_98000000i64)].into();

        correct_dao_withdrawal_fees(&mut txs, &outpoints, &compensations).unwrap();
        assert_eq!(txs[0].fee, 100);
    }

    #[test]
    fn test_correct_dao_withdrawal_fees_errors_on_missing_compensation() {
        let request = ([0xd2u8; 32], 0i32);
        let mut txs = vec![withdraw_consuming_tx(
            request,
            vec![],
            100_000_000_000,
            100_897_999_473,
            0,
        )];
        let outpoints: HashSet<(Vec<u8>, i16)> = [(request.0.to_vec(), 0i16)].into();
        let compensations: HashMap<(Vec<u8>, i16), i64> = HashMap::new();

        let err = correct_dao_withdrawal_fees(&mut txs, &outpoints, &compensations).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("missing pre-computed DAO compensation"));
        assert!(msg.contains(&hex::encode([0xd3u8; 32])));
        assert!(msg.contains(&hex::encode([0xd2u8; 32])));
    }

    #[test]
    fn test_correct_dao_withdrawal_fees_errors_on_negative_fee() {
        // Outputs exceed raw inputs + compensation: invariant violation must
        // fail fast instead of storing a bogus fee.
        let request = ([0xd2u8; 32], 0i32);
        let mut txs = vec![withdraw_consuming_tx(
            request,
            vec![],
            100_000_000_000,
            100_900_000_000,
            0,
        )];
        let outpoints: HashSet<(Vec<u8>, i16)> = [(request.0.to_vec(), 0i16)].into();
        let compensations: HashMap<(Vec<u8>, i16), i64> =
            [((request.0.to_vec(), 0i16), 8_98000000i64)].into();

        let err = correct_dao_withdrawal_fees(&mut txs, &outpoints, &compensations).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("negative fee after DAO compensation"));
        assert!(msg.contains(&hex::encode([0xd3u8; 32])));
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
    fn test_completed_dao_snapshot_boundaries_selects_prior_dates() {
        let first = chrono::NaiveDate::from_ymd_opt(2026, 7, 23).unwrap();
        let second = first + chrono::Duration::days(1);
        let latest = second + chrono::Duration::days(1);
        let mut stats = BatchStats::default();
        stats.dao_snapshot_dates.extend([latest, first, second]);
        stats
            .dao_block_numbers_by_date
            .insert(first, vec![100, 101]);
        stats
            .dao_block_numbers_by_date
            .insert(second, vec![102, 103]);
        stats.dao_block_numbers_by_date.insert(latest, vec![104]);

        assert_eq!(
            completed_dao_snapshot_boundaries(&stats).unwrap(),
            vec![
                DaoSnapshotBoundary {
                    date: first,
                    end_block: 101,
                },
                DaoSnapshotBoundary {
                    date: second,
                    end_block: 103,
                },
            ]
        );
    }

    #[test]
    fn test_completed_dao_snapshot_boundaries_require_end_block() {
        let completed = chrono::NaiveDate::from_ymd_opt(2026, 7, 24).unwrap();
        let latest = completed + chrono::Duration::days(1);
        let mut stats = BatchStats::default();
        stats.dao_snapshot_dates.extend([completed, latest]);
        stats.dao_block_numbers_by_date.insert(latest, vec![104]);

        let error = completed_dao_snapshot_boundaries(&stats).unwrap_err();
        assert!(error
            .to_string()
            .contains("missing end block for completed DAO snapshot date 2026-07-24"));
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

    #[test]
    fn test_pipeline_bulk_path_is_disabled_after_build_engine_cutover() {
        let err = ensure_pipeline_bulk_path_disabled(true, 100, 120, 1_500).unwrap_err();
        let msg = err.to_string();

        assert!(msg.contains("pipeline bulk path is disabled"));
        assert!(msg.contains("100-120"));
        assert!(msg.contains("chain_tip=1500"));
        assert!(msg.contains("run through bulk build engine first"));
    }

    // ── Live write-path DAO phase-2 fee regression tests ─────────────────
    //
    // Drives the real live-sync write path (parse → parser fee pass →
    // `write_parsed_batch`) across four single-block batches:
    // funding cellbase → DAO deposit → withdraw request (phase 1) →
    // withdrawal completion (phase 2), then asserts the fee persisted in
    // `TxIndexEntry` for the phase-2 tx. This is the storage-level truth the
    // API serves; the parser can only write a placeholder for phase-2 txs
    // because DAO compensation is unknown at parse time.
    //
    // This module also owns the single in-crate live write-path driver
    // (`indexer_for_live_write_test` + `write_live_block`); sibling test
    // modules reuse it instead of standing up a second harness.

    mod live_dao_fee {
        use super::*;
        use crate::config::Config;
        use crate::db::writer::cell_distribution::CellDistributionTracker;
        use crate::db::writer::hodl_wave::HodlWaveTracker;
        use crate::db::Repository;
        use crate::rpc::{
            BlockView, CellInput, CellOutput, HeaderView, OutPoint, Script, TransactionView,
        };
        use crate::runtime_diag::FlightRecorder;
        use crate::sync::pipeline::compute_parser_input_capacities_and_fees;
        use crate::sync::progress::SyncProgress;
        use crate::sync::shutdown::ShutdownSignal;
        use crate::sync::types::CachedCellInfo;
        use crate::sync::TEST_CELLBASE_WITNESS;
        use ckbadger_store::types::LiveCellInfo;
        use ckbadger_store::StoreRuntimeConfig;
        use std::path::PathBuf;
        use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8};

        const SECP_CODE_HASH: &str =
            "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8";
        const DAO_CODE_HASH: &str =
            "0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e";
        /// AR at the deposit block (101) and at the withdraw-request block
        /// (102). With a 1000 CKB deposit whose occupied capacity is the
        /// standard 102 CKB, free capacity is 898 CKB and the compensation is
        /// exactly 898_00000000 * 101/100 - 898_00000000 = 8_98000000.
        const AR_DEPOSIT: u64 = 10_000_000_000_000_000;
        const AR_REQUEST: u64 = 10_100_000_000_000_000;
        const DAO_COMPENSATION: i64 = 8_98000000;
        const DEPOSIT_CAPACITY: u64 = 1000_00000000;
        const FUNDING_CAPACITY: u64 = 3000_00000000;
        const CHANGE_CAPACITY: u64 = 1800_00000000;

        pub(super) fn lock_script() -> Script {
            Script {
                code_hash: SECP_CODE_HASH.to_string(),
                hash_type: "type".to_string(),
                args: format!("0x{}", "01".repeat(20)),
            }
        }

        fn dao_type_script() -> Script {
            Script {
                code_hash: DAO_CODE_HASH.to_string(),
                hash_type: "type".to_string(),
                args: "0x".to_string(),
            }
        }

        fn header(number: u64, ar: u64) -> HeaderView {
            // DAO field: C (total issuance) and U (occupied) must satisfy
            // C > U for the secondary-miner split; AR drives compensation.
            let mut dao = [0u8; 32];
            dao[0..8].copy_from_slice(&3_360_000_000_000_000_000u64.to_le_bytes());
            dao[8..16].copy_from_slice(&ar.to_le_bytes());
            dao[24..32].copy_from_slice(&100_000_000_000_000u64.to_le_bytes());
            // Epoch 40 (length 1800) starts at block 100, so the first
            // fixture batch opens the epoch stats row exactly like a real
            // epoch boundary block would.
            let epoch = (1800u64 << 40) | ((number - 100) << 24) | 40;
            HeaderView {
                version: "0x0".to_string(),
                compact_target: "0x1a08a97e".to_string(),
                timestamp: format!("0x{:x}", 1_700_000_000_000u64 + number * 1000),
                number: format!("0x{number:x}"),
                epoch: format!("0x{epoch:x}"),
                parent_hash: format!("0x{}", "11".repeat(32)),
                transactions_root: format!("0x{}", "22".repeat(32)),
                proposals_hash: format!("0x{}", "33".repeat(32)),
                extra_hash: format!("0x{}", "44".repeat(32)),
                dao: format!("0x{}", hex::encode(dao)),
                nonce: "0x1".to_string(),
                hash: format!(
                    "0x{}",
                    hex::encode({
                        let mut h = [0x55u8; 32];
                        h[0..8].copy_from_slice(&number.to_le_bytes());
                        h
                    })
                ),
            }
        }

        pub(super) fn cellbase_tx(hash_byte: u8, capacity: u64) -> TransactionView {
            TransactionView {
                hash: format!("0x{}", hex::encode([hash_byte; 32])),
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
                    capacity: format!("0x{capacity:x}"),
                    lock: lock_script(),
                    type_: None,
                }],
                outputs_data: vec!["0x".to_string()],
                witnesses: vec![TEST_CELLBASE_WITNESS.to_string()],
            }
        }

        pub(super) fn block(
            number: u64,
            ar: u64,
            transactions: Vec<TransactionView>,
        ) -> BlockResponseWithCycles {
            BlockResponseWithCycles {
                block: BlockView {
                    header: header(number, ar),
                    uncles: vec![],
                    transactions,
                    proposals: vec![],
                },
                cycles: None,
            }
        }

        pub(super) fn indexer_for_live_write_test(store: Arc<CkbadgerStore>) -> Indexer {
            // Live-path writers derive knowledge_size from DAO `U` minus the
            // genesis baseline; seed a baseline with zero virtual occupied
            // capacity so the fixture headers' small U stays non-negative.
            store
                .set_genesis_baseline(&ckbadger_store::GenesisBaseline {
                    total_issuance: 3_360_000_000_000_000_000,
                    burnt: 840_000_000_000_000_000,
                    virtual_occupied: 0,
                })
                .expect("seed genesis baseline");
            let config = Config {
                domain_data_path: "unused-domain".to_string(),
                append_only_data_path: "unused-append".to_string(),
                bulk_sync_perf_output_root: String::new(),
                build_version: "test".to_string(),
                ckb_rpc_url: "http://127.0.0.1:1".to_string(),
                poll_interval_ms: 1000,
                start_block: None,
                bulk_sync_threshold: 72,
                bulk_memory_budget_gb: None,
                fast_sync_mode: true,
                ckb_db_path: "unused-ckb-db".to_string(),
                metadata_path: None,
                network: "mainnet".to_string(),
                force_startup_cleanup: false,
                store_runtime_config: StoreRuntimeConfig::default(),
                decoder_cache_path: "unused-decoder-cache".to_string(),
                dob_decode_dir: "unused-dob-decode".to_string(),
                cycles_request_dir: None,
            };
            let cache_invalidator = crate::cache::CacheInvalidator::new(store.clone());
            Indexer {
                run_id: "live-dao-fee-test".to_string(),
                config,
                rpc: CkbRpcClient::new("http://127.0.0.1:1"),
                repo: Repository::new(store.clone()),
                writer: BatchWriter::new(store.clone(), store.clone()),
                append_only_store: store.clone(),
                progress: Arc::new(SyncProgress::new(0, 0)),
                cell_cache: Arc::new(DashMap::<([u8; 32], i16), CachedCellInfo>::new()),
                udt_cell_cache: Arc::new(DashMap::new()),
                perf: PerfStats::default(),
                parser_cell_lookup_stats: Arc::new(ParserCellLookupStats::default()),
                pipeline_perf: Arc::new(PipelinePerfStats::default()),
                bulk_build_perf: Arc::new(BulkBuildPerfStats::default()),
                adaptive_batch_controller: Arc::new(LiveBatchController::new()),
                cache_invalidator,
                last_cache_invalidation: tokio::sync::Mutex::new(0),
                was_bulk_sync_active: AtomicBool::new(false),
                bulk_sync_allowed: AtomicBool::new(false),
                rebuild_pause_flag: Arc::new(AtomicBool::new(false)),
                pipeline_reset_notify_flag: Arc::new(AtomicBool::new(false)),
                pipeline_reset_reason_code: Arc::new(AtomicU8::new(0)),
                startup_phase: AtomicU8::new(STARTUP_PHASE_NONE),
                pipeline_reset_epoch: Arc::new(AtomicU64::new(0)),
                incident_seq: AtomicU64::new(0),
                flight_recorder: FlightRecorder::new(FLIGHT_RECORDER_CAPACITY),
                repeated_warning_tracker: RepeatedWarningTracker::default(),
                incident_dir: PathBuf::from("unused-incidents"),
                bulk_sync_perf_run: std::sync::Mutex::new(None),
                shutdown: ShutdownSignal::default(),
                ckb_store: None,
                hodl_tracker: std::sync::Mutex::new(HodlWaveTracker::new()),
                cell_dist_tracker: std::sync::Mutex::new(CellDistributionTracker::new()),
            }
        }

        /// Mirror of the parser Pass 3 address-delta accumulation for the
        /// fixture: balances, live/total cell counts, and occupied deltas per
        /// lock hash. `write_parsed_batch` persists AddressBalance rows from
        /// this map, and the HODL/cell-distribution trackers baseline their
        /// per-lock live counts on those rows.
        fn accumulate_fixture_address_deltas(
            all_tx_data: &[TxData],
            input_cell_info: &HashMap<(Vec<u8>, i16), PositionedCellInfo>,
            batch_cell_infos: &HashMap<(Vec<u8>, i16), PositionedCellInfo>,
        ) -> Result<HashMap<Vec<u8>, AddressBalanceDelta>> {
            let mut changes: HashMap<Vec<u8>, AddressBalanceDelta> = HashMap::new();
            for tx_data in all_tx_data {
                let mut balance: HashMap<Vec<u8>, i128> = HashMap::new();
                let mut created: HashMap<Vec<u8>, i32> = HashMap::new();
                let mut consumed: HashMap<Vec<u8>, i32> = HashMap::new();
                let mut used: HashMap<Vec<u8>, i128> = HashMap::new();
                if !tx_data.is_cellbase {
                    for input in &tx_data.inputs {
                        let key = (
                            input.previous_tx_hash.to_vec(),
                            parsed_input_outpoint_index_i16(
                                input.previous_output_index,
                                "live fee test address deltas",
                            )?,
                        );
                        let info = input_cell_info
                            .get(&key)
                            .or_else(|| batch_cell_infos.get(&key))
                            .ok_or_else(|| {
                                anyhow!(
                                    "fixture input cell missing: outpoint=0x{}:{}",
                                    hex::encode(&key.0),
                                    key.1
                                )
                            })?;
                        *balance.entry(info.lock_script_hash.clone()).or_default() -=
                            i128::from(info.capacity);
                        *consumed.entry(info.lock_script_hash.clone()).or_default() += 1;
                        *used.entry(info.lock_script_hash.clone()).or_default() -=
                            i128::from(info.occupied_capacity);
                    }
                }
                for cell in &tx_data.cells {
                    *balance.entry(cell.lock_script_hash.clone()).or_default() +=
                        i128::from(cell.capacity);
                    *created.entry(cell.lock_script_hash.clone()).or_default() += 1;
                    *used.entry(cell.lock_script_hash.clone()).or_default() +=
                        i128::from(occupied_capacity_shannons_i64(
                            cell.lock_args.len(),
                            cell.type_args.as_ref().map(|args| args.len()),
                            cell.data_size,
                        ));
                }
                let all_addresses: HashSet<Vec<u8>> = balance
                    .keys()
                    .chain(created.keys())
                    .chain(consumed.keys())
                    .chain(used.keys())
                    .cloned()
                    .collect();
                for lock_hash in all_addresses {
                    let entry = changes
                        .entry(lock_hash.clone())
                        .or_insert(AddressBalanceDelta {
                            balance_delta: 0,
                            live_delta: 0,
                            total_delta: 0,
                            tx_delta: 0,
                            used_delta: 0,
                            first_seen_block: tx_data.block_number,
                            first_seen_tx: tx_data.hash.to_vec(),
                            last_activity_block: tx_data.block_number,
                            last_activity_tx: tx_data.hash.to_vec(),
                        });
                    entry.balance_delta += balance.get(&lock_hash).copied().unwrap_or(0);
                    entry.live_delta += created.get(&lock_hash).copied().unwrap_or(0)
                        - consumed.get(&lock_hash).copied().unwrap_or(0);
                    entry.total_delta += created.get(&lock_hash).copied().unwrap_or(0);
                    entry.tx_delta += 1;
                    entry.used_delta += used.get(&lock_hash).copied().unwrap_or(0);
                    entry.last_activity_block = tx_data.block_number;
                    entry.last_activity_tx = tx_data.hash.to_vec();
                }
            }
            Ok(changes)
        }

        /// Drive one block through the same steps the live pipeline performs:
        /// raw parse, batch cell info construction, input prefetch from the
        /// store, the parser fee pass, then `write_parsed_batch`.
        pub(super) async fn write_live_block(
            indexer: &Indexer,
            block: BlockResponseWithCycles,
        ) -> Result<()> {
            let blocks = vec![block];
            let (all_parsed_blocks, mut all_tx_data, all_input_outpoints) =
                parse_blocks_parallel(&blocks)?;

            let mut batch_cell_infos: HashMap<(Vec<u8>, i16), PositionedCellInfo> = HashMap::new();
            for tx_data in &all_tx_data {
                for (output_index, cell) in tx_data.cells.iter().enumerate() {
                    let output_index_i16 =
                        checked_usize_to_i16(output_index, "live fee test output index")?;
                    let occupied_capacity = occupied_capacity_shannons_i64(
                        cell.lock_args.len(),
                        cell.type_args.as_ref().map(|args| args.len()),
                        cell.data_size,
                    );
                    // Mirror the parser's Pass 1 (`pipeline.rs`): UDT outputs
                    // carry their parsed amount into `batch_cell_infos`, which
                    // is what the write path persists as live cell state.
                    let udt_amount =
                        parse_parsed_cell_udt_amount(cell, &tx_data.hash, output_index_i16, None)?;
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

            let outpoint_refs: Vec<(&[u8], i16)> = all_input_outpoints
                .iter()
                .map(|(hash, index)| (hash.as_slice(), *index))
                .collect();
            let input_cell_info = indexer
                .writer
                .get_full_cells_info_batch_chunk(&outpoint_refs)?;

            compute_parser_input_capacities_and_fees(
                &mut all_tx_data,
                &input_cell_info,
                &batch_cell_infos,
            )?;
            let address_balance_changes = accumulate_fixture_address_deltas(
                &all_tx_data,
                &input_cell_info,
                &batch_cell_infos,
            )?;

            let chain_tip = u64::try_from(all_parsed_blocks.last().unwrap().number)?;
            indexer
                .write_parsed_batch(
                    &blocks,
                    &all_parsed_blocks,
                    all_tx_data,
                    input_cell_info,
                    batch_cell_infos,
                    address_balance_changes,
                    HashMap::new(),
                    HashMap::new(),
                    HashMap::new(),
                    HashMap::new(),
                    HashMap::new(),
                    HashMap::new(),
                    HashMap::new(),
                    HashMap::new(),
                    HashMap::new(),
                    chain_tip,
                )
                .await?;
            Ok(())
        }

        /// Blocks 100-102: funding cellbase, DAO deposit, withdraw request.
        fn phase_setup_blocks() -> Vec<BlockResponseWithCycles> {
            // Block 100: funding cellbase (0xc0) with 3000 CKB.
            let block_100 = block(100, AR_DEPOSIT, vec![cellbase_tx(0xc0, FUNDING_CAPACITY)]);

            // Block 101: deposit tx (0xd1) spends the funding cell into a
            // 1000 CKB DAO deposit plus 1800 CKB change (200 CKB fee).
            let deposit_tx = TransactionView {
                hash: format!("0x{}", hex::encode([0xd1; 32])),
                version: "0x0".to_string(),
                cell_deps: vec![],
                header_deps: vec![],
                inputs: vec![CellInput {
                    since: "0x0".to_string(),
                    previous_output: OutPoint {
                        tx_hash: format!("0x{}", hex::encode([0xc0; 32])),
                        index: "0x0".to_string(),
                    },
                }],
                outputs: vec![
                    CellOutput {
                        capacity: format!("0x{DEPOSIT_CAPACITY:x}"),
                        lock: lock_script(),
                        type_: Some(dao_type_script()),
                    },
                    CellOutput {
                        capacity: format!("0x{CHANGE_CAPACITY:x}"),
                        lock: lock_script(),
                        type_: None,
                    },
                ],
                outputs_data: vec![format!("0x{}", "00".repeat(8)), "0x".to_string()],
                witnesses: vec!["0x".to_string()],
            };
            let block_101 = block(
                101,
                AR_DEPOSIT,
                vec![cellbase_tx(0xc1, 500_00000000), deposit_tx],
            );

            // Block 102: withdraw request tx (0xd2) converts the deposit into
            // a request cell (same capacity, data = deposit block number).
            let request_tx = TransactionView {
                hash: format!("0x{}", hex::encode([0xd2; 32])),
                version: "0x0".to_string(),
                cell_deps: vec![],
                header_deps: vec![],
                inputs: vec![CellInput {
                    since: "0x0".to_string(),
                    previous_output: OutPoint {
                        tx_hash: format!("0x{}", hex::encode([0xd1; 32])),
                        index: "0x0".to_string(),
                    },
                }],
                outputs: vec![CellOutput {
                    capacity: format!("0x{DEPOSIT_CAPACITY:x}"),
                    lock: lock_script(),
                    type_: Some(dao_type_script()),
                }],
                outputs_data: vec![format!("0x{}", hex::encode(101u64.to_le_bytes()))],
                witnesses: vec!["0x".to_string()],
            };
            let block_102 = block(
                102,
                AR_REQUEST,
                vec![cellbase_tx(0xc2, 500_00000000), request_tx],
            );

            vec![block_100, block_101, block_102]
        }

        fn stored_fee(store: &CkbadgerStore, tx_hash: [u8; 32]) -> i64 {
            let (_, _, entry) = store
                .get_tx_by_hash(&tx_hash)
                .expect("tx lookup must not fail")
                .expect("phase-2 tx must be indexed");
            entry.fee
        }

        /// A DAO withdrawal-completion (phase-2) tx whose outputs exceed its
        /// raw inputs (the common shape: single request input, single output
        /// carrying capacity + compensation - fee). The parser writes a fee=0
        /// placeholder; the live write path must persist the corrected fee
        /// `raw_inputs + compensation - outputs` in `TxIndexEntry`.
        #[tokio::test]
        async fn test_live_write_path_stores_corrected_dao_phase2_fee() {
            let dir = tempfile::tempdir().unwrap();
            let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
            std::mem::forget(dir);
            let indexer = indexer_for_live_write_test(store.clone());

            for setup_block in phase_setup_blocks() {
                write_live_block(&indexer, setup_block).await.unwrap();
            }

            // Block 103: completion tx (0xd3) consumes the request outpoint.
            // True miner fee mirrors the 2026-07-29 audit case (527 shannons):
            // output = 1000 CKB + compensation - 527.
            let expected_fee: i64 = 527;
            let output_capacity =
                i64::try_from(DEPOSIT_CAPACITY).unwrap() + DAO_COMPENSATION - expected_fee;
            let completion_tx = TransactionView {
                hash: format!("0x{}", hex::encode([0xd3; 32])),
                version: "0x0".to_string(),
                cell_deps: vec![],
                header_deps: vec![],
                inputs: vec![CellInput {
                    since: "0x0".to_string(),
                    previous_output: OutPoint {
                        tx_hash: format!("0x{}", hex::encode([0xd2; 32])),
                        index: "0x0".to_string(),
                    },
                }],
                outputs: vec![CellOutput {
                    capacity: format!("0x{output_capacity:x}"),
                    lock: lock_script(),
                    type_: None,
                }],
                outputs_data: vec!["0x".to_string()],
                witnesses: vec!["0x".to_string()],
            };
            let block_103 = block(
                103,
                10_200_000_000_000_000,
                vec![cellbase_tx(0xc3, 500_00000000), completion_tx],
            );
            write_live_block(&indexer, block_103).await.unwrap();

            assert_eq!(
                stored_fee(&store, [0xd3; 32]),
                expected_fee,
                "phase-2 tx fee stored in TxIndexEntry must include DAO compensation on the input side"
            );
        }

        /// A phase-2 tx that also spends a plain cell so its raw inputs
        /// already exceed its outputs. The parser then computes a NON-zero
        /// fee that undercounts compensation; the write path must still
        /// recompute the fee because the tx consumes a withdraw-request
        /// outpoint — the correction criterion is the input kind, never the
        /// current fee value.
        #[tokio::test]
        async fn test_live_write_path_recomputes_fee_for_phase2_with_plain_inputs() {
            let dir = tempfile::tempdir().unwrap();
            let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
            std::mem::forget(dir);
            let indexer = indexer_for_live_write_test(store.clone());

            for setup_block in phase_setup_blocks() {
                write_live_block(&indexer, setup_block).await.unwrap();
            }

            // Block 103: completion tx (0xd3) consumes the request outpoint
            // (1000 CKB) plus the plain change cell (1800 CKB). True fee is
            // 10 CKB (> compensation), so raw inputs (2800 CKB) exceed the
            // single output (2800 CKB + compensation - 10 CKB) and the parser
            // computes an undercounted placeholder of
            // 10 CKB - compensation = 1_02000000.
            let expected_fee: i64 = 10_00000000;
            let raw_inputs =
                i64::try_from(DEPOSIT_CAPACITY).unwrap() + i64::try_from(CHANGE_CAPACITY).unwrap();
            let output_capacity = raw_inputs + DAO_COMPENSATION - expected_fee;
            let completion_tx = TransactionView {
                hash: format!("0x{}", hex::encode([0xd3; 32])),
                version: "0x0".to_string(),
                cell_deps: vec![],
                header_deps: vec![],
                inputs: vec![
                    CellInput {
                        since: "0x0".to_string(),
                        previous_output: OutPoint {
                            tx_hash: format!("0x{}", hex::encode([0xd2; 32])),
                            index: "0x0".to_string(),
                        },
                    },
                    CellInput {
                        since: "0x0".to_string(),
                        previous_output: OutPoint {
                            tx_hash: format!("0x{}", hex::encode([0xd1; 32])),
                            index: "0x1".to_string(),
                        },
                    },
                ],
                outputs: vec![CellOutput {
                    capacity: format!("0x{output_capacity:x}"),
                    lock: lock_script(),
                    type_: None,
                }],
                outputs_data: vec!["0x".to_string()],
                witnesses: vec!["0x".to_string()],
            };
            let block_103 = block(
                103,
                10_200_000_000_000_000,
                vec![cellbase_tx(0xc3, 500_00000000), completion_tx],
            );
            write_live_block(&indexer, block_103).await.unwrap();

            assert_eq!(
                stored_fee(&store, [0xd3; 32]),
                expected_fee,
                "phase-2 tx with extra plain inputs must have its fee recomputed with compensation"
            );
        }
    }

    // ── Unique Cell binding: live write path vs bulk build ────────────────
    //
    // The issuance co-occurrence rule binds a Unique Cell's token metadata to
    // the single xUDT type *minted* in the same transaction. "Minted" means the
    // type appears on no input of that transaction — including inputs created
    // in earlier batches, which is the normal case for an already-issued token.
    // The bulk reducer vetoes on all resolved inputs, so the live write path
    // must veto on exactly the same set; otherwise the two sync paths persist
    // different metadata for the same chain.

    mod live_token_binding {
        use super::live_dao_fee::{
            block, cellbase_tx, indexer_for_live_write_test, lock_script, write_live_block,
        };
        use super::*;
        use crate::parser::udt::XUDT_CODE_HASH_TYPE;
        use crate::parser::ScriptParser;
        use crate::rpc::{CellInput, CellOutput, OutPoint, Script, TransactionView};
        use crate::sync::materialize_bulk_artifacts_for_test;
        use ckbadger_store::types::TokenInfo;

        const AR: u64 = 10_000_000_000_000_000;
        const FUNDING_CAPACITY: u64 = 3000_00000000;
        const CELL_CAPACITY: u64 = 200_00000000;
        const TOKEN_AMOUNT: u128 = 1_000_000;

        /// Real mainnet Unique Cell payload from the RGB++ Protocol issuance
        /// (tx 0xd088a12852664145773257eb1467cb0feca0d1d478968ce90b7f29bce24e2a4a):
        /// decimal 8, name "RGB++ Protocol", symbol "RGB++".
        const RGBPP_UNIQUE_CELL_DATA_HEX: &str = "080e5247422b2b2050726f746f636f6c055247422b2b";

        fn lock_script_b() -> Script {
            Script {
                args: format!("0x{}", "02".repeat(20)),
                ..lock_script()
            }
        }

        /// xUDT type script whose args are a plain 32-byte owner lock hash (no
        /// extension flags) — the RGB++-style issuance shape the co-occurrence
        /// rule exists for.
        fn xudt_type_script(owner_byte: u8) -> Script {
            Script {
                code_hash: XUDT_CODE_HASH_TYPE.to_string(),
                hash_type: "type".to_string(),
                args: format!("0x{}", format!("{owner_byte:02x}").repeat(32)),
            }
        }

        fn unique_type_script() -> Script {
            Script {
                code_hash: crate::sync::token_helpers::UNIQUE_CELL_CODE_HASH_MAINNET.to_string(),
                hash_type: "type".to_string(),
                args: format!("0x{}", "9a".repeat(20)),
            }
        }

        fn token_type_hash(owner_byte: u8) -> Vec<u8> {
            ScriptParser::compute_script_hash(&xudt_type_script(owner_byte))
        }

        fn amount_data(amount: u128) -> String {
            format!("0x{}", hex::encode(amount.to_le_bytes()))
        }

        fn outpoint(tx_hash_byte: u8, index: u32) -> OutPoint {
            OutPoint {
                tx_hash: format!("0x{}", hex::encode([tx_hash_byte; 32])),
                index: format!("0x{index:x}"),
            }
        }

        fn input(tx_hash_byte: u8, index: u32) -> CellInput {
            CellInput {
                since: "0x0".to_string(),
                previous_output: outpoint(tx_hash_byte, index),
            }
        }

        fn output(capacity: u64, lock: Script, type_: Option<Script>) -> CellOutput {
            CellOutput {
                capacity: format!("0x{capacity:x}"),
                lock,
                type_,
            }
        }

        /// Block 100 funds the fixture; block 101 issues token A (owner byte
        /// 0xa1) with no Unique Cell, so its stored metadata stays empty.
        fn issuance_blocks() -> Vec<BlockResponseWithCycles> {
            let block_100 = block(100, AR, vec![cellbase_tx(0xc0, FUNDING_CAPACITY)]);

            let issue_tx = TransactionView {
                hash: format!("0x{}", hex::encode([0xe1; 32])),
                version: "0x0".to_string(),
                cell_deps: vec![],
                header_deps: vec![],
                inputs: vec![input(0xc0, 0)],
                outputs: vec![
                    output(CELL_CAPACITY, lock_script(), Some(xudt_type_script(0xa1))),
                    output(FUNDING_CAPACITY - CELL_CAPACITY, lock_script(), None),
                ],
                outputs_data: vec![amount_data(TOKEN_AMOUNT), "0x".to_string()],
                witnesses: vec!["0x".to_string()],
            };
            let block_101 = block(101, AR, vec![cellbase_tx(0xc1, 500_00000000), issue_tx]);

            vec![block_100, block_101]
        }

        /// Block 102: a plain transfer of the already-issued token A that also
        /// creates a Unique Cell carrying somebody else's token info. The
        /// token's type is present on an input created in an earlier batch, so
        /// this is NOT an issuance and nothing may be bound.
        fn cross_batch_transfer_block() -> BlockResponseWithCycles {
            let transfer_tx = TransactionView {
                hash: format!("0x{}", hex::encode([0xe2; 32])),
                version: "0x0".to_string(),
                cell_deps: vec![],
                header_deps: vec![],
                // in[0]: token A cell (block 101), in[1]: change cell (block 101)
                inputs: vec![input(0xe1, 0), input(0xe1, 1)],
                outputs: vec![
                    output(CELL_CAPACITY, lock_script(), Some(unique_type_script())),
                    // token A moves to a different lock so the transfer nets out
                    // non-zero and the token row is rewritten in this batch.
                    output(CELL_CAPACITY, lock_script_b(), Some(xudt_type_script(0xa1))),
                    output(FUNDING_CAPACITY - 3 * CELL_CAPACITY, lock_script(), None),
                ],
                outputs_data: vec![
                    format!("0x{RGBPP_UNIQUE_CELL_DATA_HEX}"),
                    amount_data(TOKEN_AMOUNT),
                    "0x".to_string(),
                ],
                witnesses: vec!["0x".to_string()],
            };
            block(102, AR, vec![cellbase_tx(0xc2, 500_00000000), transfer_tx])
        }

        /// Block 102 variant: a genuine issuance of a *new* token B alongside
        /// the Unique Cell. Both sync paths must bind the metadata here.
        fn real_issuance_block() -> BlockResponseWithCycles {
            let issue_tx = TransactionView {
                hash: format!("0x{}", hex::encode([0xe3; 32])),
                version: "0x0".to_string(),
                cell_deps: vec![],
                header_deps: vec![],
                inputs: vec![input(0xe1, 1)],
                outputs: vec![
                    output(CELL_CAPACITY, lock_script(), Some(unique_type_script())),
                    output(CELL_CAPACITY, lock_script_b(), Some(xudt_type_script(0xb2))),
                    output(FUNDING_CAPACITY - 4 * CELL_CAPACITY, lock_script(), None),
                ],
                outputs_data: vec![
                    format!("0x{RGBPP_UNIQUE_CELL_DATA_HEX}"),
                    amount_data(TOKEN_AMOUNT),
                    "0x".to_string(),
                ],
                witnesses: vec!["0x".to_string()],
            };
            block(102, AR, vec![cellbase_tx(0xc2, 500_00000000), issue_tx])
        }

        async fn write_blocks_live(blocks: &[BlockResponseWithCycles]) -> Arc<CkbadgerStore> {
            let dir = tempfile::tempdir().unwrap();
            let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());
            std::mem::forget(dir);
            let indexer = indexer_for_live_write_test(store.clone());
            for block in blocks {
                write_live_block(&indexer, block.clone()).await.unwrap();
            }
            store
        }

        fn assert_same_token_metadata(live: &TokenInfo, bulk: &TokenInfo, label: &str) {
            assert_eq!(live.name, bulk.name, "{label}: token name must match");
            assert_eq!(live.symbol, bulk.symbol, "{label}: token symbol must match");
            assert_eq!(
                live.decimals, bulk.decimals,
                "{label}: token decimals must match"
            );
            assert_eq!(
                live.max_supply, bulk.max_supply,
                "{label}: token max supply must match"
            );
            assert_eq!(
                live.standard, bulk.standard,
                "{label}: token standard must match"
            );
        }

        /// Regression: a Unique Cell created in the same transaction as an
        /// ordinary transfer of a token issued in an EARLIER batch must not be
        /// treated as that token's issuance info. The live path used to screen
        /// only inputs created inside the current batch, so the token's own
        /// input was invisible and the transfer looked like a single-type mint.
        #[tokio::test]
        async fn live_write_path_does_not_bind_unique_info_to_previously_issued_token() {
            let mut blocks = issuance_blocks();
            blocks.push(cross_batch_transfer_block());
            let store = write_blocks_live(&blocks).await;

            let token = store
                .get_token(&token_type_hash(0xa1))
                .expect("token lookup must not fail")
                .expect("issued token must be indexed");

            assert_eq!(
                token.name, None,
                "a transfer that co-creates a Unique Cell must not adopt its name"
            );
            assert_eq!(
                token.symbol, None,
                "a transfer that co-creates a Unique Cell must not adopt its symbol"
            );
            assert_eq!(
                token.decimals, None,
                "a transfer that co-creates a Unique Cell must not adopt its decimals"
            );
        }

        /// Live and bulk must persist bit-identical token metadata for the same
        /// blocks — here the cross-batch transfer that only bulk classified
        /// correctly before the fix.
        #[tokio::test]
        async fn live_and_bulk_agree_on_metadata_for_cross_batch_transfer() {
            let mut blocks = issuance_blocks();
            blocks.push(cross_batch_transfer_block());
            let store = write_blocks_live(&blocks).await;

            let type_hash = token_type_hash(0xa1);
            let live = store
                .get_token(&type_hash)
                .expect("token lookup must not fail")
                .expect("issued token must be indexed on the live path");
            let bulk_snapshot =
                materialize_bulk_artifacts_for_test(&blocks).expect("bulk build must succeed");
            let bulk = bulk_snapshot
                .core
                .token_state
                .tokens
                .get(&type_hash)
                .expect("issued token must be indexed on the bulk path");

            assert_same_token_metadata(&live, bulk, "cross-batch transfer");
        }

        /// The same equivalence assertion for a real single-type issuance: both
        /// paths must bind the Unique Cell's metadata.
        #[tokio::test]
        async fn live_and_bulk_agree_on_metadata_for_real_issuance() {
            let mut blocks = issuance_blocks();
            blocks.push(real_issuance_block());
            let store = write_blocks_live(&blocks).await;

            let type_hash = token_type_hash(0xb2);
            let live = store
                .get_token(&type_hash)
                .expect("token lookup must not fail")
                .expect("newly issued token must be indexed on the live path");
            let bulk_snapshot =
                materialize_bulk_artifacts_for_test(&blocks).expect("bulk build must succeed");
            let bulk = bulk_snapshot
                .core
                .token_state
                .tokens
                .get(&type_hash)
                .expect("newly issued token must be indexed on the bulk path");

            assert_same_token_metadata(&live, bulk, "real issuance");
            assert_eq!(
                live.name.as_deref(),
                Some("RGB++ Protocol"),
                "a genuine single-type issuance must still adopt the Unique Cell name"
            );
            assert_eq!(live.symbol.as_deref(), Some("RGB++"));
            assert_eq!(live.decimals, Some(8));
        }
    }
}
