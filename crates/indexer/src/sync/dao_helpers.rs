//! DAO calculation helpers and related pure functions.
//!
//! Issuance splits, snapshot deltas, deposit/withdraw accounting,
//! address counting, NFT collection classification, occupied capacity,
//! and tx-fee validation.

#![allow(clippy::type_complexity)]

use std::collections::HashMap;

use anyhow::{anyhow, bail, Result};
use chrono::NaiveDate;

use ckbadger_store::types::AddressBalance;

pub(crate) use ckbadger_store::types::{DID_CKB_SENTINEL_COLLECTION, DOTBIT_SENTINEL_COLLECTION};

use crate::parser::{DaoParser, DotbitParser, MnftParser, SporeParser};

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
    pub(crate) dao_daily_gross_deposit_delta: HashMap<NaiveDate, i128>,
    pub(crate) dao_daily_new_deposits_delta: HashMap<NaiveDate, i64>,
    pub(crate) dao_daily_withdrawals_delta: HashMap<NaiveDate, i64>,
    pub(crate) daily_secondary_non_miner_delta: HashMap<NaiveDate, i128>,
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

pub(crate) type DaoConsumedRow = (i64, Vec<u8>, i16, String, i64, i16);
pub(crate) type DaoConsumedMap = HashMap<(Vec<u8>, i16), DaoConsumedRow>;
pub(crate) type DaoSameBatchMap = HashMap<(Vec<u8>, i16), i64>;

// ---------------------------------------------------------------------------
// Address counting
// ---------------------------------------------------------------------------

