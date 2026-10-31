//! Read-only, chain-anchored evidence for the NervosDAO topic page.

use axum::extract::{Query, State};
use ckbadger_store::{types::CachedBlockHeader, CkbadgerStore};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use ckbadger_store::derive_ops::dao_live_agg::{self, EpochPoint};
use ckbadger_store::derive_ops::DAO_LIVE_AGG;
use ckbadger_store::CF_DAO_LIVE_AGG;

use super::dao_epoch::{next_phase0_boundary, nominal_day_bucket, phase1_ready_epoch, Epoch};
use crate::derived::require_derived;
use crate::response::{ok, ApiError, ApiResult, ApiRouteError};
use crate::utils::address::{compute_script_hash, resolve_lock_address};
use crate::utils::exact_ratio::format_percent;
use crate::utils::script_to_address;
use crate::AppState;

const DAY_MS: i64 = 86_400_000;
const YEAR_365_MS: i64 = 365 * DAY_MS;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaoEpochResponse {
    pub number: i64,
    pub index: i64,
    pub length: i64,
    pub display: String,
}

impl From<Epoch> for DaoEpochResponse {
    fn from(value: Epoch) -> Self {
        Self {
            number: value.number,
            index: value.index,
            length: value.length,
            display: value.display(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaoAmountCount {
    pub capacity: String,
    pub count: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaoWindowBucket {
    /// One of 30 nominal days. Each bucket spans six exact epoch units.
    pub nominal_day: usize,
    pub from_epoch_offset: u16,
    pub through_epoch_offset: u16,
    pub capacity: String,
    pub count: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaoOwner {
    pub rank: usize,
    pub lock_script_hash: String,
    pub address: String,
    pub lock_code_hash: String,
    pub lock_hash_type: String,
    pub lock_kind: Option<String>,
    pub deposit_count: u64,
    pub capacity: String,
    pub share_percent: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaoConcentration {
    pub current_depositors: usize,
    pub top1_capacity: String,
    pub top10_capacity: String,
    pub top100_capacity: String,
    pub top1_share_percent: Option<String>,
    pub top10_share_percent: Option<String>,
    pub top100_share_percent: Option<String>,
    pub owners: Vec<DaoOwner>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaoArEvidence {
    pub tip_ar: String,
    pub genesis_ar: String,
    pub genesis_block_number: i64,
    pub tip_chain_time_ms: String,
    pub since_genesis_growth_percent: String,
    pub year_365: Option<DaoYearArEvidence>,
    pub milestones: Vec<DaoArMilestone>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaoArMilestone {
    pub block_number: i64,
    pub chain_time_ms: String,
    pub ar: String,
    pub ar_index: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaoYearArEvidence {
    pub anchor_block_number: i64,
    pub anchor_chain_time_ms: String,
    pub anchor_ar: String,
    pub elapsed_chain_time_ms: String,
    pub observed_growth_percent: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaoHeaderEvidence {
    pub block_number: i64,
    pub dao_hex: String,
    pub c: String,
    pub ar: String,
    pub s: String,
    pub u: String,
}

/// The phase-0 depositor ranking at one canonical tip (number and hash), read
/// from `dao_live_agg`'s depositor rows (J1) — the one ranking `/dao/evidence`
/// cuts its concentration from and `/dao/depositors` pages by rank (J2).
/// API memory only; the rest of the evidence is read per request from the
/// aggregate's global and epoch rows.
pub struct DaoDepositorRanking {
    /// Every current depositor, largest phase-0 capacity first (ties by lock
    /// hash): `(lock hash, capacity, deposit cells)`.
    ranking: Vec<(Vec<u8>, i128, u64)>,
    phase0_capacity: i128,
    phase0_count: u64,
    tip_hash: Vec<u8>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaoEvidenceResponse {
    pub tip_block_number: i64,
    pub tip_epoch: DaoEpochResponse,
    pub phase0: DaoAmountCount,
    pub phase1: DaoAmountCount,
    pub phase1_ready_by_epoch_lock: DaoAmountCount,
    pub phase1_waiting_for_epoch_lock: DaoAmountCount,
    pub windows: Vec<DaoWindowBucket>,
    pub concentration: DaoConcentration,
    pub ar: DaoArEvidence,
    pub header: DaoHeaderEvidence,
}

#[derive(Clone, Copy, Debug, Default)]
struct AmountCount {
    capacity: i128,
    count: u64,
}

impl AmountCount {
    /// Add one aggregate row `[capacity, cells]` (both positive).
    fn add_row(&mut self, value: &[i128], key: &[u8]) -> anyhow::Result<()> {
        anyhow::ensure!(
            value.len() == 2 && value[0] > 0 && value[1] > 0,
            "invalid dao_live_agg row 0x{}: {value:?}",
            hex::encode(key)
        );
        self.capacity = self
            .capacity
            .checked_add(value[0])
            .ok_or_else(|| anyhow::anyhow!("DAO capacity sum overflow"))?;
        self.count = self
            .count
            .checked_add(u64::try_from(value[1])?)
            .ok_or_else(|| anyhow::anyhow!("DAO cell count overflow"))?;
        Ok(())
    }

    fn response(self) -> DaoAmountCount {
        DaoAmountCount {
            capacity: self.capacity.to_string(),
            count: self.count,
        }
    }
}

fn epoch(header: &CachedBlockHeader, block: i64) -> anyhow::Result<Epoch> {
    Epoch::new(
        header.epoch_number,
        i64::from(header.epoch_index),
        i64::from(header.epoch_length),
    )
    .map_err(|error| anyhow::anyhow!("DAO header epoch invalid at block {block}: {error}"))
}

fn header_at(store: &CkbadgerStore, block: i64) -> anyhow::Result<CachedBlockHeader> {
    store
        .get_block_header(block)?
        .ok_or_else(|| anyhow::anyhow!("missing canonical DAO header at block {block}"))
}

fn dao_fields(header: &CachedBlockHeader, block: i64) -> anyhow::Result<[u64; 4]> {
    dao_header_fields(&header.dao, &format!("block {block}"))
}

/// A header's DAO field as its four little-endian u64 words `[C, AR, S, U]`
/// (RFC-0023) — the API's one decoder of header DAO bytes.
pub(crate) fn dao_header_fields(dao: &[u8], context: &str) -> anyhow::Result<[u64; 4]> {
    anyhow::ensure!(
        dao.len() == 32,
        "invalid DAO field length at {context}: {}",
        dao.len()
    );
    let mut fields = [0u64; 4];
    for (index, field) in fields.iter_mut().enumerate() {
        let start = index * 8;
        *field = u64::from_le_bytes(dao[start..start + 8].try_into()?);
    }
    anyhow::ensure!(fields[1] > 0, "zero accumulated rate at DAO {context}");
    Ok(fields)
}

fn share(numerator: i128, denominator: i128) -> anyhow::Result<Option<String>> {
    if denominator == 0 {
        return Ok(None);
    }
    Ok(Some(
        format_percent(numerator, denominator, 2).map_err(anyhow::Error::msg)?,
    ))
}

fn ar_evidence(
    store: &CkbadgerStore,
    tip_number: i64,
    tip: &CachedBlockHeader,
    tip_ar: u64,
) -> anyhow::Result<DaoArEvidence> {
    let genesis = header_at(store, 0)?;
    let genesis_ar = dao_fields(&genesis, 0)?[1];
    anyhow::ensure!(tip_ar >= genesis_ar, "DAO accumulated rate regressed from genesis to block {tip_number}: {tip_ar} < {genesis_ar}");
    let since_genesis_growth_percent =
        format_percent(i128::from(tip_ar - genesis_ar), i128::from(genesis_ar), 4)
            .map_err(anyhow::Error::msg)?;
    let chain_span = tip
        .chain_time_ms
        .checked_sub(genesis.chain_time_ms)
        .ok_or_else(|| {
            anyhow::anyhow!("DAO canonical chain time regressed at tip block {tip_number}")
        })?;
    let milestone =
        |block_number: i64, header: &CachedBlockHeader| -> anyhow::Result<DaoArMilestone> {
            let ar = dao_fields(header, block_number)?[1];
            Ok(DaoArMilestone {
                block_number,
                chain_time_ms: header.chain_time_ms.to_string(),
                ar: ar.to_string(),
                ar_index: ar_index(ar),
            })
        };
    let mut milestones = vec![milestone(0, &genesis)?];
    let mut years = 1i64;
    loop {
        let offset = years
            .checked_mul(YEAR_365_MS)
            .ok_or_else(|| anyhow::anyhow!("DAO AR milestone year overflow"))?;
        if offset >= chain_span {
            break;
        }
        let cutoff = genesis
            .chain_time_ms
            .checked_add(offset)
            .ok_or_else(|| anyhow::anyhow!("DAO AR milestone time overflow"))?;
        let block = store
            .find_last_block_at_or_before_chain_time_ms(cutoff)?
            .ok_or_else(|| anyhow::anyhow!("missing DAO AR milestone at chain_time_ms={cutoff}"))?;
        if milestones
            .last()
            .is_none_or(|last| last.block_number != block)
        {
            milestones.push(milestone(block, &header_at(store, block)?)?);
        }
        years = years
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("DAO AR milestone year overflow"))?;
    }
    if milestones
        .last()
        .is_none_or(|last| last.block_number != tip_number)
    {
        milestones.push(milestone(tip_number, tip)?);
    }
    let year_365 = if chain_span >= YEAR_365_MS {
        let cutoff = tip
            .chain_time_ms
            .checked_sub(YEAR_365_MS)
            .ok_or_else(|| anyhow::anyhow!("DAO 365-day cutoff overflow"))?;
        let block = store
            .find_last_block_at_or_before_chain_time_ms(cutoff)?
            .ok_or_else(|| {
                anyhow::anyhow!("missing DAO 365-day AR anchor before chain_time_ms={cutoff}")
            })?;
        let anchor = header_at(store, block)?;
        let anchor_ar = dao_fields(&anchor, block)?[1];
        anyhow::ensure!(tip_ar >= anchor_ar && anchor.chain_time_ms <= cutoff, "invalid DAO AR annual anchor at block {block}: anchor_ar={anchor_ar}, tip_ar={tip_ar}, anchor_time_ms={}", anchor.chain_time_ms);
        let elapsed = tip
            .chain_time_ms
            .checked_sub(anchor.chain_time_ms)
            .ok_or_else(|| anyhow::anyhow!("DAO annual chain-time overflow"))?;
        Some(DaoYearArEvidence {
            anchor_block_number: block,
            anchor_chain_time_ms: anchor.chain_time_ms.to_string(),
            anchor_ar: anchor_ar.to_string(),
            elapsed_chain_time_ms: elapsed.to_string(),
            observed_growth_percent: format_percent(
                i128::from(tip_ar - anchor_ar),
                i128::from(anchor_ar),
                4,
            )
            .map_err(anyhow::Error::msg)?,
        })
    } else {
        None
    };
    Ok(DaoArEvidence {
        tip_ar: tip_ar.to_string(),
        genesis_ar: genesis_ar.to_string(),
        genesis_block_number: 0,
        tip_chain_time_ms: tip.chain_time_ms.to_string(),
        since_genesis_growth_percent,
        year_365,
        milestones,
    })
}

/// `ar` (scaled by 10^16) as the accumulated-rate index, 16 decimals — the
/// one rendering every DAO AR reading uses.
pub(crate) fn ar_index(ar: u64) -> String {
    format!(
        "{}.{:016}",
        ar / 10_000_000_000_000_000,
        ar % 10_000_000_000_000_000
    )
}

fn derived_epoch(bytes: &[u8]) -> anyhow::Result<Epoch> {
    let point = EpochPoint::decode(bytes)?;
    Epoch::new(point.number, point.index, point.length).map_err(anyhow::Error::msg)
}

/// The ranking at the request's canonical tip, read from the depositor rows
/// on a miss.
fn build_ranking(store: &CkbadgerStore, tip_hash: &[u8]) -> anyhow::Result<DaoDepositorRanking> {
    let mut ranking = Vec::new();
    let mut total = AmountCount::default();
    store.scan_derived_prefix(
        CF_DAO_LIVE_AGG,
        &[dao_live_agg::KIND_DEPOSITOR],
        |key, value| {
            anyhow::ensure!(
                key.len() == 33,
                "invalid dao_live_agg depositor key 0x{}",
                hex::encode(key)
            );
            let mut owner = AmountCount::default();
            owner.add_row(&value, key)?;
            total.add_row(&value, key)?;
            ranking.push((key[1..].to_vec(), owner.capacity, owner.count));
            Ok(())
        },
    )?;
    ranking.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    Ok(DaoDepositorRanking {
        ranking,
        phase0_capacity: total.capacity,
        phase0_count: total.count,
        tip_hash: tip_hash.to_vec(),
    })
}

fn build_evidence(
    store: &CkbadgerStore,
    network: &str,
    tip_number: i64,
    tip: &CachedBlockHeader,
    ranking: &DaoDepositorRanking,
) -> anyhow::Result<DaoEvidenceResponse> {
    let tip_epoch = epoch(tip, tip_number)?;
    let fields = dao_fields(tip, tip_number)?;
    // An all-zero sum has no row (`derive_ops`): no row is no live DAO cell.
    let global = match store.get_derived_value(CF_DAO_LIVE_AGG, &dao_live_agg::global_key())? {
        Some(value) => value,
        None => vec![0; 4],
    };
    anyhow::ensure!(
        global.len() == 4,
        "invalid dao_live_agg global row: {global:?}"
    );
    let pair = |capacity: i128, count: i128| -> anyhow::Result<AmountCount> {
        anyhow::ensure!(
            capacity >= 0 && count >= 0 && (capacity == 0) == (count == 0),
            "invalid dao_live_agg global row: {global:?}"
        );
        Ok(AmountCount {
            capacity,
            count: u64::try_from(count)?,
        })
    };
    let phase0 = pair(global[0], global[1])?;
    let phase1 = pair(global[2], global[3])?;
    anyhow::ensure!(
        ranking.phase0_capacity == phase0.capacity && ranking.phase0_count == phase0.count,
        "DAO depositor rows ({} shannons in {} cells) do not sum to the global phase-0 row ({} in {}) at tip {tip_number}",
        ranking.phase0_capacity,
        ranking.phase0_count,
        phase0.capacity,
        phase0.count
    );
    let mut windows = [AmountCount::default(); 30];
    store.scan_derived_prefix(
        CF_DAO_LIVE_AGG,
        &[dao_live_agg::KIND_PHASE0_EPOCH],
        |key, value| {
            let deposit_epoch = derived_epoch(&key[1..])?;
            let boundary =
                next_phase0_boundary(deposit_epoch, tip_epoch).map_err(anyhow::Error::msg)?;
            let day = nominal_day_bucket(tip_epoch, boundary).map_err(anyhow::Error::msg)?;
            windows[day - 1].add_row(&value, key)
        },
    )?;
    let mut ready = AmountCount::default();
    let mut waiting = AmountCount::default();
    store.scan_derived_prefix(
        CF_DAO_LIVE_AGG,
        &[dao_live_agg::KIND_PHASE1_EPOCHS],
        |key, value| {
            anyhow::ensure!(
                key.len() == 33,
                "invalid dao_live_agg phase-1 key 0x{}",
                hex::encode(key)
            );
            let deposit_epoch = derived_epoch(&key[1..17])?;
            let request_epoch = derived_epoch(&key[17..])?;
            let ready_epoch =
                phase1_ready_epoch(deposit_epoch, request_epoch).map_err(anyhow::Error::msg)?;
            if tip_epoch
                .at_or_after(ready_epoch)
                .map_err(anyhow::Error::msg)?
            {
                ready.add_row(&value, key)
            } else {
                waiting.add_row(&value, key)
            }
        },
    )?;
    anyhow::ensure!(
        windows.iter().map(|bucket| bucket.capacity).sum::<i128>() == phase0.capacity
            && windows.iter().map(|bucket| bucket.count).sum::<u64>() == phase0.count,
        "DAO phase-0 histogram capacity does not partition live deposits at tip {tip_number}"
    );
    anyhow::ensure!(
        ready.capacity + waiting.capacity == phase1.capacity
            && ready.count + waiting.count == phase1.count,
        "DAO phase-1 readiness does not partition live requests at tip {tip_number}"
    );
    let sorted = &ranking.ranking;
    let sum_top = |limit: usize| sorted.iter().take(limit).map(|owner| owner.1).sum::<i128>();
    let top1 = sum_top(1);
    let top10 = sum_top(10);
    let top100 = sum_top(100);
    let mut owner_rows = Vec::new();
    for (index, (lock_hash, capacity, count)) in sorted.iter().take(10).enumerate() {
        let lock = store.get_lock_script(lock_hash)?.ok_or_else(|| {
            anyhow::anyhow!(
                "missing lock script for DAO owner 0x{}",
                hex::encode(lock_hash)
            )
        })?;
        let lock_hash_type = u8::try_from(lock.hash_type).map_err(|_| {
            anyhow::anyhow!(
                "invalid DAO lock hash type {} for 0x{}",
                lock.hash_type,
                hex::encode(lock_hash)
            )
        })?;
        anyhow::ensure!(
            compute_script_hash(&lock.code_hash, lock_hash_type, &lock.args) == *lock_hash,
            "DAO owner lock script payload does not hash to indexed owner 0x{}",
            hex::encode(lock_hash)
        );
        let address = script_to_address(&lock.code_hash, lock.hash_type, &lock.args, network)
            .map_err(anyhow::Error::msg)?;
        let hash_type = match lock.hash_type {
            0 => "data",
            1 => "type",
            2 => "data1",
            4 => "data2",
            value => anyhow::bail!(
                "invalid DAO lock hash type {value} for 0x{}",
                hex::encode(lock_hash)
            ),
        };
        let lock_kind = store
            .get_script_info(&lock.code_hash)?
            .and_then(|info| info.name)
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty() && !name.eq_ignore_ascii_case("unknown"));
        owner_rows.push(DaoOwner {
            rank: index + 1,
            lock_script_hash: format!("0x{}", hex::encode(lock_hash)),
            address,
            lock_code_hash: format!("0x{}", hex::encode(lock.code_hash)),
            lock_hash_type: hash_type.to_string(),
            lock_kind,
            deposit_count: *count,
            capacity: capacity.to_string(),
            share_percent: share(*capacity, phase0.capacity)?
                .ok_or_else(|| anyhow::anyhow!("DAO owner with empty phase-0 denominator"))?,
        });
    }
    let concentration = DaoConcentration {
        current_depositors: sorted.len(),
        top1_capacity: top1.to_string(),
        top10_capacity: top10.to_string(),
        top100_capacity: top100.to_string(),
        top1_share_percent: share(top1, phase0.capacity)?,
        top10_share_percent: share(top10, phase0.capacity)?,
        top100_share_percent: share(top100, phase0.capacity)?,
        owners: owner_rows,
    };
    let windows = windows
        .into_iter()
        .enumerate()
        .map(|(index, amount)| DaoWindowBucket {
            nominal_day: index + 1,
            from_epoch_offset: (index * 6) as u16,
            through_epoch_offset: ((index + 1) * 6) as u16,
            capacity: amount.capacity.to_string(),
            count: amount.count,
        })
        .collect();
    let ar = ar_evidence(store, tip_number, tip, fields[1])?;
    Ok(DaoEvidenceResponse {
        tip_block_number: tip_number,
        tip_epoch: tip_epoch.into(),
        phase0: phase0.response(),
        phase1: phase1.response(),
        phase1_ready_by_epoch_lock: ready.response(),
        phase1_waiting_for_epoch_lock: waiting.response(),
        windows,
        concentration,
        ar,
        header: DaoHeaderEvidence {
            block_number: tip_number,
            dao_hex: format!("0x{}", hex::encode(&tip.dao)),
            c: fields[0].to_string(),
            ar: fields[1].to_string(),
            s: fields[2].to_string(),
            u: fields[3].to_string(),
        },
    })
}

/// The ranking for the request's canonical tip (number and hash), read inside
/// the request's pinned view on a miss. `Ok(Err(_))` is `503 derive_pending`
/// (or a recorded derivation failure): there is no scan to fall back to.
#[allow(clippy::type_complexity)]
fn ranking_at_tip(
    state: &AppState,
) -> anyhow::Result<Result<(i64, CachedBlockHeader, Arc<DaoDepositorRanking>), ApiRouteError>> {
    if let Err(pending) = require_derived(&state.store, &DAO_LIVE_AGG) {
        return Ok(Err(pending));
    }
    let (tip_number, tip) = state
        .store
        .get_sync_tip_block()?
        .ok_or_else(|| anyhow::anyhow!("no DAO sync tip yet"))?;
    if let Some(memo) = state.dao_evidence.get(tip_number, &tip.hash) {
        return Ok(Ok((tip_number, tip, memo)));
    }
    let memo = Arc::new(build_ranking(&state.store, &tip.hash)?);
    state
        .dao_evidence
        .set(tip_number, tip.hash.clone(), memo.clone());
    Ok(Ok((tip_number, tip, memo)))
}

/// `/dao/evidence`, from `dao_live_agg` (J1): the global row, the phase-0
/// epoch rows bucketed against the tip epoch, the phase-1 epoch rows split by
/// readiness, and the depositor ranking memoized per canonical tip. No deposit
/// row is read. `503 derive_pending` until the indexer has derived the
/// aggregate. Nothing is written to any store.
pub(super) async fn get_evidence(
    State(state): State<Arc<AppState>>,
) -> ApiResult<DaoEvidenceResponse> {
    let evidence = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let (tip_number, tip, ranking) = match ranking_at_tip(&state)? {
            Ok(found) => found,
            Err(pending) => return Ok(Err(pending)),
        };
        Ok(Ok(build_evidence(
            &state.store,
            &state.ckb_network,
            tip_number,
            &tip,
            &ranking,
        )?))
    })
    .await
    .map_err(|error| ApiError::internal(format!("DAO evidence task failed: {error}")))?
    .map_err(|error| ApiError::internal(format!("DAO evidence read failed: {error:#}")))?;
    ok(evidence?)
}

#[derive(Debug, Deserialize)]
pub(super) struct DepositorsParams {
    #[serde(default = "crate::response::default_limit")]
    limit: i64,
    /// The number of depositors already served (J2: a rank, not bound to a
    /// tip; the ranking is recomputed per tip).
    cursor: Option<String>,
}

/// The block a ranking (or any DAO read-time series) was computed at.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaoAnchorResponse {
    pub block_number: i64,
    pub block_hash: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaoDepositorResponse {
    pub rank: usize,
    pub lock_script_hash: String,
    /// `null` only when the lock's script is not indexed.
    pub address: Option<String>,
    /// Phase-0 deposited capacity, shannons.
    pub capacity: String,
    pub deposit_count: u64,
    /// Share of all phase-0 capacity, two decimals.
    pub share_percent: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaoDepositorsResponse {
    pub anchor: DaoAnchorResponse,
    pub data: Vec<DaoDepositorResponse>,
    /// Every current depositor at the anchor.
    pub total: usize,
    pub limit: i64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

/// `/dao/depositors`: every current depositor ranked by phase-0 capacity
/// (DAO-12), paged by rank (J2) from the ranking `/dao/evidence` cuts its
/// concentration from at the same canonical tip. The cursor is the number of
/// depositors already served; it is not bound to a tip, so a page fetched
/// after the tip moved continues the new tip's ranking (`anchor` says which).
pub(super) async fn get_depositors(
    State(state): State<Arc<AppState>>,
    Query(params): Query<DepositorsParams>,
) -> ApiResult<DaoDepositorsResponse> {
    if !(1..=100).contains(&params.limit) {
        return Err(ApiError::bad_request("limit must be between 1 and 100"));
    }
    let start = match params.cursor.as_deref() {
        None => 0,
        Some(raw) => raw.parse::<usize>().map_err(|_| {
            ApiError::bad_request(format!(
                "invalid depositor cursor {raw:?}; expected a rank (the number of depositors already served)"
            ))
        })?,
    };
    let limit = params.limit as usize;
    let page = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let (tip_number, _tip, memo) = match ranking_at_tip(&state)? {
            Ok(found) => found,
            Err(pending) => return Ok(Err(pending)),
        };
        let rows = memo
            .ranking
            .iter()
            .enumerate()
            .skip(start)
            .take(limit)
            .map(|(index, (lock, capacity, count))| -> anyhow::Result<_> {
                let address = resolve_lock_address(&state.store, lock, &state.ckb_network)?
                    .map(|resolved| resolved.address);
                Ok(DaoDepositorResponse {
                    rank: index + 1,
                    lock_script_hash: format!("0x{}", hex::encode(lock)),
                    address,
                    capacity: capacity.to_string(),
                    deposit_count: *count,
                    share_percent: share(*capacity, memo.phase0_capacity)?.ok_or_else(|| {
                        anyhow::anyhow!("DAO depositor with an empty phase-0 denominator")
                    })?,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let served = start + rows.len();
        let has_more = served < memo.ranking.len();
        Ok(Ok(DaoDepositorsResponse {
            anchor: DaoAnchorResponse {
                block_number: tip_number,
                block_hash: format!("0x{}", hex::encode(&memo.tip_hash)),
            },
            total: memo.ranking.len(),
            limit: params.limit,
            has_more,
            next_cursor: has_more.then(|| served.to_string()),
            data: rows,
        }))
    })
    .await
    .map_err(|error| ApiError::internal(format!("DAO depositors task failed: {error}")))?
    .map_err(|error| ApiError::internal(format!("DAO depositors read failed: {error:#}")))?;
    ok(page?)
}
