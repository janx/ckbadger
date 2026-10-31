//! L1 (J1): `dao_live_agg` — aggregates over the live NervosDAO cells, so
//! `/dao/evidence` and `/dao/depositors` are served without scanning
//! deposits.
//!
//! Source rows: the `dao_deposits` lifecycle entries with status 0 (the
//! deposit cell is live) and 1 (the withdraw-request cell is live), exactly
//! the rows `/dao/evidence` used to scan per tip. A row contributes (see
//! [`dao_row_contributions`], the one definition):
//!
//! | status | keys                                                            |
//! | ------ | --------------------------------------------------------------- |
//! | 0      | global `[cap, 1, 0, 0]`, depositor(lock) `[cap, 1]`, phase-0 epoch(deposit epoch) `[cap, 1]` |
//! | 1      | global `[0, 0, cap, 1]`, phase-1 epochs(deposit epoch, request epoch) `[cap, 1]` |
//! | 2      | nothing (the claimed deposit has no live DAO cell)              |
//!
//! Capacities are exact shannons. The epochs come from the canonical headers
//! of the deposit and request blocks, so the API needs no header per deposit.

use std::collections::HashMap;

use anyhow::{anyhow, bail, ensure, Context, Result};
use ckbadger_store::derive_ops::dao_live_agg::{self, EpochPoint};
use ckbadger_store::derive_ops::{DerivedAggregateId, ReadSnapshot, DAO_LIVE_AGG};
use ckbadger_store::types::{CachedBlockHeader, DaoDepositCacheEntry};
use ckbadger_store::{keys, CF_DAO_BY_STATUS_BLOCK, CF_DAO_DEPOSITS};

use super::{
    add_delta, resume_key, BlockDeltas, ChunkBudget, DeltaMap, DerivedAggregate, LiveDeriveCtx,
};
use crate::parser::dao::DaoParser;

pub struct DaoLiveAgg;

/// The epoch position of a header.
pub(crate) fn header_epoch(header: &CachedBlockHeader, block: i64) -> Result<EpochPoint> {
    EpochPoint::new(
        header.epoch_number,
        i64::from(header.epoch_index),
        i64::from(header.epoch_length),
    )
    .with_context(|| format!("DAO header epoch at block {block}"))
}

/// What one lifecycle row contributes to `dao_live_agg`. `request_epoch` is
/// required exactly for status 1.
pub(crate) fn dao_row_contributions(
    outpoint_key: &[u8],
    entry: &DaoDepositCacheEntry,
    deposit_epoch: EpochPoint,
    request_epoch: Option<EpochPoint>,
) -> Result<Vec<(Vec<u8>, Vec<i128>)>> {
    let context = || {
        format!(
            "DAO lifecycle at outpoint_key=0x{}",
            hex::encode(outpoint_key)
        )
    };
    ensure!(
        entry.capacity > 0,
        "invalid DAO capacity {} ({})",
        entry.capacity,
        context()
    );
    let capacity = i128::from(entry.capacity);
    Ok(match entry.status {
        0 => vec![
            (dao_live_agg::global_key(), vec![capacity, 1, 0, 0]),
            (
                dao_live_agg::depositor_key(&entry.lock_script_hash).with_context(context)?,
                vec![capacity, 1],
            ),
            (
                dao_live_agg::phase0_epoch_key(deposit_epoch),
                vec![capacity, 1],
            ),
        ],
        1 => {
            let request_epoch = request_epoch.ok_or_else(|| {
                anyhow!(
                    "phase-1 DAO deposit without a request epoch ({})",
                    context()
                )
            })?;
            vec![
                (dao_live_agg::global_key(), vec![0, 0, capacity, 1]),
                (
                    dao_live_agg::phase1_epochs_key(deposit_epoch, request_epoch),
                    vec![capacity, 1],
                ),
            ]
        }
        2 => Vec::new(),
        status => bail!("invalid DAO status {status} ({})", context()),
    })
}

fn negate(contributions: Vec<(Vec<u8>, Vec<i128>)>) -> Vec<(Vec<u8>, Vec<i128>)> {
    contributions
        .into_iter()
        .map(|(key, value)| (key, value.into_iter().map(|c| -c).collect()))
        .collect()
}

#[derive(Clone, Copy)]
struct HeaderPoint {
    epoch: EpochPoint,
    ar: i64,
    timestamp: i64,
    chain_time_ms: i64,
}

fn header_point(header: &CachedBlockHeader, block: i64) -> Result<HeaderPoint> {
    let ar = DaoParser::extract_ar_from_dao_field(&header.dao)
        .ok_or_else(|| anyhow!("missing AR in DAO field at block {block}"))?;
    Ok(HeaderPoint {
        epoch: header_epoch(header, block)?,
        ar: i64::try_from(ar).map_err(|_| anyhow!("DAO AR exceeds i64 at block {block}"))?,
        timestamp: header.timestamp,
        chain_time_ms: header.chain_time_ms,
    })
}