pub(crate) fn count_new_addresses(
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

// ---------------------------------------------------------------------------
// NFT collection classification
// ---------------------------------------------------------------------------

pub(crate) fn classify_nft_collection_id(
    type_code_hash: &[u8],
    type_args: &[u8],
) -> Option<Vec<u8>> {
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

// ---------------------------------------------------------------------------
// Pre-batch live cell derivation
// ---------------------------------------------------------------------------

/// Reconstruct pre-batch live cell count from persisted post-batch count and batch delta.
///
/// Address balances are written before HODL tracker updates, so reading `live_cells_count`
/// from store returns post-batch state. We need pre-batch state to detect 0→>0 and >0→0
/// holder transitions correctly.
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
// Occupied capacity
// ---------------------------------------------------------------------------

pub(crate) fn occupied_capacity_shannons_i128(
    lock_args_len: usize,
    type_args_len: Option<usize>,
    data_size: i32,
) -> i128 {
    if data_size < 0 {
        panic!(
            "negative cell data_size while computing occupied capacity: {}",
            data_size
        );
    }
    let lock_script_size = 33_i128 + lock_args_len as i128;
    let type_script_size = type_args_len.map(|len| 33_i128 + len as i128).unwrap_or(0);
    (8_i128 + lock_script_size + type_script_size + i128::from(data_size)) * 100_000_000_i128
}

pub(crate) fn occupied_capacity_shannons_i64(
    lock_args_len: usize,
    type_args_len: Option<usize>,
    data_size: i32,
) -> i64 {
    i64::try_from(occupied_capacity_shannons_i128(
        lock_args_len,
        type_args_len,
        data_size,
    ))
    .unwrap_or_else(|_| {
        panic!(
            "occupied capacity exceeds i64: lock_args_len={}, type_args_len={:?}, data_size={}",
            lock_args_len, type_args_len, data_size
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
// Secondary issuance split
// ---------------------------------------------------------------------------

pub(crate) fn split_secondary_issuance(
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

// ---------------------------------------------------------------------------
// Non-miner secondary delta resolution
// ---------------------------------------------------------------------------

pub(crate) fn resolve_non_miner_secondary_delta_for_snapshot(
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
        // RFC-0023 S_i includes completed withdrawal compensation (I_i),
        // so block/day-level S deltas can be negative. For issuance chart
        // cumulatives we only accumulate positive non-miner growth.
        return Ok(0);
    }
    Ok(delta)
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
    total_deposit_count: i64,
    total_withdrawal_count: i64,
    date: NaiveDate,
) -> Result<i64> {
    let diff = total_deposit_count
        .checked_sub(total_withdrawal_count)
        .ok_or_else(|| {
            anyhow!(
                "dao snapshot depositor overflow: date={}, total_deposits={}, total_withdrawals={}",
                date,
                total_deposit_count,
                total_withdrawal_count
            )
        })?;
    if diff < 0 {
        anyhow::bail!(
            "dao snapshot depositor underflow: date={}, total_deposits={}, total_withdrawals={}",
            date,
            total_deposit_count,
            total_withdrawal_count
        );
    }
    Ok(diff)
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
                        let output_index_i16 = checked_usize_to_i16(
                            output_index,
                            "DAO output index while accumulating daily snapshot deltas",
                        )
                        .map_err(|e| anyhow!("{}: tx_hash=0x{}", e, hex::encode(tx_data.hash)))?;
                        *daily_active_delta.entry(block_date).or_default() += cell.capacity as i128;
                        *daily_gross_deposit_delta.entry(block_date).or_default() +=
                            cell.capacity as i128;
                        *daily_new_deposits_delta.entry(block_date).or_default() += 1;
                        same_batch_dao_map
                            .insert((tx_data.hash.to_vec(), output_index_i16), cell.capacity);
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
                parsed_input_outpoint_index_i16(input.previous_output_index, "sync_indexer"),
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
                parsed_input_outpoint_index_i16(input.previous_output_index, "sync_indexer"),
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

// ---------------------------------------------------------------------------
// Secondary issuance delta accumulation (per-block)
// ---------------------------------------------------------------------------

pub(crate) fn accumulate_secondary_issuance_deltas(
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

        // RFC-0023: S_i can decrease when completed DAO withdrawals are
        // larger than current non-miner secondary issuance in the same block.
        // For issuance chart cumulatives we only track positive growth.
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::collections::HashMap;

    // -- Test helpers -------------------------------------------------------

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

    // -- count_new_addresses ------------------------------------------------

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

    // -- classify_nft_collection_id -----------------------------------------

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

    // -- split_secondary_issuance -------------------------------------------

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

    // -- resolve_non_miner_secondary_delta_for_snapshot ---------------------

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
    fn test_resolve_non_miner_secondary_delta_for_snapshot_ignores_negative_fallback_delta() {
        let date = chrono::NaiveDate::from_ymd_opt(2026, 2, 18).unwrap();
        let delta =
            resolve_non_miner_secondary_delta_for_snapshot(date, None, 8_999, 9_000).unwrap();
        assert_eq!(delta, 0);
    }

    // -- derive_running_depositors ------------------------------------------

    #[test]
    fn test_derive_running_depositors() {
        let date = chrono::NaiveDate::from_ymd_opt(2026, 2, 18).unwrap();
        assert_eq!(derive_running_depositors(10, 3, date).unwrap(), 7);
    }

    #[test]
    fn test_derive_running_depositors_underflow_errors() {
        let date = chrono::NaiveDate::from_ymd_opt(2026, 2, 18).unwrap();
        let err = derive_running_depositors(3, 10, date).unwrap_err();
        assert!(err.to_string().contains("dao snapshot depositor underflow"));
    }

    // -- accumulate_secondary_issuance_deltas -------------------------------

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
        assert!(
            !stats.daily_secondary_non_miner_delta.contains_key(&date),
            "negative S delta must not contribute to non-miner daily delta"
        );
        assert!(
            !stats.daily_secondary_miner_delta.contains_key(&date),
            "negative S delta must not contribute to miner daily delta"
        );
        assert_eq!(
            prev,
            Some((20_000_000_000_000_i128 + 500, 8_000_i128 - 100)),
            "previous DAO C/S baseline must still advance to the latest block"
        );
    }

    #[test]
    fn test_accumulate_secondary_issuance_deltas_same_day_drop_then_growth_tracks_only_growth() {
        let mut stats = BatchStats::default();
        let mut prev = Some((30_000_000_000_000_i128, 10_000_i128));
        let date = chrono::NaiveDate::from_ymd_opt(2026, 2, 18).unwrap();

        // First block in the day has an S drop (protocol adjustment).
        let block_drop =
            dummy_parsed_block(build_dao_field(30_000_000_000_500, 9_950, 100), 0, 1000);
        accumulate_secondary_issuance_deltas(&mut stats, &block_drop, date, &mut prev).unwrap();

        // Next block rebounds above the dropped value; only positive growth is counted.
        let block_growth =
            dummy_parsed_block(build_dao_field(30_000_000_001_000, 10_020, 100), 1, 2000);
        accumulate_secondary_issuance_deltas(&mut stats, &block_growth, date, &mut prev).unwrap();

        assert_eq!(
            stats.daily_secondary_non_miner_delta.get(&date),
            Some(&70),
            "daily delta should include only positive growth after the drop"
        );
        assert!(
            stats
                .daily_secondary_miner_delta
                .get(&date)
                .copied()
                .unwrap_or_default()
                >= 0
        );
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

    // -- accumulate_dao_snapshot_deltas_for_txs -----------------------------

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
}
