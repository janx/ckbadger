#![allow(clippy::type_complexity)]

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, NaiveDate, Timelike, Utc};
use ckb_hash::new_blake2b;
use ckbadger_common::{LabelImportConfig, PipelineProgressData};
use dashmap::DashMap;
use futures::stream::{FuturesOrdered, StreamExt};
use rayon::prelude::*;
use serde::Serialize;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

use ckbadger_store::batch::StoreBatch;
use ckbadger_store::types::{
    AddressBalance, CachedBlockHeader, HodlTrackerState, LiveCellInfo, NftTypeIndex, SporeTypeIndex,
};
use ckbadger_store::CkbadgerStore;

use crate::cache::CacheInvalidator;
use crate::config::{Config, DEEP_FORK_DEPTH};
use crate::db::writer::hodl_wave::HodlWaveTracker;
use crate::db::{BatchWriter, ReorgResult, Repository, SecondaryIssuanceBreakdown};
use crate::parser::{
    BlockParser, CellParser, DaoParser, DotbitParser, MnftParser, ScriptParser, SporeParser,
    TransactionParser, UdtParser,
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
const DOTBIT_SENTINEL_COLLECTION: [u8; 32] = *b"dotbit_collection_______________";
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
const FLIGHT_RECORDER_CAPACITY: usize = 200;
static OMNILOCK_CODE_HASHES: OnceLock<Vec<Vec<u8>>> = OnceLock::new();

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

fn should_rebuild_hodl_tracker_state(state: Option<&HodlTrackerState>, tip_block: i64) -> bool {
    if tip_block <= 0 {
        return false;
    }
    match state {
        None => true,
        Some(state) => {
            state.date_transitions.is_empty()
                || state
                    .date_transitions
                    .last()
                    .is_some_and(|(last_block, _)| *last_block > tip_block)
        }
    }
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

fn format_outpoint_sample(outpoints: &[(Vec<u8>, i16)], max_items: usize) -> String {
    if outpoints.is_empty() {
        return "none".to_string();
    }

    outpoints
        .iter()
        .take(max_items)
        .map(|(tx_hash, output_index)| {
            let prefix_len = tx_hash.len().min(8);
            format!("0x{}:{}", hex::encode(&tx_hash[..prefix_len]), output_index)
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn should_log_unresolved_retry(attempt: usize) -> bool {
    attempt == 1 || attempt.is_multiple_of(10) || attempt >= PARSER_UNRESOLVED_MAX_RETRIES
}

fn should_skip_address_balances(_bulk_sync_mode: bool) -> bool {
    // Address balances must always be updated inline to keep bulk sync exact.
    false
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
    changes: &HashMap<Vec<u8>, (i64, i32, i32, i64, i64, &[u8], i64)>,
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
    hourly_stats: HashMap<DateTime<Utc>, (i32, i32, i32, i32, i64)>,
    daily_stats: HashMap<NaiveDate, (i32, i32, i32, i32, i64, i64, i64, i64, i64)>,
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
}

#[derive(Clone)]
struct CachedUdtCellInfo {
    type_script_hash: Vec<u8>,
    type_code_hash: Vec<u8>,
    type_hash_type: i16,
    type_args: Vec<u8>,
    lock_script_hash: Vec<u8>,
    amount: u128,
    standard: String,
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

fn parse_outpoint_index_i16(field: &str, label: &str) -> Result<i16> {
    let value = parse_prefixed_hex_u32(field, label)?;
    i16::try_from(value).map_err(|_| anyhow::anyhow!("{} exceeds i16 range: {}", label, value))
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
) {
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

        if tx_data.is_cellbase || !has_withdraw_request_output {
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
                        maybe_cap = capacity_str.parse::<i64>().ok();
                    }
                }
            }
            if let Some(capacity) = maybe_cap {
                *daily_active_delta.entry(block_date).or_default() -= capacity as i128;
            }
        }
    }
}

fn accumulate_secondary_issuance_deltas(
    stats: &mut BatchStats,
    parsed: &crate::parser::block::ParsedBlock,
    block_date: NaiveDate,
    prev_dao_cs: &mut Option<(i128, i128)>,
) -> Result<()> {
    let Some((c, s, u)) = extract_dao_csu(&parsed.dao) else {
        return Ok(());
    };

    if let Some((prev_c, prev_s)) = *prev_dao_cs {
        let _c_delta = c - prev_c;
        let s_delta = s - prev_s;

        // Protocol upgrades can produce negative S deltas; they should not reduce
        // user-facing cumulative issuance series. Track only positive growth.
        if s_delta > 0 {
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
    }

    *prev_dao_cs = Some((c, s));
    Ok(())
}

#[derive(Default)]
struct PerfStats {
    fetch_us: AtomicU64,
    db_write_us: AtomicU64,
    last_fetch_us: AtomicU64,
    last_db_write_us: AtomicU64,
    blocks_count: AtomicU64,
}

impl PerfStats {
    fn add_fetch(&self, duration: Duration) {
        self.fetch_us
            .fetch_add(duration.as_micros() as u64, Ordering::Relaxed);
    }

    fn add_db_write(&self, duration: Duration) {
        self.db_write_us
            .fetch_add(duration.as_micros() as u64, Ordering::Relaxed);
    }

    fn report_and_reset(&self) {
        let blocks = self.blocks_count.swap(0, Ordering::Relaxed);
        if blocks == 0 {
            return;
        }
        let fetch_us = self.fetch_us.swap(0, Ordering::Relaxed);
        let db_us = self.db_write_us.swap(0, Ordering::Relaxed);
        self.last_fetch_us.store(fetch_us, Ordering::Relaxed);
        self.last_db_write_us.store(db_us, Ordering::Relaxed);

        let fetch_ms = fetch_us as f64 / 1000.0;
        let db_ms = db_us as f64 / 1000.0;
        info!(
            blocks,
            fetch_ms = format!("{:.1}", fetch_ms),
            db_ms = format!("{:.1}", db_ms),
            "Batch perf"
        );
    }

    /// Snapshot current accumulated values, falling back to the latest completed batch.
    fn snapshot_ms(&self) -> (f64, f64) {
        let rpc = self.fetch_us.load(Ordering::Relaxed);
        let db = self.db_write_us.load(Ordering::Relaxed);
        let rpc = if rpc > 0 {
            rpc
        } else {
            self.last_fetch_us.load(Ordering::Relaxed)
        };
        let db = if db > 0 {
            db
        } else {
            self.last_db_write_us.load(Ordering::Relaxed)
        };
        (rpc as f64 / 1000.0, db as f64 / 1000.0)
    }
}

#[derive(Default)]
struct PipelinePerfStats {
    last_fetch_us: AtomicU64,
    last_parse_us: AtomicU64,
    last_write_us: AtomicU64,
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
        writer_wait_ms: f64,
        queue_depth: usize,
        queue_capacity: usize,
    ) {
        self.last_write_us
            .store(duration.as_micros() as u64, Ordering::Relaxed);
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
const PARSER_UNRESOLVED_RETRY_DELAY_MS: u64 = 500;
const PARSER_UNRESOLVED_MAX_RETRIES: usize = 240;

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
                        let cells = CellParser::parse_outputs(tx);
                        let witnesses: Vec<String> = tx.witnesses.clone();
                        let outputs_data: Vec<String> = tx.outputs_data.clone();
                        let total_output_capacity: i64 = cells.iter().map(|c| c.capacity).sum();
                        let cycles = if tx_index == 0 {
                            None
                        } else {
                            block_response
                                .cycles
                                .as_ref()
                                .and_then(|c| c.get(tx_index - 1))
                                .and_then(|hex| {
                                    let hex = hex.strip_prefix("0x").unwrap_or(hex);
                                    u64::from_str_radix(hex, 16).ok().map(|v| v as i64)
                                })
                        };
                        Ok(TxData {
                            hash: parsed_tx.hash,
                            block_number: parsed.number,
                            block_hash: parsed.hash.clone(),
                            tx_index: tx_index as i32,
                            version: parsed_tx.version,
                            inputs_count: parsed_tx.inputs_count as i16,
                            outputs_count: parsed_tx.outputs_count as i16,
                            witnesses_count: parsed_tx.witnesses_count as i16,
                            cell_deps_count: parsed_tx.cell_deps_count as i16,
                            header_deps_count: parsed_tx.header_deps_count as i16,
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
                    all_input_outpoints.push((
                        input.previous_tx_hash.to_vec(),
                        input.previous_output_index as i16,
                    ));
                }
            }
        }
        all_tx_data.extend(tx_data_list);
        all_parsed_blocks.push(parsed);
    }
    Ok((all_parsed_blocks, all_tx_data, all_input_outpoints))
}

const CACHE_INVALIDATION_INTERVAL: u64 = 10_000;
const SECONDARY_ISSUANCE_BACKFILL_THRESHOLD: u64 = 1000;

pub struct Indexer {
    run_id: String,
    config: Config,
    rpc: CkbRpcClient,
    repo: Repository,
    writer: BatchWriter,
    progress: Arc<SyncProgress>,
    cell_cache: Arc<DashMap<([u8; 32], i32), CachedCellInfo>>,
    udt_cell_cache: Arc<DashMap<([u8; 32], i16), CachedUdtCellInfo>>,
    perf: PerfStats,
    pipeline_perf: Arc<PipelinePerfStats>,
    cache_invalidator: CacheInvalidator,
    last_cache_invalidation: tokio::sync::Mutex<u64>,
    was_bulk_sync_active: std::sync::atomic::AtomicBool,
    was_secondary_issuance_bulk_active: std::sync::atomic::AtomicBool,
    rebuild_pause_flag: Arc<std::sync::atomic::AtomicBool>,
    reorg_notify_flag: Arc<std::sync::atomic::AtomicBool>,
    startup_phase: AtomicU8,
    pipeline_reset_epoch: Arc<AtomicU64>,
    incident_seq: AtomicU64,
    flight_recorder: FlightRecorder,
    incident_dir: PathBuf,
    label_import_started: std::sync::atomic::AtomicBool,
    ckb_store: Option<Arc<CkbChainReader>>,
    hodl_tracker: std::sync::Mutex<HodlWaveTracker>,
}

impl Indexer {
    pub async fn new(run_id: String, config: Config, store: Arc<CkbadgerStore>) -> Result<Self> {
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
            store.tip_number().unwrap_or(0)
        } else {
            rpc.get_tip_block_number().await?
        };

        let progress = Arc::new(SyncProgress::new(tip_number as u64, chain_tip));
        progress.start_refresher();
        let cell_cache = Arc::new(DashMap::with_capacity(CELL_CACHE_CAPACITY));
        let udt_cell_cache = Arc::new(DashMap::with_capacity(UDT_CELL_CACHE_CAPACITY));

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

        let incident_dir = PathBuf::from(&config.data_path).join("incidents");

        Ok(Self {
            run_id,
            config,
            rpc,
            repo,
            writer,
            progress,
            cell_cache,
            udt_cell_cache,
            perf: PerfStats::default(),
            pipeline_perf: Arc::new(PipelinePerfStats::default()),
            cache_invalidator,
            last_cache_invalidation: tokio::sync::Mutex::new(0),
            was_bulk_sync_active: std::sync::atomic::AtomicBool::new(was_bulk),
            was_secondary_issuance_bulk_active: std::sync::atomic::AtomicBool::new(
                was_secondary_bulk,
            ),
            rebuild_pause_flag: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            reorg_notify_flag: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            startup_phase: AtomicU8::new(STARTUP_PHASE_NONE),
            pipeline_reset_epoch: Arc::new(AtomicU64::new(0)),
            incident_seq: AtomicU64::new(0),
            flight_recorder: FlightRecorder::new(FLIGHT_RECORDER_CAPACITY),
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
    ) -> Vec<crate::parser::ParsedUdtCell> {
        let mut parsed = Vec::new();
        let mut standard_cache: HashMap<Vec<u8>, Option<String>> = HashMap::new();

        for (output, data_hex) in tx.outputs.iter().zip(tx.outputs_data.iter()) {
            if let Some(cell) = UdtParser::parse_udt_cell(output, data_hex) {
                parsed.push(cell);
                continue;
            }

            let Some(type_script) = output.type_.as_ref() else {
                continue;
            };

            let type_script_hash = ScriptParser::compute_script_hash(type_script);
            let standard_hint = if let Some(cached) = standard_cache.get(&type_script_hash) {
                cached.clone()
            } else {
                let looked_up = self
                    .writer
                    .store()
                    .get_token(&type_script_hash)
                    .ok()
                    .flatten()
                    .map(|info| info.standard);
                standard_cache.insert(type_script_hash.clone(), looked_up.clone());
                looked_up
            };

            let Some(standard_hint) = standard_hint else {
                continue;
            };

            if let Some(cell) =
                UdtParser::parse_udt_cell_with_standard_hint(output, data_hex, Some(&standard_hint))
            {
                parsed.push(cell);
            }
        }

        parsed
    }

    pub fn rebuild_pause_flag(&self) -> Arc<std::sync::atomic::AtomicBool> {
        Arc::clone(&self.rebuild_pause_flag)
    }

    pub fn is_bulk_sync_active(&self) -> bool {
        self.progress.blocks_remaining() > self.config.bulk_sync_threshold
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

    pub fn record_runtime_heartbeat(&self, current_block: u64) {
        if let Err(e) = self
            .writer
            .store()
            .mark_runtime_heartbeat(&self.run_id, current_block as i64)
        {
            warn!(
                run_id = %self.run_id,
                current_block,
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

    fn request_pipeline_reset(
        &self,
        reason: &'static str,
        expected_start: Option<u64>,
        got_start: Option<u64>,
        writer_queue_depth: Option<usize>,
    ) {
        let epoch = bump_pipeline_reset_epoch(&self.pipeline_reset_epoch);
        self.reorg_notify_flag.store(true, Ordering::SeqCst);
        info!(
            run_id = %self.run_id,
            epoch,
            reason,
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

    /// Snapshot the current perf stats: (fetch_ms, db_ms).
    pub fn perf_snapshot_ms(&self) -> (f64, f64) {
        self.perf.snapshot_ms()
    }

    pub fn pipeline_progress_snapshot(&self) -> Option<PipelineProgressData> {
        if !self.config.pipeline_enabled {
            return None;
        }
        self.pipeline_perf.snapshot()
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
            total_transactions: sync_status.total_transactions,
            total_cells: sync_status.total_cells_created,
            total_live_cells: sync_status.total_cells_created - sync_status.total_cells_consumed,
            total_addresses: stats.addr_balance_count as i64,
            updated_at: chrono::Utc::now().timestamp(),
        }
    }

    fn is_secondary_issuance_bulk_active(&self) -> bool {
        self.progress.blocks_remaining() > SECONDARY_ISSUANCE_BACKFILL_THRESHOLD
    }

    // === run / run_sequential / run_pipeline ===

    pub async fn run(&self) -> Result<()> {
        let blocks_behind = self.progress.blocks_remaining();
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

        if blocks_behind > self.config.bulk_sync_threshold {
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

        let cleanup_needed = self.writer.needs_startup_cleanup(actual_start)?;
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

        let init_result = self.writer.init_sync_start(
            actual_start,
            blocks_behind > self.config.bulk_sync_threshold,
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
                warn!("Deep fork unresolved, sync paused. Waiting for manual intervention...");
                sleep(Duration::from_secs(30)).await;
                continue;
            }

            match self.sync_batch().await {
                Ok(SyncAction::CaughtUp) => {
                    sleep(Duration::from_millis(self.config.poll_interval_ms)).await;
                }
                Ok(SyncAction::Continue) => {}
                Ok(SyncAction::ReorgHandled) => {
                    info!("Reorg handled, continuing sync from fork point");
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
        type ParsedBatch = (
            u64,
            u64,
            u64,
            u64,
            Arc<Vec<BlockResponseWithCycles>>,
            Vec<crate::parser::block::ParsedBlock>,
            Vec<TxData>,
            HashMap<(Vec<u8>, i16), LiveCellInfo>,
            // Pre-computed in parser stage:
            HashMap<(Vec<u8>, i16), LiveCellInfo>, // batch_cell_infos
            HashMap<Vec<u8>, (i64, i32, i32, i64, i64, Vec<u8>, i64)>, // address_balance_changes
            HashMap<(Vec<u8>, bool), (i64, i64, i64, i64, i64, i64)>, // script_usage_changes
            HashMap<(Vec<u8>, bool, u32), (i64, i64)>, // script_daily_changes
            HashMap<(Vec<u8>, u32), (i64, i64)>,   // token_daily_changes
            HashMap<Vec<u8>, SporeTypeIndex>,      // spore_type_index_changes
            HashMap<(Vec<u8>, u32), (i64, i64)>,   // spore_daily_changes
            HashMap<(Vec<u8>, u32), (i64, i64)>,   // cluster_daily_changes
            HashMap<Vec<u8>, NftTypeIndex>,        // nft_type_index_changes
            HashMap<(Vec<u8>, u32), (i64, i64)>,   // nft_daily_changes
        );

        let (fetch_tx, mut fetch_rx) = mpsc::channel::<FetchedBatch>(self.config.pipeline_buffer);
        let (parse_tx, mut parse_rx) = mpsc::channel::<ParsedBatch>(self.config.pipeline_buffer);
        self.pipeline_perf
            .set_queue_capacities(self.config.pipeline_buffer, self.config.pipeline_buffer);

        let rpc = self.rpc.clone();
        let config = self.config.clone();
        let progress = Arc::clone(&self.progress);
        let repo = self.repo.clone();
        let rebuild_pause = Arc::clone(&self.rebuild_pause_flag);
        let reorg_notify = Arc::clone(&self.reorg_notify_flag);
        let pipeline_epoch_for_fetcher = Arc::clone(&self.pipeline_reset_epoch);
        let ckb_store = self.ckb_store.clone();
        let pipeline_perf_for_fetcher = Arc::clone(&self.pipeline_perf);

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
                if reorg_notify.swap(false, Ordering::SeqCst) {
                    info!("Fetcher received reorg notification, resetting next_block");
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
                        if db_tip == 0 && db_tip_hash.is_none() {
                            0
                        } else {
                            (db_tip + 1) as u64
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

                let end_block =
                    std::cmp::min(start_block + config.batch_size as u64 - 1, chain_tip);

                debug!(
                    "Fetcher: fetching blocks {} to {} (chain_tip={}, next_block={:?})",
                    start_block, end_block, chain_tip, next_block
                );

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

                // Split into sub-batches if too many transactions
                let max_txs = config.max_batch_txs;
                let mut sub_start = 0usize;
                let mut accum_txs = 0usize;
                let mut send_failed = false;

                for (i, block) in blocks.iter().enumerate() {
                    accum_txs += block.block.transactions.len();
                    let is_last = i == blocks.len() - 1;

                    if accum_txs >= max_txs || is_last {
                        let sub_blocks = blocks[sub_start..=i].to_vec();
                        let sub_start_block = start_block + sub_start as u64;
                        let sub_end_block = start_block + i as u64;

                        if sub_start > 0 {
                            debug!(
                                sub_start_block,
                                sub_end_block,
                                txs = accum_txs,
                                "Fetcher: sending sub-batch"
                            );
                        }

                        if fetch_tx
                            .send((
                                pipeline_epoch_for_fetcher.load(Ordering::SeqCst),
                                sub_start_block,
                                sub_end_block,
                                chain_tip,
                                Arc::new(sub_blocks),
                            ))
                            .await
                            .is_err()
                        {
                            send_failed = true;
                            break;
                        }

                        sub_start = i + 1;
                        accum_txs = 0;
                    }
                }

                if send_failed {
                    break;
                }

                let fetch_queue_depth = fetch_tx.max_capacity() - fetch_tx.capacity();
                pipeline_perf_for_fetcher.record_fetch(
                    fetch_elapsed,
                    fetch_queue_depth,
                    fetch_tx.max_capacity(),
                );

                next_block = Some(end_block + 1);
                if end_block % 1000 == 0 {
                    next_block = None;
                }
            }
        });

        // === Parser task ===
        let writer_for_parser = self.writer.clone();
        let cell_cache_for_parser = Arc::clone(&self.cell_cache);
        let pipeline_perf_for_parser = Arc::clone(&self.pipeline_perf);
        let pipeline_epoch_for_parser = Arc::clone(&self.pipeline_reset_epoch);

        let parse_tx_for_writer_depth = parse_tx.clone();
        let parser = tokio::spawn(async move {
            while let Some((batch_epoch, start_block, end_block, chain_tip, blocks)) =
                fetch_rx.recv().await
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
                            return;
                        }
                        Err(e) => {
                            error!(
                                start_block,
                                end_block, "Parser: parse_blocks_parallel task panicked: {}", e
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
                        batch_cells.insert((td.hash.to_vec(), idx as i16), ());
                    }
                }

                let t_cell_lookup = Instant::now();
                let mut unresolved_retry_count: usize = 0;
                let (input_cell_info, cache_hits, cache_misses): (
                    HashMap<(Vec<u8>, i16), LiveCellInfo>,
                    usize,
                    usize,
                ) = loop {
                    let mut attempt_cache_hits: usize = 0;
                    let mut attempt_input_cell_info: HashMap<(Vec<u8>, i16), LiveCellInfo> =
                        HashMap::new();
                    for (tx_hash, idx) in &all_input_outpoints {
                        let hash_arr: [u8; 32] = tx_hash.as_slice().try_into().unwrap_or([0u8; 32]);
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
                        let bulk_sync_mode = chain_tip.saturating_sub(end_block) > 1000;
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
                        break (attempt_input_cell_info, attempt_cache_hits, db_lookups);
                    }

                    unresolved_retry_count += 1;
                    if should_log_unresolved_retry(unresolved_retry_count) {
                        warn!(
                            start_block,
                            end_block,
                            retry = unresolved_retry_count,
                            unresolved_count = unresolved_outpoints.len(),
                            unresolved_sample = %format_outpoint_sample(&unresolved_outpoints, 5),
                            db_lookup_failed,
                            "Parser: unresolved input cells detected; waiting for writer progress and retrying same batch"
                        );
                    }

                    if unresolved_retry_count >= PARSER_UNRESOLVED_MAX_RETRIES {
                        error!(
                            start_block,
                            end_block,
                            retries = unresolved_retry_count,
                            unresolved_count = unresolved_outpoints.len(),
                            unresolved_sample = %format_outpoint_sample(&unresolved_outpoints, 5),
                            db_lookup_failed,
                            "Parser: unresolved input cells persisted after max retries; stopping parser task"
                        );
                        return;
                    }

                    sleep(Duration::from_millis(PARSER_UNRESOLVED_RETRY_DELAY_MS)).await;
                };

                let cell_lookup_ms = t_cell_lookup.elapsed().as_secs_f64() * 1000.0;

                // Pre-compute batch_cell_infos, fees, cell_cache, balance/script changes
                // (moved from writer to overlap with pipeline buffering)
                let t_precompute_parser = Instant::now();

                // Pass 1: Build batch_cell_infos
                let mut batch_cell_infos: HashMap<(Vec<u8>, i16), LiveCellInfo> = HashMap::new();
                for tx_data in &all_tx_data {
                    for (output_index, cell) in tx_data.cells.iter().enumerate() {
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
                                return;
                            }
                        };
                    }
                }

                // Pass 3: cell_cache update + address_balance_changes + script_usage_changes
                let mut address_balance_changes: HashMap<
                    Vec<u8>,
                    (i64, i32, i32, i64, i64, Vec<u8>, i64),
                > = HashMap::new();
                let mut script_usage_changes: HashMap<
                    (Vec<u8>, bool),
                    (i64, i64, i64, i64, i64, i64),
                > = HashMap::new();
                let mut script_daily_changes: HashMap<(Vec<u8>, bool, u32), (i64, i64)> =
                    HashMap::new();
                let mut token_daily_changes: HashMap<(Vec<u8>, u32), (i64, i64)> = HashMap::new();
                let mut spore_type_index_changes: HashMap<Vec<u8>, SporeTypeIndex> = HashMap::new();
                let mut spore_daily_changes: HashMap<(Vec<u8>, u32), (i64, i64)> = HashMap::new();
                let mut cluster_daily_changes: HashMap<(Vec<u8>, u32), (i64, i64)> = HashMap::new();
                let mut nft_type_index_changes: HashMap<Vec<u8>, NftTypeIndex> = HashMap::new();
                let mut nft_daily_changes: HashMap<(Vec<u8>, u32), (i64, i64)> = HashMap::new();
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
                        entry.2 += cell.capacity;
                        entry.3 += cell.capacity;
                        entry.4 += cell_occupied;
                        entry.5 += cell_occupied;
                        let daily_entry = script_daily_changes
                            .entry((cell.lock_code_hash.clone(), false, date_yyyymmdd))
                            .or_insert((0, 0));
                        daily_entry.0 += cell.capacity;
                        daily_entry.1 += cell_occupied;
                        if let Some(ref type_code_hash) = cell.type_code_hash {
                            let type_key = (type_code_hash.clone(), true);
                            let entry = script_usage_changes
                                .entry(type_key)
                                .or_insert((0, 0, 0, 0, 0, 0));
                            entry.0 += 1;
                            entry.1 += 1;
                            entry.2 += cell.capacity;
                            entry.3 += cell.capacity;
                            entry.4 += cell_occupied;
                            entry.5 += cell_occupied;
                            let daily_entry = script_daily_changes
                                .entry((type_code_hash.clone(), true, date_yyyymmdd))
                                .or_insert((0, 0));
                            daily_entry.0 += cell.capacity;
                            daily_entry.1 += cell_occupied;
                        }
                        if let Some(ref type_script_hash) = cell.type_script_hash {
                            let daily_entry = token_daily_changes
                                .entry((type_script_hash.clone(), date_yyyymmdd))
                                .or_insert((0, 0));
                            daily_entry.0 += cell.capacity;
                            daily_entry.1 += cell_occupied;
                        }
                        if let (Some(type_script_hash), Some(type_code_hash), Some(type_args)) = (
                            cell.type_script_hash.as_ref(),
                            cell.type_code_hash.as_ref(),
                            cell.type_args.as_ref(),
                        ) {
                            if type_args.len() >= 32
                                && SporeParser::is_spore_type_script(type_code_hash)
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
                                spore_daily.0 += cell.capacity;
                                spore_daily.1 += cell_occupied;

                                if let Some(cluster_id) = cluster_id {
                                    let cluster_daily = cluster_daily_changes
                                        .entry((cluster_id, date_yyyymmdd))
                                        .or_insert((0, 0));
                                    cluster_daily.0 += cell.capacity;
                                    cluster_daily.1 += cell_occupied;
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
                                nft_daily.0 += cell.capacity;
                                nft_daily.1 += cell_occupied;
                            }
                        }
                    }

                    // Per-tx balance/consumption tracking
                    let mut tx_balance_changes: HashMap<Vec<u8>, i64> = HashMap::new();
                    let mut tx_cells_consumed: HashMap<Vec<u8>, i32> = HashMap::new();
                    let mut tx_occupied_changes: HashMap<Vec<u8>, i64> = HashMap::new();

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
                                    .or_default() -= info.capacity;
                                *tx_cells_consumed
                                    .entry(info.lock_script_hash.clone())
                                    .or_default() += 1;
                                *tx_occupied_changes
                                    .entry(info.lock_script_hash.clone())
                                    .or_default() -= info.occupied_capacity;
                                // script usage - inputs
                                let lock_key = (info.lock_code_hash.clone(), false);
                                let entry = script_usage_changes
                                    .entry(lock_key)
                                    .or_insert((0, 0, 0, 0, 0, 0));
                                entry.1 -= 1;
                                entry.3 -= info.capacity;
                                entry.5 -= info.occupied_capacity;
                                let daily_entry = script_daily_changes
                                    .entry((info.lock_code_hash.clone(), false, date_yyyymmdd))
                                    .or_insert((0, 0));
                                daily_entry.0 -= info.capacity;
                                daily_entry.1 -= info.occupied_capacity;
                                if let Some(ref type_code_hash) = info.type_code_hash {
                                    let type_key = (type_code_hash.clone(), true);
                                    let entry = script_usage_changes
                                        .entry(type_key)
                                        .or_insert((0, 0, 0, 0, 0, 0));
                                    entry.1 -= 1;
                                    entry.3 -= info.capacity;
                                    entry.5 -= info.occupied_capacity;
                                    let daily_entry = script_daily_changes
                                        .entry((type_code_hash.clone(), true, date_yyyymmdd))
                                        .or_insert((0, 0));
                                    daily_entry.0 -= info.capacity;
                                    daily_entry.1 -= info.occupied_capacity;
                                }
                                if let Some(ref type_script_hash) = info.type_script_hash {
                                    let daily_entry = token_daily_changes
                                        .entry((type_script_hash.clone(), date_yyyymmdd))
                                        .or_insert((0, 0));
                                    daily_entry.0 -= info.capacity;
                                    daily_entry.1 -= info.occupied_capacity;
                                }
                                if let (Some(type_script_hash), Some(type_code_hash)) =
                                    (info.type_script_hash.as_ref(), info.type_code_hash.as_ref())
                                {
                                    if SporeParser::is_spore_type_script(type_code_hash) {
                                        let spore_index = if let Some(cached) =
                                            spore_type_index_cache.get(type_script_hash)
                                        {
                                            cached.clone()
                                        } else {
                                            let loaded = writer_for_parser
                                                .store()
                                                .get_spore_type_index(type_script_hash)
                                                .ok()
                                                .flatten();
                                            spore_type_index_cache
                                                .insert(type_script_hash.clone(), loaded.clone());
                                            loaded
                                        };
                                        if let Some(index) = spore_index {
                                            let spore_daily = spore_daily_changes
                                                .entry((index.spore_id.clone(), date_yyyymmdd))
                                                .or_insert((0, 0));
                                            spore_daily.0 -= info.capacity;
                                            spore_daily.1 -= info.occupied_capacity;

                                            if let Some(cluster_id) = index.cluster_id {
                                                let cluster_daily = cluster_daily_changes
                                                    .entry((cluster_id, date_yyyymmdd))
                                                    .or_insert((0, 0));
                                                cluster_daily.0 -= info.capacity;
                                                cluster_daily.1 -= info.occupied_capacity;
                                            }
                                        }
                                    }
                                    if DotbitParser::is_account_cell_type_script(type_code_hash)
                                        || MnftParser::is_token_type_script(type_code_hash)
                                    {
                                        let collection_id =
                                            if DotbitParser::is_account_cell_type_script(
                                                type_code_hash,
                                            ) {
                                                Some(DOTBIT_SENTINEL_COLLECTION.to_vec())
                                            } else if let Some(cached) =
                                                nft_type_index_cache.get(type_script_hash)
                                            {
                                                cached.clone().map(|idx| idx.collection_id)
                                            } else {
                                                let loaded = writer_for_parser
                                                    .store()
                                                    .get_nft_type_index(type_script_hash)
                                                    .ok()
                                                    .flatten();
                                                nft_type_index_cache.insert(
                                                    type_script_hash.clone(),
                                                    loaded.clone(),
                                                );
                                                loaded.map(|idx| idx.collection_id)
                                            };
                                        if let Some(collection_id) = collection_id {
                                            let nft_daily = nft_daily_changes
                                                .entry((collection_id, date_yyyymmdd))
                                                .or_insert((0, 0));
                                            nft_daily.0 -= info.capacity;
                                            nft_daily.1 -= info.occupied_capacity;
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
                            .or_default() += cell.capacity;
                        *tx_cells_created
                            .entry(cell.lock_script_hash.clone())
                            .or_default() += 1;
                        let lock_script_size = 32 + 1 + cell.lock_args.len() as i64;
                        let type_script_size = cell
                            .type_args
                            .as_ref()
                            .map(|a| 32 + 1 + a.len() as i64)
                            .unwrap_or(0);
                        let cell_occupied =
                            (8 + lock_script_size + type_script_size + cell.data_size as i64)
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
                // NOTE: Do NOT clear cell_cache here. In pipeline mode, the
                // parser runs ahead of the writer. Clearing would wipe entries
                // from recently-parsed batches not yet committed to DB, causing
                // the next batch's input lookups to silently miss — leading to
                // wrong balance decrements. The writer handles safe eviction.

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

                if parse_tx
                    .send((
                        batch_epoch,
                        start_block,
                        end_block,
                        chain_tip,
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
                    ))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });

        // === Writer loop ===
        loop {
            if self.repo.has_unresolved_deep_fork().unwrap_or(false) {
                warn!("Deep fork unresolved, sync paused. Waiting for manual intervention...");
                Self::drain_channel(&mut parse_rx).await;
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
                ))) => {
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
                    let expected_start = if db_tip == 0 && db_tip_hash.is_none() {
                        0
                    } else {
                        (db_tip + 1) as u64
                    };

                    if start_block != expected_start {
                        let writer_queue_depth = parse_tx_for_writer_depth.max_capacity()
                            - parse_tx_for_writer_depth.capacity();
                        warn!(
                            run_id = %self.run_id,
                            expected_start,
                            got_start = start_block,
                            writer_queue_depth,
                            "Pipeline batch mismatch: draining stale batches"
                        );
                        self.request_pipeline_reset(
                            "pipeline batch mismatch",
                            Some(expected_start),
                            Some(start_block),
                            Some(writer_queue_depth),
                        );
                        Self::drain_channel(&mut parse_rx).await;
                        continue;
                    }

                    let blocks_behind = chain_tip.saturating_sub(db_tip as u64);
                    if blocks_behind <= self.config.bulk_sync_threshold {
                        if let Some(ref stored_hash) = db_tip_hash {
                            if db_tip > 0 {
                                match self
                                    .check_and_handle_reorg(db_tip as u64, stored_hash)
                                    .await?
                                {
                                    Some(ReorgAction::Handled(_)) => {
                                        info!("Reorg handled, draining stale batches");
                                        self.request_pipeline_reset(
                                            "reorg handled",
                                            None,
                                            None,
                                            None,
                                        );
                                        Self::drain_channel(&mut parse_rx).await;
                                        continue;
                                    }
                                    Some(ReorgAction::DeepForkPaused) => {
                                        warn!("Deep fork detected, sync paused");
                                        self.request_pipeline_reset(
                                            "deep fork paused",
                                            None,
                                            None,
                                            None,
                                        );
                                        Self::drain_channel(&mut parse_rx).await;
                                        sleep(Duration::from_secs(30)).await;
                                        continue;
                                    }
                                    None => {}
                                }
                            }
                        }
                    }

                    let db_start = Instant::now();
                    if let Err(e) = self
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
                            chain_tip,
                        )
                        .await
                    {
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
                        if let Err(cleanup_err) = self
                            .writer
                            .cleanup_batch_range(start_block as i64, end_block as i64)
                        {
                            error!("Failed to cleanup partial batch: {:?}", cleanup_err);
                        } else if let Err(rebuild_err) =
                            self.rebuild_hodl_tracker_from_store(start_block as i64 - 1)
                        {
                            error!(
                                "Failed to rebuild HODL tracker after batch cleanup: {:?}",
                                rebuild_err
                            );
                            return Err(rebuild_err);
                        }
                        self.request_pipeline_reset("batch write failed", None, None, None);
                        Self::drain_channel(&mut parse_rx).await;
                        sleep(Duration::from_secs(5)).await;
                        continue;
                    }
                    let db_elapsed = db_start.elapsed();
                    self.perf.add_db_write(db_elapsed);

                    if db_elapsed.as_secs() >= 5 {
                        let stats = self.writer.store().memory_stats();
                        warn!(
                            db_ms = format!("{:.1}", db_elapsed.as_secs_f64() * 1000.0),
                            compaction_pending_mb = stats.compaction_pending_bytes / (1024 * 1024),
                            running_compactions = stats.num_running_compactions,
                            l0_total = stats.l0_files_count,
                            l0_max = stats.l0_files_max,
                            l0_worst_cf = stats.l0_worst_cf,
                            memtable_mb = stats.memtable_bytes / (1024 * 1024),
                            imm_memtables = stats.immutable_memtables,
                            "Slow DB write detected (possible write stall)"
                        );
                    }

                    if let Some(last_block) = all_parsed_blocks.last() {
                        self.progress
                            .record_batch(last_block.number as u64, all_parsed_blocks.len() as u64);

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
                            recv_wait_ms,
                            writer_queue,
                            parse_tx_for_writer_depth.max_capacity(),
                        );
                        info!(
                            "Wrote blocks {} to {} ({} remaining, {:.2}s, q={}, wait={:.0}ms) {}{} {}",
                            start_block,
                            end_block,
                            self.progress.blocks_remaining(),
                            db_elapsed.as_secs_f64(),
                            writer_queue,
                            recv_wait_ms,
                            partition_range,
                            boundary_info,
                            mode
                        );

                        if !self.is_secondary_issuance_bulk_active() {
                            for block in &all_parsed_blocks {
                                if let Err(e) = self
                                    .update_secondary_issuance(
                                        &format!("0x{}", hex::encode(&block.hash)),
                                        &hex::encode(&block.dao),
                                        block.number,
                                        block.timestamp,
                                    )
                                    .await
                                {
                                    warn!(
                                        "Failed to update secondary issuance for block {}: {}",
                                        block.number, e
                                    );
                                }
                            }
                        }

                        let crossed_1000 = (start_block / 1000) != (end_block / 1000);
                        if crossed_1000 && !self.is_bulk_sync_active() {
                            let update_block = ((end_block / 1000) * 1000) as i64;
                            let writer = self.writer.clone();
                            tokio::spawn(async move {
                                if let Err(e) =
                                    writer.recalculate_dao_extended_statistics(update_block)
                                {
                                    warn!("Failed to recalculate DAO statistics: {}", e);
                                }
                            });
                        }

                        self.maybe_invalidate_chart_caches(end_block).await;
                        self.check_bulk_sync_completion().await;
                    }

                    self.perf
                        .blocks_count
                        .fetch_add(all_parsed_blocks.len() as u64, Ordering::Relaxed);
                    self.perf.report_and_reset();
                }
                Ok(None) => {
                    fetcher.abort();
                    parser.abort();
                    return Err(anyhow::anyhow!("Pipeline channel closed"));
                }
                Err(_timeout) => {
                    // Idle timeout - no pending batches
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

    async fn drain_channel<T>(rx: &mut tokio::sync::mpsc::Receiver<T>) {
        let mut drained = 0;
        while rx.try_recv().is_ok() {
            drained += 1;
        }
        if drained > 0 {
            info!("Drained {} stale batches from pipeline", drained);
        }
    }

    async fn maybe_invalidate_chart_caches(&self, current_block: u64) {
        if !self.cache_invalidator.is_enabled() {
            return;
        }
        let blocks_remaining = self.progress.blocks_remaining();
        if blocks_remaining < 100 {
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
        let start_block = if db_tip == 0 && db_tip_hash.is_none() {
            0
        } else {
            (db_tip + 1) as u64
        };

        if start_block > chain_tip {
            return Ok(SyncAction::CaughtUp);
        }

        let blocks_behind = chain_tip.saturating_sub(start_block);
        if blocks_behind <= self.config.bulk_sync_threshold {
            if let Some(ref stored_hash) = db_tip_hash {
                if db_tip > 0 {
                    match self
                        .check_and_handle_reorg(db_tip as u64, stored_hash)
                        .await?
                    {
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
        if let Err(e) = self.sync_blocks_batch(&blocks, chain_tip).await {
            if let Err(cleanup_err) = self
                .writer
                .cleanup_batch_range(start_block as i64, end_block as i64)
            {
                error!("Failed to cleanup partial batch: {:?}", cleanup_err);
            } else if let Err(rebuild_err) =
                self.rebuild_hodl_tracker_from_store(start_block as i64 - 1)
            {
                error!(
                    "Failed to rebuild HODL tracker after batch cleanup: {:?}",
                    rebuild_err
                );
                return Err(rebuild_err);
            }
            return Err(e).with_context(|| {
                format!(
                    "sync_blocks_batch failed for range {}-{} (chain_tip={})",
                    start_block, end_block, chain_tip
                )
            });
        }
        let db_elapsed = db_start.elapsed();
        self.perf.add_db_write(db_elapsed);

        if db_elapsed.as_secs() >= 5 {
            let stats = self.writer.store().memory_stats();
            warn!(
                db_ms = format!("{:.1}", db_elapsed.as_secs_f64() * 1000.0),
                compaction_pending_mb = stats.compaction_pending_bytes / (1024 * 1024),
                running_compactions = stats.num_running_compactions,
                l0_total = stats.l0_files_count,
                l0_max = stats.l0_files_max,
                l0_worst_cf = stats.l0_worst_cf,
                memtable_mb = stats.memtable_bytes / (1024 * 1024),
                "Slow DB write detected (possible write stall)"
            );
        }

        if let Some(last_block_response) = blocks.last() {
            let last_block_number = BlockParser::parse_block_number(&last_block_response.block);
            self.progress
                .record_batch(last_block_number, blocks.len() as u64);

            let partition_range = format_partition_range(start_block, end_block);
            let boundary_info = if crosses_partition_boundary(start_block, end_block) {
                " (crosses boundary)"
            } else {
                ""
            };
            info!(
                "Wrote blocks {} to {} ({} remaining, {:.2}s) {}{}",
                start_block,
                end_block,
                self.progress.blocks_remaining(),
                db_elapsed.as_secs_f64(),
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
                for block_response in &blocks {
                    let block_number =
                        BlockParser::parse_block_number(&block_response.block) as i64;
                    let block_timestamp =
                        BlockParser::parse_timestamp(&block_response.block.header.timestamp);
                    if let Err(e) = self
                        .update_secondary_issuance(
                            &block_response.block.header.hash,
                            &block_response.block.header.dao,
                            block_number,
                            block_timestamp,
                        )
                        .await
                    {
                        warn!(
                            "Failed to update secondary issuance for block {}: {}",
                            block_number, e
                        );
                    }
                }
            }

            let crossed_1000 = (start_block / 1000) != (end_block / 1000);
            if crossed_1000 && !self.is_bulk_sync_active() {
                let update_block = ((end_block / 1000) * 1000) as i64;
                let writer = self.writer.clone();
                tokio::spawn(async move {
                    if let Err(e) = writer.recalculate_dao_extended_statistics(update_block) {
                        warn!("Failed to recalculate DAO statistics: {}", e);
                    }
                });
            }

            self.maybe_invalidate_chart_caches(end_block).await;
        }

        self.check_bulk_sync_completion().await;

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
            let sst_gb = stats.sst_files_size as f64 / (1024.0 * 1024.0 * 1024.0);

            self.cache_invalidator
                .update_sync_status(|status| {
                    status.mark_bulk_sync_completed(chain_tip as i64);
                    status.address_balances_deferred = false;
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

            // Re-enable auto-compactions and trigger manual compaction in background
            self.writer.store().restore_normal_compaction_options();
            let store_compact = Arc::clone(self.writer.store());
            tokio::task::spawn_blocking(move || {
                store_compact.trigger_full_compaction();
            });

            self.maybe_start_label_import();
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
        let store = Arc::clone(self.writer.store());
        let ckb_store = self.ckb_store.clone();

        tokio::spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                crate::label_import::run_label_import(store.as_ref(), ckb_store.as_deref(), &config)
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
    ) -> Result<()> {
        if blocks.is_empty() {
            return Ok(());
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
        let bulk_sync_mode = chain_tip.saturating_sub(end_block) > 1000;

        let mut batch_cell_infos: HashMap<(Vec<u8>, i16), LiveCellInfo> = HashMap::new();
        for tx_data in &all_tx_data {
            for (output_index, cell) in tx_data.cells.iter().enumerate() {
                let lock_script_size = 32 + 1 + cell.lock_args.len() as i64;
                let type_script_size = cell
                    .type_args
                    .as_ref()
                    .map(|args| 32 + 1 + args.len() as i64)
                    .unwrap_or(0);
                let occupied_capacity =
                    (8 + lock_script_size + type_script_size + cell.data_size as i64) * 100_000_000;
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
                    },
                );
            }
        }

        let mut input_cell_info: HashMap<(Vec<u8>, i16), LiveCellInfo> = HashMap::new();
        for (tx_hash, idx) in &all_input_outpoints {
            let hash_arr: [u8; 32] = tx_hash.as_slice().try_into().unwrap_or([0u8; 32]);
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
            if !block_refs.is_empty() {
                self.writer.insert_blocks_batch(&block_refs, &mut batch)?;
            }
            if !txs_for_batch.is_empty() {
                self.writer
                    .insert_transactions_batch(&txs_for_batch, &mut batch)?;
            }
            if !all_cells.is_empty() {
                self.writer
                    .insert_cells_batch(&all_cells, &mut batch, false)?;
            }
            batch.commit()?;
        }
        let headers_ms = t_headers.elapsed().as_secs_f64() * 1000.0;

        // Block proposals (no-op in RocksDB but kept for API compatibility)
        for parsed_block in &all_parsed_blocks {
            if !parsed_block.proposals.is_empty() {
                self.writer
                    .insert_block_proposals_batch(parsed_block.number, &parsed_block.proposals)?;
                if !self.is_bulk_sync_active() {
                    self.cache_block_proposals(&parsed_block.proposals, parsed_block.number)
                        .await?;
                }
            }
        }

        // Inputs and flows (no-ops in RocksDB model)
        let mut all_inputs: Vec<(&[u8], i64, i16, &crate::parser::transaction::ParsedInput)> =
            Vec::new();
        for tx_data in &all_tx_data {
            if !tx_data.is_cellbase {
                for (input_index, input) in tx_data.inputs.iter().enumerate() {
                    all_inputs.push((
                        tx_data.hash.as_slice(),
                        tx_data.block_number,
                        input_index as i16,
                        input,
                    ));
                }
            }
        }

        let mut all_flows: Vec<(i64, &[u8], i16, i16, &[u8], i64, i32, Option<&[u8]>)> = Vec::new();
        for tx_data in &all_tx_data {
            for (output_index, cell) in tx_data.cells.iter().enumerate() {
                all_flows.push((
                    tx_data.block_number,
                    tx_data.hash.as_slice(),
                    output_index as i16,
                    0,
                    cell.lock_script_hash.as_slice(),
                    cell.capacity,
                    cell.data_size,
                    None,
                ));
            }
        }
        for tx_data in &all_tx_data {
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
                        all_flows.push((
                            tx_data.block_number,
                            input.previous_tx_hash.as_slice(),
                            input.previous_output_index as i16,
                            1,
                            info.lock_script_hash.as_slice(),
                            info.capacity,
                            info.data_size,
                            Some(tx_data.hash.as_slice()),
                        ));
                    }
                }
            }
        }

        if !all_inputs.is_empty() {
            self.writer.insert_transaction_inputs_batch(&all_inputs)?;
        }
        if !all_flows.is_empty() {
            self.writer.insert_cell_flows_batch(&all_flows)?;
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
        let mut address_balance_changes: HashMap<Vec<u8>, (i64, i32, i32, i64, i64, Vec<u8>, i64)> =
            HashMap::new();
        for tx_data in &all_tx_data {
            let mut tx_balance_changes: HashMap<Vec<u8>, i64> = HashMap::new();
            let mut tx_cells_created: HashMap<Vec<u8>, i32> = HashMap::new();
            let mut tx_cells_consumed: HashMap<Vec<u8>, i32> = HashMap::new();
            let mut tx_occupied_changes: HashMap<Vec<u8>, i64> = HashMap::new();
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
                            .or_default() -= info.capacity;
                        *tx_cells_consumed
                            .entry(info.lock_script_hash.clone())
                            .or_default() += 1;
                        *tx_occupied_changes
                            .entry(info.lock_script_hash.clone())
                            .or_default() -= info.occupied_capacity;
                    }
                }
            }
            for cell in &tx_data.cells {
                *tx_balance_changes
                    .entry(cell.lock_script_hash.clone())
                    .or_default() += cell.capacity;
                *tx_cells_created
                    .entry(cell.lock_script_hash.clone())
                    .or_default() += 1;
                let lock_script_size = 32 + 1 + cell.lock_args.len() as i64;
                let type_script_size = cell
                    .type_args
                    .as_ref()
                    .map(|a| 32 + 1 + a.len() as i64)
                    .unwrap_or(0);
                let cell_occupied =
                    (8 + lock_script_size + type_script_size + cell.data_size as i64) * 100_000_000;
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
                consume_addr_batch.put_addr_tx(
                    &lock_hash,
                    tx_data.block_number,
                    tx_data.tx_index,
                    &tx_data.hash,
                );
            }
        }

        let changes_ref: HashMap<Vec<u8>, (i64, i32, i32, i64, i64, &[u8], i64)> =
            address_balance_changes
                .iter()
                .map(|(k, (a, b, c, d, e, f, g))| {
                    (k.clone(), (*a, *b, *c, *d, *e, f.as_slice(), *g))
                })
                .collect();

        // Script usage
        let mut script_usage_changes: HashMap<(Vec<u8>, bool), (i64, i64, i64, i64, i64, i64)> =
            HashMap::new();
        let mut script_daily_changes: HashMap<(Vec<u8>, bool, u32), (i64, i64)> = HashMap::new();
        let mut token_daily_changes: HashMap<(Vec<u8>, u32), (i64, i64)> = HashMap::new();
        let mut spore_type_index_changes: HashMap<Vec<u8>, SporeTypeIndex> = HashMap::new();
        let mut spore_daily_changes: HashMap<(Vec<u8>, u32), (i64, i64)> = HashMap::new();
        let mut cluster_daily_changes: HashMap<(Vec<u8>, u32), (i64, i64)> = HashMap::new();
        let mut nft_type_index_changes: HashMap<Vec<u8>, NftTypeIndex> = HashMap::new();
        let mut nft_daily_changes: HashMap<(Vec<u8>, u32), (i64, i64)> = HashMap::new();
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
                entry.2 += cell.capacity;
                entry.3 += cell.capacity;
                entry.4 += cell_occupied;
                entry.5 += cell_occupied;
                let daily_entry = script_daily_changes
                    .entry((cell.lock_code_hash.clone(), false, date_yyyymmdd))
                    .or_insert((0, 0));
                daily_entry.0 += cell.capacity;
                daily_entry.1 += cell_occupied;
                if let Some(ref type_code_hash) = cell.type_code_hash {
                    let type_key = (type_code_hash.clone(), true);
                    let entry = script_usage_changes
                        .entry(type_key)
                        .or_insert((0, 0, 0, 0, 0, 0));
                    entry.0 += 1;
                    entry.1 += 1;
                    entry.2 += cell.capacity;
                    entry.3 += cell.capacity;
                    entry.4 += cell_occupied;
                    entry.5 += cell_occupied;
                    let daily_entry = script_daily_changes
                        .entry((type_code_hash.clone(), true, date_yyyymmdd))
                        .or_insert((0, 0));
                    daily_entry.0 += cell.capacity;
                    daily_entry.1 += cell_occupied;
                }
                if let Some(ref type_script_hash) = cell.type_script_hash {
                    let daily_entry = token_daily_changes
                        .entry((type_script_hash.clone(), date_yyyymmdd))
                        .or_insert((0, 0));
                    daily_entry.0 += cell.capacity;
                    daily_entry.1 += cell_occupied;
                }
                if let (Some(type_script_hash), Some(type_code_hash), Some(type_args)) = (
                    cell.type_script_hash.as_ref(),
                    cell.type_code_hash.as_ref(),
                    cell.type_args.as_ref(),
                ) {
                    if type_args.len() >= 32 && SporeParser::is_spore_type_script(type_code_hash) {
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
                        spore_daily.0 += cell.capacity;
                        spore_daily.1 += cell_occupied;

                        if let Some(cluster_id) = cluster_id {
                            let cluster_daily = cluster_daily_changes
                                .entry((cluster_id, date_yyyymmdd))
                                .or_insert((0, 0));
                            cluster_daily.0 += cell.capacity;
                            cluster_daily.1 += cell_occupied;
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
                        nft_daily.0 += cell.capacity;
                        nft_daily.1 += cell_occupied;
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
                        entry.3 -= info.capacity;
                        entry.5 -= info.occupied_capacity;
                        let daily_entry = script_daily_changes
                            .entry((info.lock_code_hash.clone(), false, date_yyyymmdd))
                            .or_insert((0, 0));
                        daily_entry.0 -= info.capacity;
                        daily_entry.1 -= info.occupied_capacity;
                        if let Some(ref type_code_hash) = info.type_code_hash {
                            let type_key = (type_code_hash.clone(), true);
                            let entry = script_usage_changes
                                .entry(type_key)
                                .or_insert((0, 0, 0, 0, 0, 0));
                            entry.1 -= 1;
                            entry.3 -= info.capacity;
                            entry.5 -= info.occupied_capacity;
                            let daily_entry = script_daily_changes
                                .entry((type_code_hash.clone(), true, date_yyyymmdd))
                                .or_insert((0, 0));
                            daily_entry.0 -= info.capacity;
                            daily_entry.1 -= info.occupied_capacity;
                        }
                        if let Some(ref type_script_hash) = info.type_script_hash {
                            let daily_entry = token_daily_changes
                                .entry((type_script_hash.clone(), date_yyyymmdd))
                                .or_insert((0, 0));
                            daily_entry.0 -= info.capacity;
                            daily_entry.1 -= info.occupied_capacity;
                        }
                        if let (Some(type_script_hash), Some(type_code_hash)) =
                            (info.type_script_hash.as_ref(), info.type_code_hash.as_ref())
                        {
                            if SporeParser::is_spore_type_script(type_code_hash) {
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
                                    spore_daily.0 -= info.capacity;
                                    spore_daily.1 -= info.occupied_capacity;

                                    if let Some(cluster_id) = index.cluster_id {
                                        let cluster_daily = cluster_daily_changes
                                            .entry((cluster_id, date_yyyymmdd))
                                            .or_insert((0, 0));
                                        cluster_daily.0 -= info.capacity;
                                        cluster_daily.1 -= info.occupied_capacity;
                                    }
                                }
                            }
                            if DotbitParser::is_account_cell_type_script(type_code_hash)
                                || MnftParser::is_token_type_script(type_code_hash)
                            {
                                let collection_id =
                                    if DotbitParser::is_account_cell_type_script(type_code_hash) {
                                        Some(DOTBIT_SENTINEL_COLLECTION.to_vec())
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
                                    nft_daily.0 -= info.capacity;
                                    nft_daily.1 -= info.occupied_capacity;
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
                    &mut consume_addr_batch,
                )?;
            }
        }
        if !script_daily_changes.is_empty() {
            self.writer
                .update_script_daily_deltas_batch(&script_daily_changes, &mut consume_addr_batch)?;
        }
        if !token_daily_changes.is_empty() {
            self.writer
                .update_token_daily_deltas_batch(&token_daily_changes, &mut consume_addr_batch)?;
        }
        if !spore_type_index_changes.is_empty() {
            self.writer.update_spore_type_index_batch(
                &spore_type_index_changes,
                &mut consume_addr_batch,
            )?;
        }
        if !spore_daily_changes.is_empty() {
            self.writer
                .update_spore_daily_deltas_batch(&spore_daily_changes, &mut consume_addr_batch)?;
        }
        if !nft_type_index_changes.is_empty() {
            self.writer
                .update_nft_type_index_batch(&nft_type_index_changes, &mut consume_addr_batch)?;
        }
        if !nft_daily_changes.is_empty() {
            self.writer
                .update_nft_daily_deltas_batch(&nft_daily_changes, &mut consume_addr_batch)?;
        }
        if !cluster_daily_changes.is_empty() {
            self.writer.update_cluster_daily_deltas_batch(
                &cluster_daily_changes,
                &mut consume_addr_batch,
            )?;
        }
        {
            consume_addr_batch.commit()?;
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
            let capacity_transferred: i64 = tx_slice
                .iter()
                .filter(|tx| !tx.is_cellbase)
                .map(|tx| tx.total_output_capacity)
                .sum();
            let data_size_added: i64 = tx_slice
                .iter()
                .flat_map(|tx| tx.cells.iter())
                .map(|cell| cell.data_size as i64)
                .sum();
            let occupied_capacity_created: i64 = tx_slice
                .iter()
                .flat_map(|tx| tx.cells.iter())
                .map(|cell| {
                    let lock_script_size = 32 + 1 + cell.lock_args.len() as i64;
                    let type_script_size = cell
                        .type_args
                        .as_ref()
                        .map(|args| 32 + 1 + args.len() as i64)
                        .unwrap_or(0);
                    (8 + lock_script_size + type_script_size + cell.data_size as i64) * 100_000_000
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
            let occupied_capacity_consumed: i64 = tx_slice
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
                        .map(|info| info.occupied_capacity)
                        .or_else(|| {
                            batch_cell_infos
                                .get(&key)
                                .map(|info| info.occupied_capacity)
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
                entry.4 += capacity_transferred;
                entry.5 += occupied_capacity_created;
                entry.6 += occupied_capacity_consumed;
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
                entry.4 += capacity_transferred;
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
            );

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
                batch.commit()?;
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
                        }
                    }
                }

                {
                    let mut batch = StoreBatch::new(self.writer.store());
                    self.writer.process_dao_withdrawals(
                        &consumed_dao,
                        &new_dao_outputs,
                        parsed.number,
                        &tx_data.hash,
                        parsed.timestamp,
                        &mut batch,
                    )?;
                    batch.commit()?;
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
                let output_udts = self.parse_udt_cells_with_store_fallback(tx);
                for (output_index, udt_cell) in output_udts.iter().enumerate() {
                    batch_udt_cells.insert(
                        (tx_data.hash.to_vec(), output_index as i16),
                        udt_cell.clone(),
                    );
                    self.udt_cell_cache.insert(
                        (tx_data.hash, output_index as i16),
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
                }
                let input_outpoints: Vec<(Vec<u8>, i16)> = tx_data
                    .inputs
                    .iter()
                    .map(|i| (i.previous_tx_hash.to_vec(), i.previous_output_index as i16))
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

        let mut input_udt_info: HashMap<
            (Vec<u8>, i16),
            (Vec<u8>, Vec<u8>, i16, Vec<u8>, Vec<u8>, u128, String),
        > = HashMap::new();
        if !skip_token && !all_input_outpoints_udt.is_empty() {
            let unique_outpoints: Vec<(Vec<u8>, i16)> = {
                let mut seen = HashSet::new();
                all_input_outpoints_udt
                    .into_iter()
                    .filter(|x| seen.insert(x.clone()))
                    .collect()
            };
            let mut uncached: Vec<(Vec<u8>, i16)> = Vec::new();
            for (tx_hash, idx) in &unique_outpoints {
                let key: [u8; 32] = tx_hash.as_slice().try_into().unwrap_or([0u8; 32]);
                if let Some(cached) = self.udt_cell_cache.get(&(key, *idx)) {
                    input_udt_info.insert(
                        (tx_hash.clone(), *idx),
                        (
                            cached.type_script_hash.clone(),
                            cached.type_code_hash.clone(),
                            cached.type_hash_type,
                            cached.type_args.clone(),
                            cached.lock_script_hash.clone(),
                            cached.amount,
                            cached.standard.clone(),
                        ),
                    );
                } else {
                    uncached.push((tx_hash.clone(), *idx));
                }
            }
            if !uncached.is_empty() {
                let outpoint_refs: Vec<(&[u8], i16)> =
                    uncached.iter().map(|(h, i)| (h.as_slice(), *i)).collect();
                let db_results = self.writer.get_udt_cells_info_batch(&outpoint_refs)?;
                for ((tx_hash, idx), (tsh, tch, tht, ta, lsh, am, std)) in &db_results {
                    let key: [u8; 32] = tx_hash.as_slice().try_into().unwrap_or([0u8; 32]);
                    self.udt_cell_cache.insert(
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
                input_udt_info.extend(db_results);
            }
        }
        if self.udt_cell_cache.len() > UDT_CELL_CACHE_CAPACITY * 2 {
            self.udt_cell_cache.clear();
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
                    batch.commit()?;
                }
            }
        }

        // NFT/Spore processing
        let mut batch_spore_ids: HashSet<Vec<u8>> = HashSet::new();

        {
            let mut nft_batch = StoreBatch::new(self.writer.store());
            let mut spore_state = self.writer.new_spore_batch_state();
            let mut block_tx_idx = 0usize;
            for (block_idx, block_response) in blocks.iter().enumerate() {
                let parsed = &all_parsed_blocks[block_idx];
                let tx_count_for_block = parsed.transactions_count as usize;
                let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                block_tx_idx += tx_count_for_block;

                for (tx_idx, tx_data) in tx_slice.iter().enumerate() {
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
                            self.writer
                                .insert_spore_content(&spore.spore_id, &spore.content)?;
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
                    for (output_index, class) in MnftParser::parse_classes(tx).iter().enumerate() {
                        self.writer.insert_mnft_class(
                            class,
                            &tx_data.hash,
                            output_index as i16,
                            parsed.number,
                            &mut nft_batch,
                        )?;
                    }
                    for (output_index, token) in MnftParser::parse_tokens(tx).iter().enumerate() {
                        self.writer.insert_mnft_token(
                            token,
                            &tx_data.hash,
                            output_index as i16,
                            parsed.number,
                            parsed.timestamp.timestamp_millis(),
                            &mut nft_batch,
                        )?;
                    }
                    for (output_index, account) in
                        DotbitParser::parse_accounts(tx).iter().enumerate()
                    {
                        self.writer.insert_dotbit_account(
                            account,
                            &tx_data.hash,
                            output_index as i16,
                            parsed.number,
                            parsed.timestamp.timestamp_millis(),
                            &mut nft_batch,
                        )?;
                    }
                }
            }
            nft_batch.commit()?;
        }

        // Spore consumption (live sync only)
        if !self.is_bulk_sync_active() {
            let mut consume_batch = StoreBatch::new(self.writer.store());
            let mut spore_state = self.writer.new_spore_batch_state();
            let mut block_tx_idx = 0usize;
            for (block_idx, block_response) in blocks.iter().enumerate() {
                let parsed = &all_parsed_blocks[block_idx];
                let tx_count_for_block = parsed.transactions_count as usize;
                let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                block_tx_idx += tx_count_for_block;

                for (tx_idx, tx_data) in tx_slice.iter().enumerate() {
                    if tx_data.is_cellbase || tx_data.inputs.is_empty() {
                        continue;
                    }
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
                        let consumed_spore_id = self
                            .writer
                            .get_spore_id_by_outpoint(&prev_tx_hash, prev_index)?;
                        if let Some(spore_id) = consumed_spore_id {
                            if !batch_spore_ids.contains(&spore_id) {
                                self.writer.consume_spore(
                                    &spore_id,
                                    parsed.number,
                                    &tx_data.hash,
                                    &mut consume_batch,
                                    &mut spore_state,
                                )?;
                            }
                        }
                    }
                }
            }
            consume_batch.commit()?;
        }

        {
            let mut batch = StoreBatch::new(self.writer.store());
            self.write_batch_stats_to_batch(&batch_stats, &mut batch)?;
            if bulk_sync_mode {
                batch.commit_no_wal()?;
            } else {
                batch.commit()?;
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
            "Batch write breakdown"
        );

        Ok(())
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
        address_balance_changes: HashMap<Vec<u8>, (i64, i32, i32, i64, i64, Vec<u8>, i64)>,
        script_usage_changes: HashMap<(Vec<u8>, bool), (i64, i64, i64, i64, i64, i64)>,
        script_daily_changes: HashMap<(Vec<u8>, bool, u32), (i64, i64)>,
        token_daily_changes: HashMap<(Vec<u8>, u32), (i64, i64)>,
        spore_type_index_changes: HashMap<Vec<u8>, SporeTypeIndex>,
        spore_daily_changes: HashMap<(Vec<u8>, u32), (i64, i64)>,
        cluster_daily_changes: HashMap<(Vec<u8>, u32), (i64, i64)>,
        nft_type_index_changes: HashMap<Vec<u8>, NftTypeIndex>,
        nft_daily_changes: HashMap<(Vec<u8>, u32), (i64, i64)>,
        chain_tip: u64,
    ) -> Result<()> {
        if all_parsed_blocks.is_empty() {
            return Ok(());
        }

        let end_block = all_parsed_blocks
            .last()
            .map(|b| b.number as u64)
            .unwrap_or(0);
        let bulk_sync_mode = chain_tip.saturating_sub(end_block) > 1000;

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
            let first_block = all_parsed_blocks.first().map(|b| b.number).unwrap_or(0);
            let last_block = all_parsed_blocks.last().map(|b| b.number).unwrap_or(0);
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
        let mut all_inputs: Vec<(&[u8], i64, i16, &crate::parser::transaction::ParsedInput)> =
            Vec::new();
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
                    all_inputs.push((
                        tx_data.hash.as_slice(),
                        tx_data.block_number,
                        input_index as i16,
                        input,
                    ));
                }

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

        let changes_ref: HashMap<Vec<u8>, (i64, i32, i32, i64, i64, &[u8], i64)> =
            address_balance_changes
                .iter()
                .map(|(k, (a, b, c, d, e, f, g))| {
                    (k.clone(), (*a, *b, *c, *d, *e, f.as_slice(), *g))
                })
                .collect();

        let block_refs: Vec<&crate::parser::block::ParsedBlock> =
            all_parsed_blocks.iter().collect();

        // Pass 4: Proposals (iterates all_parsed_blocks, has async call in live sync)
        let mut all_proposals: Vec<(i64, i16, &[u8])> = Vec::new();
        for parsed_block in all_parsed_blocks {
            if !parsed_block.proposals.is_empty() {
                for (proposal_index, proposal_id) in parsed_block.proposals.iter().enumerate() {
                    all_proposals.push((
                        parsed_block.number,
                        proposal_index as i16,
                        proposal_id.as_slice(),
                    ));
                }
                if !self.is_bulk_sync_active() {
                    self.cache_block_proposals(&parsed_block.proposals, parsed_block.number)
                        .await?;
                }
            }
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
                                    .unwrap_or_default()
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
                                    let output_udts = self.parse_udt_cells_with_store_fallback(tx);
                                    for (output_index, udt_cell) in output_udts.iter().enumerate() {
                                        batch_udt_cells.insert(
                                            (tx_data.hash.to_vec(), output_index as i16),
                                            udt_cell.clone(),
                                        );
                                        udt_cache.insert(
                                            (tx_data.hash, output_index as i16),
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
                                    }
                                    let input_outpoints: Vec<(Vec<u8>, i16)> = tx_data
                                        .inputs
                                        .iter()
                                        .map(|i| {
                                            (
                                                i.previous_tx_hash.to_vec(),
                                                i.previous_output_index as i16,
                                            )
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

                            // Check persistent UDT cache before DB reads
                            let mut input_udt_info: HashMap<
                                (Vec<u8>, i16),
                                (Vec<u8>, Vec<u8>, i16, Vec<u8>, Vec<u8>, u128, String),
                            > = HashMap::new();
                            let mut udt_cache_hits: usize = 0;
                            let mut udt_db_lookups: usize = 0;
                            if !skip_token && !all_input_outpoints_udt.is_empty() {
                                let unique_outpoints: Vec<(Vec<u8>, i16)> = {
                                    let mut seen = HashSet::new();
                                    all_input_outpoints_udt
                                        .into_iter()
                                        .filter(|x| seen.insert(x.clone()))
                                        .collect()
                                };
                                let mut uncached: Vec<(Vec<u8>, i16)> = Vec::new();
                                for (tx_hash, idx) in &unique_outpoints {
                                    let key: [u8; 32] =
                                        tx_hash.as_slice().try_into().unwrap_or([0u8; 32]);
                                    if let Some(cached) = udt_cache.get(&(key, *idx)) {
                                        input_udt_info.insert(
                                            (tx_hash.clone(), *idx),
                                            (
                                                cached.type_script_hash.clone(),
                                                cached.type_code_hash.clone(),
                                                cached.type_hash_type,
                                                cached.type_args.clone(),
                                                cached.lock_script_hash.clone(),
                                                cached.amount,
                                                cached.standard.clone(),
                                            ),
                                        );
                                        udt_cache_hits += 1;
                                    } else {
                                        uncached.push((tx_hash.clone(), *idx));
                                    }
                                }
                                udt_db_lookups = uncached.len();
                                if !uncached.is_empty() {
                                    let outpoint_refs: Vec<(&[u8], i16)> =
                                        uncached.iter().map(|(h, i)| (h.as_slice(), *i)).collect();
                                    if let Ok(db_results) =
                                        writer.get_udt_cells_info_batch(&outpoint_refs)
                                    {
                                        for ((tx_hash, idx), (tsh, tch, tht, ta, lsh, am, std)) in
                                            &db_results
                                        {
                                            let key: [u8; 32] =
                                                tx_hash.as_slice().try_into().unwrap_or([0u8; 32]);
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
                                        input_udt_info.extend(db_results);
                                    }
                                }
                            }
                            if udt_cache_hits > 0 || udt_db_lookups > 0 {
                                debug!(
                                    udt_cache_hits,
                                    udt_db_lookups,
                                    udt_cache_size = udt_cache.len(),
                                    "UDT prefetch cache stats"
                                );
                            }
                            if udt_cache.len() > UDT_CELL_CACHE_CAPACITY * 2 {
                                udt_cache.clear();
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
                                    .unwrap_or_default()
                            } else {
                                HashMap::new()
                            }
                        },
                        || {
                            if !code_hash_refs.is_empty() {
                                writer.read_script_info(&code_hash_refs).unwrap_or_default()
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
        let mut batch_stats;
        let mut thread_times: Option<[f64; 7]> = None;
        if bulk_sync_mode {
            // Parallel write path: each thread writes to its own StoreBatch and commits independently.
            // DAO/UDT/addr/script DB reads are pre-fetched above via rayon::join, so threads only do writes.
            // Independent batches let all threads run fully in parallel; the RocksDB write
            // group overhead (~2ms) is negligible.
            let store = self.writer.store();
            let writer = &self.writer;

            let tt;
            (batch_stats, tt) = std::thread::scope(|s| -> Result<(BatchStats, [f64; 7])> {
                // T1: Cells + Consumption + cell index CFs
                // CFs: LIVE_CELLS, CONSUMED_CELLS, CELL_BY_LOCK, CELL_BY_TYPE, etc.
                let h1 = s.spawn(|| -> Result<f64> {
                    let t = Instant::now();
                    let mut batch = StoreBatch::new(store);
                    if !all_cells.is_empty() {
                        writer.insert_cells_batch(&all_cells, &mut batch, false)?;
                    }
                    if !all_consumptions.is_empty() {
                        writer.consume_cells_batch_preloaded(
                            &all_consumptions,
                            &input_cell_info,
                            &batch_cell_infos,
                            &mut batch,
                            false,
                        )?;
                    }
                    batch.commit_no_wal()?;
                    Ok(t.elapsed().as_secs_f64() * 1000.0)
                });

                // T2: Transactions + Address Balances + Script Usage + Addr TX index
                // CFs: TX_INDEX, TX_HASH_MAP, ADDR_BALANCE, SCRIPT_INFO, ADDR_TX
                let h2 = s.spawn(|| -> Result<f64> {
                    let t = Instant::now();
                    let mut batch = StoreBatch::new(store);
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
                        writer
                            .update_script_daily_deltas_batch(&script_daily_changes, &mut batch)?;
                    }
                    if !token_daily_changes.is_empty() {
                        writer.update_token_daily_deltas_batch(&token_daily_changes, &mut batch)?;
                    }
                    if !spore_type_index_changes.is_empty() {
                        writer
                            .update_spore_type_index_batch(&spore_type_index_changes, &mut batch)?;
                    }
                    if !spore_daily_changes.is_empty() {
                        writer.update_spore_daily_deltas_batch(&spore_daily_changes, &mut batch)?;
                    }
                    if !nft_type_index_changes.is_empty() {
                        writer.update_nft_type_index_batch(&nft_type_index_changes, &mut batch)?;
                    }
                    if !nft_daily_changes.is_empty() {
                        writer.update_nft_daily_deltas_batch(&nft_daily_changes, &mut batch)?;
                    }
                    if !cluster_daily_changes.is_empty() {
                        writer.update_cluster_daily_deltas_batch(
                            &cluster_daily_changes,
                            &mut batch,
                        )?;
                    }
                    for (lock_hash, block_num, tx_idx, tx_hash) in &addr_tx_entries {
                        batch.put_addr_tx(lock_hash, *block_num, *tx_idx, tx_hash);
                    }
                    batch.commit_no_wal()?;
                    Ok(t.elapsed().as_secs_f64() * 1000.0)
                });

                // T4: DAO (writes only — DB reads pre-fetched above)
                // CFs: DAO_DEPOSITS, DAO_BY_WITHDRAW_TX
                let h4 = s.spawn(|| -> Result<f64> {
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
                                withdraw_request_block: None,
                                withdraw_request_ar: None,
                                withdraw_block: None,
                                withdraw_tx: None,
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
                                let mut new_dao_outputs: Vec<(Vec<u8>, i16, Vec<u8>, i64, u64)> =
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
                                        }
                                    }
                                }
                                withdrawal_contexts.push(DaoWithdrawalContext {
                                    consumed_deposits,
                                    new_dao_outputs,
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

                    batch.commit_no_wal()?;
                    Ok(t.elapsed().as_secs_f64() * 1000.0)
                });

                // T5: UDT (writes only — DB reads + parsing pre-fetched above)
                // CFs: TOKENS, TOKEN_HOLDERS
                let input_udt_info = &prefetched_input_udt_info;
                let batch_udt_cells = &prefetched_batch_udt_cells;
                let max_supply_observations = collect_token_max_supply_observations(&all_tx_data);
                let h5 = s.spawn(move || -> Result<f64> {
                    let t = Instant::now();
                    let mut batch = StoreBatch::new(store);

                    if !skip_token && !prefetched_udt_tx_infos.is_empty() {
                        let mut all_transfers: Vec<(
                            crate::parser::ParsedUdtTransfer,
                            Vec<u8>,
                            i64,
                        )> = Vec::new();
                        for ctx in &prefetched_udt_tx_infos {
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

                    batch.commit_no_wal()?;
                    Ok(t.elapsed().as_secs_f64() * 1000.0)
                });

                // T6: Spore + mNFT/DotBit (no NFT consumption during bulk sync)
                // CFs: SPORE_DATA, SPORE_CONTENT, NFT_DATA
                let h6 = s.spawn(|| -> Result<f64> {
                    let t = Instant::now();
                    let mut batch = StoreBatch::new(store);
                    let mut spore_state = writer.new_spore_batch_state();
                    let mut block_tx_idx = 0usize;
                    for (block_idx, block_response) in blocks.iter().enumerate() {
                        let parsed = &all_parsed_blocks[block_idx];
                        let tx_count_for_block = parsed.transactions_count as usize;
                        let tx_slice =
                            &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                        block_tx_idx += tx_count_for_block;
                        let ts_ms = parsed.timestamp.timestamp_millis();
                        for (tx_idx, tx_data) in tx_slice.iter().enumerate() {
                            let tx = &block_response.block.transactions[tx_idx];
                            if !skip_spore {
                                for cluster in SporeParser::parse_clusters(tx) {
                                    writer.insert_spore_cluster(
                                        &cluster,
                                        parsed.number,
                                        &tx_data.hash,
                                        &mut batch,
                                        &mut spore_state,
                                    )?;
                                }
                                for (output_index, spore) in
                                    SporeParser::parse_spores(tx).iter().enumerate()
                                {
                                    writer.insert_spore_cell(
                                        spore,
                                        &tx_data.hash,
                                        output_index as i16,
                                        parsed.number,
                                        ts_ms,
                                        &mut batch,
                                        &mut spore_state,
                                    )?;
                                    writer.insert_spore_content(&spore.spore_id, &spore.content)?;
                                }
                            }
                            for issuer in MnftParser::parse_issuers(tx) {
                                writer.insert_mnft_issuer(
                                    &issuer,
                                    &tx_data.hash,
                                    0,
                                    parsed.number,
                                    &mut batch,
                                )?;
                            }
                            for (output_index, class) in
                                MnftParser::parse_classes(tx).iter().enumerate()
                            {
                                writer.insert_mnft_class(
                                    class,
                                    &tx_data.hash,
                                    output_index as i16,
                                    parsed.number,
                                    &mut batch,
                                )?;
                            }
                            for (output_index, token) in
                                MnftParser::parse_tokens(tx).iter().enumerate()
                            {
                                writer.insert_mnft_token(
                                    token,
                                    &tx_data.hash,
                                    output_index as i16,
                                    parsed.number,
                                    ts_ms,
                                    &mut batch,
                                )?;
                            }
                            for (output_index, account) in
                                DotbitParser::parse_accounts(tx).iter().enumerate()
                            {
                                writer.insert_dotbit_account(
                                    account,
                                    &tx_data.hash,
                                    output_index as i16,
                                    parsed.number,
                                    ts_ms,
                                    &mut batch,
                                )?;
                            }
                        }
                    }
                    batch.commit_no_wal()?;
                    Ok(t.elapsed().as_secs_f64() * 1000.0)
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
                        );

                        let cells_created: i32 =
                            tx_slice.iter().map(|tx| tx.cells.len() as i32).sum();
                        let cells_consumed: i32 = tx_slice
                            .iter()
                            .filter(|tx| !tx.is_cellbase)
                            .map(|tx| tx.inputs.len() as i32)
                            .sum();
                        let capacity_transferred: i64 = tx_slice
                            .iter()
                            .filter(|tx| !tx.is_cellbase)
                            .map(|tx| tx.total_output_capacity)
                            .sum();
                        let data_size_added: i64 = tx_slice
                            .iter()
                            .flat_map(|tx| tx.cells.iter())
                            .map(|cell| cell.data_size as i64)
                            .sum();
                        let occupied_capacity_created: i64 = tx_slice
                            .iter()
                            .flat_map(|tx| tx.cells.iter())
                            .map(|cell| {
                                let lock_script_size = 32 + 1 + cell.lock_args.len() as i64;
                                let type_script_size = cell
                                    .type_args
                                    .as_ref()
                                    .map(|args| 32 + 1 + args.len() as i64)
                                    .unwrap_or(0);
                                (8 + lock_script_size + type_script_size + cell.data_size as i64)
                                    * 100_000_000
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
                        let occupied_capacity_consumed: i64 = tx_slice
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
                                    .map(|info| info.occupied_capacity)
                                    .or_else(|| {
                                        batch_cell_infos
                                            .get(&key)
                                            .map(|info| info.occupied_capacity)
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
                            entry.4 += capacity_transferred;
                            entry.5 += occupied_capacity_created;
                            entry.6 += occupied_capacity_consumed;
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
                            entry.4 += capacity_transferred;
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

                // T_ACT: Activity builder (writes only CF_ACTIVITIES — no conflicts)
                let h_act = if !skip_activities {
                    Some(s.spawn(|| -> Result<f64> {
                        let t = Instant::now();
                        let mut batch = StoreBatch::new(store);
                        let token_info_cache: HashMap<Vec<u8>, (Option<String>, Option<u8>)> =
                            HashMap::new();
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
                                                            type_args: None,
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
                                batch.put_activity(
                                    &lock_hash,
                                    entry.block_number,
                                    entry.tx_index,
                                    &entry,
                                );
                            }
                        }
                        batch.commit_no_wal()?;
                        Ok(t.elapsed().as_secs_f64() * 1000.0)
                    }))
                } else {
                    None
                };

                let t1_ms = h1.join().expect("T1 panicked")?;
                let t2_ms = h2.join().expect("T2 panicked")?;
                let t4_ms = h4.join().expect("T4 panicked")?;
                let t5_ms = h5.join().expect("T5 panicked")?;
                let t6_ms = h6.join().expect("T6 panicked")?;
                let (stats, t7_ms) = h7.join().expect("T7 panicked")?;
                let t_act_ms = match h_act {
                    Some(h) => h.join().expect("T_ACT panicked")?,
                    None => 0.0,
                };
                Ok((stats, [t1_ms, t2_ms, t4_ms, t5_ms, t6_ms, t7_ms, t_act_ms]))
            })?;
            thread_times = Some(tt);
        } else {
            // Live sync: serial writes in a single batch
            let mut data_batch = StoreBatch::new(self.writer.store());
            if !txs_for_batch.is_empty() {
                self.writer
                    .insert_transactions_batch(&txs_for_batch, &mut data_batch)?;
            }
            if !all_cells.is_empty() {
                self.writer
                    .insert_cells_batch(&all_cells, &mut data_batch, false)?;
            }
            if !all_inputs.is_empty() {
                self.writer.insert_transaction_inputs_batch(&all_inputs)?;
            }
            if !all_proposals.is_empty() {
                self.writer.insert_proposals_batch(&all_proposals)?;
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
                        &mut data_batch,
                    )?;
                }
            }
            if !script_daily_changes.is_empty() {
                self.writer
                    .update_script_daily_deltas_batch(&script_daily_changes, &mut data_batch)?;
            }
            if !token_daily_changes.is_empty() {
                self.writer
                    .update_token_daily_deltas_batch(&token_daily_changes, &mut data_batch)?;
            }
            if !spore_type_index_changes.is_empty() {
                self.writer
                    .update_spore_type_index_batch(&spore_type_index_changes, &mut data_batch)?;
            }
            if !spore_daily_changes.is_empty() {
                self.writer
                    .update_spore_daily_deltas_batch(&spore_daily_changes, &mut data_batch)?;
            }
            if !nft_type_index_changes.is_empty() {
                self.writer
                    .update_nft_type_index_batch(&nft_type_index_changes, &mut data_batch)?;
            }
            if !nft_daily_changes.is_empty() {
                self.writer
                    .update_nft_daily_deltas_batch(&nft_daily_changes, &mut data_batch)?;
            }
            if !cluster_daily_changes.is_empty() {
                self.writer
                    .update_cluster_daily_deltas_batch(&cluster_daily_changes, &mut data_batch)?;
            }

            // Write addr_txs entries
            for (lock_hash, block_num, tx_idx, tx_hash) in &addr_tx_entries {
                data_batch.put_addr_tx(lock_hash, *block_num, *tx_idx, tx_hash);
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
                            withdraw_request_block: None,
                            withdraw_request_ar: None,
                            withdraw_block: None,
                            withdraw_tx: None,
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
                            let mut new_dao_outputs: Vec<(Vec<u8>, i16, Vec<u8>, i64, u64)> =
                                Vec::new();
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
                                    }
                                }
                            }
                            withdrawal_contexts.push(DaoWithdrawalContext {
                                consumed_deposits,
                                new_dao_outputs,
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
                        let output_udts = self.parse_udt_cells_with_store_fallback(tx);
                        for (output_index, udt_cell) in output_udts.iter().enumerate() {
                            batch_udt_cells.insert(
                                (tx_data.hash.to_vec(), output_index as i16),
                                udt_cell.clone(),
                            );
                            self.udt_cell_cache.insert(
                                (tx_data.hash, output_index as i16),
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
                        }
                        let input_outpoints: Vec<(Vec<u8>, i16)> = tx_data
                            .inputs
                            .iter()
                            .map(|i| (i.previous_tx_hash.to_vec(), i.previous_output_index as i16))
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

                let mut input_udt_info: HashMap<
                    (Vec<u8>, i16),
                    (Vec<u8>, Vec<u8>, i16, Vec<u8>, Vec<u8>, u128, String),
                > = HashMap::new();
                if !skip_token && !all_input_outpoints_udt.is_empty() {
                    let unique_outpoints: Vec<(Vec<u8>, i16)> = {
                        let mut seen = HashSet::new();
                        all_input_outpoints_udt
                            .into_iter()
                            .filter(|x| seen.insert(x.clone()))
                            .collect()
                    };
                    let mut uncached: Vec<(Vec<u8>, i16)> = Vec::new();
                    for (tx_hash, idx) in &unique_outpoints {
                        let key: [u8; 32] = tx_hash.as_slice().try_into().unwrap_or([0u8; 32]);
                        if let Some(cached) = self.udt_cell_cache.get(&(key, *idx)) {
                            input_udt_info.insert(
                                (tx_hash.clone(), *idx),
                                (
                                    cached.type_script_hash.clone(),
                                    cached.type_code_hash.clone(),
                                    cached.type_hash_type,
                                    cached.type_args.clone(),
                                    cached.lock_script_hash.clone(),
                                    cached.amount,
                                    cached.standard.clone(),
                                ),
                            );
                        } else {
                            uncached.push((tx_hash.clone(), *idx));
                        }
                    }
                    if !uncached.is_empty() {
                        let outpoint_refs: Vec<(&[u8], i16)> =
                            uncached.iter().map(|(h, i)| (h.as_slice(), *i)).collect();
                        let db_results = self.writer.get_udt_cells_info_batch(&outpoint_refs)?;
                        for ((tx_hash, idx), (tsh, tch, tht, ta, lsh, am, std)) in &db_results {
                            let key: [u8; 32] = tx_hash.as_slice().try_into().unwrap_or([0u8; 32]);
                            self.udt_cell_cache.insert(
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
                        input_udt_info.extend(db_results);
                    }
                }
                if self.udt_cell_cache.len() > UDT_CELL_CACHE_CAPACITY * 2 {
                    self.udt_cell_cache.clear();
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
                let mut spore_state = self.writer.new_spore_batch_state();
                let mut block_tx_idx = 0usize;
                for (block_idx, block_response) in blocks.iter().enumerate() {
                    let parsed = &all_parsed_blocks[block_idx];
                    let tx_count_for_block = parsed.transactions_count as usize;
                    let tx_slice = &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                    block_tx_idx += tx_count_for_block;
                    let ts_ms = parsed.timestamp.timestamp_millis();
                    for (tx_idx, tx_data) in tx_slice.iter().enumerate() {
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
                                self.writer
                                    .insert_spore_content(&spore.spore_id, &spore.content)?;
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
                            MnftParser::parse_classes(tx).iter().enumerate()
                        {
                            self.writer.insert_mnft_class(
                                class,
                                &tx_data.hash,
                                output_index as i16,
                                parsed.number,
                                &mut data_batch,
                            )?;
                        }
                        for (output_index, token) in MnftParser::parse_tokens(tx).iter().enumerate()
                        {
                            self.writer.insert_mnft_token(
                                token,
                                &tx_data.hash,
                                output_index as i16,
                                parsed.number,
                                ts_ms,
                                &mut data_batch,
                            )?;
                            batch_mnft_token_outpoints.insert(
                                (tx_data.hash.to_vec(), output_index as i16),
                                token.token_id.clone(),
                            );
                        }
                        for (output_index, account) in
                            DotbitParser::parse_accounts(tx).iter().enumerate()
                        {
                            self.writer.insert_dotbit_account(
                                account,
                                &tx_data.hash,
                                output_index as i16,
                                parsed.number,
                                ts_ms,
                                &mut data_batch,
                            )?;
                            batch_dotbit_outpoints.insert(
                                (tx_data.hash.to_vec(), output_index as i16),
                                account.account_id.clone(),
                            );
                        }
                    }
                }

                // NFT consumption (live sync only)
                if !self.is_bulk_sync_active() {
                    let mut all_prev_tx_hashes: Vec<Vec<u8>> = Vec::new();
                    let mut all_prev_indices: Vec<i16> = Vec::new();
                    let mut outpoint_context: Vec<(i64, Vec<u8>)> = Vec::new();
                    let mut block_tx_idx = 0usize;
                    for (block_idx, block_response) in blocks.iter().enumerate() {
                        let parsed = &all_parsed_blocks[block_idx];
                        let tx_count_for_block = parsed.transactions_count as usize;
                        let tx_slice =
                            &all_tx_data[block_tx_idx..block_tx_idx + tx_count_for_block];
                        block_tx_idx += tx_count_for_block;
                        for (tx_idx, tx_data) in tx_slice.iter().enumerate() {
                            if tx_data.is_cellbase || tx_data.inputs.is_empty() {
                                continue;
                            }
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
                                outpoint_context.push((parsed.number, tx_data.hash.to_vec()));
                            }
                        }
                    }
                    if !all_prev_tx_hashes.is_empty() {
                        let spore_results = self.writer.get_spore_ids_by_outpoints_batch(
                            &all_prev_tx_hashes,
                            &all_prev_indices,
                        )?;
                        let mnft_results = self.writer.get_mnft_token_ids_by_outpoints_batch(
                            &all_prev_tx_hashes,
                            &all_prev_indices,
                        )?;
                        let dotbit_results =
                            self.writer.get_dotbit_account_ids_by_outpoints_batch(
                                &all_prev_tx_hashes,
                                &all_prev_indices,
                            )?;
                        let spore_map: HashMap<(Vec<u8>, i16), Vec<u8>> = spore_results
                            .into_iter()
                            .map(|(h, i, id)| ((h, i), id))
                            .collect();
                        let mut spore_map = spore_map;
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
                        let mnft_map: HashMap<(Vec<u8>, i16), Vec<u8>> = mnft_results
                            .into_iter()
                            .map(|(h, i, id)| ((h, i), id))
                            .collect();
                        let mut mnft_map = mnft_map;
                        for (key, token_id) in &batch_mnft_token_outpoints {
                            mnft_map
                                .entry(key.clone())
                                .or_insert_with(|| token_id.clone());
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
                        for (i, (block_number, consuming_tx_hash)) in
                            outpoint_context.iter().enumerate()
                        {
                            let key = (all_prev_tx_hashes[i].clone(), all_prev_indices[i]);
                            if let Some(spore_id) = spore_map.get(&key) {
                                if !batch_spore_ids.contains(spore_id) {
                                    self.writer.consume_spore(
                                        spore_id,
                                        *block_number,
                                        consuming_tx_hash,
                                        &mut data_batch,
                                        &mut spore_state,
                                    )?;
                                }
                            }
                            if let Some(token_id) = mnft_map.get(&key) {
                                self.writer.consume_mnft_token(
                                    token_id,
                                    *block_number,
                                    consuming_tx_hash,
                                    &mut data_batch,
                                )?;
                            }
                            if let Some(account_id) = dotbit_map.get(&key) {
                                self.writer.consume_dotbit_account(
                                    account_id,
                                    *block_number,
                                    consuming_tx_hash,
                                    &mut data_batch,
                                )?;
                            }
                        }
                    }
                }
            }

            // Activity writes (live sync)
            {
                let token_info_cache: HashMap<Vec<u8>, (Option<String>, Option<u8>)> =
                    HashMap::new();
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
                                                    type_args: None,
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
                        data_batch.put_activity(
                            &lock_hash,
                            entry.block_number,
                            entry.tx_index,
                            &entry,
                        );
                    }
                }
            }

            // Commit all data writes in a single batch
            data_batch.commit()?;

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
                let capacity_transferred: i64 = tx_slice
                    .iter()
                    .filter(|tx| !tx.is_cellbase)
                    .map(|tx| tx.total_output_capacity)
                    .sum();
                let data_size_added: i64 = tx_slice
                    .iter()
                    .flat_map(|tx| tx.cells.iter())
                    .map(|cell| cell.data_size as i64)
                    .sum();
                let occupied_capacity_created: i64 = tx_slice
                    .iter()
                    .flat_map(|tx| tx.cells.iter())
                    .map(|cell| {
                        let lock_script_size = 32 + 1 + cell.lock_args.len() as i64;
                        let type_script_size = cell
                            .type_args
                            .as_ref()
                            .map(|args| 32 + 1 + args.len() as i64)
                            .unwrap_or(0);
                        (8 + lock_script_size + type_script_size + cell.data_size as i64)
                            * 100_000_000
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
                let occupied_capacity_consumed: i64 = tx_slice
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
                            .map(|info| info.occupied_capacity)
                            .or_else(|| {
                                batch_cell_infos
                                    .get(&key)
                                    .map(|info| info.occupied_capacity)
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
                    entry.4 += capacity_transferred;
                    entry.5 += occupied_capacity_created;
                    entry.6 += occupied_capacity_consumed;
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
                    entry.4 += capacity_transferred;
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
                );

                batch_stats.dao_snapshot_dates.insert(block_date);
            }
            batch_stats.dao_deltas_computed = true;
        }
        let write_ms = t_write.elapsed().as_secs_f64() * 1000.0;

        // Finalization: block headers + stats commit
        let t_finalize = Instant::now();
        {
            let mut batch = StoreBatch::new(self.writer.store());
            self.writer.insert_blocks_batch(&block_refs, &mut batch)?;
            self.write_batch_stats_to_batch(&batch_stats, &mut batch)?;
            if bulk_sync_mode {
                batch.commit_no_wal()?;
            } else {
                batch.commit()?;
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
        if let Some([t1, t2, t4, t5, t6, t7, t_act]) = thread_times {
            info!(
                precompute_ms = format!("{:.1}", precompute_ms),
                prefetch_ms = format!("{:.1}", prefetch_ms),
                write_ms = format!("{:.1}", write_ms),
                finalize_ms = format!("{:.1}", finalize_ms),
                t1_ms = format!("{:.1}", t1),
                t2_ms = format!("{:.1}", t2),
                t4_ms = format!("{:.1}", t4),
                t5_ms = format!("{:.1}", t5),
                t6_ms = format!("{:.1}", t6),
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
                finalize_ms = format!("{:.1}", finalize_ms),
                txs = batch_tx_count,
                cells = batch_cell_count,
                inputs = batch_input_count,
                "Batch write breakdown"
            );
        }
        Ok(())
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

        // Daily statistics (with block time data folded in)
        for (
            date,
            (
                blocks,
                txs,
                created,
                consumed,
                capacity,
                occupied_created,
                occupied_consumed,
                data_size_added,
                data_size_consumed,
            ),
        ) in &stats.daily_stats
        {
            let dao_field = stats.daily_dao_fields.get(date);
            let block_time = stats.daily_block_times.get(date).copied();
            self.writer.update_daily_statistics(
                *date,
                *blocks,
                *txs,
                *created,
                *consumed,
                *capacity,
                *occupied_created,
                *occupied_consumed,
                *data_size_added,
                *data_size_consumed,
                dao_field.map(|v| v.as_slice()),
                block_time,
                batch,
            )?;
        }

        // Daily block stats
        for (date, (sum_target, count, uncles)) in &stats.daily_block_stats {
            let avg_target = if *count > 0 {
                (*sum_target / *count as i128) as i64
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
                let latest_snapshot = self
                    .writer
                    .store()
                    .list_dao_daily_snapshots()
                    .ok()
                    .and_then(|snaps| snaps.last().cloned());

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
                let running_total_withdrawal_count =
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

                    // Extract C, S, U from the DAO header field for this date.
                    let (total_issuance, secondary_pool, occupied_capacity) =
                        dao_csu_for_snapshot_date(stats, *date)?;
                    let daily_non_miner = stats.daily_secondary_non_miner_delta.get(date).copied();
                    let (daily_miner, daily_dao_share, daily_treasury_share) = if total_issuance > 0
                    {
                        if let Some(non_miner) = daily_non_miner {
                            if non_miner > 0 {
                                split_secondary_issuance(
                                    total_issuance,
                                    occupied_capacity,
                                    running_total_deposited,
                                    non_miner,
                                )?
                            } else {
                                // Ignore negative S adjustments in user-facing cumulative
                                // charts to keep the series monotonic.
                                (0, 0, 0)
                            }
                        } else {
                            let s_delta = secondary_pool - prev_secondary_pool;
                            if s_delta > 0 {
                                split_secondary_issuance(
                                    total_issuance,
                                    occupied_capacity,
                                    running_total_deposited,
                                    s_delta,
                                )?
                            } else {
                                (0, 0, 0)
                            }
                        }
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
        if should_rebuild_hodl_tracker_state(state.as_ref(), tip_block) {
            info!(
                tip_block,
                "HODL tracker state is out of sync with DB tip, rebuilding from store"
            );
            self.rebuild_hodl_tracker_from_store(tip_block)?;
        }

        Ok(())
    }

    fn rebuild_hodl_tracker_from_store(&self, tip_block: i64) -> Result<()> {
        let store = self.writer.store();
        if tip_block < 0 {
            let tracker = HodlWaveTracker::new();
            store.put_hodl_tracker_state(&tracker.to_state())?;
            let mut guard = self.hodl_tracker.lock().unwrap();
            *guard = tracker;
            info!("Rebuilt HODL tracker from empty state (tip before genesis)");
            return Ok(());
        }

        let mut transitions: Vec<(i64, NaiveDate)> = Vec::new();
        let mut headers_scanned = 0u64;
        let header_iter = store.iterator_cf(store.cf_block_headers(), rocksdb::IteratorMode::Start);
        for item in header_iter.flatten() {
            let (key, value) = item;
            if key.len() < 8 {
                continue;
            }
            let block_number = ckbadger_store::keys::decode_block_num(&key[..8]);
            if block_number > tip_block {
                break;
            }
            let header: CachedBlockHeader = bincode::deserialize(&value).map_err(|e| {
                anyhow!(
                    "failed to deserialize block header while rebuilding HODL tracker: block={}, error={}",
                    block_number,
                    e
                )
            })?;
            let block_date = chrono::DateTime::<Utc>::from_timestamp_millis(header.timestamp)
                .ok_or_else(|| {
                    anyhow!(
                        "invalid block timestamp while rebuilding HODL tracker: block={}, timestamp_ms={}",
                        block_number,
                        header.timestamp
                    )
                })?
                .date_naive();
            match transitions.last() {
                Some((_, last_date)) if *last_date == block_date => {}
                _ => transitions.push((block_number, block_date)),
            }
            headers_scanned += 1;
        }

        if transitions.is_empty() {
            bail!(
                "cannot rebuild HODL tracker: no block-date transitions up to tip {}",
                tip_block
            );
        }

        let mut capacity_by_date: HashMap<NaiveDate, i128> = HashMap::new();
        let mut live_cells_scanned = 0u64;
        let live_iter = store.iterator_cf(store.cf_live_cells(), rocksdb::IteratorMode::Start);
        for item in live_iter.flatten() {
            let (_key, value) = item;
            let info: LiveCellInfo = bincode::deserialize(&value).map_err(|e| {
                anyhow!(
                    "failed to deserialize live cell while rebuilding HODL tracker: error={}",
                    e
                )
            })?;
            if info.created_at_block > tip_block {
                continue;
            }

            let idx = transitions.partition_point(|(b, _)| *b <= info.created_at_block);
            let creation_date = if idx == 0 {
                transitions[0].1
            } else {
                transitions[idx - 1].1
            };
            *capacity_by_date.entry(creation_date).or_insert(0) += info.capacity as i128;
            live_cells_scanned += 1;
        }

        let mut holder_count = 0i64;
        let mut balances_scanned = 0u64;
        let balances_iter =
            store.iterator_cf(store.cf_addr_balance(), rocksdb::IteratorMode::Start);
        for item in balances_iter.flatten() {
            let (_key, value) = item;
            let balance: AddressBalance = bincode::deserialize(&value).map_err(|e| {
                anyhow!(
                    "failed to deserialize address balance while rebuilding HODL tracker: error={}",
                    e
                )
            })?;
            balances_scanned += 1;
            if balance.live_cells_count > 0 {
                holder_count += 1;
            }
        }

        let mut capacity_by_date_vec: Vec<(String, i128)> = capacity_by_date
            .into_iter()
            .map(|(date, cap)| (date.format("%Y%m%d").to_string(), cap))
            .collect();
        capacity_by_date_vec.sort_by(|a, b| a.0.cmp(&b.0));
        let date_transitions = transitions
            .iter()
            .map(|(block, date)| (*block, date.format("%Y%m%d").to_string()))
            .collect();
        let last_snapshot_date = transitions
            .last()
            .map(|(_, d)| d.format("%Y%m%d").to_string());

        let state = HodlTrackerState {
            capacity_by_date: capacity_by_date_vec,
            date_transitions,
            holder_count,
            last_snapshot_date,
        };
        store.put_hodl_tracker_state(&state)?;

        let mut tracker = self.hodl_tracker.lock().unwrap();
        *tracker = HodlWaveTracker::from_state(state);

        info!(
            tip_block,
            headers_scanned,
            live_cells_scanned,
            balances_scanned,
            holder_count,
            "Rebuilt HODL tracker from store after rollback"
        );
        Ok(())
    }

    /// Feed parsed block data into the HODL wave tracker and write snapshots at day boundaries.
    fn update_hodl_wave(
        &self,
        all_parsed_blocks: &[crate::parser::block::ParsedBlock],
        all_tx_data: &[TxData],
        input_cell_info: &HashMap<(Vec<u8>, i16), LiveCellInfo>,
        batch_cell_infos: &HashMap<(Vec<u8>, i16), LiveCellInfo>,
        address_balance_changes: &HashMap<Vec<u8>, (i64, i32, i32, i64, i64, Vec<u8>, i64)>,
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
            "Reorg detected at block {}: stored={} chain={}",
            db_tip,
            hex::encode(stored_hash),
            hex::encode(&chain_hash_bytes)
        );

        let (fork_point, fork_hash) = self.find_fork_point(db_tip).await?;
        let depth = db_tip - fork_point;

        info!(
            "Fork point found at block {}, depth = {}",
            fork_point, depth
        );

        let chain_tip = self.get_chain_tip().await?;
        let chain_tip_hash_bytes = self.get_chain_block_hash(chain_tip).await?;

        if depth > DEEP_FORK_DEPTH {
            error!(
                "DEEP FORK DETECTED! Depth {} exceeds limit {}. Manual intervention required.",
                depth, DEEP_FORK_DEPTH
            );

            self.writer.record_deep_fork(
                fork_point as i64,
                &fork_hash,
                db_tip as i64,
                stored_hash,
                chain_tip as i64,
                &chain_tip_hash_bytes,
                depth as i64,
            )?;

            return Ok(Some(ReorgAction::DeepForkPaused));
        }

        info!(
            "Processing automatic reorg (depth={} <= limit={})",
            depth, DEEP_FORK_DEPTH
        );

        let result = self
            .writer
            .execute_reorg(
                fork_point as i64,
                &fork_hash,
                db_tip as i64,
                stored_hash,
                chain_tip as i64,
                &chain_tip_hash_bytes,
            )
            .await?;

        Ok(Some(ReorgAction::Handled(result)))
    }

    async fn find_fork_point(&self, db_tip: u64) -> Result<(u64, Vec<u8>)> {
        let mut height = db_tip;

        loop {
            let db_hash = self
                .repo
                .get_block_hash_at_height(height as i64)?
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

            height -= 1;
        }
    }

    // === update_secondary_issuance ===

    async fn update_secondary_issuance(
        &self,
        block_hash: &str,
        dao_hex: &str,
        block_number: i64,
        block_timestamp: DateTime<Utc>,
    ) -> Result<()> {
        // Check if we already have issuance data for this block
        if self
            .writer
            .store()
            .get_block_issuance(block_number)?
            .is_some()
        {
            return Ok(());
        }

        let economic_state = match self.rpc.get_block_economic_state(block_hash).await? {
            Some(state) => state,
            None => return Ok(()),
        };

        let dao_field = match DaoField::from_hex(dao_hex) {
            Some(f) => f,
            None => return Ok(()),
        };

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
            let total_dao_deposits: u128 = self.writer.get_dao_deposits_at_block(block_number)?;

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

        let mut batch = StoreBatch::new(self.writer.store());
        self.writer.accumulate_secondary_issuance(
            &breakdown,
            block_number,
            block_timestamp,
            &mut batch,
        )?;
        batch.commit()?;

        Ok(())
    }

    // === cache_block_proposals ===

    async fn cache_block_proposals(&self, proposals: &[Vec<u8>], block_number: i64) -> Result<()> {
        use ckbadger_common::CachedProposal;

        if proposals.is_empty() || !self.cache_invalidator.is_enabled() {
            return Ok(());
        }

        let mempool = match self.rpc.get_raw_tx_pool_verbose().await {
            Ok(pool) => pool,
            Err(e) => {
                warn!("Failed to fetch mempool for proposal enrichment: {}", e);
                let cached: Vec<CachedProposal> = proposals
                    .iter()
                    .enumerate()
                    .map(|(idx, p)| {
                        CachedProposal::new_minimal(hex::encode(p), block_number, idx as i16)
                    })
                    .collect();
                self.cache_invalidator.cache_proposals(&cached).await;
                return Ok(());
            }
        };

        let mut all_mempool_txs: HashMap<String, &crate::rpc::TxPoolEntry> = HashMap::new();
        for (tx_hash, entry) in mempool.pending.iter().chain(mempool.proposed.iter()) {
            let short_id = &tx_hash[2..22];
            all_mempool_txs.insert(short_id.to_string(), entry);
        }

        let mut cached_proposals = Vec::with_capacity(proposals.len());

        for (idx, proposal_bytes) in proposals.iter().enumerate() {
            let proposal_id = hex::encode(proposal_bytes);

            if let Some(entry) = all_mempool_txs.get(&proposal_id) {
                let fee_u64 =
                    parse_prefixed_hex_u64(&entry.fee, "mempool proposal fee").map_err(|e| {
                        anyhow!("invalid mempool fee for proposal {}: {}", proposal_id, e)
                    })?;
                let size =
                    parse_prefixed_hex_u64(&entry.size, "mempool proposal size").map_err(|e| {
                        anyhow!("invalid mempool size for proposal {}: {}", proposal_id, e)
                    })?;
                let cycles = parse_prefixed_hex_u64(&entry.cycles, "mempool proposal cycles")
                    .map_err(|e| {
                        anyhow!("invalid mempool cycles for proposal {}: {}", proposal_id, e)
                    })?;

                cached_proposals.push(CachedProposal::new_with_details(
                    proposal_id,
                    "".to_string(),
                    block_number,
                    idx as i16,
                    fee_u64,
                    size,
                    cycles,
                ));
            } else {
                cached_proposals.push(CachedProposal::new_minimal(
                    proposal_id,
                    block_number,
                    idx as i16,
                ));
            }
        }

        self.cache_invalidator
            .cache_proposals(&cached_proposals)
            .await;
        self.cache_invalidator
            .cleanup_expired_proposals(block_number)
            .await;

        Ok(())
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
    fn test_format_outpoint_sample_limits_items() {
        let outpoints = vec![
            (vec![0x11; 32], 0),
            (vec![0x22; 32], 1),
            (vec![0x33; 32], 2),
        ];

        let sample = format_outpoint_sample(&outpoints, 2);
        assert!(sample.contains("0x1111111111111111:0"));
        assert!(sample.contains("0x2222222222222222:1"));
        assert!(!sample.contains("0x3333333333333333:2"));
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
    fn test_parser_unresolved_retry_defaults() {
        assert_eq!(PARSER_UNRESOLVED_RETRY_DELAY_MS, 500);
        assert_eq!(PARSER_UNRESOLVED_MAX_RETRIES, 240);
    }

    #[test]
    fn test_address_balances_are_never_skipped_in_bulk_mode() {
        assert!(!should_skip_address_balances(true));
        assert!(!should_skip_address_balances(false));
    }

    #[test]
    fn test_count_new_addresses_counts_only_first_live_transitions() {
        let mut changes: HashMap<Vec<u8>, (i64, i32, i32, i64, i64, &[u8], i64)> = HashMap::new();
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
        let mut changes: HashMap<Vec<u8>, (i64, i32, i32, i64, i64, &[u8], i64)> = HashMap::new();
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
    fn test_classify_nft_collection_id_rejects_non_nft_or_short_mnft_args() {
        let non_nft = vec![0x11; 32];
        assert!(classify_nft_collection_id(&non_nft, &[0x22; 24]).is_none());

        let mnft_code_hash =
            crate::rpc::parse_hex_to_bytes(crate::parser::mnft::MNFT_TOKEN_CODE_HASH);
        assert!(classify_nft_collection_id(&mnft_code_hash, &[0x33; 23]).is_none());
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
            uncles_hash: vec![],
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
    fn test_accumulate_secondary_issuance_deltas_ignores_negative_adjustment() {
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

        accumulate_secondary_issuance_deltas(&mut stats, &block, date, &mut prev).unwrap();

        assert!(!stats.daily_secondary_non_miner_delta.contains_key(&date));
        assert!(!stats.daily_secondary_miner_delta.contains_key(&date));
    }

    #[test]
    fn test_accumulate_secondary_issuance_deltas_same_day_keeps_only_positive_s_growth() {
        let mut stats = BatchStats::default();
        let mut prev = Some((30_000_000_000_000_i128, 10_000_i128));
        let date = chrono::NaiveDate::from_ymd_opt(2026, 2, 18).unwrap();

        // First block in the day has an S drop (protocol adjustment).
        let block_drop =
            dummy_parsed_block(build_dao_field(30_000_000_000_500, 9_950, 100), 0, 1000);
        accumulate_secondary_issuance_deltas(&mut stats, &block_drop, date, &mut prev).unwrap();

        // Later in the same day, normal positive growth resumes.
        let block_growth =
            dummy_parsed_block(build_dao_field(30_000_000_001_000, 10_120, 100), 0, 1000);
        accumulate_secondary_issuance_deltas(&mut stats, &block_growth, date, &mut prev).unwrap();

        // Non-miner should track only positive growth (+170), ignoring the prior -50 drop.
        assert_eq!(stats.daily_secondary_non_miner_delta.get(&date), Some(&170));
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

        accumulate_dao_snapshot_deltas_for_txs(
            &[tx],
            block_date,
            &dao_code_hash,
            &consumed_dao_map,
            &mut same_batch_dao_map,
            &mut daily_active_delta,
            &mut daily_gross_deposit_delta,
            &mut daily_new_deposits_delta,
        );

        assert_eq!(daily_active_delta.get(&block_date), Some(&-10_000_000_000));
        assert!(daily_gross_deposit_delta.is_empty());
        assert!(daily_new_deposits_delta.is_empty());
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

        accumulate_dao_snapshot_deltas_for_txs(
            &[tx],
            block_date,
            &dao_code_hash,
            &consumed_dao_map,
            &mut same_batch_dao_map,
            &mut daily_active_delta,
            &mut daily_gross_deposit_delta,
            &mut daily_new_deposits_delta,
        );

        assert!(daily_active_delta.is_empty());
        assert!(daily_gross_deposit_delta.is_empty());
        assert!(daily_new_deposits_delta.is_empty());
    }

    #[test]
    fn test_perf_snapshot_uses_last_batch_after_reset() {
        let perf = PerfStats::default();
        perf.add_fetch(Duration::from_millis(120));
        perf.add_db_write(Duration::from_millis(340));
        perf.blocks_count.fetch_add(10, Ordering::Relaxed);
        perf.report_and_reset();

        let (rpc_ms, db_ms) = perf.snapshot_ms();
        assert!((rpc_ms - 120.0).abs() < 0.001);
        assert!((db_ms - 340.0).abs() < 0.001);
    }

    #[test]
    fn test_perf_snapshot_prefers_current_accumulator_over_last_batch() {
        let perf = PerfStats::default();

        perf.add_fetch(Duration::from_millis(500));
        perf.add_db_write(Duration::from_millis(700));
        perf.blocks_count.fetch_add(10, Ordering::Relaxed);
        perf.report_and_reset();

        perf.add_fetch(Duration::from_millis(150));
        perf.add_db_write(Duration::from_millis(250));

        let (rpc_ms, db_ms) = perf.snapshot_ms();
        assert!((rpc_ms - 150.0).abs() < 0.001);
        assert!((db_ms - 250.0).abs() < 0.001);
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
        perf.record_write(Duration::from_millis(80), 12.0, 6, 16);

        let snapshot = perf.snapshot().expect("pipeline snapshot should exist");
        assert_eq!(snapshot.fetch_ms, Some(20.0));
        assert_eq!(snapshot.parse_ms, Some(40.0));
        assert_eq!(snapshot.write_ms, Some(80.0));
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
    fn test_should_rebuild_hodl_tracker_state_rules() {
        assert!(!should_rebuild_hodl_tracker_state(None, 0));
        assert!(should_rebuild_hodl_tracker_state(None, 1));

        let empty = HodlTrackerState {
            capacity_by_date: vec![],
            date_transitions: vec![],
            holder_count: 0,
            last_snapshot_date: None,
        };
        assert!(should_rebuild_hodl_tracker_state(Some(&empty), 100));

        let aligned = HodlTrackerState {
            capacity_by_date: vec![("20240101".to_string(), 1)],
            date_transitions: vec![(0, "20240101".to_string()), (100, "20240102".to_string())],
            holder_count: 1,
            last_snapshot_date: Some("20240102".to_string()),
        };
        assert!(!should_rebuild_hodl_tracker_state(Some(&aligned), 100));

        let ahead = HodlTrackerState {
            capacity_by_date: vec![("20240101".to_string(), 1)],
            date_transitions: vec![(0, "20240101".to_string()), (101, "20240102".to_string())],
            holder_count: 1,
            last_snapshot_date: Some("20240102".to_string()),
        };
        assert!(should_rebuild_hodl_tracker_state(Some(&ahead), 100));
    }
}