/// Rebuild-time integrity of a stored lifecycle row against the canonical
/// headers it names (the checks `/dao/evidence` ran on every request before
/// this aggregate existed).
fn validate_stored_row(
    outpoint_key: &[u8],
    entry: &DaoDepositCacheEntry,
    tip: i64,
    tip_ar: u64,
    deposit: &HeaderPoint,
    request: Option<&HeaderPoint>,
) -> Result<()> {
    let key = hex::encode(outpoint_key);
    ckbadger_store::dao_compensation_for_entry_at(entry, tip, tip_ar)
        .map_err(|error| anyhow!("invalid DAO lifecycle at outpoint_key=0x{key}: {error:#}"))?;
    ensure!(
        entry.deposit_block_number >= 0 && entry.deposit_block_number <= tip,
        "DAO deposit outside canonical tip: outpoint_key=0x{key}, deposit_block={}, tip={tip}",
        entry.deposit_block_number
    );
    ensure!(
        entry.lock_script_hash.len() == 32,
        "invalid DAO lock hash at outpoint_key=0x{key}"
    );
    ensure!(
        entry.deposit_ar == deposit.ar,
        "DAO deposit AR differs from canonical header at outpoint_key=0x{key}: stored={}, block_ar={}",
        entry.deposit_ar,
        deposit.ar
    );
    ensure!(
        entry.deposit_timestamp == deposit.timestamp
            && entry.deposit_chain_time_ms == deposit.chain_time_ms,
        "DAO deposit time differs from canonical header at outpoint_key=0x{key}: stored_raw={}, header_raw={}, stored_chain={}, header_chain={}",
        entry.deposit_timestamp,
        deposit.timestamp,
        entry.deposit_chain_time_ms,
        deposit.chain_time_ms
    );
    match entry.status {
        0 => ensure!(
            entry.withdraw_request_occupied_capacity.is_none(),
            "phase-0 DAO deposit has request occupied marker at outpoint_key=0x{key}"
        ),
        1 => {
            let request_block = entry.withdraw_request_block.ok_or_else(|| {
                anyhow!("phase-1 DAO deposit missing request block at outpoint_key=0x{key}")
            })?;
            ensure!(
                request_block > entry.deposit_block_number && request_block <= tip,
                "DAO request block outside lifecycle at outpoint_key=0x{key}: deposit={}, request={request_block}, tip={tip}",
                entry.deposit_block_number
            );
            ensure!(
                entry
                    .withdraw_request_tx
                    .as_ref()
                    .is_some_and(|tx| tx.len() == 32)
                    && entry
                        .withdraw_request_output_index
                        .is_some_and(|index| index >= 0),
                "DAO request outpoint malformed at outpoint_key=0x{key}"
            );
            let request = request.expect("status-1 rows resolve their request header");
            ensure!(
                entry.withdraw_request_ar == Some(request.ar),
                "DAO request AR differs from canonical header at outpoint_key=0x{key}: stored={:?}, block_ar={}",
                entry.withdraw_request_ar,
                request.ar
            );
        }
        status => bail!("invalid live DAO status {status} at outpoint_key=0x{key}"),
    }
    Ok(())
}

impl DerivedAggregate for DaoLiveAgg {
    fn id(&self) -> DerivedAggregateId {
        DAO_LIVE_AGG
    }

    /// Walks `dao_by_status_block` for statuses 0 and 1 (key = status(2) +
    /// block_desc(8) + outpoint(34)), loading each lifecycle row and its
    /// headers from the snapshot.
    fn rebuild_chunk(
        &self,
        snapshot: &ReadSnapshot<'_>,
        cursor: Option<&[u8]>,
        budget: &mut ChunkBudget,
        out: &mut DeltaMap,
    ) -> Result<Option<Vec<u8>>> {
        let Some((tip, tip_header)) = snapshot.tip()? else {
            return Ok(None);
        };
        let tip_ar = DaoParser::extract_ar_from_dao_field(&tip_header.dao)
            .ok_or_else(|| anyhow!("missing AR in DAO field at tip block {tip}"))?;
        let mut headers: HashMap<i64, HeaderPoint> = HashMap::new();
        let mut header_at = |block: i64| -> Result<HeaderPoint> {
            if let Some(point) = headers.get(&block) {
                return Ok(*point);
            }
            let header = snapshot
                .get_block_header(block)?
                .ok_or_else(|| anyhow!("missing canonical DAO header at block {block}"))?;
            let point = header_point(&header, block)?;
            headers.insert(block, point);
            Ok(point)
        };
        let start = resume_key(cursor);
        for item in snapshot.iter_from(CF_DAO_BY_STATUS_BLOCK, &start)? {
            let (index_key, _) = item.map_err(|e| anyhow!("iterating dao_by_status_block: {e}"))?;
            ensure!(
                index_key.len() == keys::DAO_BY_STATUS_BLOCK_KEY_SIZE,
                "dao_by_status_block key has {} bytes",
                index_key.len()
            );
            let status = i16::from_be_bytes([index_key[0], index_key[1]]);
            if status > 1 {
                return Ok(None);
            }
            let outpoint_key = &index_key[10..];
            let raw = snapshot
                .get(CF_DAO_DEPOSITS, outpoint_key)?
                .ok_or_else(|| {
                    anyhow!(
                        "dao_by_status_block row has no lifecycle entry: outpoint_key=0x{}",
                        hex::encode(outpoint_key)
                    )
                })?;
            let entry: DaoDepositCacheEntry = bincode::deserialize(&raw).map_err(|e| {
                anyhow!(
                    "failed to decode DAO lifecycle at outpoint_key=0x{}: {e}",
                    hex::encode(outpoint_key)
                )
            })?;
            ensure!(
                entry.status == status,
                "dao_by_status_block names status {status} but the lifecycle entry has {}: outpoint_key=0x{}",
                entry.status,
                hex::encode(outpoint_key)
            );
            let deposit = header_at(entry.deposit_block_number)?;
            let request = match (entry.status, entry.withdraw_request_block) {
                (1, Some(block)) => Some(header_at(block)?),
                _ => None,
            };
            validate_stored_row(
                outpoint_key,
                &entry,
                tip,
                tip_ar,
                &deposit,
                request.as_ref(),
            )?;
            for (key, delta) in dao_row_contributions(
                outpoint_key,
                &entry,
                deposit.epoch,
                request.map(|point| point.epoch),
            )? {
                add_delta(out, key, &delta)?;
            }
            if budget.tick(out.len()) {
                return Ok(Some(index_key.to_vec()));
            }
        }
        Ok(None)
    }

