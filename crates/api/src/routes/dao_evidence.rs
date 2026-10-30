//! Read-only, chain-anchored evidence for the NervosDAO topic page.

use axum::extract::State;
use ckbadger_store::{types::CachedBlockHeader, CkbadgerStore};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;

use super::dao_epoch::{next_phase0_boundary, nominal_day_bucket, phase1_ready_epoch, Epoch};
use crate::response::{ok, ApiError, ApiResult};
use crate::utils::address::compute_script_hash;
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
    fn add(&mut self, capacity: i64, key: &[u8]) -> anyhow::Result<()> {
        anyhow::ensure!(
            capacity > 0,
            "invalid DAO capacity at outpoint_key=0x{}: {capacity}",
            hex::encode(key)
        );
        self.capacity = self
            .capacity
            .checked_add(i128::from(capacity))
            .ok_or_else(|| anyhow::anyhow!("DAO capacity sum overflow"))?;
        self.count = self
            .count
            .checked_add(1)
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

#[derive(Clone, Copy)]
struct HeaderPoint {
    epoch: Epoch,
    ar: u64,
    timestamp: i64,
    chain_time_ms: i64,
}

fn header_point_at(
    store: &CkbadgerStore,
    cache: &mut HashMap<i64, HeaderPoint>,
    block: i64,
) -> anyhow::Result<HeaderPoint> {
    if let Some(value) = cache.get(&block) {
        return Ok(*value);
    }
    let header = header_at(store, block)?;
    let value = HeaderPoint {
        epoch: epoch(&header, block)?,
        ar: dao_fields(&header, block)?[1],
        timestamp: header.timestamp,
        chain_time_ms: header.chain_time_ms,
    };
    cache.insert(block, value);
    Ok(value)
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
                ar_index: format!(
                    "{}.{:016}",
                    ar / 10_000_000_000_000_000,
                    ar % 10_000_000_000_000_000
                ),
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

fn build_evidence(store: &CkbadgerStore, network: &str) -> anyhow::Result<DaoEvidenceResponse> {
    let (tip_number, tip) = store
        .get_sync_tip_block()?
        .ok_or_else(|| anyhow::anyhow!("no DAO sync tip yet"))?;
    let tip_epoch = epoch(&tip, tip_number)?;
    let fields = dao_fields(&tip, tip_number)?;
    let mut epoch_cache = HashMap::new();
    let mut phase0 = AmountCount::default();
    let mut phase1 = AmountCount::default();
    let mut ready = AmountCount::default();
    let mut waiting = AmountCount::default();
    let mut windows = [AmountCount::default(); 30];
    let mut owners: HashMap<Vec<u8>, AmountCount> = HashMap::new();
    let mut visit_live = |key: &[u8],
                          row: &ckbadger_store::DaoDepositCacheEntry|
     -> anyhow::Result<()> {
        ckbadger_store::dao_compensation_for_entry_at(row, tip_number, fields[1]).map_err(
            |error| {
                anyhow::anyhow!(
                    "invalid DAO lifecycle at outpoint_key=0x{}: {error:#}",
                    hex::encode(key)
                )
            },
        )?;
        anyhow::ensure!(row.deposit_block_number >= 0 && row.deposit_block_number <= tip_number, "DAO deposit outside canonical tip: outpoint_key=0x{}, deposit_block={}, tip={tip_number}", hex::encode(key), row.deposit_block_number);
        anyhow::ensure!(
            row.lock_script_hash.len() == 32,
            "invalid DAO lock hash at outpoint_key=0x{}",
            hex::encode(key)
        );
        let deposit_point = header_point_at(store, &mut epoch_cache, row.deposit_block_number)?;
        anyhow::ensure!(row.deposit_ar >= 0 && row.deposit_ar as u64 == deposit_point.ar, "DAO deposit AR differs from canonical header at outpoint_key=0x{}: stored={}, block_ar={}", hex::encode(key), row.deposit_ar, deposit_point.ar);
        anyhow::ensure!(row.deposit_timestamp == deposit_point.timestamp && row.deposit_chain_time_ms == deposit_point.chain_time_ms, "DAO deposit time differs from canonical header at outpoint_key=0x{}: stored_raw={}, header_raw={}, stored_chain={}, header_chain={}", hex::encode(key), row.deposit_timestamp, deposit_point.timestamp, row.deposit_chain_time_ms, deposit_point.chain_time_ms);
        let deposit_epoch = deposit_point.epoch;
        match row.status {
            0 => {
                anyhow::ensure!(
                    row.withdraw_request_occupied_capacity.is_none(),
                    "phase-0 DAO deposit has request occupied marker at outpoint_key=0x{}",
                    hex::encode(key)
                );
                phase0.add(row.capacity, key)?;
                owners
                    .entry(row.lock_script_hash.clone())
                    .or_default()
                    .add(row.capacity, key)?;
                let boundary =
                    next_phase0_boundary(deposit_epoch, tip_epoch).map_err(anyhow::Error::msg)?;
                let day = nominal_day_bucket(tip_epoch, boundary).map_err(anyhow::Error::msg)?;
                windows[day - 1].add(row.capacity, key)?;
            }
            1 => {
                let request_block = row.withdraw_request_block.ok_or_else(|| {
                    anyhow::anyhow!(
                        "phase-1 DAO deposit missing request block at outpoint_key=0x{}",
                        hex::encode(key)
                    )
                })?;
                anyhow::ensure!(request_block > row.deposit_block_number && request_block <= tip_number, "DAO request block outside lifecycle at outpoint_key=0x{}: deposit={}, request={}, tip={tip_number}", hex::encode(key), row.deposit_block_number, request_block);
                anyhow::ensure!(
                    row.withdraw_request_tx
                        .as_ref()
                        .is_some_and(|tx| tx.len() == 32)
                        && row
                            .withdraw_request_output_index
                            .is_some_and(|index| index >= 0),
                    "DAO request outpoint malformed at outpoint_key=0x{}",
                    hex::encode(key)
                );
                let request_point = header_point_at(store, &mut epoch_cache, request_block)?;
                anyhow::ensure!(row.withdraw_request_ar.is_some_and(|ar| ar >= 0 && ar as u64 == request_point.ar), "DAO request AR differs from canonical header at outpoint_key=0x{}: stored={:?}, block_ar={}", hex::encode(key), row.withdraw_request_ar, request_point.ar);
                let request_epoch = request_point.epoch;
                let ready_epoch =
                    phase1_ready_epoch(deposit_epoch, request_epoch).map_err(anyhow::Error::msg)?;
                phase1.add(row.capacity, key)?;
                if tip_epoch
                    .at_or_after(ready_epoch)
                    .map_err(anyhow::Error::msg)?
                {
                    ready.add(row.capacity, key)?;
                } else {
                    waiting.add(row.capacity, key)?;
                }
            }
            status => anyhow::bail!(
                "invalid live DAO status {status} at outpoint_key=0x{}",
                hex::encode(key)
            ),
        }
        Ok(())
    };
    store.scan_dao_deposits_by_status(0, &mut visit_live)?;
    store.scan_dao_deposits_by_status(1, &mut visit_live)?;
    anyhow::ensure!(
        windows.iter().map(|bucket| bucket.capacity).sum::<i128>() == phase0.capacity,
        "DAO phase-0 histogram capacity does not partition live deposits at tip {tip_number}"
    );
    anyhow::ensure!(
        ready.capacity + waiting.capacity == phase1.capacity
            && ready.count + waiting.count == phase1.count,
        "DAO phase-1 readiness does not partition live requests at tip {tip_number}"
    );
    let mut sorted: Vec<(Vec<u8>, AmountCount)> = owners.into_iter().collect();
    sorted.sort_by(|a, b| b.1.capacity.cmp(&a.1.capacity).then_with(|| a.0.cmp(&b.0)));
    let sum_top = |limit: usize| {
        sorted
            .iter()
            .take(limit)
            .map(|owner| owner.1.capacity)
            .sum::<i128>()
    };
    let top1 = sum_top(1);
    let top10 = sum_top(10);
    let top100 = sum_top(100);
    let mut owner_rows = Vec::new();
    for (index, (lock_hash, amount)) in sorted.iter().take(10).enumerate() {
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
            deposit_count: amount.count,
            capacity: amount.capacity.to_string(),
            share_percent: share(amount.capacity, phase0.capacity)?
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
    let ar = ar_evidence(store, tip_number, &tip, fields[1])?;
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

pub(super) async fn get_evidence(
    State(state): State<Arc<AppState>>,
) -> ApiResult<DaoEvidenceResponse> {
    let store = state.store.clone();
    let network = state.ckb_network.clone();
    let evidence = tokio::task::spawn_blocking(move || build_evidence(&store, &network))
        .await
        .map_err(|error| ApiError::internal(format!("DAO evidence task failed: {error}")))?
        .map_err(|error| ApiError::internal(format!("DAO evidence read failed: {error:#}")))?;
    ok(evidence)
}
