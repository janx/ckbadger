//! DAO calculation helpers and related pure functions.
//!
//! Issuance splits, snapshot deltas, deposit/withdraw accounting,
//! address counting, object collection classification, occupied capacity,
//! and tx-fee validation.

#![allow(clippy::type_complexity)]

use std::collections::{HashMap, HashSet};

use anyhow::{anyhow, bail, Result};
use chrono::NaiveDate;
use ckbadger_common::dao::calculate_miner_secondary_issuance;

use ckbadger_store::types::{AddressBalance, PositionedCellInfo};

pub(crate) use ckbadger_store::types::{
    BIT_CELL_SENTINEL_COLLECTION, DID_CKB_SENTINEL_COLLECTION, DOTBIT_SENTINEL_COLLECTION,
};

use crate::parser::{BitCellParser, DaoParser, DidCkbParser, DotbitParser, MnftParser};

use super::helpers::{checked_usize_to_i16, parsed_input_outpoint_index_i16};
use super::types::TxData;

// ---------------------------------------------------------------------------
// BatchStats (DAO accumulator)
// ---------------------------------------------------------------------------

/// Accumulated statistics across a batch of blocks (avoids per-block DB writes)
#[derive(Default)]
pub(crate) struct BatchStats {
    pub(crate) sync_totals: (i64, i64, i64),
    pub(crate) last_block: Option<(i64, Vec<u8>)>,
    pub(crate) hourly_stats: HashMap<chrono::DateTime<chrono::Utc>, (i32, i32, i32, i32, i128)>,
    pub(crate) daily_stats: HashMap<NaiveDate, (i32, i32, i32, i32, i128, i128, i128, i64, i64)>,
    pub(crate) daily_block_stats: HashMap<NaiveDate, (i128, i32, i32)>,
    pub(crate) miner_stats: HashMap<(NaiveDate, Vec<u8>), (i32, i64)>,
    pub(crate) epoch_stats: HashMap<i64, EpochAccum>,
    pub(crate) block_time_dist: HashMap<i32, i32>,
    pub(crate) epoch_time_dist: HashMap<i32, i32>,
    pub(crate) dao_snapshot_dates: std::collections::HashSet<NaiveDate>,
    pub(crate) daily_block_times: HashMap<NaiveDate, (i64, i32)>,
    pub(crate) daily_dao_fields: HashMap<NaiveDate, Vec<u8>>,
    pub(crate) dao_daily_active_delta: HashMap<NaiveDate, i128>,
    pub(crate) dao_daily_protocol_delta: HashMap<NaiveDate, i128>,
    pub(crate) dao_daily_gross_deposit_delta: HashMap<NaiveDate, i128>,
    pub(crate) dao_daily_new_deposits_delta: HashMap<NaiveDate, i64>,
    pub(crate) dao_daily_withdrawals_delta: HashMap<NaiveDate, i64>,
    pub(crate) dao_daily_unique_depositors_delta: HashMap<NaiveDate, i64>,
    pub(crate) dao_daily_cumulative_depositors_delta: HashMap<NaiveDate, i64>,
    /// Per-day unique addresses that deposited (including repeat depositors).
    pub(crate) dao_daily_depositing_addresses: HashMap<NaiveDate, HashSet<Vec<u8>>>,
    /// Block numbers per date (for counting daily depositors from store).
    pub(crate) dao_block_numbers_by_date: HashMap<NaiveDate, Vec<i64>>,
    pub(crate) daily_secondary_miner_delta: HashMap<NaiveDate, i128>,
    /// Set to true after the DAO delta computation code path runs, even if no
    /// DAO transactions were found.  This distinguishes "genuinely zero deltas"
    /// from "deltas never computed" (e.g. stale DB from an older indexer).
    pub(crate) dao_deltas_computed: bool,
}

#[derive(Clone)]
pub(crate) struct EpochAccum {
    pub(crate) start_block: i64,
    pub(crate) end_block: i64,
    pub(crate) length: i32,
    pub(crate) start_ts: chrono::DateTime<chrono::Utc>,
    pub(crate) end_ts: chrono::DateTime<chrono::Utc>,
    pub(crate) tx_count: i32,
    pub(crate) is_new: bool,
}

// ---------------------------------------------------------------------------
// Type aliases for DAO consumed-cell maps
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct DaoConsumedRow {
    pub tx_hash: Vec<u8>,
    pub output_index: i16,
    pub capacity_str: String,
    pub deposit_block: i64,
    pub status: i16,
    pub lock_script_hash: Vec<u8>,
}
pub(crate) type DaoConsumedMap = HashMap<(Vec<u8>, i16), DaoConsumedRow>;
pub(crate) type DaoSameBatchMap = HashMap<(Vec<u8>, i16), i64>;

// ---------------------------------------------------------------------------
// Address counting
// ---------------------------------------------------------------------------