    fn live_deltas(&self, ctx: &LiveDeriveCtx<'_>, out: &mut BlockDeltas) -> Result<()> {
        for transition in ctx.dao_transitions {
            let mut contributions = Vec::new();
            if let Some(old) = &transition.old {
                contributions.extend(negate(entry_contributions(
                    ctx,
                    &transition.deposit_key,
                    old,
                )?));
            }
            contributions.extend(entry_contributions(
                ctx,
                &transition.deposit_key,
                &transition.new,
            )?);
            let block = out.entry(transition.block).or_default();
            for (key, delta) in contributions {
                add_delta(block, key, &delta)?;
            }
        }
        Ok(())
    }

    fn check_row(&self, key: &[u8], value: &[i128]) -> Result<()> {
        let positive_pair = |value: &[i128]| -> Result<()> {
            ensure!(
                value.len() == 2 && value[0] > 0 && value[1] > 0,
                "expected [capacity > 0, cells > 0], got {value:?}"
            );
            Ok(())
        };
        match (key.first().copied(), key.len()) {
            (Some(dao_live_agg::KIND_GLOBAL), 1) => {
                ensure!(
                    value.len() == 4,
                    "global row has {} components",
                    value.len()
                );
                for pair in value.chunks(2) {
                    ensure!(
                        pair[0] >= 0 && pair[1] >= 0 && (pair[0] == 0) == (pair[1] == 0),
                        "global row capacity/cells disagree: {value:?}"
                    );
                }
                Ok(())
            }
            (Some(dao_live_agg::KIND_DEPOSITOR), 33) => positive_pair(value),
            (Some(dao_live_agg::KIND_PHASE0_EPOCH), 17) => {
                EpochPoint::decode(&key[1..])?;
                positive_pair(value)
            }
            (Some(dao_live_agg::KIND_PHASE1_EPOCHS), 33) => {
                EpochPoint::decode(&key[1..17])?;
                EpochPoint::decode(&key[17..])?;
                positive_pair(value)
            }
            _ => bail!("unknown dao_live_agg key 0x{}", hex::encode(key)),
        }
    }
}

/// The epoch of `block`: the live batch's own header when the block is in
/// it, else the committed canonical header.
fn epoch_at(ctx: &LiveDeriveCtx<'_>, block: i64) -> Result<EpochPoint> {
    if let Ok(index) = ctx
        .blocks
        .binary_search_by_key(&block, |point| point.number)
    {
        let point = ctx.blocks[index];
        return EpochPoint::new(
            point.epoch_number,
            i64::from(point.epoch_index),
            i64::from(point.epoch_length),
        )
        .with_context(|| format!("DAO header epoch at block {block}"));
    }
    let header = ctx
        .store
        .get_block_header(block)?
        .ok_or_else(|| anyhow!("missing canonical DAO header at block {block}"))?;
    header_epoch(&header, block)
}

fn entry_contributions(
    ctx: &LiveDeriveCtx<'_>,
    deposit_key: &[u8],
    entry: &DaoDepositCacheEntry,
) -> Result<Vec<(Vec<u8>, Vec<i128>)>> {
    if entry.status == 2 {
        return Ok(Vec::new());
    }
    let deposit_epoch = epoch_at(ctx, entry.deposit_block_number)?;
    let request_epoch = match (entry.status, entry.withdraw_request_block) {
        (1, Some(block)) => Some(epoch_at(ctx, block)?),
        (1, None) => bail!(
            "phase-1 DAO deposit missing request block at outpoint_key=0x{}",
            hex::encode(deposit_key)
        ),
        _ => None,
    };
    dao_row_contributions(deposit_key, entry, deposit_epoch, request_epoch)
}
