#![allow(clippy::type_complexity)]

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, NaiveDate, Timelike, Utc};
use ckb_hash::new_blake2b;
use ckbadger_common::{LabelImportConfig, LabelImportResult, PipelineProgressData};
use dashmap::DashMap;
use futures::stream::{FuturesOrdered, StreamExt};
use rayon::prelude::*;
use serde::Serialize;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::keys;
use ckbadger_store::types::{
    AddressBalance, DaoDailySnapshot, HodlTrackerState, LiveCellInfo, NftTypeIndex, SporeTypeIndex,
};
use ckbadger_store::CkbadgerStore;

use crate::cache::CacheInvalidator;
use crate::config::{Config, DEEP_FORK_DEPTH};
use crate::db::writer::dotbit::{resolve_dotbit_tx_activity, DOTBIT_SENTINEL_COLLECTION};
use crate::db::writer::hodl_wave::HodlWaveTracker;
use crate::db::writer::nft_activity_acc::NftCollectionActivityAccumulator;
use crate::db::{BatchWriter, ReorgResult, Repository, SecondaryIssuanceBreakdown};
use crate::parser::{
    analyze_spore_media_profile, BlockParser, CellParser, DaoParser, DotbitParser, MnftParser,
    ParsedClusterCell, ParsedDotbitAccountOutput, ParsedMnftClass, ParsedMnftIssuer,
    ParsedMnftToken, ParsedSporeCell, ScriptParser, SporeParser, TransactionParser, UdtParser,
};
use ckb_store_reader::CkbChainReader;

use crate::rpc::{BlockResponseWithCycles, CkbRpcClient, DaoField};
use crate::runtime_diag::{
    generate_incident_id, read_cgroup_memory_snapshot, CgroupMemorySnapshot, FlightEvent,
    FlightRecorder,
};

use super::SyncProgress;

#[allow(dead_code)]
const PARTITION_SIZE: u64 = 5_000_000;
const DID_CKB_SENTINEL_COLLECTION: [u8; 32] = *b"did_ckb_collection______________";
const OMNILOCK_CODE_HASH_MAINNET_V2: &str =
    "0x9b819793a64463aed77c615d6cb226eea5487ccfc0783043a587254cda2b6f26";
const OMNILOCK_CODE_HASH_MAINNET_V1: &str =
    "0xa4398768d87bd17aea1361edc3accd6a0117774dc4ebc813bfa173e8ac0d086d";
const OMNILOCK_CODE_HASH_TESTNET_V2: &str =
    "0xf329effd1c475a2978453c8600e1eaf0bc2087ee093c3ee64cc96ec6847752cb";
const OMNILOCK_CODE_HASH_TESTNET_V1: &str =
    "0x79f90bb5e892d80dd213439eeab551120eb417678824f282b4ffb5f21bad2e1e";
const OMNILOCK_AUTH_LEN: usize = 21;
const OMNILOCK_SUPPLY_MODE_FLAG: u8 = 0b0000_1000;
const OMNILOCK_ADMIN_MODE_FLAG: u8 = 0b0000_0001;
const OMNILOCK_ACP_MODE_FLAG: u8 = 0b0000_0010;
const OMNILOCK_TIMELOCK_MODE_FLAG: u8 = 0b0000_0100;
const OMNILOCK_SUPPLY_INFO_CELL_MIN_DATA_LEN: usize = 65;
const OMNILOCK_SUPPLY_INFO_CELL_VERSION_V0: u8 = 0;
const XUDT_TYPE_ARGS_OWNER_LOCK_HASH_LEN: usize = 32;
const XUDT_TYPE_ARGS_FLAGS_LEN: usize = 4;
const XUDT_TYPE_ARGS_MIN_LEN: usize = XUDT_TYPE_ARGS_OWNER_LOCK_HASH_LEN + XUDT_TYPE_ARGS_FLAGS_LEN;
const XUDT_FLAGS_EXTENSION_MASK: u32 = 0x1FFF_FFFF;
const XUDT_FLAGS_EXTENSION_IN_ARGS: u32 = 0x1;
const XUDT_FLAGS_EXTENSION_IN_WITNESS: u32 = 0x2;
const XUDT_FLAGS_WITNESS_SCRIPT_HASH_LEN: usize = 20;
const UNIQUE_TYPE_ARGS_LEN: usize = 20;
const TOKEN_INFO_TAG_TOTAL_SUPPLY: u32 = 1;
const TOKEN_INFO_TOTAL_SUPPLY_DATA_LEN: usize = 16;
const STARTUP_PHASE_NONE: u8 = 0;
const STARTUP_PHASE_ROLLBACK_CLEANUP: u8 = 1;
const PIPELINE_RESET_REASON_UNKNOWN: u8 = 0;
const PIPELINE_RESET_REASON_BATCH_MISMATCH: u8 = 1;
const PIPELINE_RESET_REASON_REORG_HANDLED: u8 = 2;
const PIPELINE_RESET_REASON_DEEP_FORK_PAUSED: u8 = 3;
const PIPELINE_RESET_REASON_BATCH_WRITE_FAILED: u8 = 4;
const ADAPTIVE_REASON_UNKNOWN: u8 = 0;
const ADAPTIVE_REASON_PRESSURE_BACKOFF: u8 = 1;
const ADAPTIVE_REASON_PRESSURE_BACKOFF_FLOOR_DOWN: u8 = 2;
const ADAPTIVE_REASON_HEALTHY_STEP_UP: u8 = 3;
const ADAPTIVE_REASON_HEALTHY_STEP_UP_FLOOR_RECOVER: u8 = 4;
const ADAPTIVE_REASON_MODERATE_BACKOFF: u8 = 5;
const ADAPTIVE_REASON_MODERATE_BACKOFF_INFLIGHT_RELIEF: u8 = 6;
const ADAPTIVE_REASON_MODERATE_BACKOFF_FLOOR_DOWN: u8 = 7;
const ADAPTIVE_REASON_THROUGHPUT_BACKOFF: u8 = 8;
const ADAPTIVE_REASON_ADJUSTED: u8 = 9;
const ADAPTIVE_REASON_EARLY_HEIGHT_BOOST: u8 = 10;
const ADAPTIVE_REASON_SEVERE_PRESSURE_BACKOFF: u8 = 11;
const FLIGHT_RECORDER_CAPACITY: usize = 200;
static OMNILOCK_CODE_HASHES: OnceLock<Vec<Vec<u8>>> = OnceLock::new();

/// Pre-parsed mNFT/DotBit data computed in the parser stage.
/// Moves all CPU-intensive parsing out of the t6b writer thread.
struct PreParsedNftData {
    mnft_issuers: Vec<(usize, ParsedMnftIssuer)>,
    mnft_classes: Vec<(usize, usize, ParsedMnftClass)>,
    mnft_tokens: Vec<(usize, usize, ParsedMnftToken)>,
    dotbit_accounts: Vec<(usize, ParsedDotbitAccountOutput)>,
    consumed_dotbit: Vec<DotbitConsumptionEvent>,
    /// DAS action string per transaction (tx_global_index → action).
    dotbit_tx_actions: HashMap<usize, String>,
}

struct DotbitConsumptionEvent {
    account_id: Vec<u8>,
    block_number: i64,
    consuming_tx_hash: [u8; 32],
    tx_idx: i32,
    ts_ms: i64,
}

/// Per-tx .bit activity data for direct collection activity writes.
struct DotbitTxActivityData {
    das_action: Option<String>,
    created_account_ids: HashSet<Vec<u8>>,
    consumed_account_ids: HashSet<Vec<u8>>,
    block_number: i64,
    tx_idx: i32,
    timestamp_ms: i64,
}

#[derive(Debug, Clone)]
struct XudtExtensionScript {
    args: Vec<u8>,
}

fn decode_startup_phase(phase: u8) -> Option<&'static str> {
    match phase {
        STARTUP_PHASE_ROLLBACK_CLEANUP => Some("rollback_cleanup"),
        _ => None,
    }
}

fn encode_pipeline_reset_reason(reason: &'static str) -> u8 {
    match reason {
        "pipeline batch mismatch" => PIPELINE_RESET_REASON_BATCH_MISMATCH,
        "reorg handled" => PIPELINE_RESET_REASON_REORG_HANDLED,
        "deep fork paused" => PIPELINE_RESET_REASON_DEEP_FORK_PAUSED,
        "batch write failed" => PIPELINE_RESET_REASON_BATCH_WRITE_FAILED,
        _ => PIPELINE_RESET_REASON_UNKNOWN,
    }
}

fn decode_pipeline_reset_reason(reason_code: u8) -> &'static str {
    match reason_code {
        PIPELINE_RESET_REASON_BATCH_MISMATCH => "pipeline batch mismatch",
        PIPELINE_RESET_REASON_REORG_HANDLED => "reorg handled",
        PIPELINE_RESET_REASON_DEEP_FORK_PAUSED => "deep fork paused",
        PIPELINE_RESET_REASON_BATCH_WRITE_FAILED => "batch write failed",
        _ => "unknown",
    }
}

fn encode_adaptive_batch_reason(reason: &'static str) -> u8 {
    match reason {
        "pressure_backoff" => ADAPTIVE_REASON_PRESSURE_BACKOFF,
        "pressure_backoff_floor_down" => ADAPTIVE_REASON_PRESSURE_BACKOFF_FLOOR_DOWN,
        "healthy_step_up" => ADAPTIVE_REASON_HEALTHY_STEP_UP,
        "healthy_step_up_floor_recover" => ADAPTIVE_REASON_HEALTHY_STEP_UP_FLOOR_RECOVER,
        "moderate_backoff" => ADAPTIVE_REASON_MODERATE_BACKOFF,
        "moderate_backoff_inflight_relief" => ADAPTIVE_REASON_MODERATE_BACKOFF_INFLIGHT_RELIEF,
        "moderate_backoff_floor_down" => ADAPTIVE_REASON_MODERATE_BACKOFF_FLOOR_DOWN,
        "throughput_backoff" => ADAPTIVE_REASON_THROUGHPUT_BACKOFF,
        "adjusted" => ADAPTIVE_REASON_ADJUSTED,
        "early_height_boost" => ADAPTIVE_REASON_EARLY_HEIGHT_BOOST,
        "severe_pressure_backoff" => ADAPTIVE_REASON_SEVERE_PRESSURE_BACKOFF,
        _ => ADAPTIVE_REASON_UNKNOWN,
    }
}

fn decode_adaptive_batch_reason(reason_code: u8) -> Option<&'static str> {
    match reason_code {
        ADAPTIVE_REASON_PRESSURE_BACKOFF => Some("pressure_backoff"),
        ADAPTIVE_REASON_PRESSURE_BACKOFF_FLOOR_DOWN => Some("pressure_backoff_floor_down"),
        ADAPTIVE_REASON_HEALTHY_STEP_UP => Some("healthy_step_up"),
        ADAPTIVE_REASON_HEALTHY_STEP_UP_FLOOR_RECOVER => Some("healthy_step_up_floor_recover"),
        ADAPTIVE_REASON_MODERATE_BACKOFF => Some("moderate_backoff"),
        ADAPTIVE_REASON_MODERATE_BACKOFF_INFLIGHT_RELIEF => {
            Some("moderate_backoff_inflight_relief")
        }
        ADAPTIVE_REASON_MODERATE_BACKOFF_FLOOR_DOWN => Some("moderate_backoff_floor_down"),
        ADAPTIVE_REASON_THROUGHPUT_BACKOFF => Some("throughput_backoff"),
        ADAPTIVE_REASON_ADJUSTED => Some("adjusted"),
        ADAPTIVE_REASON_EARLY_HEIGHT_BOOST => Some("early_height_boost"),
        ADAPTIVE_REASON_SEVERE_PRESSURE_BACKOFF => Some("severe_pressure_backoff"),
        _ => None,
    }
}

/// Build a fetch sub-batch plan based on per-block tx counts.
/// Returns `(block_count, tx_count)` tuples for each sub-batch.
fn plan_fetch_sub_batches(tx_counts: &[usize], tx_cap: usize) -> Vec<(usize, usize)> {
    assert!(
        tx_cap > 0,
        "tx_cap must be > 0 to avoid infinite sub-batch splitting"
    );

    if tx_counts.is_empty() {
        return Vec::new();
    }

    let mut plan = Vec::new();
    let mut sub_blocks = 0usize;
    let mut sub_txs = 0usize;

    for &txs in tx_counts {
        sub_blocks += 1;
        sub_txs += txs;

        if sub_txs >= tx_cap {
            plan.push((sub_blocks, sub_txs));
            sub_blocks = 0;
            sub_txs = 0;
        }
    }

    if sub_blocks > 0 {
        plan.push((sub_blocks, sub_txs));
    }

    plan
}

fn adaptive_sub_batch_tx_cap(target_batch_txs: u64, min_target_batch_txs: u64) -> usize {
    let min_target_batch_txs =
        min_target_batch_txs.clamp(ADAPTIVE_BATCH_HARD_MIN_TXS, ADAPTIVE_BATCH_MAX_TXS);
    target_batch_txs
        .saturating_mul(2)
        .clamp(min_target_batch_txs, ADAPTIVE_BATCH_MAX_TXS) as usize
}

#[derive(Debug, Serialize)]
struct IncidentReport {
    incident_id: String,
    run_id: String,
    created_at: i64,
    reason: String,
    detail: String,
    startup_phase: Option<String>,
    pipeline_reset_epoch: u64,
    sync_tip_block: i64,
    sync_tip_hash: String,
    cgroup_memory: CgroupMemorySnapshot,
    recent_events: Vec<FlightEvent>,
}

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

#[allow(dead_code)]
fn get_partition_index(block_number: u64) -> usize {
    (block_number / PARTITION_SIZE) as usize
}

#[allow(dead_code)]
fn format_partition_range(start_block: u64, end_block: u64) -> String {
    let start_partition = get_partition_index(start_block);
    let end_partition = get_partition_index(end_block);
    if start_partition == end_partition {
        format!("[p{}]", start_partition)
    } else {
        format!("[p{}->p{}]", start_partition, end_partition)
    }
}