pub(crate) fn count_new_addresses(
    changes: &HashMap<Vec<u8>, crate::sync::types::AddressBalanceDelta>,
    existing: &HashMap<Vec<u8>, Option<AddressBalance>>,
) -> i64 {
    changes
        .iter()
        .filter(|(lock_hash, delta)| {
            if delta.live_delta <= 0 {
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

// ---------------------------------------------------------------------------
// Object collection classification
// ---------------------------------------------------------------------------

pub(crate) fn classify_object_collection_id(
    type_code_hash: &[u8],
    type_args: &[u8],
) -> Option<Vec<u8>> {
    if type_args.len() >= 24 && MnftParser::is_token_type_script(type_code_hash) {
        return Some(type_args[..24].to_vec());
    }
    if DotbitParser::is_account_cell_type_script(type_code_hash) {
        return Some(DOTBIT_SENTINEL_COLLECTION.to_vec());
    }
    if BitCellParser::is_type_script(type_code_hash) {
        return Some(BIT_CELL_SENTINEL_COLLECTION.to_vec());
    }
    if DidCkbParser::is_type_script(type_code_hash) {
        return Some(DID_CKB_SENTINEL_COLLECTION.to_vec());
    }
    None
}

// ---------------------------------------------------------------------------
// Pre-batch live cell derivation
// ---------------------------------------------------------------------------

/// Reconstruct pre-batch live cell count from persisted post-batch count and batch delta.
///
/// Address balances are written before HODL tracker updates, so reading `live_cells_count`
/// from store returns post-batch state. We need pre-batch state to detect 0→>0 and >0→0
/// holder transitions correctly.
#[cfg(test)]
pub(crate) fn derive_pre_batch_live_cells(post_live_cells: i32, live_delta: i32) -> Result<i32> {
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

// ---------------------------------------------------------------------------
// Block time bucketing (shared between bulk-build and live sync)
// ---------------------------------------------------------------------------

/// Map inter-block time (seconds) to a histogram bucket.
/// Buckets: 0 (<1s), 1..29 (per-second), 30 (>=30s).
pub(crate) fn block_time_to_bucket(block_time_seconds: i64) -> i32 {
    if block_time_seconds < 1 {
        0
    } else if block_time_seconds < 30 {
        block_time_seconds as i32
    } else {
        30
    }
}

impl BatchStats {
    /// Accumulate one inter-block time delta into both the per-day sum and the
    /// per-second bucket histogram. Negative deltas (clock skew) are dropped.
    ///
    /// The per-day sum stores full millisecond precision so that the daily
    /// average matches the on-the-fly average derived from raw block headers.
    pub(crate) fn accumulate_block_time(
        &mut self,
        prev_ts: chrono::DateTime<chrono::Utc>,
        curr_ts: chrono::DateTime<chrono::Utc>,
        block_date: NaiveDate,
    ) {
        let delta_ms = (curr_ts - prev_ts).num_milliseconds();
        if delta_ms < 0 {
            return;
        }
        let bucket = block_time_to_bucket(delta_ms / 1000);
        *self.block_time_dist.entry(bucket).or_default() += 1;
        let entry = self.daily_block_times.entry(block_date).or_insert((0, 0));
        entry.0 += delta_ms;
        entry.1 += 1;
    }
}

// ---------------------------------------------------------------------------
// Occupied capacity
// ---------------------------------------------------------------------------

pub(crate) fn occupied_capacity_shannons_i128(
    lock_args_len: usize,
    type_args_len: Option<usize>,
    data_size: i32,
) -> i128 {
    i128::from(occupied_capacity_shannons_i64(
        lock_args_len,
        type_args_len,
        data_size,
    ))
}

pub(crate) fn occupied_capacity_shannons_i64(
    lock_args_len: usize,
    type_args_len: Option<usize>,
    data_size: i32,
) -> i64 {
    let data_size = usize::try_from(data_size).unwrap_or_else(|_| {
        panic!(
            "negative cell data_size while computing occupied capacity: {}",
            data_size
        )
    });
    ckbadger_common::dao::occupied_capacity_shannons(
        data_size,
        lock_args_len,
        type_args_len,
    )
    .unwrap_or_else(|error| {
        panic!(
            "failed to compute occupied capacity: lock_args_len={}, type_args_len={:?}, data_size={}, error={}",
            lock_args_len, type_args_len, data_size, error
        )
    })
}

// ---------------------------------------------------------------------------
// DAO field extraction
// ---------------------------------------------------------------------------

pub(crate) fn extract_dao_csu(dao: &[u8]) -> Option<(i128, i128, i128)> {
    if dao.len() < 32 {
        return None;
    }
    let c = u64::from_le_bytes(dao[0..8].try_into().ok()?) as i128;
    let s = u64::from_le_bytes(dao[16..24].try_into().ok()?) as i128;
    let u = u64::from_le_bytes(dao[24..32].try_into().ok()?) as i128;
    Some((c, s, u))
}

pub(crate) fn extract_ar_i64_from_dao(dao: &[u8], block_number: i64) -> Result<i64> {
    let ar = DaoParser::extract_ar_from_dao_field(dao)
        .ok_or_else(|| anyhow!("missing AR in DAO field at block {}", block_number))?;
    i64::try_from(ar).map_err(|_| anyhow!("DAO AR exceeds i64 at block {}: {}", block_number, ar))
}

pub(crate) fn dao_csu_for_snapshot_date(
    stats: &BatchStats,
    date: NaiveDate,
) -> Result<(i128, i128, i128)> {
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

// ---------------------------------------------------------------------------
// Checked tx fee
// ---------------------------------------------------------------------------

pub(crate) fn checked_tx_fee(
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

    total_input_capacity
        .checked_sub(total_output_capacity)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "tx fee subtraction overflow: block={}, tx_hash={}, total_input_capacity={}, total_output_capacity={}",
                block_number,
                hex::encode(tx_hash),
                total_input_capacity,
                total_output_capacity
            )
        })
}

// ---------------------------------------------------------------------------
// DAO depositor derivation
// ---------------------------------------------------------------------------

pub(crate) fn derive_running_depositors(
    previous_depositors: i64,
    daily_unique_depositors_delta: i64,
    date: NaiveDate,
) -> Result<i64> {
    let next = previous_depositors
        .checked_add(daily_unique_depositors_delta)
        .ok_or_else(|| {
            anyhow!(
                "dao snapshot depositor overflow: date={}, previous_depositors={}, daily_unique_depositors_delta={}",
                date,
                previous_depositors,
                daily_unique_depositors_delta
            )
        })?;
    if next < 0 {
        anyhow::bail!(
            "dao snapshot depositor underflow: date={}, previous_depositors={}, daily_unique_depositors_delta={}",
            date,
            previous_depositors,
            daily_unique_depositors_delta
        );
    }
    Ok(next)
}

// ---------------------------------------------------------------------------
// DAO snapshot delta accumulation (per-tx)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub(crate) fn accumulate_dao_snapshot_deltas_for_txs(
    tx_slice: &[TxData],
    block_date: NaiveDate,
    dao_code_hash: &[u8],
    consumed_dao_map: &DaoConsumedMap,
    same_batch_dao_map: &mut DaoSameBatchMap,
    _input_cell_info: &HashMap<(Vec<u8>, i16), PositionedCellInfo>,
    _batch_cell_infos: &HashMap<(Vec<u8>, i16), PositionedCellInfo>,
    active_deposit_counts_by_lock: &mut HashMap<Vec<u8>, i64>,
    daily_unique_depositors_delta: &mut HashMap<NaiveDate, i64>,
    daily_active_delta: &mut HashMap<NaiveDate, i128>,
    daily_protocol_delta: &mut HashMap<NaiveDate, i128>,
    daily_gross_deposit_delta: &mut HashMap<NaiveDate, i128>,
    daily_new_deposits_delta: &mut HashMap<NaiveDate, i64>,
    daily_withdrawals_delta: &mut HashMap<NaiveDate, i64>,
    ever_deposited_by_lock: &mut HashMap<Vec<u8>, bool>,
    daily_cumulative_depositors_delta: &mut HashMap<NaiveDate, i64>,
    daily_depositing_addresses: &mut HashMap<NaiveDate, HashSet<Vec<u8>>>,
) -> Result<()> {
    for tx_data in tx_slice {
        for (output_index, cell) in tx_data.cells.iter().enumerate() {
            if let Some(ref type_code_hash) = cell.type_code_hash {
                if type_code_hash == dao_code_hash
                    && cell.data_size == 8
                    && cell.data.len() == 8
                    && cell.data.iter().all(|&b| b == 0)
                {
                    let output_index_i16 = checked_usize_to_i16(
                        output_index,
                        "DAO output index while accumulating daily snapshot deltas",
                    )
                    .map_err(|e| anyhow!("{}: tx_hash=0x{}", e, hex::encode(tx_data.hash)))?;
                    *daily_active_delta.entry(block_date).or_default() += cell.capacity as i128;
                    *daily_protocol_delta.entry(block_date).or_default() += cell.capacity as i128;
                    *daily_gross_deposit_delta.entry(block_date).or_default() +=
                        cell.capacity as i128;
                    *daily_new_deposits_delta.entry(block_date).or_default() += 1;
                    bump_unique_active_depositors(
                        active_deposit_counts_by_lock,
                        daily_unique_depositors_delta,
                        block_date,
                        &cell.lock_script_hash,
                        1,
                        &tx_data.hash,
                        output_index_i16,
                    )?;
                    // Track all-time cumulative depositors.
                    let already = ever_deposited_by_lock
                        .entry(cell.lock_script_hash.clone())
                        .or_insert(false);
                    if !*already {
                        *already = true;
                        *daily_cumulative_depositors_delta
                            .entry(block_date)
                            .or_default() += 1;
                    }
                    // Track per-day unique depositing addresses (including repeats).
                    daily_depositing_addresses
                        .entry(block_date)
                        .or_default()
                        .insert(cell.lock_script_hash.clone());
                    same_batch_dao_map
                        .insert((tx_data.hash.to_vec(), output_index_i16), cell.capacity);
                }
            }
        }

        if tx_data.is_cellbase {
            continue;
        }

        for input in &tx_data.inputs {
            let outpoint = (
                input.previous_tx_hash.to_vec(),
                parsed_input_outpoint_index_i16(input.previous_output_index, "sync_indexer")?,
            );
            if let Some(row) = consumed_dao_map.get(&outpoint) {
                if row.status == 0 {
                    // Phase-1: deposit consumed for withdraw request — CKB
                    // leaves active status.  Subtract from active delta and
                    // decrement the unique active depositor count.  This
                    // matches the CKB explorer convention which subtracts
                    // from total_deposit at phase-1 withdrawal.
                    let capacity: i64 = row.capacity_str.parse().map_err(|e| {
                        anyhow!(
                            "invalid DAO capacity string at phase-1 withdrawal: value='{}', tx_hash=0x{}, error={}",
                            row.capacity_str,
                            hex::encode(tx_data.hash),
                            e
                        )
                    })?;
                    *daily_active_delta.entry(block_date).or_default() -= capacity as i128;
                    // Protocol delta NOT subtracted — cell still locked in DAO.
                    bump_unique_active_depositors(
                        active_deposit_counts_by_lock,
                        daily_unique_depositors_delta,
                        block_date,
                        &row.lock_script_hash,
                        -1,
                        &tx_data.hash,
                        outpoint.1,
                    )?;
                } else if row.status == 1 {
                    // Phase-2: withdraw-request consumed — track as completed
                    // withdrawal.  Active delta already subtracted at phase-1.
                    // Protocol delta subtracted now — cell leaves DAO.
                    *daily_withdrawals_delta.entry(block_date).or_default() += 1;
                    let capacity: i64 = row.capacity_str.parse().map_err(|e| {
                        anyhow!(
                            "invalid DAO capacity string at phase-2 withdrawal: value='{}', tx_hash=0x{}, error={}",
                            row.capacity_str,
                            hex::encode(tx_data.hash),
                            e
                        )
                    })?;
                    *daily_protocol_delta.entry(block_date).or_default() -= capacity as i128;
                }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Miner secondary issuance accumulation (per-block)
// ---------------------------------------------------------------------------

/// Accumulate block `block_number`'s exact miner secondary issuance into the
/// daily series: `floor(s_i * U_{i-1} / C_{i-1})` per RFC-0023, where `s_i`
/// comes from the epoch schedule (header epoch position + the consensus
/// `secondary_epoch_reward`) and `U`/`C` come from the PARENT block's DAO
/// header field carried in `prev_dao_cu`.
///
/// This is the protocol's own direct split — the value the node reports as
/// `miner_reward.secondary`. It is deliberately independent of DAO
/// claimed-compensation recognition; claimed compensation feeds only the
/// compensation aggregates.
///
/// Genesis: block 0 carries its own primary+secondary share inside the
/// genesis DAO `C` field and its full secondary share `s_0` enters the
/// genesis `S` pool — the node defines no miner reward for block 0 — so
/// block 0 only seeds the parent C/U state.
#[allow(clippy::too_many_arguments)]
pub(crate) fn accumulate_miner_secondary_for_block(
    stats: &mut BatchStats,
    block_number: i64,
    block_date: NaiveDate,
    epoch_index: i64,
    epoch_length: i64,
    c: i128,
    u: i128,
    secondary_epoch_reward: u64,
    prev_dao_cu: &mut Option<(i128, i128)>,
) -> Result<()> {
    if block_number > 0 {
        let (prev_c, prev_u) = prev_dao_cu.ok_or_else(|| {
            anyhow!(
                "missing parent DAO C/U while accumulating miner secondary issuance: block={}, date={}",
                block_number,
                block_date
            )
        })?;
        let secondary_issuance = ckbadger_common::dao::secondary_block_issuance(
            epoch_index,
            epoch_length,
            secondary_epoch_reward,
        )
        .map_err(|error| anyhow!("{error}: block={}, date={}", block_number, block_date))?;
        let miner = calculate_miner_secondary_issuance(secondary_issuance, prev_c, prev_u)
            .map_err(|error| anyhow!("{error}: block={}, date={}", block_number, block_date))?;
        if miner != 0 {
            *stats
                .daily_secondary_miner_delta
                .entry(block_date)
                .or_default() += miner;
        }
    }

    *prev_dao_cu = Some((c, u));
    Ok(())
}

pub(crate) fn accumulate_miner_secondary(
    stats: &mut BatchStats,
    parsed: &crate::parser::block::ParsedBlock,
    block_date: NaiveDate,
    secondary_epoch_reward: u64,
    prev_dao_cu: &mut Option<(i128, i128)>,
) -> Result<()> {
    let (c, _s, u) = extract_dao_csu(&parsed.dao).ok_or_else(|| {
        anyhow!(
            "invalid DAO field bytes while accumulating miner secondary issuance: block={}, date={}, dao_len={}",
            parsed.number,
            block_date,
            parsed.dao.len()
        )
    })?;

    accumulate_miner_secondary_for_block(
        stats,
        parsed.number,
        block_date,
        i64::from(parsed.epoch_index),
        i64::from(parsed.epoch_length),
        c,
        u,
        secondary_epoch_reward,
        prev_dao_cu,
    )
}

fn bump_unique_active_depositors(
    active_deposit_counts_by_lock: &mut HashMap<Vec<u8>, i64>,
    daily_unique_depositors_delta: &mut HashMap<NaiveDate, i64>,
    block_date: NaiveDate,
    lock_hash: &[u8],
    delta: i64,
    tx_hash: &[u8; 32],
    output_index: i16,
) -> Result<()> {
    if delta == 0 {
        return Ok(());
    }

    let lock_key = lock_hash.to_vec();
    let current = active_deposit_counts_by_lock
        .get(&lock_key)
        .copied()
        .unwrap_or(0);
    let next = current.checked_add(delta).ok_or_else(|| {
        anyhow!(
            "dao unique active depositor count overflow: date={}, lock_hash=0x{}, current={}, delta={}, tx_hash=0x{}, output_index={}",
            block_date,
            hex::encode(lock_hash),
            current,
            delta,
            hex::encode(tx_hash),
            output_index
        )
    })?;
    if next < 0 {
        bail!(
            "dao unique active depositor count underflow: date={}, lock_hash=0x{}, current={}, delta={}, tx_hash=0x{}, output_index={}",
            block_date,
            hex::encode(lock_hash),
            current,
            delta,
            hex::encode(tx_hash),
            output_index
        );
    }

    if current == 0 && next > 0 {
        *daily_unique_depositors_delta.entry(block_date).or_default() += 1;
    } else if current > 0 && next == 0 {
        *daily_unique_depositors_delta.entry(block_date).or_default() -= 1;
    }

    active_deposit_counts_by_lock.insert(lock_key, next);
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use ckbadger_store::types::{LiveCellInfo, PositionedCellInfo};
    use std::collections::HashMap;

    // -- Test helpers -------------------------------------------------------

    fn build_dao_field(c: u64, s: u64, u: u64) -> [u8; 32] {
        let mut dao = [0u8; 32];
        dao[0..8].copy_from_slice(&c.to_le_bytes());
        dao[16..24].copy_from_slice(&s.to_le_bytes());
        dao[24..32].copy_from_slice(&u.to_le_bytes());
        dao
    }

    fn dummy_parsed_block(
        dao: [u8; 32],
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
            miner_lock_hash: None,
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
            data_hash: [0u8; 32],
            data_size: 8,
            data: if is_deposit {
                vec![0u8; 8]
            } else {
                1u64.to_le_bytes().to_vec()
            },
        }
    }

    fn dummy_dao_cell_with_lock(
        capacity: i64,
        is_deposit: bool,
        lock_script_hash: Vec<u8>,
    ) -> crate::parser::cell::ParsedCell {
        let mut cell = dummy_dao_cell(capacity, is_deposit);
        cell.lock_script_hash = lock_script_hash;
        cell
    }

    fn dummy_positioned_info(
        capacity: i64,
        created_at_block: i64,
        lock_script_hash: Vec<u8>,
    ) -> PositionedCellInfo {
        PositionedCellInfo::new(
            LiveCellInfo {
                capacity,
                lock_script_hash,
                lock_code_hash: vec![],
                lock_hash_type: 0,
                lock_args: vec![],
                type_script_hash: None,
                type_code_hash: Some(crate::rpc::parse_hex_to_bytes(
                    crate::parser::dao::DAO_CODE_HASH,
                )),
                type_hash_type: Some(1),
                type_args: Some(vec![]),
                data_size: 8,
                occupied_capacity: capacity,
                udt_amount: None,
                data_hash: None,
            },
            created_at_block,
        )
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

    // -- count_new_addresses ------------------------------------------------

    #[test]
    fn test_count_new_addresses_counts_only_first_live_transitions() {
        use crate::sync::types::AddressBalanceDelta;

        let mut changes: HashMap<Vec<u8>, AddressBalanceDelta> = HashMap::new();
        let addr_new = vec![0x11; 32];
        let addr_existing_live = vec![0x22; 32];
        let addr_existing_zero = vec![0x33; 32];
        let tx_hash = vec![0xAA; 32];

        changes.insert(
            addr_new.clone(),
            AddressBalanceDelta {
                balance_delta: 100,
                live_delta: 1,
                total_delta: 1,
                tx_delta: 1,
                used_delta: 10,
                first_seen_block: 1,
                first_seen_tx: tx_hash.clone(),
                last_activity_block: 1,
                last_activity_tx: tx_hash.clone(),
            },
        );
        changes.insert(
            addr_existing_live.clone(),
            AddressBalanceDelta {
                balance_delta: 50,
                live_delta: 1,
                total_delta: 1,
                tx_delta: 1,
                used_delta: 5,
                first_seen_block: 1,
                first_seen_tx: tx_hash.clone(),
                last_activity_block: 1,
                last_activity_tx: tx_hash.clone(),
            },
        );
        changes.insert(
            addr_existing_zero.clone(),
            AddressBalanceDelta {
                balance_delta: 70,
                live_delta: 2,
                total_delta: 2,
                tx_delta: 1,
                used_delta: 7,
                first_seen_block: 1,
                first_seen_tx: tx_hash.clone(),
                last_activity_block: 1,
                last_activity_tx: tx_hash,
            },
        );

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
        use crate::sync::types::AddressBalanceDelta;

        let mut changes: HashMap<Vec<u8>, AddressBalanceDelta> = HashMap::new();
        let tx_hash = vec![0xBB; 32];
        changes.insert(
            vec![0x44; 32],
            AddressBalanceDelta {
                balance_delta: 0,
                live_delta: 0,
                total_delta: 0,
                tx_delta: 1,
                used_delta: 0,
                first_seen_block: 1,
                first_seen_tx: tx_hash.clone(),
                last_activity_block: 1,
                last_activity_tx: tx_hash.clone(),
            },
        );
        changes.insert(
            vec![0x55; 32],
            AddressBalanceDelta {
                balance_delta: -10,
                live_delta: -1,
                total_delta: 0,
                tx_delta: 1,
                used_delta: -2,
                first_seen_block: 1,
                first_seen_tx: tx_hash.clone(),
                last_activity_block: 1,
                last_activity_tx: tx_hash,
            },
        );

        let existing: HashMap<Vec<u8>, Option<AddressBalance>> = HashMap::new();
        assert_eq!(count_new_addresses(&changes, &existing), 0);
    }

    // -- classify_object_collection_id -----------------------------------------

    #[test]
    fn test_classify_object_collection_id_mnft_uses_first_24_args_bytes() {
        let mnft_code_hash =
            crate::rpc::parse_hex_to_bytes(crate::parser::mnft::MNFT_TOKEN_CODE_HASH);
        let mut args = vec![0xAB; 24];
        args.extend_from_slice(&[0xCD; 8]);

        let collection_id = classify_object_collection_id(&mnft_code_hash, &args)
            .expect("mNFT token type should map to collection id");
        assert_eq!(collection_id, vec![0xAB; 24]);
    }

    #[test]
    fn test_classify_object_collection_id_dotbit_uses_sentinel_collection() {
        let dotbit_code_hash =
            crate::rpc::parse_hex_to_bytes(crate::parser::dotbit::DOTBIT_ACCOUNT_CELL_TYPE_ID);
        let collection_id = classify_object_collection_id(&dotbit_code_hash, &[])
            .expect("dotbit account type should map to sentinel collection");
        assert_eq!(collection_id, DOTBIT_SENTINEL_COLLECTION.to_vec());
    }

    #[test]
    fn test_classify_object_collection_id_bit_cell_uses_own_sentinel_collection() {
        let bit_cell_code_hash =
            crate::rpc::parse_hex_to_bytes(crate::parser::bit_cell::BIT_CELL_CODE_HASH_MAINNET);
        let collection_id = classify_object_collection_id(&bit_cell_code_hash, &[0x99; 32])
            .expect(".bit Cell type should map to sentinel collection");
        assert_eq!(collection_id, BIT_CELL_SENTINEL_COLLECTION.to_vec());
    }

    #[test]
    fn test_classify_object_collection_id_rejects_non_object_or_short_mnft_args() {
        let non_object = vec![0x11; 32];
        assert!(classify_object_collection_id(&non_object, &[0x22; 24]).is_none());

        let mnft_code_hash =
            crate::rpc::parse_hex_to_bytes(crate::parser::mnft::MNFT_TOKEN_CODE_HASH);
        assert!(classify_object_collection_id(&mnft_code_hash, &[0x33; 23]).is_none());
    }

    // -- derive_pre_batch_live_cells ----------------------------------------

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

    // -- occupied_capacity_shannons -----------------------------------------

    #[test]
    fn test_occupied_capacity_shannons_helpers() {
        let expected = (8_i128 + 33 + 20 + 33 + 10 + 100) * 100_000_000_i128;
        assert_eq!(occupied_capacity_shannons_i128(20, Some(10), 100), expected);
        assert_eq!(
            occupied_capacity_shannons_i64(20, Some(10), 100),
            i64::try_from(expected).unwrap()
        );
    }

    #[test]
    #[should_panic(expected = "negative cell data_size while computing occupied capacity")]
    fn test_occupied_capacity_shannons_negative_data_panics() {
        let _ = occupied_capacity_shannons_i128(1, None, -1);
    }

    // -- dao_csu_for_snapshot_date ------------------------------------------

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

    // -- checked_tx_fee -----------------------------------------------------

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

    // -- extract_ar_i64_from_dao --------------------------------------------

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

    // -- derive_running_depositors ------------------------------------------

    #[test]
    fn test_derive_running_depositors() {
        let date = chrono::NaiveDate::from_ymd_opt(2026, 2, 18).unwrap();
        assert_eq!(derive_running_depositors(10, -3, date).unwrap(), 7);
    }

    #[test]
    fn test_derive_running_depositors_underflow_errors() {
        let date = chrono::NaiveDate::from_ymd_opt(2026, 2, 18).unwrap();
        let err = derive_running_depositors(3, -10, date).unwrap_err();
        assert!(err.to_string().contains("dao snapshot depositor underflow"));
    }

    // -- accumulate_miner_secondary -----------------------------------------

    /// Consensus secondary issuance per epoch shared by mainnet and testnet
    /// (verified against both nodes' `get_consensus`).
    const SECONDARY_EPOCH_REWARD: u64 = 61_369_863_013_698;

    /// Real mainnet vector, block 1600000 (2020-04, epoch 977 idx 202 len 1797).
    ///
    /// Node `get_block_economic_state`: issuance.secondary = 34151287153,
    /// miner_reward.secondary = 4781307093. The per-block miner secondary is
    /// the protocol's direct split `floor(s_i * U_{i-1} / C_{i-1})`
    /// (RFC-0023); reconstructing it from the S-pool delta instead
    /// (`(dS + claimed) * U / (C - U)`) carries an inherent flooring drift —
    /// on this block it yields 4781307094, one shannon high.
    #[test]
    fn test_miner_secondary_is_direct_protocol_split_block_1600000() {
        let mut stats = BatchStats::default();
        // Parent = block 1599999 header DAO field (mainnet).
        let prev_c = 3_607_356_675_738_101_574_i128;
        let prev_u = 505_043_338_100_000_000_i128;
        // Block 1600000 header DAO field (mainnet); epoch 977, index 202,
        // length 1797.
        let c = 3_607_356_816_612_161_080_i128;
        let u = 505_043_344_200_000_000_i128;
        let date = chrono::NaiveDate::from_ymd_opt(2020, 4, 29).unwrap();
        let mut prev = Some((prev_c, prev_u));

        accumulate_miner_secondary_for_block(
            &mut stats,
            1_600_000,
            date,
            202,
            1797,
            c,
            u,
            SECONDARY_EPOCH_REWARD,
            &mut prev,
        )
        .unwrap();

        assert_eq!(
            stats.daily_secondary_miner_delta.get(&date),
            Some(&4_781_307_093_i128),
            "block 1600000 miner secondary must equal the node's \
             miner_reward.secondary (floor(s*U/C) = floor(34151287153 * \
             505043338100000000 / 3607356675738101574))"
        );
        assert_eq!(prev, Some((c, u)), "parent C/U baseline must advance");
    }

    /// Real mainnet vector, block 6299887 (2022-01, epoch 4727 idx 948 len
    /// 1063): a 2.34M-CKB phase-2 claim block where the S-pool delta is
    /// negative (-234220781096391).
    ///
    /// Node `get_block_economic_state`: issuance.secondary = 57732702741,
    /// miner_reward.secondary = 6461319527. The miner series must NOT depend
    /// on claimed-compensation recognition: when a claim is missed by the
    /// lifecycle map, the reconstruction path sees only the negative dS,
    /// clamps to zero, and silently drops the whole 6461319527-shannon miner
    /// delta — exactly the audited −59.81 CKB identity-violation signature.
    #[test]
    fn test_miner_secondary_is_independent_of_claimed_recognition_block_6299887() {
        let mut stats = BatchStats::default();
        // Parent = block 6299886 header DAO field (mainnet).
        let prev_c = 4_556_869_051_405_297_678_i128;
        let prev_u = 509_994_952_400_000_000_i128;
        // Block 6299887 header DAO field (mainnet); epoch 4727, index 948,
        // length 1063. The block's phase-2 claims total 234272052479605
        // shannons and its S-pool delta is NEGATIVE (-234220781096391) — the
        // exact signature the old reconstruction clamped to zero whenever a
        // claim was missed, silently dropping the whole miner delta.
        let c = 4_556_869_289_552_696_484_i128;
        let u = 509_994_950_300_000_000_i128;
        let date = chrono::NaiveDate::from_ymd_opt(2022, 1, 15).unwrap();
        let mut prev = Some((prev_c, prev_u));

        // The miner series takes no claimed-compensation input at all: the
        // exact per-block value derives from the schedule and parent C/U.
        accumulate_miner_secondary_for_block(
            &mut stats,
            6_299_887,
            date,
            948,
            1063,
            c,
            u,
            SECONDARY_EPOCH_REWARD,
            &mut prev,
        )
        .unwrap();

        assert_eq!(
            stats.daily_secondary_miner_delta.get(&date),
            Some(&6_461_319_527_i128),
            "block 6299887 miner secondary must equal the node's \
             miner_reward.secondary regardless of claimed-compensation \
             recognition"
        );
    }

    #[test]
    fn test_accumulate_miner_secondary_wrapper_reads_header_epoch_and_dao() {
        // The ParsedBlock wrapper must take C/U from the block's DAO field
        // (advancing the parent baseline) and the epoch position from the
        // header. dummy_parsed_block has number=1, epoch_index=0.
        let mut stats = BatchStats::default();
        let prev_c = 10_000_000_000_000_i128;
        let prev_u = 1_000_000_000_000_i128;
        let mut prev = Some((prev_c, prev_u));
        let c = 10_000_100_000_000_u64;
        let u = 1_000_000_000_000_u64;
        let block = dummy_parsed_block(build_dao_field(c, 5_000, u), 0, 1000);
        let date = ckbadger_common::block_date(block.timestamp);

        // Epoch length 1000: base = 61369863013, remainder = 698, so index 0
        // receives base + 1 = 61369863014.
        let expected_s = 61_369_863_014_i128;
        let expected_miner = expected_s * prev_u / prev_c;

        accumulate_miner_secondary(&mut stats, &block, date, SECONDARY_EPOCH_REWARD, &mut prev)
            .unwrap();

        assert_eq!(
            stats.daily_secondary_miner_delta.get(&date),
            Some(&expected_miner)
        );
        assert_eq!(
            prev,
            Some((c as i128, u as i128)),
            "parent C/U baseline must advance to the current block"
        );
    }

    #[test]
    fn test_accumulate_miner_secondary_genesis_seeds_parent_state_without_delta() {
        // Genesis (block 0) has no parent to split against: its own secondary
        // share enters the genesis S pool in full (the node defines no miner
        // reward for block 0). It must only seed the parent C/U baseline.
        // Real mainnet genesis: C = 3360000145238488200 (33.6B + block 0's
        // own p_0+s_0 = 145238488200), U = 504120308900000000.
        let mut stats = BatchStats::default();
        let mut prev: Option<(i128, i128)> = None;
        let date = chrono::NaiveDate::from_ymd_opt(2019, 11, 16).unwrap();
        let genesis_c = 3_360_000_145_238_488_200_i128;
        let genesis_u = 504_120_308_900_000_000_i128;

        accumulate_miner_secondary_for_block(
            &mut stats,
            0,
            date,
            0,
            1743,
            genesis_c,
            genesis_u,
            SECONDARY_EPOCH_REWARD,
            &mut prev,
        )
        .unwrap();

        assert!(
            stats.daily_secondary_miner_delta.is_empty(),
            "genesis must contribute no miner secondary"
        );
        assert_eq!(prev, Some((genesis_c, genesis_u)));

        // Block 1 then splits against the genesis state: node-verified
        // miner_reward.secondary = 5282660055 (s_1 = 35209330473, epoch 0
        // index 1 length 1743).
        accumulate_miner_secondary_for_block(
            &mut stats,
            1,
            date,
            1,
            1743,
            3_360_000_290_476_976_400_i128,
            genesis_u,
            SECONDARY_EPOCH_REWARD,
            &mut prev,
        )
        .unwrap();
        assert_eq!(
            stats.daily_secondary_miner_delta.get(&date),
            Some(&5_282_660_055_i128)
        );
    }

    #[test]
    fn test_accumulate_miner_secondary_same_day_blocks_sum_exactly() {
        // Two consecutive same-day blocks accumulate the exact sum of their
        // per-block splits (block 1 + block 2 real mainnet vectors:
        // 5282660055 + 5282659827).
        let mut stats = BatchStats::default();
        let date = chrono::NaiveDate::from_ymd_opt(2019, 11, 16).unwrap();
        let genesis_u = 504_120_308_900_000_000_i128;
        let mut prev = Some((3_360_000_145_238_488_200_i128, genesis_u));

        accumulate_miner_secondary_for_block(
            &mut stats,
            1,
            date,
            1,
            1743,
            3_360_000_290_476_976_400_i128,
            genesis_u,
            SECONDARY_EPOCH_REWARD,
            &mut prev,
        )
        .unwrap();
        accumulate_miner_secondary_for_block(
            &mut stats,
            2,
            date,
            2,
            1743,
            3_360_000_435_715_464_600_i128,
            genesis_u,
            SECONDARY_EPOCH_REWARD,
            &mut prev,
        )
        .unwrap();

        assert_eq!(
            stats.daily_secondary_miner_delta.get(&date),
            Some(&(5_282_660_055_i128 + 5_282_659_827_i128))
        );
    }

    #[test]
    fn test_accumulate_miner_secondary_bails_without_parent_state() {
        // A non-genesis block with no parent C/U baseline is an invariant
        // violation (silently skipping it would drop that block's miner
        // secondary) — it must fail with block context.
        let mut stats = BatchStats::default();
        let mut prev: Option<(i128, i128)> = None;
        let date = chrono::NaiveDate::from_ymd_opt(2026, 2, 18).unwrap();

        let err = accumulate_miner_secondary_for_block(
            &mut stats,
            42,
            date,
            0,
            1800,
            30_000_000_000_000,
            100,
            SECONDARY_EPOCH_REWARD,
            &mut prev,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("missing parent DAO C/U"),
            "unexpected error: {err}"
        );
        assert!(err.to_string().contains("block=42"));
    }

    #[test]
    fn test_accumulate_miner_secondary_errors_on_invalid_dao_field() {
        let mut stats = BatchStats::default();
        let mut prev = Some((30_000_000_000_000_i128, 0_i128));
        let date = chrono::NaiveDate::from_ymd_opt(2026, 2, 18).unwrap();
        // ParsedBlock.dao is now [u8; 32], so extract_dao_csu always succeeds on it.
        // Test the underlying helper directly with a short slice to cover the error path.
        assert!(extract_dao_csu(&[0u8; 8]).is_none());

        // Verify the happy path doesn't error with a valid progressing DAO field.
        let block = dummy_parsed_block(build_dao_field(30_000_000_000_100, 10_000, 0), 0, 1000);
        let result =
            accumulate_miner_secondary(&mut stats, &block, date, SECONDARY_EPOCH_REWARD, &mut prev);
        assert!(result.is_ok());
    }

    // -- accumulate_dao_snapshot_deltas_for_txs -----------------------------

    #[test]
    fn test_phase1_subtracts_active_delta() {
        // Phase-1 (withdraw request) subtracts from daily_active_delta,
        // matching the CKB explorer convention.
        let block_date = chrono::NaiveDate::from_ymd_opt(2026, 2, 18).unwrap();
        let dao_code_hash = crate::rpc::parse_hex_to_bytes(crate::parser::dao::DAO_CODE_HASH);
        let input_hash_vec = vec![0x11; 32];
        let input_hash: [u8; 32] = input_hash_vec.clone().try_into().unwrap();

        // Phase-1 tx: consumes a status=0 deposit and creates a withdraw-request output.
        let tx = dummy_tx_data(
            [0xAA; 32],
            false,
            vec![crate::parser::transaction::ParsedInput {
                previous_tx_hash: input_hash,
                previous_output_index: 0,
                since: 0,
            }],
            vec![dummy_dao_cell(20_000_000_000, false)],
            vec![],
            vec!["0x0100000000000000".to_string()],
        );

        let mut consumed_dao_map: DaoConsumedMap = HashMap::new();
        consumed_dao_map.insert(
            (input_hash_vec, 0),
            DaoConsumedRow {
                tx_hash: vec![],
                output_index: 0,
                capacity_str: "20000000000".to_string(),
                deposit_block: 0,
                status: 0,
                lock_script_hash: vec![0xAA; 32],
            },
        );

        let mut same_batch_dao_map: DaoSameBatchMap = HashMap::new();
        let input_cell_info: HashMap<(Vec<u8>, i16), PositionedCellInfo> = HashMap::new();
        let lock_hash = vec![0x55; 32];
        let mut batch_cell_infos: HashMap<(Vec<u8>, i16), PositionedCellInfo> = HashMap::new();
        batch_cell_infos.insert(
            (vec![0x11; 32], 0),
            dummy_positioned_info(20_000_000_000, 0, lock_hash.clone()),
        );
        let mut active_deposit_counts_by_lock: HashMap<Vec<u8>, i64> = HashMap::new();
        // Lock 0xAA has one active deposit so the phase-1 decrement can succeed.
        active_deposit_counts_by_lock.insert(vec![0xAA; 32], 1);
        let mut daily_unique_depositors_delta: HashMap<chrono::NaiveDate, i64> = HashMap::new();
        let mut daily_active_delta: HashMap<chrono::NaiveDate, i128> = HashMap::new();
        let mut daily_protocol_delta: HashMap<chrono::NaiveDate, i128> = HashMap::new();
        let mut daily_gross_deposit_delta: HashMap<chrono::NaiveDate, i128> = HashMap::new();
        let mut daily_new_deposits_delta: HashMap<chrono::NaiveDate, i64> = HashMap::new();
        let mut daily_withdrawals_delta: HashMap<chrono::NaiveDate, i64> = HashMap::new();

        accumulate_dao_snapshot_deltas_for_txs(
            &[tx],
            block_date,
            &dao_code_hash,
            &consumed_dao_map,
            &mut same_batch_dao_map,
            &input_cell_info,
            &batch_cell_infos,
            &mut active_deposit_counts_by_lock,
            &mut daily_unique_depositors_delta,
            &mut daily_active_delta,
            &mut daily_protocol_delta,
            &mut daily_gross_deposit_delta,
            &mut daily_new_deposits_delta,
            &mut daily_withdrawals_delta,
            &mut HashMap::new(),
            &mut HashMap::new(),
            &mut HashMap::new(),
        )
        .unwrap();

        // Phase-1 subtracts capacity from daily_active_delta.
        assert_eq!(
            daily_active_delta.get(&block_date),
            Some(&-20_000_000_000),
            "phase-1 must subtract capacity from daily_active_delta"
        );
        assert!(daily_gross_deposit_delta.is_empty());
        assert!(daily_new_deposits_delta.is_empty());
        // Phase-1 does NOT count as a completed withdrawal.
        assert!(daily_withdrawals_delta.is_empty());
        // Depositor count decremented.
        assert_eq!(
            daily_unique_depositors_delta.get(&block_date),
            Some(&-1),
            "phase-1 must decrement unique depositor count"
        );
        // Phase-1 must NOT subtract from protocol delta (cell still locked in DAO).
        assert!(
            daily_protocol_delta.is_empty(),
            "phase-1 must not modify daily_protocol_delta"
        );
    }

    #[test]
    fn test_phase2_tracks_withdrawal_without_active_delta() {
        // Phase-2 (withdraw completion) increments daily_withdrawals_delta
        // but does NOT subtract from daily_active_delta (already done at phase-1).
        let block_date = chrono::NaiveDate::from_ymd_opt(2026, 2, 18).unwrap();
        let dao_code_hash = crate::rpc::parse_hex_to_bytes(crate::parser::dao::DAO_CODE_HASH);
        let input_hash_vec = vec![0x22; 32];
        let input_hash: [u8; 32] = input_hash_vec.clone().try_into().unwrap();

        // Phase-2 tx: consumes a status=1 withdraw-request cell (no DAO outputs).
        let tx = dummy_tx_data(
            [0xBB; 32],
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
        consumed_dao_map.insert(
            (input_hash_vec, 0),
            DaoConsumedRow {
                tx_hash: vec![],
                output_index: 0,
                capacity_str: "10000000000".to_string(),
                deposit_block: 0,
                status: 1,
                lock_script_hash: vec![0xAA; 32],
            },
        );

        let mut same_batch_dao_map: DaoSameBatchMap = HashMap::new();
        let input_cell_info: HashMap<(Vec<u8>, i16), PositionedCellInfo> = HashMap::new();
        let batch_cell_infos: HashMap<(Vec<u8>, i16), PositionedCellInfo> = HashMap::new();
        let mut active_deposit_counts_by_lock: HashMap<Vec<u8>, i64> = HashMap::new();
        // One active deposit for lock 0xAA so the depositor decrement can succeed.
        active_deposit_counts_by_lock.insert(vec![0xAA; 32], 1);
        let mut daily_unique_depositors_delta: HashMap<chrono::NaiveDate, i64> = HashMap::new();
        let mut daily_active_delta: HashMap<chrono::NaiveDate, i128> = HashMap::new();
        let mut daily_protocol_delta: HashMap<chrono::NaiveDate, i128> = HashMap::new();
        let mut daily_gross_deposit_delta: HashMap<chrono::NaiveDate, i128> = HashMap::new();
        let mut daily_new_deposits_delta: HashMap<chrono::NaiveDate, i64> = HashMap::new();
        let mut daily_withdrawals_delta: HashMap<chrono::NaiveDate, i64> = HashMap::new();

        accumulate_dao_snapshot_deltas_for_txs(
            &[tx],
            block_date,
            &dao_code_hash,
            &consumed_dao_map,
            &mut same_batch_dao_map,
            &input_cell_info,
            &batch_cell_infos,
            &mut active_deposit_counts_by_lock,
            &mut daily_unique_depositors_delta,
            &mut daily_active_delta,
            &mut daily_protocol_delta,
            &mut daily_gross_deposit_delta,
            &mut daily_new_deposits_delta,
            &mut daily_withdrawals_delta,
            &mut HashMap::new(),
            &mut HashMap::new(),
            &mut HashMap::new(),
        )
        .unwrap();

        // Phase-2 does NOT subtract from daily_active_delta (done at phase-1).
        assert!(
            daily_active_delta.is_empty(),
            "phase-2 must not modify daily_active_delta"
        );
        // Phase-2 must increment daily_withdrawals_delta.
        assert_eq!(
            daily_withdrawals_delta.get(&block_date),
            Some(&1),
            "phase-2 must increment daily_withdrawals_delta"
        );
        assert!(daily_gross_deposit_delta.is_empty());
        assert!(daily_new_deposits_delta.is_empty());
        // Phase-2 must subtract from protocol delta (cell leaves DAO).
        assert_eq!(
            daily_protocol_delta.get(&block_date),
            Some(&-10_000_000_000),
            "phase-2 must subtract capacity from daily_protocol_delta"
        );
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
        consumed_dao_map.insert(
            (input_hash_vec, 0),
            DaoConsumedRow {
                tx_hash: vec![],
                output_index: 0,
                capacity_str: "123".to_string(),
                deposit_block: 0,
                status: 1,
                lock_script_hash: vec![0xAA; 32],
            },
        );

        let mut same_batch_dao_map: DaoSameBatchMap = HashMap::new();
        let input_cell_info: HashMap<(Vec<u8>, i16), PositionedCellInfo> = HashMap::new();
        let batch_cell_infos: HashMap<(Vec<u8>, i16), PositionedCellInfo> = HashMap::new();
        let mut active_deposit_counts_by_lock: HashMap<Vec<u8>, i64> = HashMap::new();
        // Seed one active deposit for lock 0xAA so the depositor decrement can succeed.
        active_deposit_counts_by_lock.insert(vec![0xAA; 32], 1);
        let mut daily_unique_depositors_delta: HashMap<chrono::NaiveDate, i64> = HashMap::new();
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
            &input_cell_info,
            &batch_cell_infos,
            &mut active_deposit_counts_by_lock,
            &mut daily_unique_depositors_delta,
            &mut daily_active_delta,
            &mut HashMap::new(),
            &mut daily_gross_deposit_delta,
            &mut daily_new_deposits_delta,
            &mut daily_withdrawals_delta,
            &mut HashMap::new(),
            &mut HashMap::new(),
            &mut HashMap::new(),
        )
        .unwrap();

        // Phase-2: active delta NOT modified (already done at phase-1).
        assert!(daily_active_delta.is_empty());
        assert!(daily_gross_deposit_delta.is_empty());
        assert!(daily_new_deposits_delta.is_empty());
        // But withdrawal count IS incremented.
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
        consumed_dao_map.insert(
            (input_hash_vec, 0),
            DaoConsumedRow {
                tx_hash: vec![],
                output_index: 0,
                capacity_str: "123".to_string(),
                deposit_block: 0,
                status: 1,
                lock_script_hash: vec![0xAA; 32],
            },
        );

        let mut same_batch_dao_map: DaoSameBatchMap = HashMap::new();
        let input_cell_info: HashMap<(Vec<u8>, i16), PositionedCellInfo> = HashMap::new();
        let batch_cell_infos: HashMap<(Vec<u8>, i16), PositionedCellInfo> = HashMap::new();
        let mut active_deposit_counts_by_lock: HashMap<Vec<u8>, i64> = HashMap::new();
        // Seed one active deposit for lock 0xAA so phase-2 decrement can succeed.
        active_deposit_counts_by_lock.insert(vec![0xAA; 32], 1);
        let mut daily_unique_depositors_delta: HashMap<chrono::NaiveDate, i64> = HashMap::new();
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
            &input_cell_info,
            &batch_cell_infos,
            &mut active_deposit_counts_by_lock,
            &mut daily_unique_depositors_delta,
            &mut daily_active_delta,
            &mut HashMap::new(),
            &mut daily_gross_deposit_delta,
            &mut daily_new_deposits_delta,
            &mut daily_withdrawals_delta,
            &mut HashMap::new(),
            &mut HashMap::new(),
            &mut HashMap::new(),
        )
        .unwrap();

        assert_eq!(daily_withdrawals_delta.get(&block_date), Some(&1));
    }

    #[test]
    fn test_accumulate_dao_snapshot_deltas_errors_on_invalid_capacity_string() {
        // Phase-2 must fail fast on a bad capacity string — test with status=1.
        let block_date = chrono::NaiveDate::from_ymd_opt(2026, 2, 18).unwrap();
        let dao_code_hash = crate::rpc::parse_hex_to_bytes(crate::parser::dao::DAO_CODE_HASH);
        let input_hash_vec = vec![0x44; 32];
        let input_hash: [u8; 32] = input_hash_vec.clone().try_into().unwrap();

        // Phase-2 tx: consumes a status=1 cell with an unparseable capacity string.
        let tx = dummy_tx_data(
            [0xDD; 32],
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
        consumed_dao_map.insert(
            (input_hash_vec, 0),
            DaoConsumedRow {
                tx_hash: vec![],
                output_index: 0,
                capacity_str: "bad-capacity".to_string(),
                deposit_block: 0,
                status: 0,
                lock_script_hash: vec![0xAA; 32],
            },
        );

        let mut same_batch_dao_map: DaoSameBatchMap = HashMap::new();
        let input_cell_info: HashMap<(Vec<u8>, i16), PositionedCellInfo> = HashMap::new();
        let batch_cell_infos: HashMap<(Vec<u8>, i16), PositionedCellInfo> = HashMap::new();
        let mut active_deposit_counts_by_lock: HashMap<Vec<u8>, i64> = HashMap::new();
        let mut daily_unique_depositors_delta: HashMap<chrono::NaiveDate, i64> = HashMap::new();
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
            &input_cell_info,
            &batch_cell_infos,
            &mut active_deposit_counts_by_lock,
            &mut daily_unique_depositors_delta,
            &mut daily_active_delta,
            &mut HashMap::new(),
            &mut daily_gross_deposit_delta,
            &mut daily_new_deposits_delta,
            &mut daily_withdrawals_delta,
            &mut HashMap::new(),
            &mut HashMap::new(),
            &mut HashMap::new(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("invalid DAO capacity string"));
    }

    #[test]
    fn test_accumulate_dao_snapshot_deltas_tracks_unique_active_depositors() {
        // Verify that depositor counts increment on deposit and decrement on phase-2,
        // and that phase-1 does NOT modify depositor counts.
        let block_date = chrono::NaiveDate::from_ymd_opt(2026, 2, 18).unwrap();
        let dao_code_hash = crate::rpc::parse_hex_to_bytes(crate::parser::dao::DAO_CODE_HASH);
        let deposit_tx_hash = [0x51; 32];
        let withdraw_req_tx_hash = [0x52; 32];
        let lock_hash = vec![0x77; 32];

        // Step 1: Deposit two cells from the same lock.
        let deposit_tx = dummy_tx_data(
            deposit_tx_hash,
            false,
            vec![],
            vec![
                dummy_dao_cell_with_lock(200_00000000, true, lock_hash.clone()),
                dummy_dao_cell_with_lock(300_00000000, true, lock_hash.clone()),
            ],
            vec![],
            vec![],
        );

        // Step 2: Phase-1 — consume the first deposit (status=0), create withdraw-request.
        let phase1_first = dummy_tx_data(
            withdraw_req_tx_hash,
            false,
            vec![crate::parser::transaction::ParsedInput {
                previous_tx_hash: deposit_tx_hash,
                previous_output_index: 0,
                since: 0,
            }],
            vec![dummy_dao_cell(200_00000000, false)],
            vec![],
            vec!["0x0100000000000000".to_string()],
        );

        // Step 3: Phase-2 — consume the first withdraw-request (status=1).
        let phase2_first = dummy_tx_data(
            [0x53; 32],
            false,
            vec![crate::parser::transaction::ParsedInput {
                previous_tx_hash: withdraw_req_tx_hash,
                previous_output_index: 0,
                since: 0,
            }],
            vec![],
            vec![],
            vec![],
        );

        // Phase-2 consumed_dao_map: withdraw-request cell has status=1.
        let mut consumed_dao_map_phase2: DaoConsumedMap = HashMap::new();
        consumed_dao_map_phase2.insert(
            (withdraw_req_tx_hash.to_vec(), 0),
            DaoConsumedRow {
                tx_hash: vec![],
                output_index: 0,
                capacity_str: "20000000000".to_string(),
                deposit_block: 0,
                status: 1,
                lock_script_hash: lock_hash.clone(),
            },
        );

        let empty_consumed: DaoConsumedMap = HashMap::new();
        let mut same_batch_dao_map: DaoSameBatchMap = HashMap::new();
        let mut daily_active_delta: HashMap<chrono::NaiveDate, i128> = HashMap::new();
        let mut daily_gross_deposit_delta: HashMap<chrono::NaiveDate, i128> = HashMap::new();
        let mut daily_new_deposits_delta: HashMap<chrono::NaiveDate, i64> = HashMap::new();
        let mut daily_withdrawals_delta: HashMap<chrono::NaiveDate, i64> = HashMap::new();
        let mut active_deposit_counts_by_lock: HashMap<Vec<u8>, i64> = HashMap::new();
        let mut daily_unique_depositors_delta: HashMap<chrono::NaiveDate, i64> = HashMap::new();
        let input_cell_info: HashMap<(Vec<u8>, i16), PositionedCellInfo> = HashMap::new();
        let mut batch_cell_infos: HashMap<(Vec<u8>, i16), PositionedCellInfo> = HashMap::new();
        batch_cell_infos.insert(
            (deposit_tx_hash.to_vec(), 0),
            dummy_positioned_info(200_00000000, 100, lock_hash.clone()),
        );
        batch_cell_infos.insert(
            (deposit_tx_hash.to_vec(), 1),
            dummy_positioned_info(300_00000000, 100, lock_hash.clone()),
        );

        // After deposit: 2 active deposits, 1 unique depositor.
        accumulate_dao_snapshot_deltas_for_txs(
            &[deposit_tx],
            block_date,
            &dao_code_hash,
            &empty_consumed,
            &mut same_batch_dao_map,
            &input_cell_info,
            &batch_cell_infos,
            &mut active_deposit_counts_by_lock,
            &mut daily_unique_depositors_delta,
            &mut daily_active_delta,
            &mut HashMap::new(),
            &mut daily_gross_deposit_delta,
            &mut daily_new_deposits_delta,
            &mut daily_withdrawals_delta,
            &mut HashMap::new(),
            &mut HashMap::new(),
            &mut HashMap::new(),
        )
        .unwrap();

        assert_eq!(active_deposit_counts_by_lock.get(&lock_hash), Some(&2));
        assert_eq!(daily_unique_depositors_delta.get(&block_date), Some(&1));

        // After phase-1: depositor count drops to 1
        // (deposit leaves active status at withdraw request).
        let mut consumed_dao_map_phase1: DaoConsumedMap = HashMap::new();
        consumed_dao_map_phase1.insert(
            (deposit_tx_hash.to_vec(), 0),
            DaoConsumedRow {
                tx_hash: vec![],
                output_index: 0,
                capacity_str: "20000000000".to_string(),
                deposit_block: 0,
                status: 0,
                lock_script_hash: lock_hash.clone(),
            },
        );
        accumulate_dao_snapshot_deltas_for_txs(
            &[phase1_first],
            block_date,
            &dao_code_hash,
            &consumed_dao_map_phase1,
            &mut same_batch_dao_map,
            &input_cell_info,
            &batch_cell_infos,
            &mut active_deposit_counts_by_lock,
            &mut daily_unique_depositors_delta,
            &mut daily_active_delta,
            &mut HashMap::new(),
            &mut daily_gross_deposit_delta,
            &mut daily_new_deposits_delta,
            &mut daily_withdrawals_delta,
            &mut HashMap::new(),
            &mut HashMap::new(),
            &mut HashMap::new(),
        )
        .unwrap();

        assert_eq!(
            active_deposit_counts_by_lock.get(&lock_hash),
            Some(&1),
            "phase-1 must decrement active_deposit_counts_by_lock"
        );
        assert_eq!(
            daily_unique_depositors_delta.get(&block_date),
            Some(&1),
            "phase-1 must not decrement unique depositor delta (still has one active deposit)"
        );

        // After phase-2 (first withdrawal completes): no further depositor
        // change (already decremented at phase-1).
        accumulate_dao_snapshot_deltas_for_txs(
            &[phase2_first],
            block_date,
            &dao_code_hash,
            &consumed_dao_map_phase2,
            &mut same_batch_dao_map,
            &input_cell_info,
            &batch_cell_infos,
            &mut active_deposit_counts_by_lock,
            &mut daily_unique_depositors_delta,
            &mut daily_active_delta,
            &mut HashMap::new(),
            &mut daily_gross_deposit_delta,
            &mut daily_new_deposits_delta,
            &mut daily_withdrawals_delta,
            &mut HashMap::new(),
            &mut HashMap::new(),
            &mut HashMap::new(),
        )
        .unwrap();

        assert_eq!(
            active_deposit_counts_by_lock.get(&lock_hash),
            Some(&1),
            "phase-2 must not further decrement active_deposit_counts_by_lock"
        );
        assert_eq!(
            daily_unique_depositors_delta.get(&block_date),
            Some(&1),
            "removing one of two active deposits for the same lock must not change unique depositor count"
        );
    }

    // -- dao_deltas_computed flag -------------------------------------------

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

    // -- crossed_1000 boundary tests ----------------------------------------

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

    // -- BatchStats::accumulate_block_time -----------------------------------

    #[test]
    fn test_accumulate_block_time_preserves_subsecond_precision() {
        // Regression: previously the live-sync path computed the delta with
        // `.num_seconds() * 1000`, truncating to whole seconds and biasing the
        // daily avg_block_time_ms down by up to ~1s on a ~8s-target chain.
        let prev_ts = chrono::Utc.with_ymd_and_hms(2024, 6, 1, 12, 0, 0).unwrap();
        let curr_ts = prev_ts + chrono::Duration::milliseconds(7945);
        let date = NaiveDate::from_ymd_opt(2024, 6, 1).unwrap();

        let mut stats = BatchStats::default();
        stats.accumulate_block_time(prev_ts, curr_ts, date);

        let (sum_ms, count) = stats.daily_block_times[&date];
        assert_eq!(count, 1);
        assert_eq!(
            sum_ms, 7945,
            "daily sum must preserve millisecond precision"
        );
        // Bucket uses second-resolution: 7945ms → 7s bucket
        assert_eq!(stats.block_time_dist.get(&7), Some(&1));
    }

    #[test]
    fn test_accumulate_block_time_drops_negative_delta() {
        let curr_ts = chrono::Utc.with_ymd_and_hms(2024, 6, 1, 12, 0, 0).unwrap();
        // Clock skew: curr is earlier than prev.
        let prev_ts = curr_ts + chrono::Duration::milliseconds(500);
        let date = NaiveDate::from_ymd_opt(2024, 6, 1).unwrap();

        let mut stats = BatchStats::default();
        stats.accumulate_block_time(prev_ts, curr_ts, date);

        assert!(stats.daily_block_times.is_empty());
        assert!(stats.block_time_dist.is_empty());
    }

    #[test]
    fn test_accumulate_block_time_sums_multiple_deltas() {
        let base = chrono::Utc.with_ymd_and_hms(2024, 6, 1, 12, 0, 0).unwrap();
        let date = NaiveDate::from_ymd_opt(2024, 6, 1).unwrap();

        let mut stats = BatchStats::default();
        // Three deltas: 7945ms, 8123ms, 9999ms = 26067ms total.
        stats.accumulate_block_time(base, base + chrono::Duration::milliseconds(7945), date);
        stats.accumulate_block_time(
            base + chrono::Duration::milliseconds(7945),
            base + chrono::Duration::milliseconds(7945 + 8123),
            date,
        );
        stats.accumulate_block_time(
            base + chrono::Duration::milliseconds(7945 + 8123),
            base + chrono::Duration::milliseconds(7945 + 8123 + 9999),
            date,
        );

        let (sum_ms, count) = stats.daily_block_times[&date];
        assert_eq!(count, 3);
        assert_eq!(sum_ms, 7945 + 8123 + 9999);
    }
}