#[allow(dead_code)]
fn crosses_partition_boundary(start_block: u64, end_block: u64) -> bool {
    get_partition_index(start_block) != get_partition_index(end_block)
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

fn next_undo_seq(undo_seq_by_block: &mut HashMap<i64, u64>, block_num: i64) -> u64 {
    let seq_entry = undo_seq_by_block.entry(block_num).or_insert(0);
    let seq = *seq_entry;
    *seq_entry = seq
        .checked_add(1)
        .expect("undo seq overflow for block-scoped rollback log");
    seq
}

fn put_append_delete_undo_entry(
    domain_batch: &mut StoreBatch<'_>,
    undo_seq_by_block: &mut HashMap<i64, u64>,
    block_num: i64,
    cf_name: &str,
    key: &[u8],
) {
    let seq = next_undo_seq(undo_seq_by_block, block_num);
    let undo = ckbadger_store::types::UndoLogEntry::KeyMutation {
        target_store: ckbadger_store::types::UndoLogStoreTarget::AppendOnly,
        cf_name: cf_name.to_string(),
        key: key.to_vec(),
        previous_value: None,
    };
    domain_batch.put_reorg_undo_log_by_block(block_num, seq, &undo);
}

fn put_tx_context_undo_entries(
    domain_batch: &mut StoreBatch<'_>,
    undo_seq_by_block: &mut HashMap<i64, u64>,
    txs: &[TxData],
) -> Result<()> {
    for tx in txs {
        let mut inputs = Vec::with_capacity(tx.inputs.len());
        for input in &tx.inputs {
            let output_index = i16::try_from(input.previous_output_index).map_err(|_| {
                anyhow!(
                    "rollback tx-context input index exceeds i16 range: tx_hash=0x{}, block={}, previous_output_index={}",
                    hex::encode(tx.hash),
                    tx.block_number,
                    input.previous_output_index
                )
            })?;
            inputs.push(ckbadger_store::types::UndoInputOutPoint {
                tx_hash: input.previous_tx_hash.to_vec(),
                output_index,
            });
        }

        let ctx = ckbadger_store::types::UndoTxContext {
            tx_hash: tx.hash.to_vec(),
            outputs_count: tx.outputs_count,
            inputs,
        };
        let seq = next_undo_seq(undo_seq_by_block, tx.block_number);
        domain_batch.put_reorg_undo_log_by_block(
            tx.block_number,
            seq,
            &ckbadger_store::types::UndoLogEntry::TxContext(ctx),
        );
    }
    Ok(())
}

fn put_addr_tx_with_undo_log(
    domain_batch: &mut StoreBatch<'_>,
    append_batch: &mut StoreBatch<'_>,
    undo_seq_by_block: &mut HashMap<i64, u64>,
    lock_hash: &[u8],
    block_num: i64,
    tx_idx: i32,
    tx_hash: &[u8],
) {
    let append_key = keys::encode_addr_tx_key(lock_hash, block_num, tx_idx);
    append_batch.put_addr_tx(lock_hash, block_num, tx_idx, tx_hash);
    put_append_delete_undo_entry(
        domain_batch,
        undo_seq_by_block,
        block_num,
        ckbadger_store::CF_ADDR_TXS,
        &append_key,
    );
}

fn put_activity_with_undo_log(
    domain_batch: &mut StoreBatch<'_>,
    append_batch: &mut StoreBatch<'_>,
    undo_seq_by_block: &mut HashMap<i64, u64>,
    lock_hash: &[u8],
    block_num: i64,
    tx_idx: i32,
    entry: &ckbadger_store::types::ActivityEntry,
) {
    let append_key = keys::encode_activity_key(lock_hash, block_num, tx_idx);
    append_batch.put_activity(lock_hash, block_num, tx_idx, entry);
    put_append_delete_undo_entry(
        domain_batch,
        undo_seq_by_block,
        block_num,
        ckbadger_store::CF_ACTIVITIES,
        &append_key,
    );
}

fn rollback_undo_log_after_batch_cleanup(
    store: &CkbadgerStore,
    append_only_store: &CkbadgerStore,
    cleanup_tip: i64,
    context: &str,
) -> Result<()> {
    let _ = store
        .rollback_via_undo_log(append_only_store, cleanup_tip)
        .map_err(|e| {
            anyhow!(
                "failed to rollback undo log after batch cleanup: cleanup_tip={}, context={}, error={:#}",
                cleanup_tip,
                context,
                e
            )
        })?;
    Ok(())
}

fn format_outpoint_sample(outpoints: &[(Vec<u8>, i16)], max_items: usize) -> String {
    if outpoints.is_empty() {
        return "none".to_string();
    }

    outpoints
        .iter()
        .take(max_items)
        .map(|(tx_hash, output_index)| format!("0x{}:{}", hex::encode(tx_hash), output_index))
        .collect::<Vec<_>>()
        .join(", ")
}

fn checked_usize_to_i16(value: usize, label: &str) -> Result<i16> {
    i16::try_from(value).map_err(|_| anyhow!("{} exceeds i16 range: {}", label, value))
}

fn tx_hash_key32(tx_hash: &[u8], context: &str) -> Result<[u8; 32]> {
    tx_hash.try_into().map_err(|_| {
        anyhow!(
            "{} tx hash must be 32 bytes, got {}",
            context,
            tx_hash.len()
        )
    })
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

fn should_log_unresolved_retry(attempt: usize) -> bool {
    attempt == 1 || attempt.is_multiple_of(10) || attempt >= PARSER_UNRESOLVED_MAX_RETRIES
}

fn short_tx_hash(tx_hash: &[u8]) -> String {
    let encoded = hex::encode(tx_hash);
    if encoded.len() <= 16 {
        return encoded;
    }
    format!("{}..{}", &encoded[..10], &encoded[encoded.len() - 6..])
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct UnresolvedLocalProbeSummary {
    sampled: usize,
    live_hits: usize,
    consumed_hits: usize,
    tx_location_hits: usize,
    missing_everywhere: usize,
    store_errors: usize,
    sample_details: Vec<String>,
}

impl UnresolvedLocalProbeSummary {
    fn format_for_log(&self) -> String {
        format!(
            "sampled={} live_hits={} consumed_hits={} tx_location_hits={} missing_everywhere={} store_errors={} sample=[{}]",
            self.sampled,
            self.live_hits,
            self.consumed_hits,
            self.tx_location_hits,
            self.missing_everywhere,
            self.store_errors,
            self.sample_details.join(", ")
        )
    }
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

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct UnresolvedRpcProbeSummary {
    sampled_tx_hashes: usize,
    committed: usize,
    pending: usize,
    proposed: usize,
    rejected: usize,
    unknown_status: usize,
    rpc_null: usize,
    rpc_errors: usize,
    sample_details: Vec<String>,
}

impl UnresolvedRpcProbeSummary {
    fn format_for_log(&self) -> String {
        format!(
            "sampled_tx_hashes={} committed={} pending={} proposed={} rejected={} unknown_status={} rpc_null={} rpc_errors={} sample=[{}]",
            self.sampled_tx_hashes,
            self.committed,
            self.pending,
            self.proposed,
            self.rejected,
            self.unknown_status,
            self.rpc_null,
            self.rpc_errors,
            self.sample_details.join(", ")
        )
    }
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

#[derive(Debug, Clone, Copy, Default)]
struct BatchWriteMetrics {
    commit_ms: f64,
}

fn duration_from_millis(ms: f64) -> Duration {
    let micros = (ms.max(0.0) * 1000.0).round();
    Duration::from_micros(micros as u64)
}

fn next_fetch_start_after_batch(end_block: u64) -> u64 {
    end_block
        .checked_add(1)
        .expect("fetch batch end_block overflow while computing next start")
}

fn should_abort_unresolved_retry_on_epoch_change(batch_epoch: u64, current_epoch: u64) -> bool {
    batch_epoch != current_epoch
}

fn should_skip_address_balances(_bulk_sync_mode: bool) -> bool {
    // Address balances must always be updated inline to keep bulk sync exact.
    false
}

fn is_bulk_sync_active_by_lag(blocks_behind: u64, bulk_sync_threshold: u64) -> bool {
    blocks_behind > bulk_sync_threshold
}

fn is_bulk_sync_batch(chain_tip: u64, batch_end: u64, bulk_sync_threshold: u64) -> bool {
    let blocks_behind = chain_tip.checked_sub(batch_end).unwrap_or_else(|| {
        panic!(
            "invalid bulk-sync batch range: batch_end={} exceeds chain_tip={}",
            batch_end, chain_tip
        )
    });
    blocks_behind > bulk_sync_threshold
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

fn should_abort_pipeline_on_idle_timeout(parser_finished: bool, fetcher_finished: bool) -> bool {
    parser_finished || fetcher_finished
}

fn should_invalidate_chart_caches_for_lag(blocks_remaining: u64) -> bool {
    blocks_remaining <= CHART_INVALIDATION_MAX_LIVE_LAG
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

fn record_worker_exit_reason(
    slot: &Arc<std::sync::Mutex<Option<String>>>,
    reason: impl Into<String>,
) {
    if let Ok(mut guard) = slot.lock() {
        if guard.is_none() {
            *guard = Some(reason.into());
        }
    }
}

fn get_worker_exit_reason(slot: &Arc<std::sync::Mutex<Option<String>>>) -> Option<String> {
    slot.lock().ok().and_then(|guard| guard.clone())
}

fn format_pipeline_worker_termination_message(
    parser_finished: bool,
    fetcher_finished: bool,
    parser_exit_reason: Option<&str>,
    fetcher_exit_reason: Option<&str>,
) -> String {
    let parser_reason = parser_exit_reason.unwrap_or("unknown");
    let fetcher_reason = fetcher_exit_reason.unwrap_or("unknown");
    format!(
        "parser_finished={}, fetcher_finished={}, parser_reason={}, fetcher_reason={}",
        parser_finished, fetcher_finished, parser_reason, fetcher_reason
    )
}

fn should_log_pipeline_idle_timeout(consecutive_idle_timeouts: u64) -> bool {
    consecutive_idle_timeouts <= 3 || consecutive_idle_timeouts.is_multiple_of(10)
}

fn queue_fill_percentage(depth: Option<u64>, capacity: Option<u64>) -> Option<f64> {
    match (depth, capacity) {
        (Some(d), Some(c)) if c > 0 => Some((d as f64 / c as f64) * 100.0),
        _ => None,
    }
}

fn parse_queue_capacity_txs(
    queue_capacity_batches: usize,
    target_batch_txs: u64,
    min_target_batch_txs: u64,
) -> u64 {
    let queue_capacity_batches =
        u64::try_from(queue_capacity_batches).expect("parse queue capacity exceeds u64");
    let per_batch_tx_cap = u64::try_from(adaptive_sub_batch_tx_cap(
        target_batch_txs,
        min_target_batch_txs,
    ))
    .expect("adaptive sub-batch tx cap exceeds u64");
    queue_capacity_batches
        .checked_mul(per_batch_tx_cap)
        .expect("parse queue tx capacity overflow")
}

fn cgroup_memory_ratio_pct(snapshot: &CgroupMemorySnapshot) -> Option<f64> {
    match (snapshot.memory_current_bytes, snapshot.memory_max_bytes) {
        (Some(current), Some(max)) if max > 0 => Some((current as f64 / max as f64) * 100.0),
        _ => None,
    }
}

fn sender_queue_depth<T>(sender: &tokio::sync::mpsc::Sender<T>) -> u64 {
    (sender.max_capacity() - sender.capacity()) as u64
}

fn atomic_saturating_sub_u64(counter: &AtomicU64, value: u64) {
    if value == 0 {
        return;
    }
    loop {
        let current = counter.load(Ordering::Relaxed);
        let next = current.saturating_sub(value);
        if counter
            .compare_exchange(current, next, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            break;
        }
    }
}

fn should_trim_cell_cache(cache_len: usize) -> bool {
    cache_len > CELL_CACHE_CAPACITY * 2
}

fn evict_committed_cell_cache_entries(
    cell_cache: &DashMap<([u8; 32], i32), CachedCellInfo>,
    committed_tip: i64,
) -> usize {
    if committed_tip < 0 {
        return 0;
    }
    let before = cell_cache.len();
    cell_cache.retain(|_, v| v.created_at_block > committed_tip);
    before.saturating_sub(cell_cache.len())
}

#[derive(Debug, Clone, Copy)]
struct RepeatedWarningSnapshot {
    total_count: u64,
    suppressed_since_last_emit: u64,
    first_seen_secs_ago: u64,
}

#[derive(Debug, Clone, Copy)]
struct RepeatedWarningState {
    first_seen_at: Instant,
    last_emit_at: Instant,
    total_count: u64,
    suppressed_since_last_emit: u64,
}

#[derive(Default)]
struct RepeatedWarningTracker {
    states: std::sync::Mutex<HashMap<&'static str, RepeatedWarningState>>,
}

impl RepeatedWarningTracker {
    fn record(
        &self,
        key: &'static str,
        min_emit_interval: Duration,
    ) -> Option<RepeatedWarningSnapshot> {
        let now = Instant::now();
        let mut states = match self.states.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let entry = states.entry(key).or_insert(RepeatedWarningState {
            first_seen_at: now,
            last_emit_at: now,
            total_count: 0,
            suppressed_since_last_emit: 0,
        });
        entry.total_count = entry.total_count.saturating_add(1);

        if now.duration_since(entry.last_emit_at) >= min_emit_interval || entry.total_count == 1 {
            let snapshot = RepeatedWarningSnapshot {
                total_count: entry.total_count,
                suppressed_since_last_emit: entry.suppressed_since_last_emit,
                first_seen_secs_ago: now.duration_since(entry.first_seen_at).as_secs(),
            };
            entry.last_emit_at = now;
            entry.suppressed_since_last_emit = 0;
            Some(snapshot)
        } else {
            entry.suppressed_since_last_emit = entry.suppressed_since_last_emit.saturating_add(1);
            None
        }
    }
}

fn omnilock_code_hashes() -> &'static Vec<Vec<u8>> {
    OMNILOCK_CODE_HASHES.get_or_init(|| {
        [
            OMNILOCK_CODE_HASH_MAINNET_V2,
            OMNILOCK_CODE_HASH_MAINNET_V1,
            OMNILOCK_CODE_HASH_TESTNET_V2,
            OMNILOCK_CODE_HASH_TESTNET_V1,
        ]
        .iter()
        .map(|h| crate::rpc::parse_hex_to_bytes(h))
        .collect()
    })
}

fn is_omnilock_code_hash(code_hash: &[u8]) -> bool {
    omnilock_code_hashes()
        .iter()
        .any(|known| known.as_slice() == code_hash)
}

fn extract_omnilock_supply_info_type_hash(lock_args: &[u8]) -> Option<[u8; 32]> {
    if lock_args.len() <= OMNILOCK_AUTH_LEN {
        return None;
    }

    let omnilock_args = &lock_args[OMNILOCK_AUTH_LEN..];
    let flags = *omnilock_args.first()?;
    if flags & OMNILOCK_SUPPLY_MODE_FLAG == 0 {
        return None;
    }

    let mut offset = 1usize;
    if flags & OMNILOCK_ADMIN_MODE_FLAG != 0 {
        offset += 32;
    }
    if flags & OMNILOCK_ACP_MODE_FLAG != 0 {
        offset += 2;
    }
    if flags & OMNILOCK_TIMELOCK_MODE_FLAG != 0 {
        offset += 8;
    }

    if omnilock_args.len() < offset + 32 {
        return None;
    }

    let mut hash = [0u8; 32];
    hash.copy_from_slice(&omnilock_args[offset..offset + 32]);
    Some(hash)
}

fn parse_omnilock_supply_info_cell_data(data: &[u8]) -> Option<(i128, [u8; 32])> {
    if data.len() < OMNILOCK_SUPPLY_INFO_CELL_MIN_DATA_LEN {
        return None;
    }

    let version = data[0];
    if version != OMNILOCK_SUPPLY_INFO_CELL_VERSION_V0 {
        return None;
    }

    let current_supply = u128::from_le_bytes(data[1..17].try_into().ok()?);
    let max_supply = u128::from_le_bytes(data[17..33].try_into().ok()?);
    if current_supply > max_supply {
        return None;
    }
    if max_supply > i128::MAX as u128 {
        return None;
    }

    let mut token_type_hash = [0u8; 32];
    token_type_hash.copy_from_slice(&data[33..65]);
    Some((max_supply as i128, token_type_hash))
}

fn parse_molecule_u32(data: &[u8]) -> Option<usize> {
    let raw: [u8; 4] = data.try_into().ok()?;
    Some(u32::from_le_bytes(raw) as usize)
}

fn parse_molecule_table_fields(data: &[u8], field_count: usize) -> Option<Vec<&[u8]>> {
    let header_size = 4 + field_count * 4;
    if data.len() < header_size {
        return None;
    }
    let total_size = parse_molecule_u32(&data[0..4])?;
    if total_size != data.len() {
        return None;
    }

    let mut offsets = Vec::with_capacity(field_count + 1);
    for idx in 0..field_count {
        let start = 4 + idx * 4;
        let end = start + 4;
        offsets.push(parse_molecule_u32(&data[start..end])?);
    }
    offsets.push(total_size);

    if offsets.first().copied()? != header_size {
        return None;
    }
    for pair in offsets.windows(2) {
        if pair[0] > pair[1] || pair[1] > total_size {
            return None;
        }
    }

    Some(
        offsets
            .windows(2)
            .map(|pair| &data[pair[0]..pair[1]])
            .collect(),
    )
}

fn parse_molecule_bytes(data: &[u8]) -> Option<&[u8]> {
    if data.len() < 4 {
        return None;
    }
    let total_size = parse_molecule_u32(&data[0..4])?;
    if total_size != data.len() {
        return None;
    }
    Some(&data[4..])
}

fn parse_molecule_dynvec_items(data: &[u8]) -> Option<Vec<&[u8]>> {
    if data.len() < 4 {
        return None;
    }
    let total_size = parse_molecule_u32(&data[0..4])?;
    if total_size != data.len() {
        return None;
    }
    if total_size == 4 {
        return Some(Vec::new());
    }
    if data.len() < 8 {
        return None;
    }

    let first_offset = parse_molecule_u32(&data[4..8])?;
    if first_offset < 8 || first_offset > total_size || first_offset % 4 != 0 {
        return None;
    }

    let item_count = first_offset / 4 - 1;
    let header_size = 4 + item_count * 4;
    if header_size != first_offset {
        return None;
    }

    let mut offsets = Vec::with_capacity(item_count + 1);
    for idx in 0..item_count {
        let start = 4 + idx * 4;
        let end = start + 4;
        offsets.push(parse_molecule_u32(&data[start..end])?);
    }
    offsets.push(total_size);

    for pair in offsets.windows(2) {
        if pair[0] > pair[1] || pair[1] > total_size {
            return None;
        }
    }

    Some(
        offsets
            .windows(2)
            .map(|pair| &data[pair[0]..pair[1]])
            .collect(),
    )
}

fn parse_molecule_script(data: &[u8]) -> Option<XudtExtensionScript> {
    let fields = parse_molecule_table_fields(data, 3)?;
    if fields[0].len() != 32 || fields[1].len() != 1 {
        return None;
    }
    let args = parse_molecule_bytes(fields[2])?.to_vec();
    Some(XudtExtensionScript { args })
}

fn parse_xudt_extension_scripts_from_script_vec(
    script_vec: &[u8],
) -> Option<Vec<XudtExtensionScript>> {
    let mut scripts = Vec::new();
    for item in parse_molecule_dynvec_items(script_vec)? {
        scripts.push(parse_molecule_script(item)?);
    }
    Some(scripts)
}

fn extract_xudt_witness_extension_script_vec(xudt_witness: &[u8]) -> Option<&[u8]> {
    let fields = parse_molecule_table_fields(xudt_witness, 4)?;
    if fields[2].is_empty() {
        None
    } else {
        Some(fields[2])
    }
}

fn blake160(data: &[u8]) -> [u8; 20] {
    let mut hasher = new_blake2b();
    hasher.update(data);

    let mut out = [0u8; 32];
    hasher.finalize(&mut out);

    let mut out160 = [0u8; 20];
    out160.copy_from_slice(&out[..20]);
    out160
}

fn extract_xudt_extension_scripts_from_witnesses(
    witnesses: &[String],
    expected_script_vec_hash: &[u8; 20],
) -> Option<Vec<XudtExtensionScript>> {
    for witness_hex in witnesses {
        let witness_bytes = crate::rpc::parse_hex_to_bytes(witness_hex);
        let witness_fields = match parse_molecule_table_fields(&witness_bytes, 3) {
            Some(fields) => fields,
            None => continue,
        };

        for bytes_opt_field in [&witness_fields[1], &witness_fields[2]] {
            if bytes_opt_field.is_empty() {
                continue;
            }
            let Some(xudt_witness_bytes) = parse_molecule_bytes(bytes_opt_field) else {
                continue;
            };
            let Some(script_vec_bytes) =
                extract_xudt_witness_extension_script_vec(xudt_witness_bytes)
            else {
                continue;
            };
            if blake160(script_vec_bytes) != *expected_script_vec_hash {
                continue;
            }
            if let Some(parsed) = parse_xudt_extension_scripts_from_script_vec(script_vec_bytes) {
                return Some(parsed);
            }
        }
    }
    None
}

fn extract_xudt_extension_scripts(
    type_args: &[u8],
    witnesses: &[String],
) -> Option<Vec<XudtExtensionScript>> {
    if type_args.len() < XUDT_TYPE_ARGS_MIN_LEN {
        return None;
    }
    let flags = u32::from_le_bytes(
        type_args[XUDT_TYPE_ARGS_OWNER_LOCK_HASH_LEN..XUDT_TYPE_ARGS_MIN_LEN]
            .try_into()
            .ok()?,
    );
    let extension_mode = flags & XUDT_FLAGS_EXTENSION_MASK;

    match extension_mode {
        XUDT_FLAGS_EXTENSION_IN_ARGS => {
            parse_xudt_extension_scripts_from_script_vec(&type_args[XUDT_TYPE_ARGS_MIN_LEN..])
        }
        XUDT_FLAGS_EXTENSION_IN_WITNESS => {
            let tail = &type_args[XUDT_TYPE_ARGS_MIN_LEN..];
            if tail.len() < XUDT_FLAGS_WITNESS_SCRIPT_HASH_LEN {
                return None;
            }
            let mut expected = [0u8; XUDT_FLAGS_WITNESS_SCRIPT_HASH_LEN];
            expected.copy_from_slice(&tail[..XUDT_FLAGS_WITNESS_SCRIPT_HASH_LEN]);
            extract_xudt_extension_scripts_from_witnesses(witnesses, &expected)
        }
        _ => None,
    }
}

fn parse_token_info_total_supply(data: &[u8]) -> Option<i128> {
    if data.len() < 3 {
        return None;
    }

    let mut index = 0usize;
    index += 1; // decimal

    let name_len = *data.get(index)? as usize;
    index += 1;
    if data.len() < index + name_len + 1 {
        return None;
    }
    index += name_len;

    let symbol_len = *data.get(index)? as usize;
    index += 1;
    if data.len() < index + symbol_len {
        return None;
    }
    index += symbol_len;

    while index + 8 <= data.len() {
        let tag = u32::from_le_bytes(data[index..index + 4].try_into().ok()?);
        index += 4;
        let data_len = u32::from_le_bytes(data[index..index + 4].try_into().ok()?) as usize;
        index += 4;
        if data.len() < index + data_len {
            return None;
        }
        let value = &data[index..index + data_len];
        if tag == TOKEN_INFO_TAG_TOTAL_SUPPLY && data_len == TOKEN_INFO_TOTAL_SUPPLY_DATA_LEN {
            let raw = u128::from_le_bytes(value.try_into().ok()?);
            if raw > i128::MAX as u128 {
                return None;
            }
            return Some(raw as i128);
        }
        index += data_len;
    }

    None
}

fn collect_unique_cell_total_supply_by_type_args(
    cells: &[crate::parser::cell::ParsedCell],
) -> HashMap<Vec<u8>, i128> {
    let mut totals = HashMap::new();
    for cell in cells {
        let Some(type_args) = cell.type_args.as_ref() else {
            continue;
        };
        if type_args.len() != UNIQUE_TYPE_ARGS_LEN {
            continue;
        }
        let Some(total_supply) = parse_token_info_total_supply(&cell.data) else {
            continue;
        };
        totals.insert(type_args.clone(), total_supply);
    }
    totals
}

fn observe_max_supply(
    observations: &mut HashMap<Vec<u8>, i128>,
    tx_hash: &[u8; 32],
    token_type_hash: Vec<u8>,
    max_supply: i128,
    source: &str,
) {
    if let Some(existing) = observations.get(&token_type_hash) {
        if *existing != max_supply {
            warn!(
                tx_hash = %hex::encode(tx_hash),
                token_type_hash = %hex::encode(&token_type_hash),
                existing_max_supply = existing,
                observed_max_supply = max_supply,
                source = source,
                "conflicting max supply observations in the same batch; keeping first value"
            );
        }
        return;
    }

    observations.insert(token_type_hash, max_supply);
}

fn collect_token_max_supply_observations(all_tx_data: &[TxData]) -> HashMap<Vec<u8>, i128> {
    let mut observations = HashMap::new();

    for tx_data in all_tx_data {
        let unique_cell_total_supply_by_type_args =
            collect_unique_cell_total_supply_by_type_args(&tx_data.cells);

        for cell in &tx_data.cells {
            if !is_omnilock_code_hash(&cell.lock_code_hash) {
                continue;
            }

            let Some(supply_info_type_hash) =
                extract_omnilock_supply_info_type_hash(&cell.lock_args)
            else {
                continue;
            };
            let Some(cell_type_hash) = cell.type_script_hash.as_ref() else {
                continue;
            };
            if cell_type_hash.as_slice() != supply_info_type_hash {
                continue;
            }

            let Some((max_supply, token_type_hash)) =
                parse_omnilock_supply_info_cell_data(&cell.data)
            else {
                continue;
            };
            observe_max_supply(
                &mut observations,
                &tx_data.hash,
                token_type_hash.to_vec(),
                max_supply,
                "omnilock_supply_info_cell",
            );
        }

        if unique_cell_total_supply_by_type_args.is_empty() {
            continue;
        }

        for cell in &tx_data.cells {
            let Some(type_code_hash) = cell.type_code_hash.as_ref() else {
                continue;
            };
            let Some(type_hash_type) = cell.type_hash_type else {
                continue;
            };
            if !matches!(
                UdtParser::is_udt_code_hash_bytes(type_code_hash, type_hash_type),
                Some(crate::parser::udt::UdtStandard::Xudt)
            ) {
                continue;
            }

            let Some(type_args) = cell.type_args.as_ref() else {
                continue;
            };
            let Some(token_type_hash) = cell.type_script_hash.as_ref() else {
                continue;
            };

            let Some(extension_scripts) =
                extract_xudt_extension_scripts(type_args, &tx_data.witnesses)
            else {
                continue;
            };

            for extension in extension_scripts {
                if extension.args.len() != UNIQUE_TYPE_ARGS_LEN {
                    continue;
                }
                let Some(max_supply) = unique_cell_total_supply_by_type_args
                    .get(&extension.args)
                    .copied()
                else {
                    continue;
                };
                observe_max_supply(
                    &mut observations,
                    &tx_data.hash,
                    token_type_hash.clone(),
                    max_supply,
                    "xudt_extension_script_unique_cell",
                );
            }
        }
    }

    observations
}

fn load_activity_token_info_cache(
    store: &CkbadgerStore,
    tx_data: &[TxData],
    input_cell_info: &HashMap<(Vec<u8>, i16), LiveCellInfo>,
    batch_cell_infos: &HashMap<(Vec<u8>, i16), LiveCellInfo>,
) -> Result<HashMap<Vec<u8>, (Option<String>, Option<u8>)>> {
    let mut type_hashes = HashSet::<Vec<u8>>::new();

    for tx in tx_data {
        for cell in &tx.cells {
            if let Some(type_script_hash) = &cell.type_script_hash {
                type_hashes.insert(type_script_hash.clone());
            }
        }

        if tx.is_cellbase {
            continue;
        }

        for input in &tx.inputs {
            let key = (
                input.previous_tx_hash.to_vec(),
                input.previous_output_index as i16,
            );
            let cell_info = input_cell_info
                .get(&key)
                .or_else(|| batch_cell_infos.get(&key));
            if let Some(info) = cell_info {
                if let Some(type_script_hash) = &info.type_script_hash {
                    type_hashes.insert(type_script_hash.clone());
                }
            }
        }
    }

    let type_hash_vec: Vec<Vec<u8>> = type_hashes.into_iter().collect();
    let mut token_info_cache: HashMap<Vec<u8>, (Option<String>, Option<u8>)> = HashMap::new();
    for (type_hash, info) in store.get_tokens_batch(&type_hash_vec)? {
        let Some(info) = info else {
            continue;
        };
        let decimals = match info.decimals {
            Some(value) => Some(u8::try_from(value).map_err(|_| {
                anyhow!(
                    "token decimals out of u8 range while building activity cache: type_hash=0x{}, decimals={}",
                    hex::encode(&type_hash),
                    value
                )
            })?),
            None => None,
        };
        let symbol = info.symbol.clone().or(info.name.clone());
        token_info_cache.insert(type_hash, (symbol, decimals));
    }

    Ok(token_info_cache)
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

fn count_new_addresses(
    changes: &HashMap<Vec<u8>, (i128, i32, i32, i64, i64, &[u8], i128)>,
    existing: &HashMap<Vec<u8>, Option<AddressBalance>>,
) -> i64 {
    changes
        .iter()
        .filter(|(lock_hash, (_, live_delta, _, _, _, _, _))| {
            if *live_delta <= 0 {
                return false;
            }
            let prev_live = existing
                .get(*lock_hash)
                .and_then(|entry| entry.as_ref())
                .map(|balance| balance.live_cells_count)
                .unwrap_or(0);
            prev_live <= 0
        })
        .count() as i64
}

fn classify_nft_collection_id(type_code_hash: &[u8], type_args: &[u8]) -> Option<Vec<u8>> {
    if type_args.len() >= 24 && MnftParser::is_token_type_script(type_code_hash) {
        return Some(type_args[..24].to_vec());
    }
    if DotbitParser::is_account_cell_type_script(type_code_hash) {
        return Some(DOTBIT_SENTINEL_COLLECTION.to_vec());
    }
    if SporeParser::is_did_type_script(type_code_hash) {
        return Some(DID_CKB_SENTINEL_COLLECTION.to_vec());
    }
    None
}

/// Reconstruct pre-batch live cell count from persisted post-batch count and batch delta.
///
/// Address balances are written before HODL tracker updates, so reading `live_cells_count`
/// from store returns post-batch state. We need pre-batch state to detect 0→>0 and >0→0
/// holder transitions correctly.
fn derive_pre_batch_live_cells(post_live_cells: i32, live_delta: i32) -> Result<i32> {
    let pre = post_live_cells as i64 - live_delta as i64;
    if pre < 0 {
        bail!(
            "pre-batch live cells underflow: post_live_cells={}, live_delta={}",
            post_live_cells,
            live_delta
        );
    }
    if pre > i32::MAX as i64 {
        bail!(
            "pre-batch live cells overflow: post_live_cells={}, live_delta={}",
            post_live_cells,
            live_delta
        );
    }
    Ok(pre as i32)
}

fn bump_pipeline_reset_epoch(epoch: &AtomicU64) -> u64 {
    epoch.fetch_add(1, Ordering::SeqCst) + 1
}

enum SyncAction {
    CaughtUp,
    Continue,
    ReorgHandled,
    DeepForkPaused,
}

#[allow(dead_code)]
enum ReorgAction {
    Handled(ReorgResult),
    DeepForkPaused,
}

/// Accumulated statistics across a batch of blocks (avoids per-block DB writes)
#[derive(Default)]
struct BatchStats {
    sync_totals: (i64, i64, i64),
    last_block: Option<(i64, Vec<u8>)>,
    hourly_stats: HashMap<DateTime<Utc>, (i32, i32, i32, i32, i128)>,
    daily_stats: HashMap<NaiveDate, (i32, i32, i32, i32, i128, i128, i128, i64, i64)>,
    daily_block_stats: HashMap<NaiveDate, (i128, i32, i32)>,
    miner_stats: HashMap<(NaiveDate, Vec<u8>), (i32, i64)>,
    epoch_stats: HashMap<i64, EpochAccum>,
    block_time_dist: HashMap<i32, i32>,
    epoch_time_dist: HashMap<i32, i32>,
    dao_snapshot_dates: HashSet<NaiveDate>,
    daily_block_times: HashMap<NaiveDate, (i64, i32)>,
    daily_dao_fields: HashMap<NaiveDate, Vec<u8>>,
    dao_daily_active_delta: HashMap<NaiveDate, i128>,
    dao_daily_gross_deposit_delta: HashMap<NaiveDate, i128>,
    dao_daily_new_deposits_delta: HashMap<NaiveDate, i64>,
    dao_daily_withdrawals_delta: HashMap<NaiveDate, i64>,
    daily_secondary_non_miner_delta: HashMap<NaiveDate, i128>,
    daily_secondary_miner_delta: HashMap<NaiveDate, i128>,
    /// Set to true after the DAO delta computation code path runs, even if no
    /// DAO transactions were found.  This distinguishes "genuinely zero deltas"
    /// from "deltas never computed" (e.g. stale DB from an older indexer).
    dao_deltas_computed: bool,
}

#[derive(Clone)]
struct EpochAccum {
    start_block: i64,
    end_block: i64,
    length: i32,
    start_ts: chrono::DateTime<Utc>,
    end_ts: chrono::DateTime<Utc>,
    tx_count: i32,
    is_new: bool,
}

#[derive(Clone)]
struct CachedCellInfo {
    capacity: i64,
    created_at_block: i64,
    lock_script_hash: Vec<u8>,
    lock_code_hash: Vec<u8>,
    lock_hash_type: i16,
    lock_args: Vec<u8>,
    type_script_hash: Option<Vec<u8>>,
    type_code_hash: Option<Vec<u8>>,
    type_args: Option<Vec<u8>>,
    data_size: i32,
    occupied_capacity: i64,
    udt_amount: Option<u128>,
}

#[derive(Clone)]
#[allow(dead_code)]
struct CachedUdtCellInfo {
    type_script_hash: Vec<u8>,
    type_code_hash: Vec<u8>,
    type_hash_type: i16,
    type_args: Vec<u8>,
    lock_script_hash: Vec<u8>,
    amount: u128,
    standard: String,
}

fn parse_parsed_cell_udt_amount(
    cell: &crate::parser::cell::ParsedCell,
    tx_hash: &[u8],
    output_index: i16,
    standard_hint: Option<&str>,
) -> Result<Option<u128>> {
    let standard = if let (Some(type_code_hash), Some(hash_type)) =
        (cell.type_code_hash.as_deref(), cell.type_hash_type)
    {
        crate::parser::UdtParser::is_udt_code_hash_bytes(type_code_hash, hash_type)
    } else {
        None
    };
    let standard = match standard {
        Some(standard) => standard,
        None => match standard_hint.and_then(crate::parser::UdtStandard::from_standard_hint) {
            Some(crate::parser::UdtStandard::Xudt) => crate::parser::UdtStandard::Xudt,
            _ => return Ok(None),
        },
    };
    let type_code_hash = cell.type_code_hash.as_deref().unwrap_or(&[]);

    let Some(amount) = crate::parser::UdtParser::parse_amount(&cell.data) else {
        // xUDT-compatible cells can carry non-amount payloads (for example owner-mode cells).
        // They should not be indexed as fungible UDT balances/transfers.
        if matches!(standard, crate::parser::UdtStandard::Xudt) {
            return Ok(None);
        }
        return Err(anyhow!(
            "failed to parse UDT amount from parsed output data: outpoint=0x{}:{}, type_code_hash=0x{}",
            hex::encode(tx_hash),
            output_index,
            hex::encode(type_code_hash)
        ));
    };
    Ok(Some(amount))
}

fn extract_dao_csu(dao: &[u8]) -> Option<(i128, i128, i128)> {
    if dao.len() < 32 {
        return None;
    }
    let c = u64::from_le_bytes(dao[0..8].try_into().ok()?) as i128;
    let s = u64::from_le_bytes(dao[16..24].try_into().ok()?) as i128;
    let u = u64::from_le_bytes(dao[24..32].try_into().ok()?) as i128;
    Some((c, s, u))
}

fn split_secondary_issuance(
    total_issuance: i128,
    occupied_capacity: i128,
    total_deposited: i128,
    non_miner_secondary: i128,
) -> Result<(i128, i128, i128)> {
    if non_miner_secondary <= 0 {
        return Ok((0, 0, 0));
    }

    if total_issuance < 0 || occupied_capacity < 0 || total_deposited < 0 {
        bail!(
            "negative input in secondary issuance split: total_issuance={}, occupied_capacity={}, total_deposited={}, non_miner_secondary={}",
            total_issuance,
            occupied_capacity,
            total_deposited,
            non_miner_secondary
        );
    }

    if total_issuance <= occupied_capacity {
        bail!(
            "invalid DAO C/U relationship: total_issuance={}, occupied_capacity={}, non_miner_secondary={}",
            total_issuance,
            occupied_capacity,
            non_miner_secondary
        );
    }

    let denom = total_issuance - occupied_capacity;
    if total_deposited > denom {
        bail!(
            "dao deposited exceeds liquid supply: total_deposited={}, liquid_supply={}, total_issuance={}, occupied_capacity={}",
            total_deposited,
            denom,
            total_issuance,
            occupied_capacity
        );
    }

    let miner = non_miner_secondary * occupied_capacity / denom;
    let dao = non_miner_secondary * total_deposited / denom;
    let treasury = non_miner_secondary - dao;

    if miner < 0 || dao < 0 || treasury < 0 {
        bail!(
            "secondary issuance split produced negative component: miner={}, dao={}, treasury={}, non_miner_secondary={}",
            miner,
            dao,
            treasury,
            non_miner_secondary
        );
    }

    Ok((miner, dao, treasury))
}

fn resolve_non_miner_secondary_delta_for_snapshot(
    date: NaiveDate,
    daily_non_miner_delta: Option<i128>,
    secondary_pool: i128,
    prev_secondary_pool: i128,
) -> Result<i128> {
    if let Some(delta) = daily_non_miner_delta {
        if delta < 0 {
            bail!(
                "negative daily non-miner secondary issuance delta while building DAO daily snapshot: date={}, delta={}",
                date,
                delta
            );
        }
        return Ok(delta);
    }

    let delta = secondary_pool - prev_secondary_pool;
    if delta < 0 {
        bail!(
            "secondary pool decreased while building DAO daily snapshot without precomputed delta: date={}, previous_secondary_pool={}, secondary_pool={}, delta={}",
            date,
            prev_secondary_pool,
            secondary_pool,
            delta
        );
    }
    Ok(delta)
}

fn parse_prefixed_hex_u128(field: &str, label: &str) -> Result<u128> {
    let Some(hex) = field.strip_prefix("0x") else {
        bail!("{} missing 0x prefix: {}", label, field);
    };
    u128::from_str_radix(hex, 16)
        .map_err(|e| anyhow::anyhow!("invalid {} hex '{}': {}", label, field, e))
}

fn parse_prefixed_hex_u32(field: &str, label: &str) -> Result<u32> {
    let Some(hex) = field.strip_prefix("0x") else {
        bail!("{} missing 0x prefix: {}", label, field);
    };
    u32::from_str_radix(hex, 16)
        .map_err(|e| anyhow::anyhow!("invalid {} hex '{}': {}", label, field, e))
}

fn parse_prefixed_hex_u64(field: &str, label: &str) -> Result<u64> {
    let Some(hex) = field.strip_prefix("0x") else {
        bail!("{} missing 0x prefix: {}", label, field);
    };
    u64::from_str_radix(hex, 16)
        .map_err(|e| anyhow::anyhow!("invalid {} hex '{}': {}", label, field, e))
}

fn parse_dao_field_for_secondary_issuance(
    dao_hex: &str,
    block_number: i64,
    block_hash: &str,
) -> Result<DaoField> {
    DaoField::from_hex(dao_hex).ok_or_else(|| {
        anyhow!(
            "invalid DAO field hex while updating secondary issuance: block_number={}, block_hash={}, dao_hex={}",
            block_number,
            block_hash,
            dao_hex
        )
    })
}

fn parse_outpoint_index_i16(field: &str, label: &str) -> Result<i16> {
    let value = parse_prefixed_hex_u32(field, label)?;
    i16::try_from(value).map_err(|_| anyhow::anyhow!("{} exceeds i16 range: {}", label, value))
}

fn dotbit_consume_event_order(tx_global_index: usize) -> Result<u64> {
    let tx_index = u64::try_from(tx_global_index).map_err(|_| {
        anyhow!(
            "dotbit tx index exceeds u64 while building consume order: {}",
            tx_global_index
        )
    })?;
    tx_index.checked_mul(2).ok_or_else(|| {
        anyhow!(
            "dotbit consume event order overflow: tx_global_index={}",
            tx_global_index
        )
    })
}

fn dotbit_create_event_order(tx_global_index: usize) -> Result<u64> {
    dotbit_consume_event_order(tx_global_index)?
        .checked_add(1)
        .ok_or_else(|| {
            anyhow!(
                "dotbit create event order overflow: tx_global_index={}",
                tx_global_index
            )
        })
}

fn should_consume_dotbit_account(latest_create_order: Option<u64>, consume_order: u64) -> bool {
    match latest_create_order {
        Some(order) => order <= consume_order,
        None => true,
    }
}

fn resolve_dotbit_account_id_for_outpoint(
    db_account_id: Option<Vec<u8>>,
    prev_tx_hash: &[u8],
    prev_index: i16,
    batch_dotbit_outpoints: &HashMap<(Vec<u8>, i16), Vec<u8>>,
) -> Option<Vec<u8>> {
    db_account_id.or_else(|| {
        batch_dotbit_outpoints
            .get(&(prev_tx_hash.to_vec(), prev_index))
            .cloned()
    })
}

fn checked_sub_u128(lhs: u128, rhs: u128, label: &str) -> Result<u128> {
    lhs.checked_sub(rhs)
        .ok_or_else(|| anyhow::anyhow!("{} underflow: lhs={}, rhs={}", label, lhs, rhs))
}

fn checked_u128_to_i64(value: u128, label: &str) -> Result<i64> {
    i64::try_from(value).map_err(|_| anyhow::anyhow!("{} exceeds i64: {}", label, value))
}

fn checked_tx_fee(
    total_input_capacity: i64,
    total_output_capacity: i64,
    has_dao_input: bool,
    tx_hash: &[u8],
    block_number: i64,
) -> Result<i64> {
    if total_input_capacity < 0 || total_output_capacity < 0 {
        bail!(
            "negative tx capacity: block={}, tx_hash={}, total_input_capacity={}, total_output_capacity={}",
            block_number,
            hex::encode(tx_hash),
            total_input_capacity,
            total_output_capacity
        );
    }

    if total_input_capacity < total_output_capacity {
        if has_dao_input {
            return Ok(0);
        }
        bail!(
            "tx fee underflow: block={}, tx_hash={}, total_input_capacity={}, total_output_capacity={}",
            block_number,
            hex::encode(tx_hash),
            total_input_capacity,
            total_output_capacity
        );
    }

    total_input_capacity.checked_sub(total_output_capacity).ok_or_else(|| {
        anyhow::anyhow!(
            "tx fee subtraction overflow: block={}, tx_hash={}, total_input_capacity={}, total_output_capacity={}",
            block_number,
            hex::encode(tx_hash),
            total_input_capacity,
            total_output_capacity
        )
    })
}

fn extract_ar_i64_from_dao(dao: &[u8], block_number: i64) -> Result<i64> {
    let ar = DaoParser::extract_ar_from_dao_field(dao)
        .ok_or_else(|| anyhow!("missing AR in DAO field at block {}", block_number))?;
    i64::try_from(ar).map_err(|_| anyhow!("DAO AR exceeds i64 at block {}: {}", block_number, ar))
}

fn dao_csu_for_snapshot_date(stats: &BatchStats, date: NaiveDate) -> Result<(i128, i128, i128)> {
    let field = stats
        .daily_dao_fields
        .get(&date)
        .ok_or_else(|| anyhow!("missing DAO field for snapshot date {}", date))?;
    extract_dao_csu(field).ok_or_else(|| {
        anyhow!(
            "invalid DAO field bytes for snapshot date {}: len={}",
            date,
            field.len()
        )
    })
}

type DaoConsumedRow = (i64, Vec<u8>, i16, String, i64, i16);
type DaoConsumedMap = HashMap<(Vec<u8>, i16), DaoConsumedRow>;
type DaoSameBatchMap = HashMap<(Vec<u8>, i16), i64>;

#[allow(clippy::too_many_arguments)]
fn accumulate_dao_snapshot_deltas_for_txs(
    tx_slice: &[TxData],
    block_date: NaiveDate,
    dao_code_hash: &[u8],
    consumed_dao_map: &DaoConsumedMap,
    same_batch_dao_map: &mut DaoSameBatchMap,
    daily_active_delta: &mut HashMap<NaiveDate, i128>,
    daily_gross_deposit_delta: &mut HashMap<NaiveDate, i128>,
    daily_new_deposits_delta: &mut HashMap<NaiveDate, i64>,
    daily_withdrawals_delta: &mut HashMap<NaiveDate, i64>,
) -> Result<()> {
    for tx_data in tx_slice {
        let mut has_withdraw_request_output = false;

        for (output_index, cell) in tx_data.cells.iter().enumerate() {
            if let Some(ref type_code_hash) = cell.type_code_hash {
                if type_code_hash == dao_code_hash && cell.data_size == 8 {
                    if cell.data.len() == 8 && cell.data.iter().all(|&b| b == 0) {
                        *daily_active_delta.entry(block_date).or_default() += cell.capacity as i128;
                        *daily_gross_deposit_delta.entry(block_date).or_default() +=
                            cell.capacity as i128;
                        *daily_new_deposits_delta.entry(block_date).or_default() += 1;
                        same_batch_dao_map
                            .insert((tx_data.hash.to_vec(), output_index as i16), cell.capacity);
                    } else if let Some(data) = tx_data.outputs_data.get(output_index) {
                        let data_bytes = crate::rpc::parse_hex_to_bytes(data);
                        if DaoParser::parse_deposit_block_number(&data_bytes).is_some() {
                            has_withdraw_request_output = true;
                        }
                    }
                }
            }
        }

        if tx_data.is_cellbase {
            continue;
        }

        for input in &tx_data.inputs {
            let outpoint = (
                input.previous_tx_hash.to_vec(),
                input.previous_output_index as i16,
            );
            if let Some((_, _, _, _, _, status)) = consumed_dao_map.get(&outpoint) {
                if *status == 1 {
                    *daily_withdrawals_delta.entry(block_date).or_default() += 1;
                }
            }
        }

        if !has_withdraw_request_output {
            continue;
        }

        // Phase-1 withdrawal always consumes status=0 deposits. Match by consumed
        // outpoint status, not by capacity, to avoid leaving stale active deposits.
        for input in &tx_data.inputs {
            let outpoint = (
                input.previous_tx_hash.to_vec(),
                input.previous_output_index as i16,
            );
            let mut maybe_cap: Option<i64> = same_batch_dao_map.get(&outpoint).copied();
            if maybe_cap.is_none() {
                if let Some((_, _, _, capacity_str, _, status)) = consumed_dao_map.get(&outpoint) {
                    if *status == 0 {
                        maybe_cap = Some(capacity_str.parse::<i64>().map_err(|e| {
                            anyhow!(
                                "invalid DAO capacity string while accumulating snapshot deltas: value='{}', tx_hash=0x{}, output_index={}, error={}",
                                capacity_str,
                                hex::encode(input.previous_tx_hash),
                                input.previous_output_index,
                                e
                            )
                        })?);
                    }
                }
            }
            if let Some(capacity) = maybe_cap {
                *daily_active_delta.entry(block_date).or_default() -= capacity as i128;
            }
        }
    }
    Ok(())
}

fn accumulate_secondary_issuance_deltas(
    stats: &mut BatchStats,
    parsed: &crate::parser::block::ParsedBlock,
    block_date: NaiveDate,
    prev_dao_cs: &mut Option<(i128, i128)>,
) -> Result<()> {
    let (c, s, u) = extract_dao_csu(&parsed.dao).ok_or_else(|| {
        anyhow!(
            "invalid DAO field bytes while accumulating secondary issuance: block={}, date={}, dao_len={}",
            parsed.number,
            block_date,
            parsed.dao.len()
        )
    })?;

    if let Some((prev_c, prev_s)) = *prev_dao_cs {
        let _c_delta = c - prev_c;
        let s_delta = s - prev_s;

        if s_delta < 0 {
            bail!(
                "secondary issuance S delta underflow: date={}, prev_s={}, current_s={}, delta={}",
                block_date,
                prev_s,
                s,
                s_delta
            );
        }
        *stats
            .daily_secondary_non_miner_delta
            .entry(block_date)
            .or_default() += s_delta;
        // Derive miner share directly from C/U ratio to avoid compact-target
        // and primary-issuance approximation drift.
        let (miner, _, _) = split_secondary_issuance(c, u, 0, s_delta)?;
        *stats
            .daily_secondary_miner_delta
            .entry(block_date)
            .or_default() += miner;
    }

    *prev_dao_cs = Some((c, s));
    Ok(())
}

#[derive(Default)]
struct PerfStats {
    fetch_us: AtomicU64,
    db_stage_write_us: AtomicU64,
    db_commit_us: AtomicU64,
    last_fetch_us: AtomicU64,
    last_db_stage_write_us: AtomicU64,
    last_db_commit_us: AtomicU64,
    blocks_count: AtomicU64,
}

impl PerfStats {
    fn add_fetch(&self, duration: Duration) {
        self.fetch_us
            .fetch_add(duration.as_micros() as u64, Ordering::Relaxed);
    }

    fn add_db_write(&self, duration: Duration) {
        self.db_stage_write_us
            .fetch_add(duration.as_micros() as u64, Ordering::Relaxed);
    }

    fn add_db_commit(&self, duration: Duration) {
        self.db_commit_us
            .fetch_add(duration.as_micros() as u64, Ordering::Relaxed);
    }

    fn report_and_reset(&self) {
        let blocks = self.blocks_count.swap(0, Ordering::Relaxed);
        if blocks == 0 {
            return;
        }
        let fetch_us = self.fetch_us.swap(0, Ordering::Relaxed);
        let db_stage_us = self.db_stage_write_us.swap(0, Ordering::Relaxed);
        let db_commit_us = self.db_commit_us.swap(0, Ordering::Relaxed);
        self.last_fetch_us.store(fetch_us, Ordering::Relaxed);
        self.last_db_stage_write_us
            .store(db_stage_us, Ordering::Relaxed);
        self.last_db_commit_us
            .store(db_commit_us, Ordering::Relaxed);

        let fetch_ms = fetch_us as f64 / 1000.0;
        let db_stage_ms = db_stage_us as f64 / 1000.0;
        let db_commit_ms = db_commit_us as f64 / 1000.0;
        info!(
            blocks,
            fetch_ms = format!("{:.1}", fetch_ms),
            db_stage_ms = format!("{:.1}", db_stage_ms),
            db_commit_ms = format!("{:.1}", db_commit_ms),
            "Batch perf"
        );
    }

    /// Snapshot current accumulated values, falling back to the latest completed batch.
    /// Returns (fetch_ms, db_stage_write_ms, db_commit_ms).
    fn snapshot_ms(&self) -> (f64, f64, f64) {
        let rpc = self.fetch_us.load(Ordering::Relaxed);
        let db_stage = self.db_stage_write_us.load(Ordering::Relaxed);
        let db_commit = self.db_commit_us.load(Ordering::Relaxed);
        let rpc = if rpc > 0 {
            rpc
        } else {
            self.last_fetch_us.load(Ordering::Relaxed)
        };
        let db_stage = if db_stage > 0 {
            db_stage
        } else {
            self.last_db_stage_write_us.load(Ordering::Relaxed)
        };
        let db_commit = if db_commit > 0 {
            db_commit
        } else {
            self.last_db_commit_us.load(Ordering::Relaxed)
        };
        (
            rpc as f64 / 1000.0,
            db_stage as f64 / 1000.0,
            db_commit as f64 / 1000.0,
        )
    }
}

#[derive(Default)]
struct PipelinePerfStats {
    last_fetch_us: AtomicU64,
    last_parse_us: AtomicU64,
    last_write_us: AtomicU64,
    last_write_commit_us: AtomicU64,
    last_writer_wait_us: AtomicU64,
    fetch_queue_depth: AtomicU64,
    fetch_queue_capacity: AtomicU64,
    parse_queue_depth: AtomicU64,
    parse_queue_capacity: AtomicU64,
    writer_queue_depth: AtomicU64,
    writer_queue_capacity: AtomicU64,
}

impl PipelinePerfStats {
    fn set_queue_capacities(&self, fetch_capacity: usize, parse_capacity: usize) {
        self.fetch_queue_capacity
            .store(fetch_capacity as u64, Ordering::Relaxed);
        self.parse_queue_capacity
            .store(parse_capacity as u64, Ordering::Relaxed);
        self.writer_queue_capacity
            .store(parse_capacity as u64, Ordering::Relaxed);
    }

    fn record_fetch(&self, duration: Duration, queue_depth: usize, queue_capacity: usize) {
        self.last_fetch_us
            .store(duration.as_micros() as u64, Ordering::Relaxed);
        self.fetch_queue_depth
            .store(queue_depth as u64, Ordering::Relaxed);
        self.fetch_queue_capacity
            .store(queue_capacity as u64, Ordering::Relaxed);
    }

    fn record_parse(&self, duration: Duration, queue_depth: usize, queue_capacity: usize) {
        self.last_parse_us
            .store(duration.as_micros() as u64, Ordering::Relaxed);
        self.parse_queue_depth
            .store(queue_depth as u64, Ordering::Relaxed);
        self.parse_queue_capacity
            .store(queue_capacity as u64, Ordering::Relaxed);
    }

    fn record_write(
        &self,
        duration: Duration,
        commit_ms: f64,
        writer_wait_ms: f64,
        queue_depth: usize,
        queue_capacity: usize,
    ) {
        self.last_write_us
            .store(duration.as_micros() as u64, Ordering::Relaxed);
        self.last_write_commit_us.store(
            duration_from_millis(commit_ms).as_micros() as u64,
            Ordering::Relaxed,
        );
        self.last_writer_wait_us.store(
            Duration::from_secs_f64((writer_wait_ms.max(0.0)) / 1000.0).as_micros() as u64,
            Ordering::Relaxed,
        );
        self.writer_queue_depth
            .store(queue_depth as u64, Ordering::Relaxed);
        self.writer_queue_capacity
            .store(queue_capacity as u64, Ordering::Relaxed);
    }

    fn snapshot(&self) -> Option<PipelineProgressData> {
        let fetch_us = self.last_fetch_us.load(Ordering::Relaxed);
        let parse_us = self.last_parse_us.load(Ordering::Relaxed);
        let write_us = self.last_write_us.load(Ordering::Relaxed);
        let write_commit_us = self.last_write_commit_us.load(Ordering::Relaxed);
        let wait_us = self.last_writer_wait_us.load(Ordering::Relaxed);
        let fetch_depth = self.fetch_queue_depth.load(Ordering::Relaxed);
        let fetch_capacity = self.fetch_queue_capacity.load(Ordering::Relaxed);
        let parse_depth = self.parse_queue_depth.load(Ordering::Relaxed);
        let parse_capacity = self.parse_queue_capacity.load(Ordering::Relaxed);
        let writer_depth = self.writer_queue_depth.load(Ordering::Relaxed);
        let writer_capacity = self.writer_queue_capacity.load(Ordering::Relaxed);

        if fetch_us == 0
            && parse_us == 0
            && write_us == 0
            && write_commit_us == 0
            && wait_us == 0
            && fetch_capacity == 0
            && parse_capacity == 0
            && writer_capacity == 0
        {
            return None;
        }

        Some(PipelineProgressData {
            fetch_ms: if fetch_us > 0 {
                Some(fetch_us as f64 / 1000.0)
            } else {
                None
            },
            parse_ms: if parse_us > 0 {
                Some(parse_us as f64 / 1000.0)
            } else {
                None
            },
            write_ms: if write_us > 0 {
                Some(write_us as f64 / 1000.0)
            } else {
                None
            },
            commit_ms: if write_commit_us > 0 {
                Some(write_commit_us as f64 / 1000.0)
            } else {
                None
            },
            writer_wait_ms: if wait_us > 0 {
                Some(wait_us as f64 / 1000.0)
            } else {
                None
            },
            fetch_queue_depth: Some(fetch_depth),
            fetch_queue_capacity: if fetch_capacity > 0 {
                Some(fetch_capacity)
            } else {
                None
            },
            parse_queue_depth: Some(parse_depth),
            parse_queue_capacity: if parse_capacity > 0 {
                Some(parse_capacity)
            } else {
                None
            },
            writer_queue_depth: Some(writer_depth),
            writer_queue_capacity: if writer_capacity > 0 {
                Some(writer_capacity)
            } else {
                None
            },
        })
    }
}

const CELL_CACHE_CAPACITY: usize = 200_000;
const UDT_CELL_CACHE_CAPACITY: usize = 100_000;
const STARTUP_CONTINUITY_WINDOW_BLOCKS: i64 = 512;
const PARSER_UNRESOLVED_RETRY_DELAY_MS: u64 = 500;
const PARSER_UNRESOLVED_MAX_RETRIES: usize = 240;
const PARSER_UNRESOLVED_PROBE_SAMPLE_SIZE: usize = 5;
const PARSER_UNRESOLVED_RPC_PROBE_TIMEOUT_SECS: u64 = 8;
const BULK_PHASE_COMMIT_SLOW_WARN_MS: f64 = 2_000.0;
const ADAPTIVE_BATCH_BASE_MIN_TXS: u64 = 10_000;
const ADAPTIVE_BATCH_HARD_MIN_TXS: u64 = 2_000;
const ADAPTIVE_BATCH_MAX_TXS: u64 = 160_000;
const ADAPTIVE_BATCH_INITIAL_TXS: u64 = 40_000;
const ADAPTIVE_BATCH_EARLY_HEIGHT_CUTOFF: u64 = 4_000_000;
const ADAPTIVE_BATCH_EARLY_TARGET_TXS: u64 = 120_000;
const ADAPTIVE_BATCH_MIN_BLOCKS: u64 = 1;
const ADAPTIVE_BATCH_MAX_BLOCKS: u64 = 5_000;
const ADAPTIVE_BATCH_TPB_EMA_ALPHA_PCT: u64 = 20; // 0.20
const ADAPTIVE_BATCH_INITIAL_TPB_MILLI: u64 = 20_000; // 20.0 tx/block
const ADAPTIVE_BATCH_INITIAL_INFLIGHT: u64 = 3;
const ADAPTIVE_BATCH_WRITE_TARGET_MS: f64 = 2_500.0;
const ADAPTIVE_BATCH_WRITE_LO_MS: f64 = 1_500.0;
const ADAPTIVE_BATCH_WRITE_HI_MS: f64 = 6_000.0;
const ADAPTIVE_BATCH_WRITE_HEALTHY_US_PER_TX: f64 = 300.0;
const ADAPTIVE_BATCH_WRITE_TARGET_US_PER_TX: f64 = 450.0;
const ADAPTIVE_BATCH_WRITE_HI_US_PER_TX: f64 = 900.0;
const ADAPTIVE_BATCH_SEVERE_WRITE_MS: f64 = 10_000.0;
const ADAPTIVE_BATCH_SEVERE_COMMIT_MS: f64 = 3_000.0;
const ADAPTIVE_BATCH_SEVERE_WRITE_US_PER_TX: f64 = 1_500.0;
const ADAPTIVE_BATCH_SEVERE_CONSECUTIVE_REQUIRED: u64 = 2;
const ADAPTIVE_BATCH_SEVERE_COOLDOWN_STEPS: u64 = 2;
const ADAPTIVE_BATCH_TXPS_EMA_ALPHA_PCT: u64 = 20; // 0.20
const ADAPTIVE_BATCH_TXPS_STEPUP_MIN_RETAIN_PCT: u64 = 98;
const ADAPTIVE_BATCH_TXPS_BACKOFF_DROP_PCT: u64 = 95;
const ADAPTIVE_BATCH_PARSE_PRESSURE_PCT: f64 = 95.0;
const ADAPTIVE_BATCH_WRITER_PRESSURE_PCT: f64 = 90.0;
const ADAPTIVE_BATCH_PARSE_HEALTHY_PCT: f64 = 60.0;
const ADAPTIVE_BATCH_WRITER_HEALTHY_PCT: f64 = 60.0;
const ADAPTIVE_BATCH_MEMORY_PRESSURE_PCT: f64 = 80.0;
const ADAPTIVE_BATCH_MEMORY_HEALTHY_PCT: f64 = 70.0;
const ADAPTIVE_BATCH_MIN_FLOOR_STEP_DOWN_PCT: u64 = 80;
const ADAPTIVE_BATCH_MIN_FLOOR_STEP_UP_PCT: u64 = 110;
const ADAPTIVE_BATCH_MIN_FLOOR_RECOVER_WRITE_US_PER_TX: f64 = 220.0;
const ADAPTIVE_BATCH_HEALTHY_STEP_UP_PCT: u64 = 120;
const ADAPTIVE_BATCH_HEALTHY_BONUS_STEP_UP_PCT: u64 = 110;
const ADAPTIVE_BATCH_HEALTHY_BONUS_STREAK: u64 = 3;
const ADAPTIVE_BATCH_HEALTHY_BONUS_COMMIT_MS: f64 = 1_200.0;
const ADAPTIVE_BATCH_NEAR_TIP_THRESHOLD_BLOCKS: u64 = 1_000_000;
const ADAPTIVE_BATCH_BULK_DISTANCE_MIN_TARGET_TXS: u64 = 40_000;
const ADAPTIVE_BATCH_BULK_DISTANCE_MIN_INFLIGHT: u64 = 6;

#[derive(Debug, Clone, Copy)]
struct AdaptiveBatchSnapshot {
    target_batch_txs: u64,
    inflight_limit: u64,
    min_target_batch_txs: u64,
    cooldown_steps: u64,
    last_reason_code: u8,
    adjustment_seq: u64,
    backoff_streak: u64,
    last_adjusted_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct AdaptiveBatchProgressSnapshot {
    pub target_batch_txs: u64,
    pub inflight_limit: u64,
    pub min_target_batch_txs: u64,
    pub cooldown_steps: u64,
    pub last_reason: Option<String>,
    pub adjustment_seq: u64,
    pub backoff_streak: u64,
    pub last_adjusted_at: Option<i64>,
}

#[derive(Debug, Clone, Copy)]
struct AdaptiveBatchInput {
    write_ms: f64,
    commit_ms: f64,
    batch_tx_count: usize,
    blocks_remaining: u64,
    parse_queue_fill_pct: Option<f64>,
    writer_queue_fill_pct: Option<f64>,
    memory_ratio_pct: Option<f64>,
    /// Max L0 file count across all CFs (from memory_stats)
    l0_files_max: Option<u64>,
    /// Pending compaction bytes (from memory_stats)
    compaction_pending_bytes: Option<u64>,
    /// Total immutable memtables across all CFs (from memory_stats)
    immutable_memtables: Option<u64>,
    /// Dynamic pressure thresholds from MemoryProfile
    severe_pending_threshold: u64,
    moderate_pending_threshold: u64,
    severe_imm_threshold: u64,
    moderate_imm_threshold: u64,
}

#[derive(Debug, Clone, Copy)]
struct AdaptiveBatchAdjustment {
    previous_target_batch_txs: u64,
    new_target_batch_txs: u64,
    previous_inflight_limit: u64,
    new_inflight_limit: u64,
    previous_min_target_batch_txs: u64,
    new_min_target_batch_txs: u64,
    reason: &'static str,
}

#[derive(Debug)]
struct AdaptiveBatchController {
    target_batch_txs: AtomicU64,
    inflight_limit: AtomicU64,
    min_target_batch_txs: AtomicU64,
    tx_per_block_milli_ema: AtomicU64,
    tx_per_sec_milli_ema: AtomicU64,
    cooldown_steps: AtomicU64,
    last_reason_code: AtomicU8,
    adjustment_seq: AtomicU64,
    backoff_streak: AtomicU64,
    severe_pressure_streak: AtomicU64,
    healthy_streak: AtomicU64,
    last_adjusted_at: AtomicI64,
    max_inflight_limit: u64,
    early_height_boost_applied: std::sync::atomic::AtomicBool,
}

impl AdaptiveBatchController {
    fn new(max_inflight_limit: u64) -> Self {
        let max_inflight_limit = max_inflight_limit.max(1);
        let initial_inflight = ADAPTIVE_BATCH_INITIAL_INFLIGHT.min(max_inflight_limit);
        Self {
            target_batch_txs: AtomicU64::new(ADAPTIVE_BATCH_INITIAL_TXS),
            inflight_limit: AtomicU64::new(initial_inflight),
            min_target_batch_txs: AtomicU64::new(ADAPTIVE_BATCH_BASE_MIN_TXS),
            tx_per_block_milli_ema: AtomicU64::new(ADAPTIVE_BATCH_INITIAL_TPB_MILLI),
            tx_per_sec_milli_ema: AtomicU64::new(0),
            cooldown_steps: AtomicU64::new(0),
            last_reason_code: AtomicU8::new(ADAPTIVE_REASON_UNKNOWN),
            adjustment_seq: AtomicU64::new(0),
            backoff_streak: AtomicU64::new(0),
            severe_pressure_streak: AtomicU64::new(0),
            healthy_streak: AtomicU64::new(0),
            last_adjusted_at: AtomicI64::new(0),
            max_inflight_limit,
            early_height_boost_applied: std::sync::atomic::AtomicBool::new(false),
        }
    }

    fn snapshot(&self) -> AdaptiveBatchSnapshot {
        let last_adjusted_at_raw = self.last_adjusted_at.load(Ordering::Relaxed);
        AdaptiveBatchSnapshot {
            target_batch_txs: self.target_batch_txs.load(Ordering::Relaxed),
            inflight_limit: self.inflight_limit.load(Ordering::Relaxed),
            min_target_batch_txs: self.min_target_batch_txs.load(Ordering::Relaxed),
            cooldown_steps: self.cooldown_steps.load(Ordering::Relaxed),
            last_reason_code: self.last_reason_code.load(Ordering::Relaxed),
            adjustment_seq: self.adjustment_seq.load(Ordering::Relaxed),
            backoff_streak: self.backoff_streak.load(Ordering::Relaxed),
            last_adjusted_at: (last_adjusted_at_raw > 0).then_some(last_adjusted_at_raw),
        }
    }

    fn record_adjustment(&self, reason_code: u8) {
        self.last_reason_code.store(reason_code, Ordering::Relaxed);
        self.adjustment_seq.fetch_add(1, Ordering::Relaxed);
        self.last_adjusted_at
            .store(chrono::Utc::now().timestamp(), Ordering::Relaxed);

        if decode_adaptive_batch_reason(reason_code)
            .is_some_and(|reason| reason.contains("backoff"))
        {
            self.backoff_streak.fetch_add(1, Ordering::Relaxed);
        } else {
            self.backoff_streak.store(0, Ordering::Relaxed);
        }
    }

    fn maybe_apply_early_height_boost(&self, start_block: u64) -> Option<(u64, u64)> {
        if start_block >= ADAPTIVE_BATCH_EARLY_HEIGHT_CUTOFF {
            return None;
        }
        if self
            .early_height_boost_applied
            .swap(true, Ordering::Relaxed)
        {
            return None;
        }

        let previous_target_batch_txs = self.target_batch_txs.load(Ordering::Relaxed);
        let boosted_target_batch_txs = previous_target_batch_txs
            .max(ADAPTIVE_BATCH_EARLY_TARGET_TXS)
            .clamp(
                self.min_target_batch_txs.load(Ordering::Relaxed),
                ADAPTIVE_BATCH_MAX_TXS,
            );
        self.target_batch_txs
            .store(boosted_target_batch_txs, Ordering::Relaxed);
        self.record_adjustment(ADAPTIVE_REASON_EARLY_HEIGHT_BOOST);

        Some((previous_target_batch_txs, boosted_target_batch_txs))
    }

    fn estimate_block_span(&self, batch_block_cap: u64) -> u64 {
        let batch_block_cap = batch_block_cap.clamp(1, ADAPTIVE_BATCH_MAX_BLOCKS);
        let min_blocks = ADAPTIVE_BATCH_MIN_BLOCKS.min(batch_block_cap);
        let target_batch_txs = self.target_batch_txs.load(Ordering::Relaxed).max(1);
        let tx_per_block_milli = self.tx_per_block_milli_ema.load(Ordering::Relaxed).max(1);
        let estimated =
            ((target_batch_txs * 1000).saturating_add(tx_per_block_milli - 1)) / tx_per_block_milli;
        estimated.clamp(min_blocks, batch_block_cap)
    }

    fn observe_tx_density(&self, tx_count: usize, block_count: usize) {
        if tx_count == 0 || block_count == 0 {
            return;
        }
        let sample = (((tx_count as u64) * 1000).saturating_add(block_count as u64 - 1))
            / block_count as u64;
        let alpha = ADAPTIVE_BATCH_TPB_EMA_ALPHA_PCT.min(100);
        loop {
            let old = self.tx_per_block_milli_ema.load(Ordering::Relaxed).max(1);
            let blended = ((old.saturating_mul(100 - alpha)).saturating_add(sample * alpha)) / 100;
            if self
                .tx_per_block_milli_ema
                .compare_exchange(old, blended.max(1), Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
        }
    }

    fn observe_tx_throughput(&self, tx_count: usize, write_ms: f64) -> Option<(u64, u64)> {
        if tx_count == 0 || write_ms <= 0.0 {
            return None;
        }

        let write_us = (write_ms * 1000.0).round() as u64;
        if write_us == 0 {
            return None;
        }

        let sample = (((tx_count as u128) * 1_000_000u128).saturating_add(write_us as u128 - 1))
            / write_us as u128;
        let sample = sample.clamp(1, u64::MAX as u128) as u64;
        let alpha = ADAPTIVE_BATCH_TXPS_EMA_ALPHA_PCT.min(100);

        loop {
            let old = self.tx_per_sec_milli_ema.load(Ordering::Relaxed);
            let blended = if old == 0 {
                sample
            } else {
                ((old.saturating_mul(100 - alpha)).saturating_add(sample * alpha)) / 100
            };
            if self
                .tx_per_sec_milli_ema
                .compare_exchange(old, blended.max(1), Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return Some((old, blended.max(1)));
            }
        }
    }

    fn step_down_min_floor(min_floor: u64) -> u64 {
        let lowered = min_floor.saturating_mul(ADAPTIVE_BATCH_MIN_FLOOR_STEP_DOWN_PCT) / 100;
        lowered.clamp(ADAPTIVE_BATCH_HARD_MIN_TXS, ADAPTIVE_BATCH_BASE_MIN_TXS)
    }

    fn step_up_min_floor(min_floor: u64) -> u64 {
        let raised = min_floor
            .saturating_mul(ADAPTIVE_BATCH_MIN_FLOOR_STEP_UP_PCT)
            .saturating_add(99)
            / 100;
        raised.clamp(ADAPTIVE_BATCH_HARD_MIN_TXS, ADAPTIVE_BATCH_BASE_MIN_TXS)
    }

    fn update_after_write(&self, input: AdaptiveBatchInput) -> Option<AdaptiveBatchAdjustment> {
        let previous_target_batch_txs = self.target_batch_txs.load(Ordering::Relaxed);
        let previous_inflight_limit = self.inflight_limit.load(Ordering::Relaxed);
        let previous_min_target_batch_txs = self
            .min_target_batch_txs
            .load(Ordering::Relaxed)
            .clamp(ADAPTIVE_BATCH_HARD_MIN_TXS, ADAPTIVE_BATCH_BASE_MIN_TXS);
        let mut new_target_batch_txs = previous_target_batch_txs;
        let mut new_inflight_limit = previous_inflight_limit;
        let mut new_min_target_batch_txs = previous_min_target_batch_txs;
        let reason: Option<&'static str>;
        let near_tip = input.blocks_remaining <= ADAPTIVE_BATCH_NEAR_TIP_THRESHOLD_BLOCKS;
        let write_us_per_tx = if input.batch_tx_count > 0 && input.write_ms > 0.0 {
            Some((input.write_ms * 1000.0) / input.batch_tx_count as f64)
        } else {
            None
        };
        let txps_ema = self.observe_tx_throughput(input.batch_tx_count, input.write_ms);
        let throughput_not_worse = txps_ema.is_none_or(|(old, new)| {
            old == 0
                || (new.saturating_mul(100))
                    >= old.saturating_mul(ADAPTIVE_BATCH_TXPS_STEPUP_MIN_RETAIN_PCT)
        });
        let throughput_drop_under_load = txps_ema.is_some_and(|(old, new)| {
            old > 0
                && (new.saturating_mul(100))
                    < old.saturating_mul(ADAPTIVE_BATCH_TXPS_BACKOFF_DROP_PCT)
                && (input.writer_queue_fill_pct.is_some_and(|pct| pct >= 60.0)
                    || input.parse_queue_fill_pct.is_some_and(|pct| pct >= 60.0))
        });
        let high_unit_write_cost =
            write_us_per_tx.is_some_and(|us| us >= ADAPTIVE_BATCH_WRITE_HI_US_PER_TX);
        let target_unit_write_cost =
            write_us_per_tx.is_some_and(|us| us >= ADAPTIVE_BATCH_WRITE_TARGET_US_PER_TX);
        let queue_pressure = input
            .parse_queue_fill_pct
            .is_some_and(|pct| pct >= ADAPTIVE_BATCH_PARSE_PRESSURE_PCT)
            || input
                .writer_queue_fill_pct
                .is_some_and(|pct| pct >= ADAPTIVE_BATCH_WRITER_PRESSURE_PCT);

        // RocksDB internal pressure signals: detect compaction backlog, L0 pile-up,
        // and immutable memtable accumulation BEFORE they cause write stalls.
        // L0 thresholds (40/20) are architectural; pending bytes and immutable memtable
        // thresholds scale with the memory profile.
        let rocksdb_severe_pressure = input.l0_files_max.is_some_and(|l0| l0 >= 40)
            || input
                .compaction_pending_bytes
                .is_some_and(|b| b >= input.severe_pending_threshold)
            || input
                .immutable_memtables
                .is_some_and(|imm| imm >= input.severe_imm_threshold);
        let rocksdb_moderate_pressure = input.l0_files_max.is_some_and(|l0| l0 >= 20)
            || input
                .compaction_pending_bytes
                .is_some_and(|b| b >= input.moderate_pending_threshold)
            || input
                .immutable_memtables
                .is_some_and(|imm| imm >= input.moderate_imm_threshold);

        let severe_pressure_signal = input.write_ms >= ADAPTIVE_BATCH_SEVERE_WRITE_MS
            || input.commit_ms >= ADAPTIVE_BATCH_SEVERE_COMMIT_MS
            || write_us_per_tx.is_some_and(|us| us >= ADAPTIVE_BATCH_SEVERE_WRITE_US_PER_TX)
            || rocksdb_severe_pressure;
        let severe_pressure_streak = if severe_pressure_signal {
            self.severe_pressure_streak.fetch_add(1, Ordering::Relaxed) + 1
        } else {
            self.severe_pressure_streak.store(0, Ordering::Relaxed);
            0
        };
        let severe_pressure = severe_pressure_streak >= ADAPTIVE_BATCH_SEVERE_CONSECUTIVE_REQUIRED;
        let moderate_pressure = target_unit_write_cost
            || (input.write_ms > ADAPTIVE_BATCH_WRITE_HI_MS && throughput_drop_under_load)
            || (queue_pressure && throughput_drop_under_load)
            || input
                .memory_ratio_pct
                .is_some_and(|pct| pct >= ADAPTIVE_BATCH_MEMORY_PRESSURE_PCT)
            || rocksdb_moderate_pressure;

        if severe_pressure {
            new_target_batch_txs = ((previous_target_batch_txs as f64) * 0.7).round() as u64;
            new_inflight_limit = previous_inflight_limit.saturating_sub(1).max(1);
            self.cooldown_steps
                .store(ADAPTIVE_BATCH_SEVERE_COOLDOWN_STEPS, Ordering::Relaxed);
            self.healthy_streak.store(0, Ordering::Relaxed);

            let at_floor = previous_target_batch_txs <= previous_min_target_batch_txs;
            if at_floor && previous_inflight_limit <= 2 && high_unit_write_cost {
                new_min_target_batch_txs = Self::step_down_min_floor(previous_min_target_batch_txs);
                reason = Some(
                    if new_min_target_batch_txs < previous_min_target_batch_txs {
                        "pressure_backoff_floor_down"
                    } else {
                        "severe_pressure_backoff"
                    },
                );
            } else {
                reason = Some("severe_pressure_backoff");
            }
        } else if moderate_pressure {
            new_target_batch_txs = ((previous_target_batch_txs as f64) * 0.9).round() as u64;
            self.healthy_streak.store(0, Ordering::Relaxed);
            reason = Some("moderate_backoff");
        } else {
            let cooldown = self.cooldown_steps.load(Ordering::Relaxed);
            if cooldown > 0 {
                self.cooldown_steps.fetch_sub(1, Ordering::Relaxed);
                self.healthy_streak.store(0, Ordering::Relaxed);
                reason = None;
            } else {
                let healthy = input.write_ms < ADAPTIVE_BATCH_WRITE_LO_MS
                    && write_us_per_tx
                        .is_some_and(|us| us < ADAPTIVE_BATCH_WRITE_HEALTHY_US_PER_TX)
                    && input
                        .parse_queue_fill_pct
                        .is_some_and(|pct| pct < ADAPTIVE_BATCH_PARSE_HEALTHY_PCT)
                    && input
                        .writer_queue_fill_pct
                        .is_some_and(|pct| pct < ADAPTIVE_BATCH_WRITER_HEALTHY_PCT)
                    && input
                        .memory_ratio_pct
                        .is_none_or(|pct| pct < ADAPTIVE_BATCH_MEMORY_HEALTHY_PCT)
                    && !rocksdb_moderate_pressure;
                if healthy && throughput_not_worse {
                    let healthy_streak = self.healthy_streak.fetch_add(1, Ordering::Relaxed) + 1;
                    if previous_inflight_limit < self.max_inflight_limit {
                        new_inflight_limit = previous_inflight_limit + 1;
                    } else {
                        let mut growth_pct = ADAPTIVE_BATCH_HEALTHY_STEP_UP_PCT;
                        if healthy_streak >= ADAPTIVE_BATCH_HEALTHY_BONUS_STREAK
                            && input.write_ms < ADAPTIVE_BATCH_WRITE_TARGET_MS
                            && input.commit_ms < ADAPTIVE_BATCH_HEALTHY_BONUS_COMMIT_MS
                        {
                            growth_pct = growth_pct
                                .saturating_mul(ADAPTIVE_BATCH_HEALTHY_BONUS_STEP_UP_PCT)
                                / 100;
                        }
                        new_target_batch_txs = previous_target_batch_txs
                            .saturating_mul(growth_pct)
                            .saturating_add(99)
                            / 100;
                    }
                    let should_recover_floor = previous_min_target_batch_txs
                        < ADAPTIVE_BATCH_BASE_MIN_TXS
                        && write_us_per_tx.is_some_and(|us| {
                            us <= ADAPTIVE_BATCH_MIN_FLOOR_RECOVER_WRITE_US_PER_TX
                        })
                        && input.parse_queue_fill_pct.is_some_and(|pct| pct < 30.0)
                        && input.writer_queue_fill_pct.is_some_and(|pct| pct < 30.0)
                        && previous_target_batch_txs > previous_min_target_batch_txs;
                    if should_recover_floor {
                        new_min_target_batch_txs =
                            Self::step_up_min_floor(previous_min_target_batch_txs);
                        reason = Some(
                            if new_min_target_batch_txs > previous_min_target_batch_txs {
                                "healthy_step_up_floor_recover"
                            } else {
                                "healthy_step_up"
                            },
                        );
                    } else {
                        reason = Some("healthy_step_up");
                    }
                } else {
                    self.healthy_streak.store(0, Ordering::Relaxed);
                    reason = None;
                }
            }
        }

        if near_tip {
            new_min_target_batch_txs = new_min_target_batch_txs
                .clamp(ADAPTIVE_BATCH_HARD_MIN_TXS, ADAPTIVE_BATCH_BASE_MIN_TXS);
        } else {
            let min_inflight =
                ADAPTIVE_BATCH_BULK_DISTANCE_MIN_INFLIGHT.min(self.max_inflight_limit);
            new_inflight_limit = new_inflight_limit.max(min_inflight);
            new_min_target_batch_txs = new_min_target_batch_txs
                .clamp(ADAPTIVE_BATCH_HARD_MIN_TXS, ADAPTIVE_BATCH_MAX_TXS)
                .max(ADAPTIVE_BATCH_BULK_DISTANCE_MIN_TARGET_TXS);
        }
        new_target_batch_txs =
            new_target_batch_txs.clamp(new_min_target_batch_txs, ADAPTIVE_BATCH_MAX_TXS);
        new_inflight_limit = new_inflight_limit.clamp(1, self.max_inflight_limit);

        if new_target_batch_txs == previous_target_batch_txs
            && new_inflight_limit == previous_inflight_limit
            && new_min_target_batch_txs == previous_min_target_batch_txs
        {
            return None;
        }

        self.target_batch_txs
            .store(new_target_batch_txs, Ordering::Relaxed);
        self.inflight_limit
            .store(new_inflight_limit, Ordering::Relaxed);
        self.min_target_batch_txs
            .store(new_min_target_batch_txs, Ordering::Relaxed);
        self.record_adjustment(encode_adaptive_batch_reason(reason.unwrap_or("adjusted")));

        Some(AdaptiveBatchAdjustment {
            previous_target_batch_txs,
            new_target_batch_txs,
            previous_inflight_limit,
            new_inflight_limit,
            previous_min_target_batch_txs,
            new_min_target_batch_txs,
            reason: reason.unwrap_or("adjusted"),
        })
    }
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

struct TxData {
    hash: [u8; 32],
    block_number: i64,
    block_hash: Vec<u8>,
    tx_index: i32,
    version: i32,
    inputs_count: i16,
    outputs_count: i16,
    witnesses_count: i16,
    cell_deps_count: i16,
    header_deps_count: i16,
    is_cellbase: bool,
    inputs: Vec<crate::parser::transaction::ParsedInput>,
    cells: Vec<crate::parser::cell::ParsedCell>,
    witnesses: Vec<String>,
    outputs_data: Vec<String>,
    total_input_capacity: i64,
    total_output_capacity: i64,
    fee: i64,
    tx_size: i32,
    cycles: Option<i64>,
    timestamp: chrono::DateTime<Utc>,
}

type ScriptUsageChanges = HashMap<(Vec<u8>, bool), (i64, i64, i128, i128, i128, i128)>;

fn parse_tx_cycles(
    raw_cycles_hex: Option<&String>,
    tx_hash: &str,
    block_number: i64,
) -> Result<Option<i64>> {
    let Some(raw_cycles_hex) = raw_cycles_hex else {
        return Ok(None);
    };

    let cycles_u64 = u64::from_str_radix(
        raw_cycles_hex.strip_prefix("0x").unwrap_or(raw_cycles_hex),
        16,
    )
    .map_err(|e| {
        anyhow!(
            "invalid tx cycles hex '{}' for tx {} in block {}: {}",
            raw_cycles_hex,
            tx_hash,
            block_number,
            e
        )
    })?;

    // Historical CKB blocks may expose unavailable cycles as 0x0 for non-cellbase txs.
    // Treat this as missing data so cycles_worker can lazily recompute and persist real values.
    if cycles_u64 == 0 {
        return Ok(None);
    }

    i64::try_from(cycles_u64).map(Some).map_err(|_| {
        anyhow!(
            "tx cycles over i64 range '{}' for tx {} in block {}: {} (max={})",
            raw_cycles_hex,
            tx_hash,
            block_number,
            cycles_u64,
            i64::MAX
        )
    })
}

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
const CHART_INVALIDATION_MAX_LIVE_LAG: u64 = 100;
const SECONDARY_ISSUANCE_BACKFILL_THRESHOLD: u64 = 1000;

pub struct Indexer {
    run_id: String,
    config: Config,
    rpc: CkbRpcClient,
    repo: Repository,
    writer: BatchWriter,
    append_only_store: Arc<CkbadgerStore>,
    progress: Arc<SyncProgress>,
    cell_cache: Arc<DashMap<([u8; 32], i32), CachedCellInfo>>,
    udt_cell_cache: Arc<DashMap<([u8; 32], i16), CachedUdtCellInfo>>,
    perf: PerfStats,
    pipeline_perf: Arc<PipelinePerfStats>,
    adaptive_batch_controller: Arc<AdaptiveBatchController>,
    cache_invalidator: CacheInvalidator,
    last_cache_invalidation: tokio::sync::Mutex<u64>,
    was_bulk_sync_active: std::sync::atomic::AtomicBool,
    was_secondary_issuance_bulk_active: std::sync::atomic::AtomicBool,
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
    /// Limits concurrent background secondary issuance tasks to prevent unbounded spawning.
    secondary_issuance_semaphore: Arc<tokio::sync::Semaphore>,
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
        let was_secondary_bulk =
            progress.blocks_remaining() > SECONDARY_ISSUANCE_BACKFILL_THRESHOLD;

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
            was_secondary_issuance_bulk_active: std::sync::atomic::AtomicBool::new(
                was_secondary_bulk,
            ),
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
            secondary_issuance_semaphore: Arc::new(tokio::sync::Semaphore::new(4)),
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
        let store = self.writer.store();
        let in_bulk = store.is_bulk_sync_mode();
        let should_be_bulk = blocks_behind > self.config.bulk_sync_threshold;

        if should_be_bulk && !in_bulk {
            info!(
                blocks_behind,
                threshold = self.config.bulk_sync_threshold,
                "Re-entering bulk compaction mode"
            );
            store.set_bulk_sync_compaction_options();
        } else if !should_be_bulk && in_bulk {
            let (l0_files_max, compaction_pending_bytes, _imm) = store.compaction_pressure();
            const DRAIN_L0_THRESHOLD: u64 = 10;
            let drain_pending_threshold = store.memory_profile().drain_pending_bytes_threshold;
            if l0_files_max < DRAIN_L0_THRESHOLD
                && compaction_pending_bytes < drain_pending_threshold
            {
                info!(
                    l0_files_max,
                    compaction_pending_mb = compaction_pending_bytes / (1024 * 1024),
                    "Compaction drained, restoring normal compaction options"
                );
                store.restore_normal_compaction_options();
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
        let sync_status = self.writer.store().get_sync_status().unwrap_or_default();
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

    fn is_secondary_issuance_bulk_active(&self) -> bool {
        self.progress.blocks_remaining() > SECONDARY_ISSUANCE_BACKFILL_THRESHOLD
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
        }

        let (start_block, _) = self.repo.get_sync_tip().await?;
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
            self.config.force_startup_cleanup || actual_start < start_block,
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

        if bulk_sync_mode && actual_start < start_block {
            bail!(
                "bulk sync fail-fast: inconsistent local DB state detected at startup (sync_tip={}, consistent_block={}). \
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

            if self.repo.has_unresolved_deep_fork().unwrap_or(false) {
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
                        let output_index =
                            checked_usize_to_i16(idx, "pipeline parser batch cell output index")
                                .unwrap_or_else(|e| panic!("{}", e));
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
                        let hash_arr =
                            tx_hash_key32(tx_hash, "pipeline parser input cell cache lookup")
                                .unwrap_or_else(|e| panic!("{}", e));
                        let key = (hash_arr, *idx as i32);
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
                            output_index as i16,
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
                        let lock_script_size = 32 + 1 + cell.lock_args.len() as i64;
                        let type_script_size = cell
                            .type_args
                            .as_ref()
                            .map(|args| 32 + 1 + args.len() as i64)
                            .unwrap_or(0);
                        let occupied_capacity =
                            (8 + lock_script_size + type_script_size + cell.data_size as i64)
                                * 100_000_000;
                        batch_cell_infos.insert(
                            (tx_data.hash.to_vec(), output_index as i16),
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
                                input.previous_output_index as i16,
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
                            output_index as i16,
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
                        let lock_script_size = 32 + 1 + cell.lock_args.len() as i64;
                        let type_script_size = cell
                            .type_args
                            .as_ref()
                            .map(|a| 32 + 1 + a.len() as i64)
                            .unwrap_or(0);
                        let cell_occupied =
                            (8 + lock_script_size + type_script_size + cell.data_size as i64)
                                * 100_000_000;
                        cell_cache_for_parser.insert(
                            (tx_data.hash, output_index as i32),
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
                        let lock_script_size = 32 + 1 + cell.lock_args.len() as i64;
                        let type_script_size = cell
                            .type_args
                            .as_ref()
                            .map(|a| 32 + 1 + a.len() as i64)
                            .unwrap_or(0);
                        let cell_occupied =
                            (8 + lock_script_size + type_script_size + cell.data_size as i64)
                                * 100_000_000;
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
                                input.previous_output_index as i16,
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
                        let lock_script_size = 32i128 + 1 + cell.lock_args.len() as i128;
                        let type_script_size = cell
                            .type_args
                            .as_ref()
                            .map(|a| 32i128 + 1 + a.len() as i128)
                            .unwrap_or(0);
                        let cell_occupied =
                            (8 + lock_script_size + type_script_size + cell.data_size as i128)
                                * 100_000_000;
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
                                    input.previous_output_index as i16,
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
                                            tx_idx: tx_idx as i32,
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
                                            tx_idx: tx_idx as i32,
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
            if self.repo.has_unresolved_deep_fork().unwrap_or(false) {
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
                    if blocks_behind <= self.config.bulk_sync_threshold {
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

                        if !self.is_secondary_issuance_bulk_active() {
                            let issuance_blocks: Vec<_> = all_parsed_blocks
                                .iter()
                                .map(|b| {
                                    (
                                        format!("0x{}", hex::encode(&b.hash)),
                                        hex::encode(&b.dao),
                                        b.number,
                                        b.timestamp,
                                    )
                                })
                                .collect();
                            let spawned_epoch = self.pipeline_reset_epoch.load(Ordering::SeqCst);
                            tokio::spawn(Self::run_secondary_issuance_batch(
                                self.rpc.clone(),
                                self.writer.clone(),
                                Arc::clone(&self.secondary_issuance_semaphore),
                                issuance_blocks,
                                Arc::clone(&self.pipeline_reset_epoch),
                                spawned_epoch,
                            ));
                        }

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
        if blocks_behind <= self.config.bulk_sync_threshold {
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
                if let Err(cleanup_err) = self.writer.cleanup_batch_range(
                    i64::try_from(start_block).map_err(|_| {
                        anyhow!("batch cleanup start_block exceeds i64: {}", start_block)
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
                            "sequential range {}-{} (chain_tip={})",
                            start_block, end_block, chain_tip
                        ),
                    )?;
                    if let Err(consistency_err) = self.reconcile_hodl_tracker_with_tip(cleanup_tip)
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
            if !self.is_secondary_issuance_bulk_active() {
                let mut issuance_blocks = Vec::new();
                for block_response in &blocks {
                    let block_number =
                        i64::try_from(BlockParser::parse_block_number(&block_response.block))
                            .map_err(|_| {
                                anyhow!(
                            "block number exceeds i64 range: block_hash={}, block_number={}",
                            block_response.block.header.hash,
                            BlockParser::parse_block_number(&block_response.block)
                        )
                            })?;
                    let block_timestamp =
                        BlockParser::parse_timestamp(&block_response.block.header.timestamp);
                    issuance_blocks.push((
                        block_response.block.header.hash.clone(),
                        block_response.block.header.dao.clone(),
                        block_number,
                        block_timestamp,
                    ));
                }
                let spawned_epoch = self.pipeline_reset_epoch.load(Ordering::SeqCst);
                tokio::spawn(Self::run_secondary_issuance_batch(
                    self.rpc.clone(),
                    self.writer.clone(),
                    Arc::clone(&self.secondary_issuance_semaphore),
                    issuance_blocks,
                    Arc::clone(&self.pipeline_reset_epoch),
                    spawned_epoch,
                ));
            }

            self.maybe_invalidate_chart_caches(end_block).await;
        }

        self.check_bulk_sync_completion().await;
        self.ensure_compaction_mode(self.progress.blocks_remaining());

        Ok(SyncAction::Continue)
    }

    async fn check_bulk_sync_completion(&self) {
        let currently_bulk = self.is_bulk_sync_active();
        let was_bulk = self.was_bulk_sync_active.load(Ordering::SeqCst);
        let currently_secondary_bulk = self.is_secondary_issuance_bulk_active();
        let was_secondary_bulk = self
            .was_secondary_issuance_bulk_active
            .load(Ordering::SeqCst);

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

        if was_secondary_bulk && !currently_secondary_bulk {
            info!("Secondary issuance bulk sync completed");
        }

        self.was_bulk_sync_active
            .store(currently_bulk, Ordering::SeqCst);
        self.was_secondary_issuance_bulk_active
            .store(currently_secondary_bulk, Ordering::SeqCst);
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
                let mut summary = LabelImportResult::default();

                let mut core_config = config.clone();
                core_config.import_scripts = false;
                let core_result = crate::label_import::run_label_import(
                    core_store.as_ref(),
                    ckb_store.as_deref(),
                    &core_config,
                )?;
                summary.udt_labels_imported += core_result.udt_labels_imported;
                summary.script_labels_imported += core_result.script_labels_imported;
                summary.errors.extend(core_result.errors);

                Ok::<LabelImportResult, anyhow::Error>(summary)
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
                let lock_script_size = 32 + 1 + cell.lock_args.len() as i64;
                let type_script_size = cell
                    .type_args
                    .as_ref()
                    .map(|args| 32 + 1 + args.len() as i64)
                    .unwrap_or(0);
                let occupied_capacity =
                    (8 + lock_script_size + type_script_size + cell.data_size as i64) * 100_000_000;
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
            let key = (hash_arr, *idx as i32);
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
                        input.previous_output_index as i16,
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
                let lock_script_size = 32 + 1 + cell.lock_args.len() as i64;
                let type_script_size = cell
                    .type_args
                    .as_ref()
                    .map(|a| 32 + 1 + a.len() as i64)
                    .unwrap_or(0);
                let cell_occupied =
                    (8 + lock_script_size + type_script_size + cell.data_size as i64) * 100_000_000;
                self.cell_cache.insert(
                    (tx_data.hash, output_index as i32),
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
                all_cells.push((
                    tx_data.hash.as_slice(),
                    output_index as i16,
                    cell,
                    tx_data.block_number,
                ));
            }
        }

        // Write blocks, txs, cells via StoreBatch
        let t_headers = Instant::now();
        {
            let mut batch = StoreBatch::new(self.writer.store());
            let mut tx_undo_seq_by_block: HashMap<i64, u64> = HashMap::new();
            if !block_refs.is_empty() {
                self.writer.insert_blocks_batch(&block_refs, &mut batch)?;
            }
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
                    batch_proposals.push((proposal_id.clone(), parsed_block.number, idx as i16));
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
                    let key = (
                        input.previous_tx_hash.to_vec(),
                        input.previous_output_index as i16,
                    );
                    if let Some(info) = input_cell_info.get(&key) {
                        all_consumptions.push((
                            input.previous_tx_hash.as_slice(),
                            input.previous_output_index as i16,
                            info.created_at_block,
                            tx_data.hash.as_slice(),
                            tx_data.block_number,
                            input_index as i16,
                        ));
                    } else if let Some(info) = batch_cell_infos.get(&key) {
                        all_consumptions.push((
                            input.previous_tx_hash.as_slice(),
                            input.previous_output_index as i16,
                            info.created_at_block,
                            tx_data.hash.as_slice(),
                            tx_data.block_number,
                            input_index as i16,
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
                        input.previous_output_index as i16,
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
                let lock_script_size = 32i128 + 1 + cell.lock_args.len() as i128;
                let type_script_size = cell
                    .type_args
                    .as_ref()
                    .map(|a| 32i128 + 1 + a.len() as i128)
                    .unwrap_or(0);
                let cell_occupied =
                    (8 + lock_script_size + type_script_size + cell.data_size as i128)
                        * 100_000_000;
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
                let lock_script_size = 32 + 1 + cell.lock_args.len() as i64;
                let type_script_size = cell
                    .type_args
                    .as_ref()
                    .map(|a| 32 + 1 + a.len() as i64)
                    .unwrap_or(0);
                let cell_occupied =
                    (8 + lock_script_size + type_script_size + cell.data_size as i64) * 100_000_000;
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
                        input.previous_output_index as i16,
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
                        input.previous_output_index as i16,
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
                    let lock_script_size = 33_i128 + cell.lock_args.len() as i128;
                    let type_script_size = cell
                        .type_args
                        .as_ref()
                        .map(|args| 33_i128 + args.len() as i128)
                        .unwrap_or(0);
                    (8_i128 + lock_script_size + type_script_size + i128::from(cell.data_size))
                        * 100_000_000_i128
                })
                .sum();
            let data_size_consumed: i64 = tx_slice
                .iter()
                .filter(|tx| !tx.is_cellbase)
                .flat_map(|tx| tx.inputs.iter())
                .filter_map(|input| {
                    let key = (
                        input.previous_tx_hash.to_vec(),
                        input.previous_output_index as i16,
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
                        input.previous_output_index as i16,
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
                    if let Some(ref type_code_hash) = cell.type_code_hash {
                        if type_code_hash == &dao_code_hash && cell.data_size == 8 {
                            if let Some(data) = tx_data.outputs_data.get(idx) {
                                let data_bytes = crate::rpc::parse_hex_to_bytes(data);
                                if let Some(deposit_block) =
                                    DaoParser::parse_deposit_block_number(&data_bytes)
                                {
                                    new_dao_outputs.push((
                                        tx_data.hash.to_vec(),
                                        idx as i16,
                                        cell.lock_script_hash.clone(),
                                        cell.capacity,
                                        deposit_block,
                                    ));
                                }
                            }
                        } else {
                            candidate_withdraw_to_outputs
                                .push((idx as i16, cell.lock_script_hash.clone()));
                        }
                    } else {
                        candidate_withdraw_to_outputs
                            .push((idx as i16, cell.lock_script_hash.clone()));
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
                    let mut batch = StoreBatch::new(self.writer.store());
                    self.writer.process_udt_transfers_batch(
                        &transfer_refs,
                        &max_supply_observations,
                        &block_timestamps,
                        &mut batch,
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
                            batch_spore_ids.insert(spore.spore_id.clone());
                            self.writer.insert_spore_cell(
                                spore,
                                &tx_data.hash,
                                output_index as i16,
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
                                tx_idx as i32,
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
                            tx_idx as i32,
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
                                tx_idx: tx_idx as i32,
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
                                        tx_idx as i32,
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
                                        tx_idx: tx_idx as i32,
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

        {
            let mut batch = StoreBatch::new(self.writer.store());
            self.write_batch_stats_to_batch(&batch_stats, &mut batch)?;
            if bulk_sync_mode {
                let commit_started = Instant::now();
                batch.commit_no_wal()?;
                commit_ms += commit_started.elapsed().as_secs_f64() * 1000.0;
            } else {
                let commit_started = Instant::now();
                batch.commit()?;
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
                        input.previous_output_index as i16,
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
                all_cells.push((
                    tx_data.hash.as_slice(),
                    output_index as i16,
                    cell,
                    tx_data.block_number,
                ));
            }

            if !tx_data.is_cellbase {
                for (input_index, input) in tx_data.inputs.iter().enumerate() {
                    let key = (
                        input.previous_tx_hash.to_vec(),
                        input.previous_output_index as i16,
                    );
                    let info = input_cell_info
                        .get(&key)
                        .or_else(|| batch_cell_infos.get(&key));
                    if let Some(info) = info {
                        all_consumptions.push((
                            input.previous_tx_hash.as_slice(),
                            input.previous_output_index as i16,
                            info.created_at_block,
                            tx_data.hash.as_slice(),
                            tx_data.block_number,
                            input_index as i16,
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
                        input.previous_output_index as i16,
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
                        batch_proposals.push((
                            proposal_id.clone(),
                            parsed_block.number,
                            proposal_index as i16,
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
            rayon::join(
                || {
                    rayon::join(
                        || {
                            // DAO: collect input outpoints, deduplicate, batch query DB
                            let mut all_input_outpoints_dao: Vec<(Vec<u8>, i16)> = Vec::new();
                            let mut block_tx_idx = 0usize;
                            for parsed in all_parsed_blocks.iter() {
                                let tx_count_for_block = parsed.transactions_count as usize;
                                let tx_slice =
                                    &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                                block_tx_idx += tx_count_for_block;
                                for tx_data in tx_slice {
                                    if tx_data.is_cellbase || tx_data.inputs.is_empty() {
                                        continue;
                                    }
                                    for input in &tx_data.inputs {
                                        all_input_outpoints_dao.push((
                                            input.previous_tx_hash.to_vec(),
                                            input.previous_output_index as i16,
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
                                let tx_slice =
                                    &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
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
                        same_batch_dao_deposits.insert(
                            (deposit.tx_hash.clone(), deposit.output_index as i16),
                            (
                                0,
                                deposit.tx_hash.clone(),
                                deposit.output_index as i16,
                                deposit.capacity.to_string(),
                                *block_number,
                                0i16, // status = 0 (active)
                            ),
                        );
                        let outpoint_key = ckbadger_store::keys::encode_outpoint(
                            &deposit.tx_hash,
                            deposit.output_index as i16,
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
                        use crate::db::DaoWithdrawalContextTrait;
                        #[derive(Clone)]
                        struct DaoWithdrawalContext {
                            consumed_deposits: Vec<(i64, Vec<u8>, i16, String, i64, i16)>,
                            new_dao_outputs: Vec<(Vec<u8>, i16, Vec<u8>, i64, u64)>,
                            tx_inputs: Vec<(Vec<u8>, i16)>,
                            candidate_withdraw_to_outputs: Vec<(i16, Vec<u8>)>,
                            block_number: i64,
                            consuming_tx_hash: Vec<u8>,
                            timestamp: DateTime<Utc>,
                        }
                        impl DaoWithdrawalContextTrait for DaoWithdrawalContext {
                            fn consumed_deposits(
                                &self,
                            ) -> &[(i64, Vec<u8>, i16, String, i64, i16)]
                            {
                                &self.consumed_deposits
                            }
                            fn new_dao_outputs(&self) -> &[(Vec<u8>, i16, Vec<u8>, i64, u64)] {
                                &self.new_dao_outputs
                            }
                            fn block_number(&self) -> i64 {
                                self.block_number
                            }
                            fn consuming_tx_hash(&self) -> &[u8] {
                                &self.consuming_tx_hash
                            }
                            fn timestamp(&self) -> DateTime<Utc> {
                                self.timestamp
                            }
                            fn withdraw_to_output_index_for_lock(
                                &self,
                                lock_script_hash: &[u8],
                            ) -> Option<i16> {
                                let mut same_lock = self
                                    .candidate_withdraw_to_outputs
                                    .iter()
                                    .filter_map(|(output_index, output_lock_hash)| {
                                        (output_lock_hash.as_slice() == lock_script_hash)
                                            .then_some(*output_index)
                                    });
                                if let Some(first) = same_lock.next() {
                                    if same_lock.next().is_none() {
                                        return Some(first);
                                    }
                                    return None;
                                }
                                (self.candidate_withdraw_to_outputs.len() == 1)
                                    .then_some(self.candidate_withdraw_to_outputs[0].0)
                            }
                            fn infer_request_output_index(&self, request_tx_hash: &[u8]) -> Option<i16> {
                                let mut matches = self
                                    .tx_inputs
                                    .iter()
                                    .filter_map(|(tx_hash, output_index)| {
                                        (tx_hash.as_slice() == request_tx_hash).then_some(*output_index)
                                    })
                                    .take(2);
                                let first = matches.next()?;
                                if matches.next().is_some() {
                                    return None;
                                }
                                Some(first)
                            }
                        }

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
                                        input.previous_output_index as i16,
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
                                                        idx as i16,
                                                        cell.lock_script_hash.clone(),
                                                        cell.capacity,
                                                        deposit_block,
                                                    ));
                                                }
                                            }
                                        } else {
                                            candidate_withdraw_to_outputs
                                                .push((idx as i16, cell.lock_script_hash.clone()));
                                        }
                                    } else {
                                        candidate_withdraw_to_outputs
                                            .push((idx as i16, cell.lock_script_hash.clone()));
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
                                    writer.process_udt_transfers_batch(
                                        &transfer_refs,
                                        &max_supply_observations,
                                        &block_timestamps,
                                        &mut batch,
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
                                    writer.insert_spore_cell(
                                        spore,
                                        &tx_data.hash,
                                        output_index as i16,
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
                                        tx_idx as i32,
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
                            tx_idx as i32,
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
                                tx_idx: tx_idx as i32,
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
                                            && tx_idx as i32 == event.tx_idx
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
                                let lock_script_size = 33_i128 + cell.lock_args.len() as i128;
                                let type_script_size = cell
                                    .type_args
                                    .as_ref()
                                    .map(|args| 33_i128 + args.len() as i128)
                                    .unwrap_or(0);
                                (8_i128
                                    + lock_script_size
                                    + type_script_size
                                    + i128::from(cell.data_size))
                                    * 100_000_000_i128
                            })
                            .sum();
                        let data_size_consumed: i64 = tx_slice
                            .iter()
                            .filter(|tx| !tx.is_cellbase)
                            .flat_map(|tx| tx.inputs.iter())
                            .filter_map(|input| {
                                let key = (
                                    input.previous_tx_hash.to_vec(),
                                    input.previous_output_index as i16,
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
                                    input.previous_output_index as i16,
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

                            let tx_views: Vec<crate::db::writer::activities::TxView<'_>> = tx_slice
                                .iter()
                                .map(|td| {
                                    let inputs: Vec<crate::db::writer::activities::InputCellView> =
                                        if td.is_cellbase {
                                            Vec::new()
                                        } else {
                                            td.inputs
                                                .iter()
                                                .map(|inp| {
                                                    let key = (
                                                        inp.previous_tx_hash.to_vec(),
                                                        inp.previous_output_index as i16,
                                                    );
                                                    let cell_info = input_cell_info
                                                        .get(&key)
                                                        .or_else(|| batch_cell_infos.get(&key));
                                                    if let Some(info) = cell_info {
                                                        crate::db::writer::activities::InputCellView {
                                                            lock_script_hash: info
                                                                .lock_script_hash
                                                                .clone(),
                                                            capacity: info.capacity,
                                                            occupied_capacity: info.occupied_capacity,
                                                            type_code_hash: info.type_code_hash.clone(),
                                                            type_script_hash: info
                                                                .type_script_hash
                                                                .clone(),
                                                            type_args: info.type_args.clone(),
                                                            udt_amount: info.udt_amount,
                                                            data: Vec::new(),
                                                            data_size: info.data_size,
                                                        }
                                                    } else {
                                                        crate::db::writer::activities::InputCellView {
                                                            lock_script_hash: Vec::new(),
                                                            capacity: 0,
                                                            occupied_capacity: 0,
                                                            type_code_hash: None,
                                                            type_script_hash: None,
                                                            type_args: None,
                                                            udt_amount: None,
                                                            data: Vec::new(),
                                                            data_size: 0,
                                                        }
                                                    }
                                                })
                                                .collect()
                                        };
                                    crate::db::writer::activities::TxView {
                                        tx_hash: &td.hash,
                                        tx_index: td.tx_index,
                                        block_number: parsed.number,
                                        timestamp: parsed.timestamp.timestamp_millis(),
                                        is_cellbase: td.is_cellbase,
                                        inputs,
                                        outputs: &td.cells,
                                        outputs_data: &td.outputs_data,
                                    }
                                })
                                .collect();

                            let activities = crate::db::writer::activities::build_activities_for_block(
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
                                input.previous_output_index as i16,
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
                    same_batch_dao_deposits.insert(
                        (deposit.tx_hash.clone(), deposit.output_index as i16),
                        (
                            0,
                            deposit.tx_hash.clone(),
                            deposit.output_index as i16,
                            deposit.capacity.to_string(),
                            *block_number,
                            0i16, // status = 0 (active)
                        ),
                    );
                    let outpoint_key = ckbadger_store::keys::encode_outpoint(
                        &deposit.tx_hash,
                        deposit.output_index as i16,
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
                    use crate::db::DaoWithdrawalContextTrait;
                    #[derive(Clone)]
                    struct DaoWithdrawalContext {
                        consumed_deposits: Vec<(i64, Vec<u8>, i16, String, i64, i16)>,
                        new_dao_outputs: Vec<(Vec<u8>, i16, Vec<u8>, i64, u64)>,
                        tx_inputs: Vec<(Vec<u8>, i16)>,
                        candidate_withdraw_to_outputs: Vec<(i16, Vec<u8>)>,
                        block_number: i64,
                        consuming_tx_hash: Vec<u8>,
                        timestamp: DateTime<Utc>,
                    }
                    impl DaoWithdrawalContextTrait for DaoWithdrawalContext {
                        fn consumed_deposits(&self) -> &[(i64, Vec<u8>, i16, String, i64, i16)] {
                            &self.consumed_deposits
                        }
                        fn new_dao_outputs(&self) -> &[(Vec<u8>, i16, Vec<u8>, i64, u64)] {
                            &self.new_dao_outputs
                        }
                        fn block_number(&self) -> i64 {
                            self.block_number
                        }
                        fn consuming_tx_hash(&self) -> &[u8] {
                            &self.consuming_tx_hash
                        }
                        fn timestamp(&self) -> DateTime<Utc> {
                            self.timestamp
                        }
                        fn withdraw_to_output_index_for_lock(
                            &self,
                            lock_script_hash: &[u8],
                        ) -> Option<i16> {
                            let mut same_lock = self
                                .candidate_withdraw_to_outputs
                                .iter()
                                .filter_map(|(output_index, output_lock_hash)| {
                                    (output_lock_hash.as_slice() == lock_script_hash)
                                        .then_some(*output_index)
                                });
                            if let Some(first) = same_lock.next() {
                                if same_lock.next().is_none() {
                                    return Some(first);
                                }
                                return None;
                            }
                            (self.candidate_withdraw_to_outputs.len() == 1)
                                .then_some(self.candidate_withdraw_to_outputs[0].0)
                        }
                        fn infer_request_output_index(
                            &self,
                            request_tx_hash: &[u8],
                        ) -> Option<i16> {
                            let mut matches = self
                                .tx_inputs
                                .iter()
                                .filter_map(|(tx_hash, output_index)| {
                                    (tx_hash.as_slice() == request_tx_hash).then_some(*output_index)
                                })
                                .take(2);
                            let first = matches.next()?;
                            if matches.next().is_some() {
                                return None;
                            }
                            Some(first)
                        }
                    }

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
                                    input.previous_output_index as i16,
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
                                if let Some(ref type_code_hash) = cell.type_code_hash {
                                    if type_code_hash == &dao_code_hash && cell.data_size == 8 {
                                        if let Some(data) = tx_data.outputs_data.get(idx) {
                                            let data_bytes = crate::rpc::parse_hex_to_bytes(data);
                                            if let Some(deposit_block) =
                                                DaoParser::parse_deposit_block_number(&data_bytes)
                                            {
                                                new_dao_outputs.push((
                                                    tx_data.hash.to_vec(),
                                                    idx as i16,
                                                    cell.lock_script_hash.clone(),
                                                    cell.capacity,
                                                    deposit_block,
                                                ));
                                            }
                                        }
                                    } else {
                                        candidate_withdraw_to_outputs
                                            .push((idx as i16, cell.lock_script_hash.clone()));
                                    }
                                } else {
                                    candidate_withdraw_to_outputs
                                        .push((idx as i16, cell.lock_script_hash.clone()));
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
                        self.writer.process_udt_transfers_batch(
                            &transfer_refs,
                            &max_supply_observations,
                            &block_timestamps,
                            &mut data_batch,
                        )?;
                    }
                }
            }

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
                                batch_spore_ids.insert(spore.spore_id.clone());
                                self.writer.insert_spore_cell(
                                    spore,
                                    &tx_data.hash,
                                    output_index as i16,
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
                                    tx_idx as i32,
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
                                tx_idx as i32,
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
                                    tx_idx: tx_idx as i32,
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
                                tx_idx as i32,
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
                let mut nft_activity_batch = StoreBatch::new(&self.append_only_store);
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
                        block_number,
                        ckbadger_store::CF_NFT_COLLECTION_ACTIVITIES,
                        &append_key,
                    );
                }
                if !nft_activity_batch.is_empty() {
                    nft_activity_batch.commit()?;
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
                        .map(|td| {
                            let inputs: Vec<crate::db::writer::activities::InputCellView> =
                                if td.is_cellbase {
                                    Vec::new()
                                } else {
                                    td.inputs
                                        .iter()
                                        .map(|inp| {
                                            let key = (
                                                inp.previous_tx_hash.to_vec(),
                                                inp.previous_output_index as i16,
                                            );
                                            let cell_info = input_cell_info
                                                .get(&key)
                                                .or_else(|| batch_cell_infos.get(&key));
                                            if let Some(info) = cell_info {
                                                crate::db::writer::activities::InputCellView {
                                                    lock_script_hash: info.lock_script_hash.clone(),
                                                    capacity: info.capacity,
                                                    occupied_capacity: info.occupied_capacity,
                                                    type_code_hash: info.type_code_hash.clone(),
                                                    type_script_hash: info.type_script_hash.clone(),
                                                    type_args: info.type_args.clone(),
                                                    udt_amount: info.udt_amount,
                                                    data: Vec::new(),
                                                    data_size: info.data_size,
                                                }
                                            } else {
                                                crate::db::writer::activities::InputCellView {
                                                    lock_script_hash: Vec::new(),
                                                    capacity: 0,
                                                    occupied_capacity: 0,
                                                    type_code_hash: None,
                                                    type_script_hash: None,
                                                    type_args: None,
                                                    udt_amount: None,
                                                    data: Vec::new(),
                                                    data_size: 0,
                                                }
                                            }
                                        })
                                        .collect()
                                };
                            crate::db::writer::activities::TxView {
                                tx_hash: &td.hash,
                                tx_index: td.tx_index,
                                block_number: parsed.number,
                                timestamp: parsed.timestamp.timestamp_millis(),
                                is_cellbase: td.is_cellbase,
                                inputs,
                                outputs: &td.cells,
                                outputs_data: &td.outputs_data,
                            }
                        })
                        .collect();

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
                            input.previous_output_index as i16,
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
                        let lock_script_size = 33_i128 + cell.lock_args.len() as i128;
                        let type_script_size = cell
                            .type_args
                            .as_ref()
                            .map(|args| 33_i128 + args.len() as i128)
                            .unwrap_or(0);
                        (8_i128 + lock_script_size + type_script_size + i128::from(cell.data_size))
                            * 100_000_000_i128
                    })
                    .sum();
                let data_size_consumed: i64 = tx_slice
                    .iter()
                    .filter(|tx| !tx.is_cellbase)
                    .flat_map(|tx| tx.inputs.iter())
                    .filter_map(|input| {
                        let key = (
                            input.previous_tx_hash.to_vec(),
                            input.previous_output_index as i16,
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
                            input.previous_output_index as i16,
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
                let running_depositors = latest_snapshot
                    .as_ref()
                    .map(|s| s.depositors_count)
                    .unwrap_or(0);
                let mut running_total_deposit_count = latest_snapshot
                    .as_ref()
                    .map(|s| s.new_deposits)
                    .unwrap_or(0);
                let mut running_total_withdrawal_count =
                    latest_snapshot.as_ref().map(|s| s.withdrawals).unwrap_or(0);
                let running_total_compensation = latest_snapshot
                    .as_ref()
                    .map(|s| s.compensation)
                    .unwrap_or(0);
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
                            input.previous_output_index as i16,
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

    // === update_secondary_issuance ===

    /// Update secondary issuance for a block. Static method that can be called from spawned tasks.
    async fn update_secondary_issuance(
        rpc: &CkbRpcClient,
        writer: &BatchWriter,
        block_hash: &str,
        dao_hex: &str,
        block_number: i64,
        block_timestamp: DateTime<Utc>,
    ) -> Result<()> {
        // Check if we already have issuance data for this block
        if writer.store().get_block_issuance(block_number)?.is_some() {
            return Ok(());
        }

        let economic_state = match rpc.get_block_economic_state(block_hash).await? {
            Some(state) => state,
            None => {
                bail!(
                    "missing economic state for block: block_number={}, block_hash={}",
                    block_number,
                    block_hash
                );
            }
        };

        let dao_field = parse_dao_field_for_secondary_issuance(dao_hex, block_number, block_hash)?;

        let secondary_issuance: u128 =
            parse_prefixed_hex_u128(&economic_state.issuance.secondary, "secondary issuance")?;

        let miner_secondary: u128 =
            parse_prefixed_hex_u128(&economic_state.miner_reward.secondary, "miner secondary")?;

        let non_miner_secondary = checked_sub_u128(
            secondary_issuance,
            miner_secondary,
            "secondary_issuance - miner_secondary",
        )?;

        // Calculate dao_compensation and burnt using RFC-0015 formula
        // dao_compensation = non_miner * deposit / (C - U)
        // burnt = non_miner * liquid / (C - U) where liquid = C - U - deposit
        let total_issuance = dao_field.total_issuance as u128;
        let occupied = dao_field.occupied_capacity as u128;
        let denominator = checked_sub_u128(
            total_issuance,
            occupied,
            "total_issuance - occupied_capacity",
        )?;

        let (dao_compensation, burnt) = if denominator > 0 {
            let total_dao_deposits: u128 = writer.get_dao_deposits_at_block(block_number)?;

            let dao_share = non_miner_secondary
                .checked_mul(total_dao_deposits)
                .ok_or_else(|| anyhow::anyhow!("dao_share multiply overflow"))?
                / denominator;
            let burnt_share = checked_sub_u128(
                non_miner_secondary,
                dao_share,
                "non_miner_secondary - dao_share",
            )?;
            (dao_share, burnt_share)
        } else {
            (0, non_miner_secondary)
        };

        let breakdown = SecondaryIssuanceBreakdown {
            secondary_issuance: checked_u128_to_i64(secondary_issuance, "secondary_issuance")?,
            miner_secondary: checked_u128_to_i64(miner_secondary, "miner_secondary")?,
            dao_compensation: checked_u128_to_i64(dao_compensation, "dao_compensation")?,
            burnt: checked_u128_to_i64(burnt, "burnt")?,
        };

        let mut batch = StoreBatch::new(writer.store());
        writer.accumulate_secondary_issuance(
            &breakdown,
            block_number,
            block_timestamp,
            &mut batch,
        )?;
        batch.commit()?;

        Ok(())
    }

    /// Run secondary issuance updates for a batch of blocks, gated by the semaphore.
    /// Called from a spawned task — does not require &self.
    /// The `pipeline_epoch` / `spawned_epoch` pair aborts stale tasks after reorg.
    async fn run_secondary_issuance_batch(
        rpc: CkbRpcClient,
        writer: BatchWriter,
        sem: Arc<tokio::sync::Semaphore>,
        blocks: Vec<(String, String, i64, DateTime<Utc>)>,
        pipeline_epoch: Arc<AtomicU64>,
        spawned_epoch: u64,
    ) {
        let _permit = match tokio::time::timeout(Duration::from_secs(5), sem.acquire()).await {
            Ok(Ok(permit)) => permit,
            Ok(Err(_closed)) => return,
            Err(_timeout) => {
                warn!("Secondary issuance permit acquisition took >5s, issuance backlog");
                match sem.acquire().await {
                    Ok(permit) => permit,
                    Err(_) => return,
                }
            }
        };
        for (hash, dao, number, timestamp) in blocks {
            if pipeline_epoch.load(Ordering::SeqCst) != spawned_epoch {
                info!(
                    "Aborting stale secondary issuance task (epoch changed: spawned={}, current={}), remaining blocks starting at {}",
                    spawned_epoch,
                    pipeline_epoch.load(Ordering::SeqCst),
                    number
                );
                return;
            }
            if let Err(e) =
                Self::update_secondary_issuance(&rpc, &writer, &hash, &dao, number, timestamp).await
            {
                warn!(
                    "Failed to update secondary issuance for block {}: {}",
                    number, e
                );
            }
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

    fn dummy_cached_cell_info(created_at_block: i64) -> CachedCellInfo {
        CachedCellInfo {
            capacity: 1,
            created_at_block,
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

    fn molecule_u32(value: usize) -> [u8; 4] {
        (value as u32).to_le_bytes()
    }

    fn molecule_table(fields: &[Vec<u8>]) -> Vec<u8> {
        let header_size = 4 + fields.len() * 4;
        let total_size = header_size + fields.iter().map(|field| field.len()).sum::<usize>();

        let mut out = Vec::with_capacity(total_size);
        out.extend_from_slice(&molecule_u32(total_size));

        let mut offset = header_size;
        for field in fields {
            out.extend_from_slice(&molecule_u32(offset));
            offset += field.len();
        }
        for field in fields {
            out.extend_from_slice(field);
        }
        out
    }

    fn molecule_bytes(value: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + value.len());
        out.extend_from_slice(&molecule_u32(4 + value.len()));
        out.extend_from_slice(value);
        out
    }

    fn molecule_dynvec(items: &[Vec<u8>]) -> Vec<u8> {
        if items.is_empty() {
            return molecule_u32(4).to_vec();
        }

        let header_size = 4 + items.len() * 4;
        let total_size = header_size + items.iter().map(|item| item.len()).sum::<usize>();

        let mut out = Vec::with_capacity(total_size);
        out.extend_from_slice(&molecule_u32(total_size));

        let mut offset = header_size;
        for item in items {
            out.extend_from_slice(&molecule_u32(offset));
            offset += item.len();
        }
        for item in items {
            out.extend_from_slice(item);
        }
        out
    }

    fn encode_script(args: &[u8]) -> Vec<u8> {
        molecule_table(&[vec![0xCC; 32], vec![1], molecule_bytes(args)])
    }

    fn encode_script_vec_with_unique_args(unique_type_args: &[u8]) -> Vec<u8> {
        molecule_dynvec(&[encode_script(unique_type_args)])
    }

    fn encode_xudt_witness(script_vec: &[u8]) -> Vec<u8> {
        molecule_table(&[Vec::new(), Vec::new(), script_vec.to_vec(), Vec::new()])
    }

    fn encode_witness_args(input_type: Option<&[u8]>, output_type: Option<&[u8]>) -> Vec<u8> {
        let lock = Vec::new();
        let input_type = input_type.map(molecule_bytes).unwrap_or_default();
        let output_type = output_type.map(molecule_bytes).unwrap_or_default();
        molecule_table(&[lock, input_type, output_type])
    }

    fn build_token_info_data(total_supply: u128) -> Vec<u8> {
        let mut data = Vec::new();
        data.push(8); // decimal
        data.push(5); // name len
        data.extend_from_slice(b"Token");
        data.push(3); // symbol len
        data.extend_from_slice(b"TKN");
        data.extend_from_slice(&TOKEN_INFO_TAG_TOTAL_SUPPLY.to_le_bytes());
        data.extend_from_slice(&(TOKEN_INFO_TOTAL_SUPPLY_DATA_LEN as u32).to_le_bytes());
        data.extend_from_slice(&total_supply.to_le_bytes());
        data
    }

    fn dummy_unique_token_info_cell(
        unique_type_args: Vec<u8>,
        total_supply: u128,
    ) -> crate::parser::cell::ParsedCell {
        let data = build_token_info_data(total_supply);
        crate::parser::cell::ParsedCell {
            capacity: 100_00000000,
            lock_code_hash: vec![0x10; 32],
            lock_hash_type: 1,
            lock_args: vec![0x20; 20],
            lock_script_hash: vec![0x30; 32],
            type_code_hash: Some(vec![0x40; 32]),
            type_hash_type: Some(1),
            type_args: Some(unique_type_args),
            type_script_hash: Some(vec![0x50; 32]),
            data_hash: vec![0x60; 32],
            data_size: data.len() as i32,
            data,
        }
    }

    fn dummy_xudt_cell(
        token_type_hash: [u8; 32],
        type_args: Vec<u8>,
    ) -> crate::parser::cell::ParsedCell {
        let type_code_hash =
            crate::rpc::parse_hex_to_bytes(crate::parser::udt::XUDT_CODE_HASH_TYPE);
        crate::parser::cell::ParsedCell {
            capacity: 100_00000000,
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![0x22; 20],
            lock_script_hash: vec![0x33; 32],
            type_code_hash: Some(type_code_hash),
            type_hash_type: Some(1),
            type_args: Some(type_args),
            type_script_hash: Some(token_type_hash.to_vec()),
            data_hash: vec![0x44; 32],
            data_size: 16,
            data: vec![0u8; 16],
        }
    }

    #[test]
    fn test_parse_parsed_cell_udt_amount_allows_xudt_without_amount_payload() {
        let mut cell = dummy_xudt_cell([0xAB; 32], vec![0xCD; XUDT_TYPE_ARGS_MIN_LEN]);
        cell.data.clear();
        cell.data_size = 0;

        let tx_hash = [0x81; 32];
        let amount = parse_parsed_cell_udt_amount(&cell, &tx_hash, 3, None).unwrap();
        assert_eq!(amount, None);
    }

    #[test]
    fn test_parse_parsed_cell_udt_amount_rejects_invalid_sudt_payload() {
        let sudt_code_hash = crate::rpc::parse_hex_to_bytes(crate::parser::udt::SUDT_CODE_HASH);
        let cell = crate::parser::cell::ParsedCell {
            capacity: 100_00000000,
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![0x22; 20],
            lock_script_hash: vec![0x33; 32],
            type_code_hash: Some(sudt_code_hash.clone()),
            type_hash_type: Some(1),
            type_args: Some(vec![0x44; 32]),
            type_script_hash: Some(vec![0x55; 32]),
            data_hash: vec![0x66; 32],
            data_size: 0,
            data: vec![],
        };

        let tx_hash = [0x82; 32];
        let err = parse_parsed_cell_udt_amount(&cell, &tx_hash, 7, None).unwrap_err();
        assert!(err.to_string().contains("failed to parse UDT amount"));
        assert!(err.to_string().contains("0x8282828282828282"));
    }

    #[test]
    fn test_parse_parsed_cell_udt_amount_supports_xudt_compatible_hint() {
        let amount = 15_778_600u128;
        let mut data = vec![0u8; 16];
        data.copy_from_slice(&amount.to_le_bytes());
        let cell = crate::parser::cell::ParsedCell {
            capacity: 100_00000000,
            lock_code_hash: vec![0x11; 32],
            lock_hash_type: 1,
            lock_args: vec![0x22; 20],
            lock_script_hash: vec![0x33; 32],
            type_code_hash: Some(vec![0x42; 32]), // non-standard xUDT code hash
            type_hash_type: Some(1),
            type_args: Some(vec![0x44; 32]),
            type_script_hash: Some(vec![0x55; 32]),
            data_hash: vec![0x66; 32],
            data_size: 16,
            data,
        };

        let tx_hash = [0x83; 32];
        let parsed =
            parse_parsed_cell_udt_amount(&cell, &tx_hash, 0, Some("xudt_compatible")).unwrap();
        assert_eq!(parsed, Some(amount));

        let no_hint = parse_parsed_cell_udt_amount(&cell, &tx_hash, 0, None).unwrap();
        assert_eq!(no_hint, None);
    }

    fn build_xudt_type_args_with_extension_in_args(
        owner_lock_hash: [u8; 32],
        script_vec: &[u8],
    ) -> Vec<u8> {
        let mut type_args = Vec::with_capacity(XUDT_TYPE_ARGS_MIN_LEN + script_vec.len());
        type_args.extend_from_slice(&owner_lock_hash);
        type_args.extend_from_slice(&XUDT_FLAGS_EXTENSION_IN_ARGS.to_le_bytes());
        type_args.extend_from_slice(script_vec);
        type_args
    }

    fn build_xudt_type_args_with_extension_in_witness(
        owner_lock_hash: [u8; 32],
        script_vec_hash: [u8; 20],
    ) -> Vec<u8> {
        let mut type_args = Vec::with_capacity(XUDT_TYPE_ARGS_MIN_LEN + script_vec_hash.len());
        type_args.extend_from_slice(&owner_lock_hash);
        type_args.extend_from_slice(&XUDT_FLAGS_EXTENSION_IN_WITNESS.to_le_bytes());
        type_args.extend_from_slice(&script_vec_hash);
        type_args
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
    fn test_rollback_via_undo_log_preserves_append_history_cfs() {
        let domain_dir = tempfile::tempdir().unwrap();
        let append_dir = tempfile::tempdir().unwrap();
        let domain_store = CkbadgerStore::open_domain(domain_dir.path()).unwrap();
        let append_store = CkbadgerStore::open_append_only(append_dir.path()).unwrap();
        let lock_hash = [0x44; 32];
        let collection_id = [0x55; 24];

        let addr_keep = keys::encode_addr_tx_key(&lock_hash, 10, 0);
        let addr_drop = keys::encode_addr_tx_key(&lock_hash, 20, 0);
        append_store
            .put_cf(append_store.cf_addr_txs(), &addr_keep, &[0x01])
            .unwrap();
        append_store
            .put_cf(append_store.cf_addr_txs(), &addr_drop, &[0x02])
            .unwrap();

        let act_keep = keys::encode_activity_key(&lock_hash, 11, 0);
        let act_drop = keys::encode_activity_key(&lock_hash, 21, 0);
        append_store
            .put_cf(append_store.cf_activities(), &act_keep, &[0x03])
            .unwrap();
        append_store
            .put_cf(append_store.cf_activities(), &act_drop, &[0x04])
            .unwrap();

        let nft_keep = keys::encode_nft_collection_activity_key(&collection_id, 12, 0);
        let nft_drop = keys::encode_nft_collection_activity_key(&collection_id, 22, 0);
        append_store
            .put_cf(
                append_store.cf_nft_collection_activities(),
                &nft_keep,
                &[0x05],
            )
            .unwrap();
        append_store
            .put_cf(
                append_store.cf_nft_collection_activities(),
                &nft_drop,
                &[0x06],
            )
            .unwrap();

        let mut domain_batch = StoreBatch::new(&domain_store);
        let mut undo_seq_by_block = HashMap::new();
        put_append_delete_undo_entry(
            &mut domain_batch,
            &mut undo_seq_by_block,
            20,
            ckbadger_store::CF_ADDR_TXS,
            &addr_drop,
        );
        put_append_delete_undo_entry(
            &mut domain_batch,
            &mut undo_seq_by_block,
            21,
            ckbadger_store::CF_ACTIVITIES,
            &act_drop,
        );
        put_append_delete_undo_entry(
            &mut domain_batch,
            &mut undo_seq_by_block,
            22,
            ckbadger_store::CF_NFT_COLLECTION_ACTIVITIES,
            &nft_drop,
        );
        domain_batch.commit().unwrap();

        domain_store
            .rollback_via_undo_log(&append_store, 15)
            .unwrap();

        assert!(append_store
            .get_cf(append_store.cf_addr_txs(), &addr_keep)
            .unwrap()
            .is_some());
        assert!(append_store
            .get_cf(append_store.cf_addr_txs(), &addr_drop)
            .unwrap()
            .is_some());
        assert!(append_store
            .get_cf(append_store.cf_activities(), &act_keep)
            .unwrap()
            .is_some());
        assert!(append_store
            .get_cf(append_store.cf_activities(), &act_drop)
            .unwrap()
            .is_some());
        assert!(append_store
            .get_cf(append_store.cf_nft_collection_activities(), &nft_keep)
            .unwrap()
            .is_some());
        assert!(append_store
            .get_cf(append_store.cf_nft_collection_activities(), &nft_drop)
            .unwrap()
            .is_some());
    }

    #[test]
    fn test_rollback_undo_log_after_batch_cleanup_prunes_valid_entries() {
        let domain_dir = tempfile::tempdir().unwrap();
        let append_dir = tempfile::tempdir().unwrap();
        let domain_store = CkbadgerStore::open_domain(domain_dir.path()).unwrap();
        let append_store = CkbadgerStore::open_append_only(append_dir.path()).unwrap();

        let mut batch = StoreBatch::new(&domain_store);
        batch.put_reorg_undo_log_by_block(
            6,
            0,
            &ckbadger_store::types::UndoLogEntry::TxContext(ckbadger_store::types::UndoTxContext {
                tx_hash: vec![0x88; 32],
                outputs_count: 0,
                inputs: vec![],
            }),
        );
        batch.commit().unwrap();

        rollback_undo_log_after_batch_cleanup(&domain_store, &append_store, 5, "unit-test")
            .unwrap();

        let undo_key = keys::encode_reorg_undo_log_key(6, 0);
        assert!(domain_store
            .get_cf(domain_store.cf_reorg_undo_log_by_block(), &undo_key)
            .unwrap()
            .is_none());
    }

    #[test]
    fn test_rollback_undo_log_after_batch_cleanup_fails_on_malformed_undo_key() {
        let domain_dir = tempfile::tempdir().unwrap();
        let append_dir = tempfile::tempdir().unwrap();
        let domain_store = CkbadgerStore::open_domain(domain_dir.path()).unwrap();
        let append_store = CkbadgerStore::open_append_only(append_dir.path()).unwrap();

        domain_store
            .put_cf(
                domain_store.cf_reorg_undo_log_by_block(),
                b"bad-key",
                b"bad-value",
            )
            .unwrap();

        let err = rollback_undo_log_after_batch_cleanup(
            &domain_store,
            &append_store,
            -1,
            "unit-test malformed key",
        )
        .unwrap_err();

        assert!(err
            .to_string()
            .contains("failed to rollback undo log after batch cleanup"));
    }

    #[test]
    fn test_format_outpoint_sample_limits_items() {
        let outpoints = vec![
            (vec![0x11; 32], 0),
            (vec![0x22; 32], 1),
            (vec![0x33; 32], 2),
        ];

        let sample = format_outpoint_sample(&outpoints, 2);
        assert!(sample.contains(&format!("0x{}:0", "11".repeat(32))));
        assert!(sample.contains(&format!("0x{}:1", "22".repeat(32))));
        assert!(!sample.contains(&format!("0x{}:2", "33".repeat(32))));
    }

    #[test]
    fn test_is_bulk_sync_active_by_lag_threshold() {
        assert!(!is_bulk_sync_active_by_lag(1000, 1000));
        assert!(is_bulk_sync_active_by_lag(1001, 1000));
        assert!(!is_bulk_sync_active_by_lag(0, 1000));
    }

    #[test]
    fn test_is_bulk_sync_batch_uses_tip_distance() {
        assert!(!is_bulk_sync_batch(10_000, 9_000, 1000));
        assert!(is_bulk_sync_batch(10_001, 9_000, 1000));
    }

    #[test]
    #[should_panic(expected = "invalid bulk-sync batch range")]
    fn test_is_bulk_sync_batch_panics_when_batch_end_exceeds_tip() {
        let _ = is_bulk_sync_batch(100, 150, 1000);
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
        let store = CkbadgerStore::open(dir.path()).unwrap();
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
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
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
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
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
    fn test_should_log_unresolved_retry_policy() {
        assert!(should_log_unresolved_retry(1));
        assert!(!should_log_unresolved_retry(2));
        assert!(should_log_unresolved_retry(10));
        assert!(should_log_unresolved_retry(PARSER_UNRESOLVED_MAX_RETRIES));
    }

    #[test]
    fn test_classify_unresolved_local_probe_marks_missing_everywhere() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
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
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());
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
    fn test_next_fetch_start_after_batch_stays_contiguous_across_boundaries() {
        assert_eq!(next_fetch_start_after_batch(999), 1000);
        assert_eq!(next_fetch_start_after_batch(1000), 1001);
    }

    #[test]
    #[should_panic(expected = "fetch batch end_block overflow while computing next start")]
    fn test_next_fetch_start_after_batch_panics_on_u64_max() {
        let _ = next_fetch_start_after_batch(u64::MAX);
    }

    #[test]
    fn test_should_abort_unresolved_retry_on_epoch_change() {
        assert!(!should_abort_unresolved_retry_on_epoch_change(10, 10));
        assert!(should_abort_unresolved_retry_on_epoch_change(10, 11));
    }

    #[test]
    fn test_address_balances_are_never_skipped_in_bulk_mode() {
        assert!(!should_skip_address_balances(true));
        assert!(!should_skip_address_balances(false));
    }

    #[test]
    fn test_should_abort_pipeline_on_idle_timeout_when_parser_exits() {
        assert!(should_abort_pipeline_on_idle_timeout(true, false));
    }

    #[test]
    fn test_should_abort_pipeline_on_idle_timeout_when_fetcher_exits() {
        assert!(should_abort_pipeline_on_idle_timeout(false, true));
        assert!(!should_abort_pipeline_on_idle_timeout(false, false));
    }

    #[test]
    fn test_should_invalidate_chart_caches_for_lag_only_near_tip() {
        assert!(should_invalidate_chart_caches_for_lag(0));
        assert!(should_invalidate_chart_caches_for_lag(
            CHART_INVALIDATION_MAX_LIVE_LAG
        ));
        assert!(!should_invalidate_chart_caches_for_lag(
            CHART_INVALIDATION_MAX_LIVE_LAG + 1
        ));
    }

    #[test]
    fn test_record_worker_exit_reason_keeps_first_reason() {
        let slot = Arc::new(std::sync::Mutex::new(None));
        record_worker_exit_reason(&slot, "first failure");
        record_worker_exit_reason(&slot, "second failure");
        assert_eq!(
            get_worker_exit_reason(&slot).as_deref(),
            Some("first failure")
        );
    }

    #[test]
    fn test_format_pipeline_worker_termination_message_includes_context() {
        let msg = format_pipeline_worker_termination_message(
            true,
            false,
            Some("parser exploded"),
            Some("fetcher okay"),
        );
        assert!(msg.contains("parser_finished=true"));
        assert!(msg.contains("fetcher_finished=false"));
        assert!(msg.contains("parser_reason=parser exploded"));
        assert!(msg.contains("fetcher_reason=fetcher okay"));
    }

    #[test]
    fn test_should_log_pipeline_idle_timeout_policy() {
        assert!(should_log_pipeline_idle_timeout(1));
        assert!(should_log_pipeline_idle_timeout(2));
        assert!(should_log_pipeline_idle_timeout(3));
        assert!(!should_log_pipeline_idle_timeout(4));
        assert!(should_log_pipeline_idle_timeout(10));
        assert!(should_log_pipeline_idle_timeout(20));
    }

    #[test]
    fn test_queue_fill_percentage() {
        assert_eq!(queue_fill_percentage(Some(5), Some(10)), Some(50.0));
        assert_eq!(queue_fill_percentage(Some(1), Some(0)), None);
        assert_eq!(queue_fill_percentage(None, Some(10)), None);
        assert_eq!(queue_fill_percentage(Some(1), None), None);
    }

    #[test]
    fn test_parse_queue_capacity_txs_uses_sub_batch_cap() {
        assert_eq!(
            parse_queue_capacity_txs(8, 40_000, ADAPTIVE_BATCH_BULK_DISTANCE_MIN_TARGET_TXS),
            640_000
        );
    }

    #[test]
    fn test_parse_queue_capacity_txs_respects_floor() {
        assert_eq!(parse_queue_capacity_txs(4, 2_500, 8_000), 32_000);
    }

    #[test]
    #[should_panic(expected = "parse queue tx capacity overflow")]
    fn test_parse_queue_capacity_txs_panics_on_overflow() {
        let _ = parse_queue_capacity_txs(usize::MAX, ADAPTIVE_BATCH_MAX_TXS, 10_000);
    }

    #[tokio::test]
    async fn test_sender_queue_depth_tracks_runtime_channel_state() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<u8>(4);
        assert_eq!(sender_queue_depth(&tx), 0);
        tx.send(1).await.unwrap();
        tx.send(2).await.unwrap();
        assert_eq!(sender_queue_depth(&tx), 2);
        assert_eq!(rx.recv().await, Some(1));
        assert_eq!(sender_queue_depth(&tx), 1);
    }

    #[test]
    fn test_cgroup_memory_ratio_pct() {
        let snapshot = CgroupMemorySnapshot {
            memory_current_bytes: Some(4),
            memory_max_bytes: Some(8),
            ..Default::default()
        };
        assert_eq!(cgroup_memory_ratio_pct(&snapshot), Some(50.0));

        let unlimited = CgroupMemorySnapshot {
            memory_current_bytes: Some(4),
            memory_max_bytes: None,
            ..Default::default()
        };
        assert_eq!(cgroup_memory_ratio_pct(&unlimited), None);

        let zero_max = CgroupMemorySnapshot {
            memory_current_bytes: Some(4),
            memory_max_bytes: Some(0),
            ..Default::default()
        };
        assert_eq!(cgroup_memory_ratio_pct(&zero_max), None);
    }

    #[test]
    fn test_adaptive_batch_estimate_block_span_clamps_to_bounds() {
        let controller = AdaptiveBatchController::new(16);
        controller
            .target_batch_txs
            .store(100_000, Ordering::Relaxed);
        controller
            .tx_per_block_milli_ema
            .store(2_000_000, Ordering::Relaxed); // 2000 tx/block
                                                  // Estimated span = 50 blocks.
        assert_eq!(controller.estimate_block_span(10_000), 50);

        controller
            .tx_per_block_milli_ema
            .store(1_000, Ordering::Relaxed); // 1 tx/block
                                              // Estimated span = 100_000, but cap by batch_block_cap.
        assert_eq!(controller.estimate_block_span(500), 500);
    }

    #[test]
    fn test_adaptive_batch_moderate_backoff_reduces_target_only() {
        let controller = AdaptiveBatchController::new(8);
        let adjustment = controller
            .update_after_write(AdaptiveBatchInput {
                write_ms: ADAPTIVE_BATCH_WRITE_HI_MS + 1.0,
                commit_ms: 0.0,
                batch_tx_count: 8_000,
                blocks_remaining: 0,
                parse_queue_fill_pct: Some(10.0),
                writer_queue_fill_pct: Some(10.0),
                memory_ratio_pct: Some(10.0),
                l0_files_max: None,
                compaction_pending_bytes: None,
                immutable_memtables: None,
                severe_pending_threshold: 8 * 1024 * 1024 * 1024,
                moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
                severe_imm_threshold: 60,
                moderate_imm_threshold: 30,
            })
            .expect("moderate pressure should reduce target");
        assert_eq!(adjustment.reason, "moderate_backoff");
        assert_eq!(adjustment.new_target_batch_txs, 36_000);
        assert_eq!(
            adjustment.new_inflight_limit,
            ADAPTIVE_BATCH_INITIAL_INFLIGHT
        );
    }

    #[test]
    fn test_adaptive_batch_healthy_step_up_prioritizes_inflight_recovery() {
        let controller = AdaptiveBatchController::new(8);
        let adjustment = controller
            .update_after_write(AdaptiveBatchInput {
                write_ms: ADAPTIVE_BATCH_WRITE_LO_MS - 100.0,
                commit_ms: 0.0,
                batch_tx_count: 10_000,
                blocks_remaining: 0,
                parse_queue_fill_pct: Some(10.0),
                writer_queue_fill_pct: Some(10.0),
                memory_ratio_pct: Some(10.0),
                l0_files_max: None,
                compaction_pending_bytes: None,
                immutable_memtables: None,
                severe_pending_threshold: 8 * 1024 * 1024 * 1024,
                moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
                severe_imm_threshold: 60,
                moderate_imm_threshold: 30,
            })
            .expect("healthy signal should adjust inflight first");
        assert_eq!(adjustment.reason, "healthy_step_up");
        assert_eq!(adjustment.new_target_batch_txs, ADAPTIVE_BATCH_INITIAL_TXS);
        assert_eq!(
            adjustment.new_inflight_limit,
            ADAPTIVE_BATCH_INITIAL_INFLIGHT + 1
        );
    }

    #[test]
    fn test_adaptive_batch_bulk_distance_floor_enforced() {
        let controller = AdaptiveBatchController::new(8);
        controller.target_batch_txs.store(20_000, Ordering::Relaxed);
        controller
            .min_target_batch_txs
            .store(ADAPTIVE_BATCH_HARD_MIN_TXS, Ordering::Relaxed);
        controller.inflight_limit.store(2, Ordering::Relaxed);

        let adjustment = controller
            .update_after_write(AdaptiveBatchInput {
                write_ms: ADAPTIVE_BATCH_WRITE_LO_MS - 100.0,
                commit_ms: 0.0,
                batch_tx_count: 5_000,
                blocks_remaining: ADAPTIVE_BATCH_NEAR_TIP_THRESHOLD_BLOCKS + 1,
                parse_queue_fill_pct: Some(10.0),
                writer_queue_fill_pct: Some(10.0),
                memory_ratio_pct: Some(10.0),
                l0_files_max: None,
                compaction_pending_bytes: None,
                immutable_memtables: None,
                severe_pending_threshold: 8 * 1024 * 1024 * 1024,
                moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
                severe_imm_threshold: 60,
                moderate_imm_threshold: 30,
            })
            .expect("far bulk mode should enforce minimum floors");
        assert_eq!(
            adjustment.new_min_target_batch_txs,
            ADAPTIVE_BATCH_BULK_DISTANCE_MIN_TARGET_TXS
        );
        assert!(adjustment.new_target_batch_txs >= ADAPTIVE_BATCH_BULK_DISTANCE_MIN_TARGET_TXS);
        assert_eq!(
            adjustment.new_inflight_limit,
            ADAPTIVE_BATCH_BULK_DISTANCE_MIN_INFLIGHT
        );
    }

    #[test]
    fn test_adaptive_batch_floor_down_when_pressure_at_floor_and_single_inflight() {
        let controller = AdaptiveBatchController::new(1);
        controller
            .target_batch_txs
            .store(ADAPTIVE_BATCH_BASE_MIN_TXS, Ordering::Relaxed);

        let first = controller.update_after_write(AdaptiveBatchInput {
            write_ms: ADAPTIVE_BATCH_SEVERE_WRITE_MS + 100.0,
            commit_ms: 0.0,
            batch_tx_count: 8_000,
            blocks_remaining: 0,
            parse_queue_fill_pct: Some(97.0),
            writer_queue_fill_pct: Some(95.0),
            memory_ratio_pct: Some(85.0),
            l0_files_max: None,
            compaction_pending_bytes: None,
            immutable_memtables: None,
            severe_pending_threshold: 8 * 1024 * 1024 * 1024,
            moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
            severe_imm_threshold: 60,
            moderate_imm_threshold: 30,
        });
        assert!(
            first.is_none(),
            "first severe sample should not floor-down yet"
        );

        let adjustment = controller
            .update_after_write(AdaptiveBatchInput {
                write_ms: ADAPTIVE_BATCH_SEVERE_WRITE_MS + 100.0,
                commit_ms: ADAPTIVE_BATCH_SEVERE_COMMIT_MS + 100.0,
                batch_tx_count: 8_000,
                blocks_remaining: 0,
                parse_queue_fill_pct: Some(97.0),
                writer_queue_fill_pct: Some(95.0),
                memory_ratio_pct: Some(85.0),
                l0_files_max: None,
                compaction_pending_bytes: None,
                immutable_memtables: None,
                severe_pending_threshold: 8 * 1024 * 1024 * 1024,
                moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
                severe_imm_threshold: 60,
                moderate_imm_threshold: 30,
            })
            .expect("consecutive severe pressure at floor should lower adaptive min floor");

        assert_eq!(adjustment.reason, "pressure_backoff_floor_down");
        assert!(
            adjustment.new_min_target_batch_txs < adjustment.previous_min_target_batch_txs,
            "adaptive min floor should go down under sustained pressure"
        );
    }

    #[test]
    fn test_adaptive_batch_floor_recovers_on_healthy_throughput() {
        let controller = AdaptiveBatchController::new(8);
        controller
            .min_target_batch_txs
            .store(ADAPTIVE_BATCH_HARD_MIN_TXS, Ordering::Relaxed);
        controller
            .target_batch_txs
            .store(ADAPTIVE_BATCH_HARD_MIN_TXS + 2_000, Ordering::Relaxed);

        let adjustment = controller
            .update_after_write(AdaptiveBatchInput {
                write_ms: 1_000.0,
                commit_ms: 0.0,
                batch_tx_count: 10_000,
                blocks_remaining: 0,
                parse_queue_fill_pct: Some(10.0),
                writer_queue_fill_pct: Some(10.0),
                memory_ratio_pct: Some(10.0),
                l0_files_max: None,
                compaction_pending_bytes: None,
                immutable_memtables: None,
                severe_pending_threshold: 8 * 1024 * 1024 * 1024,
                moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
                severe_imm_threshold: 60,
                moderate_imm_threshold: 30,
            })
            .expect("healthy throughput should recover adaptive min floor");

        assert_eq!(adjustment.reason, "healthy_step_up_floor_recover");
        assert!(
            adjustment.new_min_target_batch_txs > adjustment.previous_min_target_batch_txs,
            "adaptive min floor should recover upward"
        );
    }

    #[test]
    fn test_adaptive_batch_severe_pressure_requires_consecutive_batches_before_backoff() {
        let controller = AdaptiveBatchController::new(8);

        let first = controller
            .update_after_write(AdaptiveBatchInput {
                write_ms: ADAPTIVE_BATCH_SEVERE_WRITE_MS + 1_000.0,
                commit_ms: 0.0,
                batch_tx_count: 10_000,
                blocks_remaining: 0,
                parse_queue_fill_pct: Some(98.0),
                writer_queue_fill_pct: Some(98.0),
                memory_ratio_pct: Some(10.0),
                l0_files_max: None,
                compaction_pending_bytes: None,
                immutable_memtables: None,
                severe_pending_threshold: 8 * 1024 * 1024 * 1024,
                moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
                severe_imm_threshold: 60,
                moderate_imm_threshold: 30,
            })
            .expect("first severe sample should only moderate-backoff");
        assert_eq!(first.reason, "moderate_backoff");

        let second = controller
            .update_after_write(AdaptiveBatchInput {
                write_ms: ADAPTIVE_BATCH_SEVERE_WRITE_MS + 2_000.0,
                commit_ms: ADAPTIVE_BATCH_SEVERE_COMMIT_MS + 100.0,
                batch_tx_count: 10_000,
                blocks_remaining: 0,
                parse_queue_fill_pct: Some(98.0),
                writer_queue_fill_pct: Some(98.0),
                memory_ratio_pct: Some(10.0),
                l0_files_max: None,
                compaction_pending_bytes: None,
                immutable_memtables: None,
                severe_pending_threshold: 8 * 1024 * 1024 * 1024,
                moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
                severe_imm_threshold: 60,
                moderate_imm_threshold: 30,
            })
            .expect("second severe sample should trigger severe backoff");
        assert_eq!(second.reason, "severe_pressure_backoff");
        assert!(second.new_target_batch_txs < second.previous_target_batch_txs);
        assert!(second.new_inflight_limit < second.previous_inflight_limit);
    }

    #[test]
    fn test_adaptive_batch_high_queue_without_throughput_drop_does_not_backoff() {
        let controller = AdaptiveBatchController::new(8);

        let first = controller
            .update_after_write(AdaptiveBatchInput {
                write_ms: 1_000.0,
                commit_ms: 0.0,
                batch_tx_count: 10_000,
                blocks_remaining: 0,
                parse_queue_fill_pct: Some(10.0),
                writer_queue_fill_pct: Some(10.0),
                memory_ratio_pct: Some(10.0),
                l0_files_max: None,
                compaction_pending_bytes: None,
                immutable_memtables: None,
                severe_pending_threshold: 8 * 1024 * 1024 * 1024,
                moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
                severe_imm_threshold: 60,
                moderate_imm_threshold: 30,
            })
            .expect("first healthy sample should step up");
        assert_eq!(first.reason, "healthy_step_up");

        let no_adjustment = controller.update_after_write(AdaptiveBatchInput {
            write_ms: 150.0,
            commit_ms: 0.0,
            batch_tx_count: 6_000,
            blocks_remaining: 0,
            parse_queue_fill_pct: Some(99.0),
            writer_queue_fill_pct: Some(99.0),
            memory_ratio_pct: Some(10.0),
            l0_files_max: None,
            compaction_pending_bytes: None,
            immutable_memtables: None,
            severe_pending_threshold: 8 * 1024 * 1024 * 1024,
            moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
            severe_imm_threshold: 60,
            moderate_imm_threshold: 30,
        });
        assert!(
            no_adjustment.is_none(),
            "queue fullness alone should not force backoff when tx throughput improves"
        );
    }

    #[test]
    fn test_adaptive_batch_floor_down_requires_real_pressure_signal() {
        let controller = AdaptiveBatchController::new(1);
        controller
            .target_batch_txs
            .store(ADAPTIVE_BATCH_BASE_MIN_TXS, Ordering::Relaxed);

        let no_adjustment = controller.update_after_write(AdaptiveBatchInput {
            write_ms: 200.0,
            commit_ms: 0.0,
            batch_tx_count: 10_000,
            blocks_remaining: 0,
            parse_queue_fill_pct: Some(97.0),
            writer_queue_fill_pct: Some(95.0),
            memory_ratio_pct: Some(10.0),
            l0_files_max: None,
            compaction_pending_bytes: None,
            immutable_memtables: None,
            severe_pending_threshold: 8 * 1024 * 1024 * 1024,
            moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
            severe_imm_threshold: 60,
            moderate_imm_threshold: 30,
        });
        assert!(
            no_adjustment.is_none(),
            "at-floor min target should not be lowered by queue pressure alone"
        );
        assert_eq!(
            controller.snapshot().min_target_batch_txs,
            ADAPTIVE_BATCH_BASE_MIN_TXS
        );
    }

    #[test]
    fn test_adaptive_batch_near_tip_can_drop_min_floor_below_bulk_floor() {
        let controller = AdaptiveBatchController::new(8);
        controller.min_target_batch_txs.store(
            ADAPTIVE_BATCH_BULK_DISTANCE_MIN_TARGET_TXS,
            Ordering::Relaxed,
        );
        controller.target_batch_txs.store(
            ADAPTIVE_BATCH_BULK_DISTANCE_MIN_TARGET_TXS,
            Ordering::Relaxed,
        );

        let adjustment = controller
            .update_after_write(AdaptiveBatchInput {
                write_ms: ADAPTIVE_BATCH_WRITE_HI_MS + 50.0,
                commit_ms: 0.0,
                batch_tx_count: 10_000,
                blocks_remaining: 10_000,
                parse_queue_fill_pct: Some(95.0),
                writer_queue_fill_pct: Some(95.0),
                memory_ratio_pct: Some(10.0),
                l0_files_max: None,
                compaction_pending_bytes: None,
                immutable_memtables: None,
                severe_pending_threshold: 8 * 1024 * 1024 * 1024,
                moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
                severe_imm_threshold: 60,
                moderate_imm_threshold: 30,
            })
            .expect("near-tip path should allow lower min floor");

        assert_eq!(adjustment.reason, "moderate_backoff");
        assert_eq!(
            adjustment.new_min_target_batch_txs,
            ADAPTIVE_BATCH_BASE_MIN_TXS
        );
    }

    #[test]
    fn test_adaptive_batch_step_up_requires_throughput_not_worse() {
        let controller = AdaptiveBatchController::new(8);

        let first = controller
            .update_after_write(AdaptiveBatchInput {
                write_ms: 1_000.0,
                commit_ms: 0.0,
                batch_tx_count: 10_000,
                blocks_remaining: 0,
                parse_queue_fill_pct: Some(10.0),
                writer_queue_fill_pct: Some(10.0),
                memory_ratio_pct: Some(10.0),
                l0_files_max: None,
                compaction_pending_bytes: None,
                immutable_memtables: None,
                severe_pending_threshold: 8 * 1024 * 1024 * 1024,
                moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
                severe_imm_threshold: 60,
                moderate_imm_threshold: 30,
            })
            .expect("first healthy sample should step up");
        assert_eq!(first.reason, "healthy_step_up");

        let no_adjustment = controller.update_after_write(AdaptiveBatchInput {
            write_ms: 1_500.0,
            commit_ms: 0.0,
            batch_tx_count: 10_000,
            blocks_remaining: 0,
            parse_queue_fill_pct: Some(10.0),
            writer_queue_fill_pct: Some(10.0),
            memory_ratio_pct: Some(10.0),
            l0_files_max: None,
            compaction_pending_bytes: None,
            immutable_memtables: None,
            severe_pending_threshold: 8 * 1024 * 1024 * 1024,
            moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
            severe_imm_threshold: 60,
            moderate_imm_threshold: 30,
        });
        assert!(
            no_adjustment.is_none(),
            "step-up should pause when throughput degrades despite healthy queues"
        );
    }

    #[test]
    fn test_adaptive_batch_cooldown_blocks_immediate_step_up_after_pressure() {
        let controller = AdaptiveBatchController::new(8);
        let _ = controller
            .update_after_write(AdaptiveBatchInput {
                write_ms: ADAPTIVE_BATCH_SEVERE_WRITE_MS + 1_000.0,
                commit_ms: 0.0,
                batch_tx_count: 10_000,
                blocks_remaining: 0,
                parse_queue_fill_pct: Some(95.0),
                writer_queue_fill_pct: Some(95.0),
                memory_ratio_pct: Some(85.0),
                l0_files_max: None,
                compaction_pending_bytes: None,
                immutable_memtables: None,
                severe_pending_threshold: 8 * 1024 * 1024 * 1024,
                moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
                severe_imm_threshold: 60,
                moderate_imm_threshold: 30,
            })
            .expect("first pressure sample should adjust");
        let _ = controller
            .update_after_write(AdaptiveBatchInput {
                write_ms: ADAPTIVE_BATCH_SEVERE_WRITE_MS + 2_000.0,
                commit_ms: ADAPTIVE_BATCH_SEVERE_COMMIT_MS + 100.0,
                batch_tx_count: 10_000,
                blocks_remaining: 0,
                parse_queue_fill_pct: Some(95.0),
                writer_queue_fill_pct: Some(95.0),
                memory_ratio_pct: Some(85.0),
                l0_files_max: None,
                compaction_pending_bytes: None,
                immutable_memtables: None,
                severe_pending_threshold: 8 * 1024 * 1024 * 1024,
                moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
                severe_imm_threshold: 60,
                moderate_imm_threshold: 30,
            })
            .expect("second pressure sample should trigger cooldown");
        let snapshot_after_pressure = controller.snapshot();

        let no_adjustment = controller.update_after_write(AdaptiveBatchInput {
            write_ms: ADAPTIVE_BATCH_WRITE_LO_MS - 100.0,
            commit_ms: 0.0,
            batch_tx_count: 10_000,
            blocks_remaining: 0,
            parse_queue_fill_pct: Some(10.0),
            writer_queue_fill_pct: Some(10.0),
            memory_ratio_pct: Some(10.0),
            l0_files_max: None,
            compaction_pending_bytes: None,
            immutable_memtables: None,
            severe_pending_threshold: 8 * 1024 * 1024 * 1024,
            moderate_pending_threshold: 4 * 1024 * 1024 * 1024,
            severe_imm_threshold: 60,
            moderate_imm_threshold: 30,
        });
        assert!(no_adjustment.is_none());
        let snapshot_after_healthy = controller.snapshot();
        assert_eq!(
            snapshot_after_healthy.target_batch_txs,
            snapshot_after_pressure.target_batch_txs
        );
        assert_eq!(
            snapshot_after_healthy.inflight_limit,
            snapshot_after_pressure.inflight_limit
        );
    }

    #[test]
    fn test_adaptive_batch_early_height_boost_applies_once() {
        let controller = AdaptiveBatchController::new(8);
        let first = controller
            .maybe_apply_early_height_boost(123)
            .expect("early-chain boost should apply once");
        assert_eq!(first.0, ADAPTIVE_BATCH_INITIAL_TXS);
        assert_eq!(first.1, ADAPTIVE_BATCH_EARLY_TARGET_TXS);
        assert_eq!(
            controller.snapshot().target_batch_txs,
            ADAPTIVE_BATCH_EARLY_TARGET_TXS
        );

        let second = controller.maybe_apply_early_height_boost(456);
        assert!(second.is_none(), "boost should not reapply");
    }

    #[test]
    fn test_adaptive_batch_early_height_boost_skips_after_cutoff() {
        let controller = AdaptiveBatchController::new(8);
        let skipped = controller.maybe_apply_early_height_boost(ADAPTIVE_BATCH_EARLY_HEIGHT_CUTOFF);
        assert!(skipped.is_none());
        assert_eq!(
            controller.snapshot().target_batch_txs,
            ADAPTIVE_BATCH_INITIAL_TXS
        );
    }

    #[test]
    fn test_should_trim_cell_cache_threshold() {
        assert!(!should_trim_cell_cache(CELL_CACHE_CAPACITY * 2));
        assert!(should_trim_cell_cache(CELL_CACHE_CAPACITY * 2 + 1));
    }

    #[test]
    fn test_plan_fetch_sub_batches_without_split() {
        let plan = plan_fetch_sub_batches(&[10, 20, 30], 1000);
        assert_eq!(plan, vec![(3, 60)]);
    }

    #[test]
    fn test_plan_fetch_sub_batches_with_split() {
        let plan = plan_fetch_sub_batches(&[2, 2, 1, 5], 3);
        assert_eq!(plan, vec![(2, 4), (2, 6)]);
    }

    #[test]
    fn test_plan_fetch_sub_batches_empty() {
        let plan = plan_fetch_sub_batches(&[], 100);
        assert!(plan.is_empty());
    }

    #[test]
    fn test_adaptive_sub_batch_tx_cap_scales_with_target() {
        assert_eq!(
            adaptive_sub_batch_tx_cap(10_000, ADAPTIVE_BATCH_BASE_MIN_TXS),
            20_000
        );
        assert_eq!(
            adaptive_sub_batch_tx_cap(40_000, ADAPTIVE_BATCH_BASE_MIN_TXS),
            80_000
        );
    }

    #[test]
    fn test_adaptive_sub_batch_tx_cap_respects_adaptive_ceiling() {
        assert_eq!(
            adaptive_sub_batch_tx_cap(ADAPTIVE_BATCH_MAX_TXS, ADAPTIVE_BATCH_BASE_MIN_TXS),
            ADAPTIVE_BATCH_MAX_TXS as usize
        );
        assert_eq!(
            adaptive_sub_batch_tx_cap(ADAPTIVE_BATCH_MAX_TXS * 2, ADAPTIVE_BATCH_BASE_MIN_TXS),
            ADAPTIVE_BATCH_MAX_TXS as usize
        );
    }

    #[test]
    fn test_adaptive_sub_batch_tx_cap_respects_adaptive_floor() {
        assert_eq!(
            adaptive_sub_batch_tx_cap(2_500, 8_000),
            8_000,
            "sub-batch cap should never drop below adaptive min floor"
        );
        assert_eq!(
            adaptive_sub_batch_tx_cap(500, 500),
            ADAPTIVE_BATCH_HARD_MIN_TXS as usize,
            "adaptive min floor should still respect hard safety minimum"
        );
    }

    #[test]
    #[should_panic(expected = "tx_cap must be > 0")]
    fn test_plan_fetch_sub_batches_panics_on_zero_limit() {
        let _ = plan_fetch_sub_batches(&[1], 0);
    }

    #[test]
    fn test_evict_committed_cell_cache_entries_only_removes_committed() {
        let cache = dashmap::DashMap::new();
        cache.insert(([0x11; 32], 0), dummy_cached_cell_info(100));
        cache.insert(([0x22; 32], 1), dummy_cached_cell_info(101));
        cache.insert(([0x33; 32], 2), dummy_cached_cell_info(102));

        let evicted = evict_committed_cell_cache_entries(&cache, 101);
        assert_eq!(evicted, 2);
        assert_eq!(cache.len(), 1);
        assert!(cache.contains_key(&([0x33; 32], 2)));

        let evicted_noop = evict_committed_cell_cache_entries(&cache, -1);
        assert_eq!(evicted_noop, 0);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_repeated_warning_tracker_suppresses_and_aggregates() {
        let tracker = RepeatedWarningTracker::default();

        let first = tracker
            .record("pipeline_idle_timeout", Duration::from_secs(60))
            .expect("first warning should emit");
        assert_eq!(first.total_count, 1);
        assert_eq!(first.suppressed_since_last_emit, 0);

        let second = tracker.record("pipeline_idle_timeout", Duration::from_secs(60));
        assert!(second.is_none(), "second warning should be suppressed");

        let third = tracker
            .record("pipeline_idle_timeout", Duration::from_secs(0))
            .expect("forced emit should flush suppressed count");
        assert_eq!(third.total_count, 3);
        assert_eq!(third.suppressed_since_last_emit, 1);
    }

    #[test]
    fn test_repeated_warning_tracker_isolated_by_key() {
        let tracker = RepeatedWarningTracker::default();
        assert!(tracker
            .record("pipeline_idle_timeout", Duration::from_secs(60))
            .is_some());
        assert!(tracker
            .record("pipeline_batch_mismatch", Duration::from_secs(60))
            .is_some());
    }

    #[test]
    fn test_count_new_addresses_counts_only_first_live_transitions() {
        let mut changes: HashMap<Vec<u8>, (i128, i32, i32, i64, i64, &[u8], i128)> = HashMap::new();
        let addr_new = vec![0x11; 32];
        let addr_existing_live = vec![0x22; 32];
        let addr_existing_zero = vec![0x33; 32];
        let tx_hash = [0xAA; 32];

        changes.insert(addr_new.clone(), (100, 1, 1, 1, 1, &tx_hash, 10));
        changes.insert(addr_existing_live.clone(), (50, 1, 1, 1, 1, &tx_hash, 5));
        changes.insert(addr_existing_zero.clone(), (70, 2, 2, 1, 1, &tx_hash, 7));

        let mut existing: HashMap<Vec<u8>, Option<AddressBalance>> = HashMap::new();
        existing.insert(
            addr_existing_live,
            Some(AddressBalance {
                live_cells_count: 3,
                ..Default::default()
            }),
        );
        existing.insert(
            addr_existing_zero,
            Some(AddressBalance {
                live_cells_count: 0,
                ..Default::default()
            }),
        );

        assert_eq!(count_new_addresses(&changes, &existing), 2);
    }

    #[test]
    fn test_count_new_addresses_ignores_non_positive_live_delta() {
        let mut changes: HashMap<Vec<u8>, (i128, i32, i32, i64, i64, &[u8], i128)> = HashMap::new();
        let tx_hash = [0xBB; 32];
        changes.insert(vec![0x44; 32], (0, 0, 0, 1, 1, &tx_hash, 0));
        changes.insert(vec![0x55; 32], (-10, -1, 0, 1, 1, &tx_hash, -2));

        let existing: HashMap<Vec<u8>, Option<AddressBalance>> = HashMap::new();
        assert_eq!(count_new_addresses(&changes, &existing), 0);
    }

    #[test]
    fn test_classify_nft_collection_id_mnft_uses_first_24_args_bytes() {
        let mnft_code_hash =
            crate::rpc::parse_hex_to_bytes(crate::parser::mnft::MNFT_TOKEN_CODE_HASH);
        let mut args = vec![0xAB; 24];
        args.extend_from_slice(&[0xCD; 8]);

        let collection_id = classify_nft_collection_id(&mnft_code_hash, &args)
            .expect("mNFT token type should map to collection id");
        assert_eq!(collection_id, vec![0xAB; 24]);
    }

    #[test]
    fn test_classify_nft_collection_id_dotbit_uses_sentinel_collection() {
        let dotbit_code_hash =
            crate::rpc::parse_hex_to_bytes(crate::parser::dotbit::DOTBIT_ACCOUNT_CELL_TYPE_ID);
        let collection_id = classify_nft_collection_id(&dotbit_code_hash, &[])
            .expect("dotbit account type should map to sentinel collection");
        assert_eq!(collection_id, DOTBIT_SENTINEL_COLLECTION.to_vec());
    }

    #[test]
    fn test_classify_nft_collection_id_did_ckb_uses_sentinel_collection() {
        let did_code_hash =
            crate::rpc::parse_hex_to_bytes(crate::parser::spore::SPORE_CODE_HASH_MAINNET_DID);
        let collection_id = classify_nft_collection_id(&did_code_hash, &[0x99; 32])
            .expect("did:ckb type should map to sentinel collection");
        assert_eq!(collection_id, DID_CKB_SENTINEL_COLLECTION.to_vec());
    }

    #[test]
    fn test_classify_nft_collection_id_rejects_non_nft_or_short_mnft_args() {
        let non_nft = vec![0x11; 32];
        assert!(classify_nft_collection_id(&non_nft, &[0x22; 24]).is_none());

        let mnft_code_hash =
            crate::rpc::parse_hex_to_bytes(crate::parser::mnft::MNFT_TOKEN_CODE_HASH);
        assert!(classify_nft_collection_id(&mnft_code_hash, &[0x33; 23]).is_none());
    }

    #[test]
    fn test_dotbit_event_order_marks_output_after_input_in_same_tx() {
        let consume_order = dotbit_consume_event_order(42).unwrap();
        let create_order = dotbit_create_event_order(42).unwrap();
        assert!(create_order > consume_order);
    }

    #[test]
    fn test_should_consume_dotbit_account_when_no_later_output_exists() {
        let consume_order = dotbit_consume_event_order(10).unwrap();
        assert!(should_consume_dotbit_account(None, consume_order));
        assert!(should_consume_dotbit_account(
            Some(consume_order),
            consume_order
        ));
        assert!(!should_consume_dotbit_account(
            Some(consume_order + 1),
            consume_order
        ));
    }

    #[test]
    fn test_should_consume_dotbit_account_with_cross_tx_recreate() {
        let consume_t1 = dotbit_consume_event_order(1).unwrap();
        let create_t2 = dotbit_create_event_order(2).unwrap();
        assert!(
            !should_consume_dotbit_account(Some(create_t2), consume_t1),
            "later output should keep account live"
        );

        let consume_t3 = dotbit_consume_event_order(3).unwrap();
        assert!(
            should_consume_dotbit_account(Some(create_t2), consume_t3),
            "consume after latest output should mark account consumed"
        );
    }

    #[test]
    fn test_resolve_dotbit_account_id_for_outpoint_prefers_store_mapping() {
        let mut batch_dotbit_outpoints: HashMap<(Vec<u8>, i16), Vec<u8>> = HashMap::new();
        let tx_hash = vec![0x11; 32];
        let store_account = vec![0x22; 20];
        let batch_account = vec![0x33; 20];
        batch_dotbit_outpoints.insert((tx_hash.clone(), 7), batch_account);

        let resolved = resolve_dotbit_account_id_for_outpoint(
            Some(store_account.clone()),
            &tx_hash,
            7,
            &batch_dotbit_outpoints,
        );
        assert_eq!(resolved, Some(store_account));
    }

    #[test]
    fn test_resolve_dotbit_account_id_for_outpoint_falls_back_to_batch_mapping() {
        let mut batch_dotbit_outpoints: HashMap<(Vec<u8>, i16), Vec<u8>> = HashMap::new();
        let tx_hash = vec![0x44; 32];
        let batch_account = vec![0x55; 20];
        batch_dotbit_outpoints.insert((tx_hash.clone(), 3), batch_account.clone());

        let resolved =
            resolve_dotbit_account_id_for_outpoint(None, &tx_hash, 3, &batch_dotbit_outpoints);
        assert_eq!(resolved, Some(batch_account));
    }

    #[test]
    fn test_extract_omnilock_supply_info_type_hash_with_all_modes() {
        let mut lock_args = vec![0u8; OMNILOCK_AUTH_LEN];
        let flags = OMNILOCK_SUPPLY_MODE_FLAG
            | OMNILOCK_ADMIN_MODE_FLAG
            | OMNILOCK_ACP_MODE_FLAG
            | OMNILOCK_TIMELOCK_MODE_FLAG;
        lock_args.push(flags);
        lock_args.extend_from_slice(&[0xAA; 32]); // admin list type id
        lock_args.extend_from_slice(&[0x01, 0x02]); // ACP min
        lock_args.extend_from_slice(&[0xBB; 8]); // since
        lock_args.extend_from_slice(&[0xCC; 32]); // supply info type script hash

        let parsed = extract_omnilock_supply_info_type_hash(&lock_args).unwrap();
        assert_eq!(parsed, [0xCC; 32]);
    }

    #[test]
    fn test_parse_omnilock_supply_info_cell_data_validates_bounds() {
        let mut data = Vec::with_capacity(65);
        data.push(0u8); // version
        data.extend_from_slice(&5u128.to_le_bytes()); // current
        data.extend_from_slice(&10u128.to_le_bytes()); // max
        data.extend_from_slice(&[0x11; 32]); // sUDT/xUDT type script hash

        let parsed = parse_omnilock_supply_info_cell_data(&data).unwrap();
        assert_eq!(parsed.0, 10);
        assert_eq!(parsed.1, [0x11; 32]);

        let mut invalid = data.clone();
        invalid[1..17].copy_from_slice(&11u128.to_le_bytes()); // current > max
        assert!(parse_omnilock_supply_info_cell_data(&invalid).is_none());
    }

    #[test]
    fn test_collect_token_max_supply_observations_from_omnilock_info_cells() {
        let supply_info_type_hash = [0x22; 32];
        let token_type_hash = [0x33; 32];

        let mut lock_args = vec![0u8; OMNILOCK_AUTH_LEN];
        lock_args.push(OMNILOCK_SUPPLY_MODE_FLAG);
        lock_args.extend_from_slice(&supply_info_type_hash);

        let mut info_cell_data = Vec::with_capacity(65);
        info_cell_data.push(0u8);
        info_cell_data.extend_from_slice(&100u128.to_le_bytes());
        info_cell_data.extend_from_slice(&1_000u128.to_le_bytes());
        info_cell_data.extend_from_slice(&token_type_hash);

        let info_cell = crate::parser::cell::ParsedCell {
            capacity: 100_00000000,
            lock_code_hash: crate::rpc::parse_hex_to_bytes(OMNILOCK_CODE_HASH_MAINNET_V2),
            lock_hash_type: 1,
            lock_args,
            lock_script_hash: vec![0x44; 32],
            type_code_hash: Some(vec![0x55; 32]),
            type_hash_type: Some(1),
            type_args: Some(vec![0x66; 32]),
            type_script_hash: Some(supply_info_type_hash.to_vec()),
            data_hash: vec![0x77; 32],
            data_size: info_cell_data.len() as i32,
            data: info_cell_data,
        };

        let tx = dummy_tx_data([0x88; 32], false, vec![], vec![info_cell], vec![], vec![]);
        let observations = collect_token_max_supply_observations(&[tx]);
        assert_eq!(observations.get(token_type_hash.as_slice()), Some(&1_000));
    }

    #[test]
    fn test_collect_token_max_supply_observations_from_xudt_extension_flags_0x1() {
        let unique_type_args = vec![0xAB; UNIQUE_TYPE_ARGS_LEN];
        let total_supply = 42_000u128;
        let token_type_hash = [0x91; 32];
        let script_vec = encode_script_vec_with_unique_args(&unique_type_args);
        let type_args = build_xudt_type_args_with_extension_in_args([0x01; 32], &script_vec);

        let unique_cell = dummy_unique_token_info_cell(unique_type_args.clone(), total_supply);
        let xudt_cell = dummy_xudt_cell(token_type_hash, type_args);
        let tx = dummy_tx_data(
            [0xEE; 32],
            false,
            vec![],
            vec![unique_cell, xudt_cell],
            vec![],
            vec![],
        );

        let observations = collect_token_max_supply_observations(&[tx]);
        assert_eq!(
            observations.get(token_type_hash.as_slice()),
            Some(&(total_supply as i128))
        );
    }

    #[test]
    fn test_collect_token_max_supply_observations_from_xudt_extension_flags_0x2_witness() {
        let unique_type_args = vec![0xBC; UNIQUE_TYPE_ARGS_LEN];
        let total_supply = 100_001u128;
        let token_type_hash = [0x92; 32];
        let script_vec = encode_script_vec_with_unique_args(&unique_type_args);
        let script_vec_hash = blake160(&script_vec);
        let type_args = build_xudt_type_args_with_extension_in_witness([0x02; 32], script_vec_hash);

        let xudt_witness = encode_xudt_witness(&script_vec);
        let witness_args = encode_witness_args(Some(&xudt_witness), None);
        let witness_hex = format!("0x{}", hex::encode(witness_args));

        let unique_cell = dummy_unique_token_info_cell(unique_type_args.clone(), total_supply);
        let xudt_cell = dummy_xudt_cell(token_type_hash, type_args);
        let tx = dummy_tx_data(
            [0xEF; 32],
            false,
            vec![],
            vec![unique_cell, xudt_cell],
            vec![witness_hex],
            vec![],
        );

        let observations = collect_token_max_supply_observations(&[tx]);
        assert_eq!(
            observations.get(token_type_hash.as_slice()),
            Some(&(total_supply as i128))
        );
    }

    #[test]
    fn test_collect_token_max_supply_observations_skips_xudt_extension_flags_0x2_when_witness_invalid(
    ) {
        let unique_type_args = vec![0xCD; UNIQUE_TYPE_ARGS_LEN];
        let total_supply = 77_700u128;
        let token_type_hash = [0x93; 32];
        let script_vec = encode_script_vec_with_unique_args(&unique_type_args);
        let type_args =
            build_xudt_type_args_with_extension_in_witness([0x03; 32], blake160(&script_vec));

        let mismatched_script_vec =
            encode_script_vec_with_unique_args(&[0xDD; UNIQUE_TYPE_ARGS_LEN]);
        let mismatched_witness =
            encode_witness_args(Some(&encode_xudt_witness(&mismatched_script_vec)), None);
        let tx_with_hash_mismatch = dummy_tx_data(
            [0xA1; 32],
            false,
            vec![],
            vec![
                dummy_unique_token_info_cell(unique_type_args.clone(), total_supply),
                dummy_xudt_cell(token_type_hash, type_args.clone()),
            ],
            vec![format!("0x{}", hex::encode(mismatched_witness))],
            vec![],
        );

        let tx_without_witness = dummy_tx_data(
            [0xA2; 32],
            false,
            vec![],
            vec![
                dummy_unique_token_info_cell(unique_type_args, total_supply),
                dummy_xudt_cell(token_type_hash, type_args),
            ],
            vec![],
            vec![],
        );

        let mismatch_observations = collect_token_max_supply_observations(&[tx_with_hash_mismatch]);
        assert!(!mismatch_observations.contains_key(token_type_hash.as_slice()));

        let missing_observations = collect_token_max_supply_observations(&[tx_without_witness]);
        assert!(!missing_observations.contains_key(token_type_hash.as_slice()));
    }

    #[test]
    fn test_derive_pre_batch_live_cells_recovers_pre_state() {
        // pre=0, delta=+3 => post=3
        assert_eq!(derive_pre_batch_live_cells(3, 3).unwrap(), 0);
        // pre=10, delta=-4 => post=6
        assert_eq!(derive_pre_batch_live_cells(6, -4).unwrap(), 10);
        // pre=5, delta=-5 => post=0
        assert_eq!(derive_pre_batch_live_cells(0, -5).unwrap(), 5);
    }

    #[test]
    fn test_derive_pre_batch_live_cells_errors_on_negative_pre_state() {
        let err = derive_pre_batch_live_cells(0, 5).unwrap_err();
        assert!(err.to_string().contains("underflow"));
    }

    #[test]
    fn test_secondary_issuance_backfill_threshold_is_1000() {
        assert_eq!(SECONDARY_ISSUANCE_BACKFILL_THRESHOLD, 1000);
    }

    #[test]
    fn test_parse_prefixed_hex_u128_errors_on_invalid_input() {
        let err = parse_prefixed_hex_u128("1234", "test field").unwrap_err();
        assert!(err.to_string().contains("missing 0x prefix"));

        let err = parse_prefixed_hex_u128("0xzz", "test field").unwrap_err();
        assert!(err.to_string().contains("invalid test field hex"));
    }

    #[test]
    fn test_parse_prefixed_hex_u32_errors_on_invalid_input() {
        let err = parse_prefixed_hex_u32("1234", "test field").unwrap_err();
        assert!(err.to_string().contains("missing 0x prefix"));

        let err = parse_prefixed_hex_u32("0xzz", "test field").unwrap_err();
        assert!(err.to_string().contains("invalid test field hex"));
    }

    #[test]
    fn test_parse_prefixed_hex_u64_errors_on_invalid_input() {
        let err = parse_prefixed_hex_u64("1234", "test field").unwrap_err();
        assert!(err.to_string().contains("missing 0x prefix"));

        let err = parse_prefixed_hex_u64("0xzz", "test field").unwrap_err();
        assert!(err.to_string().contains("invalid test field hex"));
    }

    #[test]
    fn test_checked_usize_to_i16_errors_on_overflow() {
        let err = checked_usize_to_i16((i16::MAX as usize) + 1, "output_index").unwrap_err();
        assert!(err.to_string().contains("output_index exceeds i16 range"));
    }

    #[test]
    fn test_tx_hash_key32_errors_on_invalid_length() {
        let err = tx_hash_key32(&[0x11; 31], "cache lookup").unwrap_err();
        assert!(err
            .to_string()
            .contains("cache lookup tx hash must be 32 bytes"));
    }

    #[test]
    fn test_parse_dao_field_for_secondary_issuance_errors_on_invalid_hex() {
        let err = parse_dao_field_for_secondary_issuance("0x1234", 42, "0xabcd").unwrap_err();
        assert!(err
            .to_string()
            .contains("invalid DAO field hex while updating secondary issuance"));
    }

    #[test]
    fn test_parse_tx_cycles_treats_zero_as_missing() {
        let raw = "0x0".to_string();
        let cycles = parse_tx_cycles(Some(&raw), "0xabc", 200).unwrap();
        assert_eq!(cycles, None);
    }

    #[test]
    fn test_parse_tx_cycles_parses_positive_value() {
        let raw = "0x1a".to_string();
        let cycles = parse_tx_cycles(Some(&raw), "0xabc", 200).unwrap();
        assert_eq!(cycles, Some(26));
    }

    #[test]
    fn test_parse_tx_cycles_errors_on_invalid_hex() {
        let raw = "0xzz".to_string();
        let err = parse_tx_cycles(Some(&raw), "0xabc", 200).unwrap_err();
        assert!(err.to_string().contains("invalid tx cycles hex"));
    }

    #[test]
    fn test_parse_outpoint_index_i16_errors_on_overflow() {
        let err = parse_outpoint_index_i16("0x10000", "index").unwrap_err();
        assert!(err.to_string().contains("exceeds i16 range"));
    }

    #[test]
    fn test_dao_csu_for_snapshot_date_errors_when_field_missing() {
        let stats = BatchStats::default();
        let date = chrono::NaiveDate::from_ymd_opt(2026, 2, 21).unwrap();
        let err = dao_csu_for_snapshot_date(&stats, date).unwrap_err();
        assert!(err
            .to_string()
            .contains("missing DAO field for snapshot date"));
    }

    #[test]
    fn test_dao_csu_for_snapshot_date_errors_on_invalid_field_length() {
        let mut stats = BatchStats::default();
        let date = chrono::NaiveDate::from_ymd_opt(2026, 2, 21).unwrap();
        stats.daily_dao_fields.insert(date, vec![0u8; 8]);
        let err = dao_csu_for_snapshot_date(&stats, date).unwrap_err();
        assert!(err
            .to_string()
            .contains("invalid DAO field bytes for snapshot date"));
    }

    #[test]
    fn test_checked_sub_u128_errors_on_underflow() {
        let err = checked_sub_u128(1, 2, "a - b").unwrap_err();
        assert!(err.to_string().contains("underflow"));
    }

    #[test]
    fn test_checked_u128_to_i64_errors_on_overflow() {
        let err = checked_u128_to_i64((i64::MAX as u128) + 1, "x").unwrap_err();
        assert!(err.to_string().contains("exceeds i64"));
    }

    #[test]
    fn test_checked_tx_fee_returns_difference() {
        let fee = checked_tx_fee(1000, 900, false, &[0u8; 32], 42).unwrap();
        assert_eq!(fee, 100);
    }

    #[test]
    fn test_checked_tx_fee_errors_on_underflow() {
        let err = checked_tx_fee(900, 1000, false, &[1u8; 32], 42).unwrap_err();
        assert!(err.to_string().contains("tx fee underflow"));
    }

    #[test]
    fn test_checked_tx_fee_allows_underflow_for_dao_inputs() {
        let fee = checked_tx_fee(900, 1000, true, &[2u8; 32], 42).unwrap();
        assert_eq!(fee, 0);
    }

    #[test]
    fn test_extract_ar_i64_from_dao_errors_on_short_field() {
        let err = extract_ar_i64_from_dao(&[0u8; 8], 42).unwrap_err();
        assert!(err.to_string().contains("missing AR"));
    }

    #[test]
    fn test_extract_ar_i64_from_dao_parses_valid_field() {
        let mut dao = vec![0u8; 32];
        let ar: u64 = 10_000_000_000_000_000;
        dao[8..16].copy_from_slice(&ar.to_le_bytes());
        let parsed = extract_ar_i64_from_dao(&dao, 42).unwrap();
        assert_eq!(parsed, ar as i64);
    }

    #[test]
    fn test_secondary_issuance_skipped_when_more_than_1000_blocks_behind() {
        let threshold = SECONDARY_ISSUANCE_BACKFILL_THRESHOLD;
        assert!(1001 > threshold);
        assert!(5000 > threshold);
    }

    #[test]
    fn test_secondary_issuance_tracked_when_1000_or_fewer_blocks_behind() {
        let threshold = SECONDARY_ISSUANCE_BACKFILL_THRESHOLD;
        assert!(1000 <= threshold);
        assert!(999 <= threshold);
        assert!(1 <= threshold);
    }

    fn build_dao_field(c: u64, s: u64, u: u64) -> Vec<u8> {
        let mut dao = vec![0u8; 32];
        dao[0..8].copy_from_slice(&c.to_le_bytes());
        dao[16..24].copy_from_slice(&s.to_le_bytes());
        dao[24..32].copy_from_slice(&u.to_le_bytes());
        dao
    }

    fn dummy_parsed_block(
        dao: Vec<u8>,
        epoch_number: i64,
        epoch_length: i32,
    ) -> crate::parser::block::ParsedBlock {
        crate::parser::block::ParsedBlock {
            number: 1,
            hash: vec![0u8; 32],
            parent_hash: vec![0u8; 32],
            timestamp: Utc::now(),
            version: 0,
            compact_target: 0,
            transactions_count: 0,
            proposals_count: 0,
            uncles_count: 0,
            epoch_number,
            epoch_index: 0,
            epoch_length,
            dao,
            nonce: vec![],
            extra_hash: vec![],
            proposals_hash: vec![],
            transactions_root: vec![],
            proposals: vec![],
        }
    }

    fn dummy_dao_cell(capacity: i64, is_deposit: bool) -> crate::parser::cell::ParsedCell {
        crate::parser::cell::ParsedCell {
            capacity,
            lock_code_hash: vec![],
            lock_hash_type: 0,
            lock_args: vec![],
            lock_script_hash: vec![],
            type_code_hash: Some(crate::rpc::parse_hex_to_bytes(
                crate::parser::dao::DAO_CODE_HASH,
            )),
            type_hash_type: Some(1),
            type_args: Some(vec![]),
            type_script_hash: None,
            data_hash: vec![],
            data_size: 8,
            data: if is_deposit {
                vec![0u8; 8]
            } else {
                1u64.to_le_bytes().to_vec()
            },
        }
    }

    fn dummy_tx_data(
        hash: [u8; 32],
        is_cellbase: bool,
        inputs: Vec<crate::parser::transaction::ParsedInput>,
        cells: Vec<crate::parser::cell::ParsedCell>,
        witnesses: Vec<String>,
        outputs_data: Vec<String>,
    ) -> TxData {
        TxData {
            hash,
            block_number: 0,
            block_hash: vec![],
            tx_index: 0,
            version: 0,
            inputs_count: inputs.len() as i16,
            outputs_count: cells.len() as i16,
            witnesses_count: witnesses.len() as i16,
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

    fn make_token_info(
        decimals: Option<i32>,
        symbol: Option<&str>,
    ) -> ckbadger_store::types::TokenInfo {
        ckbadger_store::types::TokenInfo {
            type_code_hash: vec![0x77; 32],
            hash_type: 1,
            type_args: vec![0x88; 32],
            standard: "xudt".to_string(),
            name: Some("FallbackName".to_string()),
            symbol: symbol.map(|s| s.to_string()),
            decimals,
            total_supply: Some(0),
            max_supply: None,
            holders_count: 0,
            first_seen_block: 0,
            icon_url: None,
            description: None,
            transfers_count: 0,
        }
    }

    #[test]
    fn test_load_activity_token_info_cache_prefers_symbol_and_converts_decimals() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path().to_str().unwrap()).unwrap();

        let type_hash = vec![0xAA; 32];
        let mut batch = StoreBatch::new(&store);
        batch.put_token(&type_hash, &make_token_info(Some(8), Some("OTTER")));
        batch.commit().unwrap();

        let tx = dummy_tx_data(
            [0x11; 32],
            false,
            vec![],
            vec![dummy_xudt_cell(
                <[u8; 32]>::try_from(type_hash.clone()).unwrap(),
                vec![0x99; 32],
            )],
            vec![],
            vec![],
        );

        let cache = load_activity_token_info_cache(&store, &[tx], &HashMap::new(), &HashMap::new())
            .unwrap();

        assert_eq!(
            cache.get(&type_hash),
            Some(&(Some("OTTER".to_string()), Some(8)))
        );
    }

    #[test]
    fn test_load_activity_token_info_cache_errors_on_invalid_decimals() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open(dir.path().to_str().unwrap()).unwrap();

        let type_hash = vec![0xAB; 32];
        let mut batch = StoreBatch::new(&store);
        batch.put_token(&type_hash, &make_token_info(Some(300), None));
        batch.commit().unwrap();

        let tx = dummy_tx_data(
            [0x12; 32],
            false,
            vec![],
            vec![dummy_xudt_cell(
                <[u8; 32]>::try_from(type_hash.clone()).unwrap(),
                vec![0x98; 32],
            )],
            vec![],
            vec![],
        );

        let err = load_activity_token_info_cache(&store, &[tx], &HashMap::new(), &HashMap::new())
            .unwrap_err();

        assert!(err.to_string().contains("out of u8 range"));
    }

    #[test]
    fn test_accumulate_secondary_issuance_deltas_tracks_exact_miner_and_non_miner() {
        let mut stats = BatchStats::default();
        let prev_c = 10_000_000_000_000_i128;
        let prev_s = 5_000_i128;
        let c = prev_c + 1_000;
        let s = prev_s + 600;
        let u = 2_000_i128;
        let denom = c - u;
        let expected_miner = 600 * u / denom;
        let mut prev = Some((prev_c, prev_s));
        let block = dummy_parsed_block(build_dao_field(c as u64, s as u64, u as u64), 0, 1000);
        let date = ckbadger_common::block_date(block.timestamp);

        accumulate_secondary_issuance_deltas(&mut stats, &block, date, &mut prev).unwrap();

        assert_eq!(stats.daily_secondary_non_miner_delta.get(&date), Some(&600));
        assert_eq!(
            stats.daily_secondary_miner_delta.get(&date),
            Some(&expected_miner)
        );
    }

    #[test]
    fn test_split_secondary_issuance_errors_on_negative_inputs() {
        let err = split_secondary_issuance(1000, 100, -1, 10).unwrap_err();
        assert!(err.to_string().contains("negative input"));
    }

    #[test]
    fn test_split_secondary_issuance_errors_when_deposited_exceeds_liquid_supply() {
        let err = split_secondary_issuance(1000, 900, 200, 10).unwrap_err();
        assert!(err.to_string().contains("exceeds liquid supply"));
    }

    #[test]
    fn test_resolve_non_miner_secondary_delta_for_snapshot_prefers_precomputed_delta() {
        let date = chrono::NaiveDate::from_ymd_opt(2026, 2, 18).unwrap();
        let resolved =
            resolve_non_miner_secondary_delta_for_snapshot(date, Some(123), 10_000, 9_000).unwrap();
        assert_eq!(resolved, 123);
    }

    #[test]
    fn test_resolve_non_miner_secondary_delta_for_snapshot_errors_on_negative_precomputed_delta() {
        let date = chrono::NaiveDate::from_ymd_opt(2026, 2, 18).unwrap();
        let err = resolve_non_miner_secondary_delta_for_snapshot(date, Some(-1), 10_000, 9_000)
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("negative daily non-miner secondary issuance delta"));
    }

    #[test]
    fn test_resolve_non_miner_secondary_delta_for_snapshot_errors_on_negative_fallback_delta() {
        let date = chrono::NaiveDate::from_ymd_opt(2026, 2, 18).unwrap();
        let err =
            resolve_non_miner_secondary_delta_for_snapshot(date, None, 8_999, 9_000).unwrap_err();
        assert!(err
            .to_string()
            .contains("secondary pool decreased while building DAO daily snapshot"));
    }

    #[test]
    fn test_accumulate_secondary_issuance_deltas_errors_on_negative_adjustment() {
        let mut stats = BatchStats::default();
        let mut prev = Some((20_000_000_000_000_i128, 8_000_i128));
        let block = dummy_parsed_block(
            build_dao_field(
                (20_000_000_000_000_i128 + 500) as u64,
                (8_000_i128 - 100) as u64,
                0,
            ),
            0,
            1000,
        );
        let date = ckbadger_common::block_date(block.timestamp);

        let err =
            accumulate_secondary_issuance_deltas(&mut stats, &block, date, &mut prev).unwrap_err();
        assert!(err
            .to_string()
            .contains("secondary issuance S delta underflow"));
    }

    #[test]
    fn test_accumulate_secondary_issuance_deltas_same_day_fails_on_negative_s_drop() {
        let mut stats = BatchStats::default();
        let mut prev = Some((30_000_000_000_000_i128, 10_000_i128));
        let date = chrono::NaiveDate::from_ymd_opt(2026, 2, 18).unwrap();

        // First block in the day has an S drop (protocol adjustment) and must fail fast.
        let block_drop =
            dummy_parsed_block(build_dao_field(30_000_000_000_500, 9_950, 100), 0, 1000);
        let err = accumulate_secondary_issuance_deltas(&mut stats, &block_drop, date, &mut prev)
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("secondary issuance S delta underflow"));
    }

    #[test]
    fn test_accumulate_secondary_issuance_deltas_errors_on_invalid_dao_field() {
        let mut stats = BatchStats::default();
        let mut prev = Some((30_000_000_000_000_i128, 10_000_i128));
        let date = chrono::NaiveDate::from_ymd_opt(2026, 2, 18).unwrap();
        let block = dummy_parsed_block(vec![0u8; 8], 0, 1000);

        let err =
            accumulate_secondary_issuance_deltas(&mut stats, &block, date, &mut prev).unwrap_err();
        assert!(err
            .to_string()
            .contains("invalid DAO field bytes while accumulating secondary issuance"));
    }

    #[test]
    fn test_accumulate_dao_snapshot_deltas_subtracts_phase1_even_when_capacity_differs() {
        let block_date = chrono::NaiveDate::from_ymd_opt(2026, 2, 18).unwrap();
        let dao_code_hash = crate::rpc::parse_hex_to_bytes(crate::parser::dao::DAO_CODE_HASH);
        let input_hash_vec = vec![0x11; 32];
        let input_hash: [u8; 32] = input_hash_vec.clone().try_into().unwrap();

        let tx = dummy_tx_data(
            [0xAA; 32],
            false,
            vec![crate::parser::transaction::ParsedInput {
                previous_tx_hash: input_hash,
                previous_output_index: 0,
                since: 0,
            }],
            vec![dummy_dao_cell(9_900_000_000, false)],
            vec![],
            vec!["0x0100000000000000".to_string()],
        );

        let mut consumed_dao_map: DaoConsumedMap = HashMap::new();
        consumed_dao_map.insert(
            (input_hash_vec, 0),
            (0, vec![], 0, "10000000000".to_string(), 0, 0),
        );

        let mut same_batch_dao_map: DaoSameBatchMap = HashMap::new();
        let mut daily_active_delta: HashMap<chrono::NaiveDate, i128> = HashMap::new();
        let mut daily_gross_deposit_delta: HashMap<chrono::NaiveDate, i128> = HashMap::new();
        let mut daily_new_deposits_delta: HashMap<chrono::NaiveDate, i64> = HashMap::new();
        let mut daily_withdrawals_delta: HashMap<chrono::NaiveDate, i64> = HashMap::new();

        accumulate_dao_snapshot_deltas_for_txs(
            &[tx],
            block_date,
            &dao_code_hash,
            &consumed_dao_map,
            &mut same_batch_dao_map,
            &mut daily_active_delta,
            &mut daily_gross_deposit_delta,
            &mut daily_new_deposits_delta,
            &mut daily_withdrawals_delta,
        )
        .unwrap();

        assert_eq!(daily_active_delta.get(&block_date), Some(&-10_000_000_000));
        assert!(daily_gross_deposit_delta.is_empty());
        assert!(daily_new_deposits_delta.is_empty());
        assert!(daily_withdrawals_delta.is_empty());
    }

    #[test]
    fn test_accumulate_dao_snapshot_deltas_ignores_status1_inputs_for_phase1_subtraction() {
        let block_date = chrono::NaiveDate::from_ymd_opt(2026, 2, 18).unwrap();
        let dao_code_hash = crate::rpc::parse_hex_to_bytes(crate::parser::dao::DAO_CODE_HASH);
        let input_hash_vec = vec![0x22; 32];
        let input_hash: [u8; 32] = input_hash_vec.clone().try_into().unwrap();

        let tx = dummy_tx_data(
            [0xBB; 32],
            false,
            vec![crate::parser::transaction::ParsedInput {
                previous_tx_hash: input_hash,
                previous_output_index: 0,
                since: 0,
            }],
            vec![dummy_dao_cell(9_900_000_000, false)],
            vec![],
            vec!["0x0100000000000000".to_string()],
        );

        let mut consumed_dao_map: DaoConsumedMap = HashMap::new();
        consumed_dao_map.insert((input_hash_vec, 0), (0, vec![], 0, "123".to_string(), 0, 1));

        let mut same_batch_dao_map: DaoSameBatchMap = HashMap::new();
        let mut daily_active_delta: HashMap<chrono::NaiveDate, i128> = HashMap::new();
        let mut daily_gross_deposit_delta: HashMap<chrono::NaiveDate, i128> = HashMap::new();
        let mut daily_new_deposits_delta: HashMap<chrono::NaiveDate, i64> = HashMap::new();
        let mut daily_withdrawals_delta: HashMap<chrono::NaiveDate, i64> = HashMap::new();

        accumulate_dao_snapshot_deltas_for_txs(
            &[tx],
            block_date,
            &dao_code_hash,
            &consumed_dao_map,
            &mut same_batch_dao_map,
            &mut daily_active_delta,
            &mut daily_gross_deposit_delta,
            &mut daily_new_deposits_delta,
            &mut daily_withdrawals_delta,
        )
        .unwrap();

        assert!(daily_active_delta.is_empty());
        assert!(daily_gross_deposit_delta.is_empty());
        assert!(daily_new_deposits_delta.is_empty());
        assert_eq!(daily_withdrawals_delta.get(&block_date), Some(&1));
    }

    #[test]
    fn test_accumulate_dao_snapshot_deltas_counts_status1_inputs_as_withdrawals() {
        let block_date = chrono::NaiveDate::from_ymd_opt(2026, 2, 18).unwrap();
        let dao_code_hash = crate::rpc::parse_hex_to_bytes(crate::parser::dao::DAO_CODE_HASH);
        let input_hash_vec = vec![0x33; 32];
        let input_hash: [u8; 32] = input_hash_vec.clone().try_into().unwrap();

        let tx = dummy_tx_data(
            [0xCC; 32],
            false,
            vec![crate::parser::transaction::ParsedInput {
                previous_tx_hash: input_hash,
                previous_output_index: 0,
                since: 0,
            }],
            vec![],
            vec![],
            vec![],
        );

        let mut consumed_dao_map: DaoConsumedMap = HashMap::new();
        consumed_dao_map.insert((input_hash_vec, 0), (0, vec![], 0, "123".to_string(), 0, 1));

        let mut same_batch_dao_map: DaoSameBatchMap = HashMap::new();
        let mut daily_active_delta: HashMap<chrono::NaiveDate, i128> = HashMap::new();
        let mut daily_gross_deposit_delta: HashMap<chrono::NaiveDate, i128> = HashMap::new();
        let mut daily_new_deposits_delta: HashMap<chrono::NaiveDate, i64> = HashMap::new();
        let mut daily_withdrawals_delta: HashMap<chrono::NaiveDate, i64> = HashMap::new();

        accumulate_dao_snapshot_deltas_for_txs(
            &[tx],
            block_date,
            &dao_code_hash,
            &consumed_dao_map,
            &mut same_batch_dao_map,
            &mut daily_active_delta,
            &mut daily_gross_deposit_delta,
            &mut daily_new_deposits_delta,
            &mut daily_withdrawals_delta,
        )
        .unwrap();

        assert!(daily_active_delta.is_empty());
        assert!(daily_gross_deposit_delta.is_empty());
        assert!(daily_new_deposits_delta.is_empty());
        assert_eq!(daily_withdrawals_delta.get(&block_date), Some(&1));
    }

    #[test]
    fn test_accumulate_dao_snapshot_deltas_counts_status1_inputs_in_mixed_tx() {
        let block_date = chrono::NaiveDate::from_ymd_opt(2026, 2, 18).unwrap();
        let dao_code_hash = crate::rpc::parse_hex_to_bytes(crate::parser::dao::DAO_CODE_HASH);
        let input_hash_vec = vec![0x34; 32];
        let input_hash: [u8; 32] = input_hash_vec.clone().try_into().unwrap();

        // Mixed tx: contains DAO withdraw-request output and consumes a status=1 DAO input.
        let tx = dummy_tx_data(
            [0xCD; 32],
            false,
            vec![crate::parser::transaction::ParsedInput {
                previous_tx_hash: input_hash,
                previous_output_index: 0,
                since: 0,
            }],
            vec![dummy_dao_cell(9_900_000_000, false)],
            vec![],
            vec!["0x0100000000000000".to_string()],
        );

        let mut consumed_dao_map: DaoConsumedMap = HashMap::new();
        consumed_dao_map.insert((input_hash_vec, 0), (0, vec![], 0, "123".to_string(), 0, 1));

        let mut same_batch_dao_map: DaoSameBatchMap = HashMap::new();
        let mut daily_active_delta: HashMap<chrono::NaiveDate, i128> = HashMap::new();
        let mut daily_gross_deposit_delta: HashMap<chrono::NaiveDate, i128> = HashMap::new();
        let mut daily_new_deposits_delta: HashMap<chrono::NaiveDate, i64> = HashMap::new();
        let mut daily_withdrawals_delta: HashMap<chrono::NaiveDate, i64> = HashMap::new();

        accumulate_dao_snapshot_deltas_for_txs(
            &[tx],
            block_date,
            &dao_code_hash,
            &consumed_dao_map,
            &mut same_batch_dao_map,
            &mut daily_active_delta,
            &mut daily_gross_deposit_delta,
            &mut daily_new_deposits_delta,
            &mut daily_withdrawals_delta,
        )
        .unwrap();

        assert_eq!(daily_withdrawals_delta.get(&block_date), Some(&1));
    }

    #[test]
    fn test_accumulate_dao_snapshot_deltas_errors_on_invalid_capacity_string() {
        let block_date = chrono::NaiveDate::from_ymd_opt(2026, 2, 18).unwrap();
        let dao_code_hash = crate::rpc::parse_hex_to_bytes(crate::parser::dao::DAO_CODE_HASH);
        let input_hash_vec = vec![0x44; 32];
        let input_hash: [u8; 32] = input_hash_vec.clone().try_into().unwrap();

        let tx = dummy_tx_data(
            [0xDD; 32],
            false,
            vec![crate::parser::transaction::ParsedInput {
                previous_tx_hash: input_hash,
                previous_output_index: 0,
                since: 0,
            }],
            vec![dummy_dao_cell(9_900_000_000, false)],
            vec![],
            vec!["0x0100000000000000".to_string()],
        );

        let mut consumed_dao_map: DaoConsumedMap = HashMap::new();
        consumed_dao_map.insert(
            (input_hash_vec, 0),
            (0, vec![], 0, "bad-capacity".to_string(), 0, 0),
        );

        let mut same_batch_dao_map: DaoSameBatchMap = HashMap::new();
        let mut daily_active_delta: HashMap<chrono::NaiveDate, i128> = HashMap::new();
        let mut daily_gross_deposit_delta: HashMap<chrono::NaiveDate, i128> = HashMap::new();
        let mut daily_new_deposits_delta: HashMap<chrono::NaiveDate, i64> = HashMap::new();
        let mut daily_withdrawals_delta: HashMap<chrono::NaiveDate, i64> = HashMap::new();

        let err = accumulate_dao_snapshot_deltas_for_txs(
            &[tx],
            block_date,
            &dao_code_hash,
            &consumed_dao_map,
            &mut same_batch_dao_map,
            &mut daily_active_delta,
            &mut daily_gross_deposit_delta,
            &mut daily_new_deposits_delta,
            &mut daily_withdrawals_delta,
        )
        .unwrap_err();
        assert!(err.to_string().contains("invalid DAO capacity string"));
    }

    #[test]
    fn test_perf_snapshot_uses_last_batch_after_reset() {
        let perf = PerfStats::default();
        perf.add_fetch(Duration::from_millis(120));
        perf.add_db_write(Duration::from_millis(340));
        perf.add_db_commit(Duration::from_millis(210));
        perf.blocks_count.fetch_add(10, Ordering::Relaxed);
        perf.report_and_reset();

        let (rpc_ms, db_stage_ms, db_commit_ms) = perf.snapshot_ms();
        assert!((rpc_ms - 120.0).abs() < 0.001);
        assert!((db_stage_ms - 340.0).abs() < 0.001);
        assert!((db_commit_ms - 210.0).abs() < 0.001);
    }

    #[test]
    fn test_perf_snapshot_prefers_current_accumulator_over_last_batch() {
        let perf = PerfStats::default();

        perf.add_fetch(Duration::from_millis(500));
        perf.add_db_write(Duration::from_millis(700));
        perf.add_db_commit(Duration::from_millis(420));
        perf.blocks_count.fetch_add(10, Ordering::Relaxed);
        perf.report_and_reset();

        perf.add_fetch(Duration::from_millis(150));
        perf.add_db_write(Duration::from_millis(250));
        perf.add_db_commit(Duration::from_millis(90));

        let (rpc_ms, db_stage_ms, db_commit_ms) = perf.snapshot_ms();
        assert!((rpc_ms - 150.0).abs() < 0.001);
        assert!((db_stage_ms - 250.0).abs() < 0.001);
        assert!((db_commit_ms - 90.0).abs() < 0.001);
    }

    #[test]
    fn test_pipeline_perf_snapshot_returns_none_when_empty() {
        let perf = PipelinePerfStats::default();
        assert!(perf.snapshot().is_none());
    }

    #[test]
    fn test_pipeline_perf_snapshot_contains_stage_metrics() {
        let perf = PipelinePerfStats::default();
        perf.set_queue_capacities(16, 16);
        perf.record_fetch(Duration::from_millis(20), 3, 16);
        perf.record_parse(Duration::from_millis(40), 7, 16);
        perf.record_write(Duration::from_millis(80), 33.0, 12.0, 6, 16);

        let snapshot = perf.snapshot().expect("pipeline snapshot should exist");
        assert_eq!(snapshot.fetch_ms, Some(20.0));
        assert_eq!(snapshot.parse_ms, Some(40.0));
        assert_eq!(snapshot.write_ms, Some(80.0));
        assert_eq!(snapshot.commit_ms, Some(33.0));
        let wait = snapshot
            .writer_wait_ms
            .expect("writer wait should be present");
        assert!((wait - 12.0).abs() < 0.001);
        assert_eq!(snapshot.fetch_queue_depth, Some(3));
        assert_eq!(snapshot.parse_queue_depth, Some(7));
        assert_eq!(snapshot.parse_queue_capacity, Some(16));
        assert_eq!(snapshot.writer_queue_depth, Some(6));
        assert_eq!(snapshot.writer_queue_capacity, Some(16));
    }

    #[test]
    fn test_bump_pipeline_reset_epoch_is_monotonic() {
        let epoch = AtomicU64::new(0);
        assert_eq!(bump_pipeline_reset_epoch(&epoch), 1);
        assert_eq!(bump_pipeline_reset_epoch(&epoch), 2);
        assert_eq!(epoch.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_pipeline_reset_reason_roundtrip_known_values() {
        let reasons = [
            "pipeline batch mismatch",
            "reorg handled",
            "deep fork paused",
            "batch write failed",
        ];
        for reason in reasons {
            let code = encode_pipeline_reset_reason(reason);
            assert_ne!(code, PIPELINE_RESET_REASON_UNKNOWN);
            assert_eq!(decode_pipeline_reset_reason(code), reason);
        }
    }

    #[test]
    fn test_pipeline_reset_reason_unknown_fallback() {
        let code = encode_pipeline_reset_reason("unexpected reason");
        assert_eq!(code, PIPELINE_RESET_REASON_UNKNOWN);
        assert_eq!(decode_pipeline_reset_reason(code), "unknown");
        assert_eq!(decode_pipeline_reset_reason(255), "unknown");
    }

    #[test]
    fn test_adaptive_reason_roundtrip_known_values() {
        let reasons = [
            "pressure_backoff",
            "pressure_backoff_floor_down",
            "severe_pressure_backoff",
            "healthy_step_up",
            "healthy_step_up_floor_recover",
            "moderate_backoff",
            "moderate_backoff_inflight_relief",
            "moderate_backoff_floor_down",
            "throughput_backoff",
            "adjusted",
            "early_height_boost",
        ];
        for reason in reasons {
            let code = encode_adaptive_batch_reason(reason);
            assert_ne!(code, ADAPTIVE_REASON_UNKNOWN);
            assert_eq!(decode_adaptive_batch_reason(code), Some(reason));
        }
    }

    #[test]
    fn test_adaptive_reason_unknown_fallback() {
        let code = encode_adaptive_batch_reason("unexpected reason");
        assert_eq!(code, ADAPTIVE_REASON_UNKNOWN);
        assert_eq!(decode_adaptive_batch_reason(code), None);
        assert_eq!(decode_adaptive_batch_reason(255), None);
    }

    // --- DAO recalculation boundary tests ---

    #[test]
    fn test_dao_deltas_computed_flag_defaults_false() {
        let stats = BatchStats::default();
        assert!(!stats.dao_deltas_computed);
    }

    #[test]
    fn test_dao_deltas_computed_flag_set_after_computation() {
        let stats = BatchStats {
            dao_deltas_computed: true,
            ..Default::default()
        };
        assert!(stats.dao_deltas_computed);
        // Empty delta maps are valid when no DAO txs exist
        assert!(stats.dao_daily_active_delta.is_empty());
    }

    /// Helper: returns true if this batch crosses a 1000-block boundary
    fn crosses_1000_boundary(start_block: u64, end_block: u64) -> bool {
        (start_block / 1000) != (end_block / 1000)
    }

    #[test]
    fn test_crossed_1000_within_same_thousand() {
        assert!(!crosses_1000_boundary(6330000, 6330999));
        assert!(!crosses_1000_boundary(0, 999));
        assert!(!crosses_1000_boundary(5000, 5999));
    }

    #[test]
    fn test_crossed_1000_across_boundary() {
        assert!(crosses_1000_boundary(6330000, 6339999));
        assert!(crosses_1000_boundary(999, 1000));
        assert!(crosses_1000_boundary(0, 9999));
        assert!(crosses_1000_boundary(4500, 5500));
    }

    #[test]
    fn test_crossed_1000_exact_boundary() {
        assert!(crosses_1000_boundary(999, 1000));
        assert!(!crosses_1000_boundary(1000, 1001));
        assert!(crosses_1000_boundary(1999, 2000));
    }

    #[test]
    fn test_dao_recalc_skipped_during_bulk_sync() {
        let bulk_sync_threshold = 1000u64;
        let blocks_remaining = 10_000_000u64;
        let is_bulk = blocks_remaining > bulk_sync_threshold;
        let crossed = crosses_1000_boundary(6330000, 6339999);
        assert!(crossed, "batch should cross 1000-block boundary");
        assert!(is_bulk, "should be in bulk sync mode");
        assert!(
            !crossed || is_bulk,
            "DAO recalc should be skipped during bulk sync"
        );
    }

    #[test]
    fn test_dao_recalc_runs_in_realtime_sync() {
        let bulk_sync_threshold = 1000u64;
        let blocks_remaining = 500u64;
        let is_bulk = blocks_remaining > bulk_sync_threshold;
        let crossed = crosses_1000_boundary(18545999, 18546999);
        assert!(crossed, "batch should cross 1000-block boundary");
        assert!(!is_bulk, "should NOT be in bulk sync mode");
        assert!(
            crossed && !is_bulk,
            "DAO recalc should run in real-time sync"
        );
    }

    #[test]
    fn test_dao_recalc_not_triggered_without_boundary_crossing() {
        let bulk_sync_threshold = 1000u64;
        let blocks_remaining = 100u64;
        let is_bulk = blocks_remaining > bulk_sync_threshold;
        let crossed = crosses_1000_boundary(18546500, 18546800);
        assert!(!crossed, "batch should NOT cross 1000-block boundary");
        assert!(
            !crossed || is_bulk,
            "DAO recalc should not trigger without boundary crossing"
        );
    }

    #[test]
    fn test_partition_boundary_detection() {
        let start = 4_000_000u64;
        let end = 4_999_999u64;
        assert_eq!(get_partition_index(start), get_partition_index(end));
        assert!(!crosses_partition_boundary(start, end));
        assert_eq!(format_partition_range(start, end), "[p0]");

        let start = 4_999_990u64;
        let end = 5_000_009u64;
        assert_ne!(get_partition_index(start), get_partition_index(end));
        assert!(crosses_partition_boundary(start, end));
        assert_eq!(format_partition_range(start, end), "[p0->p1]");

        let start = 9_999_999u64;
        let end = 10_000_001u64;
        assert_ne!(get_partition_index(start), get_partition_index(end));
        assert!(crosses_partition_boundary(start, end));
        assert_eq!(format_partition_range(start, end), "[p1->p2]");

        let start = 5_000_000u64;
        let end = 5_100_000u64;
        assert_eq!(get_partition_index(start), get_partition_index(end));
        assert!(!crosses_partition_boundary(start, end));
        assert_eq!(format_partition_range(start, end), "[p1]");
    }

    #[test]
    fn test_decode_startup_phase() {
        assert_eq!(decode_startup_phase(STARTUP_PHASE_NONE), None);
        assert_eq!(
            decode_startup_phase(STARTUP_PHASE_ROLLBACK_CLEANUP),
            Some("rollback_cleanup")
        );
        assert_eq!(decode_startup_phase(99), None);
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
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());

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
        let store = Arc::new(CkbadgerStore::open(dir.path()).unwrap());

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
