//! Exact read projections of the existing token indexes; no persistent state.
use super::{
    activities::{participant_lock_kind, GlobalActivityResponse},
    asset_feed,
    transactions::TransactionResponse,
};
use crate::utils::address::resolve_lock_address;
use crate::{
    response::{default_limit, ok, ApiError, ApiResult, CursorPaginatedResponse},
    utils::{apply_owned_capacity_delta, parse_hash32},
    AppState,
};
use anyhow::{ensure, Context};
use axum::extract::{Path, Query, State};
use chrono::{Datelike, Days, NaiveDate};
use ckbadger_common::{stats_bucket::stats_day, TokenBalance};
use ckbadger_store::{keys, CkbadgerStore, TokenTransferRecord};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};

#[derive(Deserialize)]
pub(super) struct FeedParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
}

/// Visit every transfer row, validating key, payload and canonical transaction.
/// Repeated transfer legs in a transaction share its validated identity.
fn visit_transfers(
    store: &CkbadgerStore,
    hash: &[u8],
    mut visit: impl FnMut(i64, i32, i32, TokenTransferRecord) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let mut previous: Option<(Vec<u8>, (i64, i32, i64))> = None;
    for item in store.prefix_iterator_cf(store.cf_token_transfers(), hash) {
        let (key, value) = item?;
        if !key.starts_with(hash) {
            break;
        }
        ensure!(
            key.len() == 44,
            "token transfer malformed key token=0x{} key=0x{}",
            hex::encode(hash),
            hex::encode(&key)
        );
        let (block, sequence) = keys::decode_token_transfer_key(&key);
        let row: TokenTransferRecord = bincode::deserialize(&value)
            .with_context(|| format!("token transfer decode key=0x{}", hex::encode(&key)))?;
        ensure!(
            block >= 0
                && sequence >= 0
                && row.block_number == block
                && row.tx_hash.len() == 32
                && !(row.is_mint && row.is_burn),
            "token transfer invalid identity/flags key=0x{} tx=0x{}",
            hex::encode(&key),
            hex::encode(&row.tx_hash)
        );
        let location = if let Some((_, location)) =
            previous.as_ref().filter(|(hash, _)| hash == &row.tx_hash)
        {
            *location
        } else {
            let (height, index, entry) =
                store.get_tx_by_hash(&row.tx_hash)?.with_context(|| {
                    format!(
                        "token transfer missing canonical tx=0x{} block={block}",
                        hex::encode(&row.tx_hash)
                    )
                })?;
            ensure!(
                entry.timestamp == row.timestamp,
                "token transfer timestamp mismatch tx=0x{} block={block}",
                hex::encode(&row.tx_hash)
            );
            previous = Some((row.tx_hash.clone(), (height, index, entry.timestamp)));
            (height, index, entry.timestamp)
        };
        ensure!(
            location.2 == row.timestamp,
            "token transfer timestamp mismatch tx=0x{} block={block}",
            hex::encode(&row.tx_hash)
        );
        ensure!(
            location.0 == block,
            "token transfer canonical block mismatch tx=0x{} transfer={block} canonical={}",
            hex::encode(&row.tx_hash),
            location.0
        );
        visit(block, sequence, location.1, row)?;
    }
    Ok(())
}

fn token_scope(state: &AppState, hash: &[u8]) -> anyhow::Result<Vec<asset_feed::AssetTransaction>> {
    let mut hashes = HashSet::new();
    visit_transfers(&state.store, hash, |_, _, _, row| {
        hashes.insert(row.tx_hash);
        Ok(())
    })?;
    asset_feed::canonical_scope(&state.store, hashes)
}

pub(super) async fn activity_feed(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
    Query(params): Query<FeedParams>,
) -> ApiResult<CursorPaginatedResponse<GlobalActivityResponse>> {
    let hash = parse_hash32(&hash, "type_hash")?;
    require_token(&state.store, &hash)?;
    tokio::task::spawn_blocking(move || {
        let scope = token_scope(&state, &hash).map_err(internal)?;
        asset_feed::activities(&state, scope, params.limit, params.cursor.as_deref())
    })
    .await
    .map_err(internal)?
}
pub(super) async fn transactions(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
    Query(params): Query<FeedParams>,
) -> ApiResult<CursorPaginatedResponse<TransactionResponse>> {
    let hash = parse_hash32(&hash, "type_hash")?;
    require_token(&state.store, &hash)?;
    tokio::task::spawn_blocking(move || {
        let scope = token_scope(&state, &hash).map_err(internal)?;
        asset_feed::transactions(&state, scope, params.limit, params.cursor.as_deref())
    })
    .await
    .map_err(internal)?
}
fn internal(e: impl std::fmt::Display) -> crate::response::ApiRouteError {
    ApiError::internal(e.to_string())
}
fn require_token(
    store: &CkbadgerStore,
    hash: &[u8],
) -> Result<ckbadger_store::TokenInfo, crate::response::ApiRouteError> {
    store
        .get_token(hash)
        .map_err(internal)?
        .ok_or_else(|| ApiError::not_found("Token not found"))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct History {
    source: &'static str,
    complete_through: String,
    anchor: Anchor,
    daily: Vec<Day>,
    weekly: Vec<Week>,
    supply_events: Vec<SupplyEvent>,
    mint_count: i64,
    burn_count: i64,
    transfers_count: i64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Anchor {
    block_number: i64,
    block_hash: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Day {
    date: String,
    supply: String,
    capacity: String,
    occupied_capacity: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Week {
    week_start: String,
    transfers_count: i64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SupplyEvent {
    tx_hash: String,
    block_number: i64,
    tx_index: i32,
    transfer_sequence: i32,
    timestamp: String,
    kind: &'static str,
    amount: String,
    supply_after: String,
    lock_hash: String,
    address: Option<String>,
    lock_kind: Option<String>,
}

pub(super) async fn history(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> ApiResult<History> {
    let hash = parse_hash32(&hash, "type_hash")?;
    require_token(&state.store, &hash)?;
    let result = tokio::task::spawn_blocking(move || build_history(&state, &hash))
        .await
        .map_err(internal)?
        .map_err(internal)?;
    ok(result)
}
fn build_history(state: &AppState, hash: &[u8]) -> anyhow::Result<History> {
    let store = &state.store;
    let (tip, header) = store
        .get_sync_tip_block()?
        .context("token history requires indexed tip header")?;
    let tip_day = stats_day(header.chain_time_ms)?;
    let last = tip_day.pred_opt().context("token history date underflow")?;
    let mut previous_date: Option<(i64, NaiveDate)> = None;
    let mut weeks = BTreeMap::<NaiveDate, i64>::new();
    let mut events = Vec::new();
    let mut count = 0i64;
    visit_transfers(store, hash, |block, seq, index, row| {
        ensure!(
            block <= tip,
            "token history transfer after tip: block={block} tip={tip}"
        );
        let date = if let Some((_, date)) = previous_date.filter(|(number, _)| *number == block) {
            date
        } else {
            let header = store
                .get_block_header(block)?
                .with_context(|| format!("token history missing header block={block}"))?;
            ensure!(
                header.chain_time_ms >= header.timestamp,
                "token history invalid chain time block={block}"
            );
            let date = stats_day(header.chain_time_ms)?;
            previous_date = Some((block, date));
            date
        };
        let monday = date
            .checked_sub_days(Days::new(u64::from(date.weekday().num_days_from_monday())))
            .context("token week underflow")?;
        let total = weeks.entry(monday).or_default();
        *total = total
            .checked_add(1)
            .context("weekly token transfer count overflow")?;
        count = count
            .checked_add(1)
            .context("token transfer count overflow")?;
        if row.is_mint || row.is_burn {
            events.push((block, seq, index, date, row));
        }
        Ok(())
    })?;
    events.sort_by_key(|(block, seq, index, _, _)| (*block, *index, *seq));
    let mut supply = TokenBalance::zero();
    let mut daily_supply = BTreeMap::new();
    let mut supply_events = Vec::new();
    let mut mint_count = 0i64;
    let mut burn_count = 0i64;
    for (block, seq, index, date, row) in events {
        let amount = TokenBalance::from(row.amount);
        supply = if row.is_mint {
            mint_count = mint_count.checked_add(1).context("mint count overflow")?;
            supply.checked_add(&amount)
        } else {
            burn_count = burn_count.checked_add(1).context("burn count overflow")?;
            supply.checked_sub(&amount)
        }
        .with_context(|| {
            format!(
                "token supply overflow/underflow token=0x{} block={block} sequence={seq}",
                hex::encode(hash)
            )
        })?;
        daily_supply.insert(date, supply.clone());
        let lock = if row.is_mint {
            row.to_lock_hash.as_slice()
        } else {
            row.from_lock_hash
                .as_deref()
                .context("token burn missing sender lock")?
        };
        let lock32: [u8; 32] = lock.try_into().with_context(|| {
            format!("token supply event invalid lock block={block} sequence={seq}")
        })?;
        supply_events.push(SupplyEvent {
            tx_hash: format!("0x{}", hex::encode(&row.tx_hash)),
            block_number: block,
            tx_index: index,
            transfer_sequence: seq,
            timestamp: row.timestamp.to_string(),
            kind: if row.is_mint { "mint" } else { "burn" },
            amount: amount.to_string(),
            supply_after: supply.to_string(),
            lock_hash: format!("0x{}", hex::encode(lock)),
            address: resolve_lock_address(store, lock, &state.ckb_network)?
                .map(|resolved| resolved.address),
            lock_kind: participant_lock_kind(store, &lock32, None)?,
        });
    }
    let (_, current) = store.aggregate_token_holder_stats(hash)?;
    ensure!(supply==current,"token supply history disagrees with current holders: token=0x{} history={supply} holders={current}",hex::encode(hash));
    let mut capacities = BTreeMap::new();
    for (key, delta) in store.list_token_daily_deltas(hash)? {
        let date = NaiveDate::parse_from_str(&key.to_string(), "%Y%m%d")?;
        ensure!(
            date <= tip_day,
            "token capacity delta after indexed tip: date={date} tip={tip_day}"
        );
        capacities.insert(date, delta);
    }
    let first = daily_supply.keys().chain(capacities.keys()).min().copied();
    let mut daily = Vec::new();
    let (mut capacity, mut occupied) = (0, 0);
    let mut cumulative = TokenBalance::zero();
    if let Some(mut date) = first {
        while date <= tip_day {
            if let Some(value) = daily_supply.get(&date) {
                cumulative = value.clone();
            }
            if let Some(delta) = capacities.get(&date) {
                (capacity, occupied) = apply_owned_capacity_delta(
                    capacity,
                    occupied,
                    delta.owned_capacity_delta,
                    delta.owned_knowledge_delta,
                    &format!("token history token=0x{} date={date}", hex::encode(hash)),
                )?;
            }
            // Zero-net daily rows are legitimately absent after mint/burn or
            // create/consume within one day. Positive end-of-day supply still
            // requires cells, so it cannot coexist with zero indexed capacity.
            ensure!(
                cumulative.is_zero() || (capacity > 0 && occupied > 0),
                "token history missing capacity history for positive supply: token=0x{} date={date} supply={cumulative} capacity={capacity} occupied={occupied}",
                hex::encode(hash)
            );
            if date <= last {
                daily.push(Day {
                    date: date.to_string(),
                    supply: cumulative.to_string(),
                    capacity: capacity.to_string(),
                    occupied_capacity: occupied.to_string(),
                });
            }
            date = date.succ_opt().context("token history date overflow")?;
        }
    }
    let mut weekly = Vec::new();
    if let Some(mut week) = weeks.keys().next().copied() {
        loop {
            let end = week
                .checked_add_days(Days::new(7))
                .context("token week overflow")?;
            if end > tip_day {
                break;
            }
            weekly.push(Week {
                week_start: week.to_string(),
                transfers_count: weeks.get(&week).copied().unwrap_or(0),
            });
            week = end;
        }
    }
    Ok(History {
        source: "indexed_chain_time",
        complete_through: last.to_string(),
        anchor: Anchor {
            block_number: tip,
            block_hash: format!("0x{}", hex::encode(header.hash)),
        },
        daily,
        weekly,
        supply_events,
        mint_count,
        burn_count,
        transfers_count: count,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Distribution {
    holders_count: i64,
    total_supply: String,
    largest_holder_balance: String,
    next_nine_balance: String,
    remaining_balance: String,
    unit: &'static str,
    decimals: Option<i32>,
    buckets: Vec<Bucket>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Bucket {
    lower_bound: String,
    upper_bound: Option<String>,
    holders_count: i64,
    balance: String,
}
pub(super) async fn distribution(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> ApiResult<Distribution> {
    let hash = parse_hash32(&hash, "type_hash")?;
    let token = require_token(&state.store, &hash)?;
    let result = tokio::task::spawn_blocking(move || {
        build_distribution(&state.store, &hash, token.decimals)
    })
    .await
    .map_err(internal)?
    .map_err(internal)?;
    ok(result)
}
fn build_distribution(
    store: &CkbadgerStore,
    hash: &[u8],
    decimals: Option<i32>,
) -> anyhow::Result<Distribution> {
    let scale = usize::try_from(decimals.unwrap_or(0)).context("negative token decimals")?;
    ensure!(scale <= 255, "invalid token decimals {scale}"); // Unknown decimals explicitly use raw units.
    let mut thresholds = Vec::new();
    for exponent in 0..6 {
        thresholds.push(format!("1{}", "0".repeat(scale + exponent)));
    }
    let mut totals = vec![(0i64, TokenBalance::zero()); 7];
    let mut largest = Vec::new();
    let mut total = TokenBalance::zero();
    let mut count = 0i64;
    for item in store.prefix_iterator_cf(store.cf_token_holders(), hash) {
        let (key, value) = item?;
        if !key.starts_with(hash) {
            break;
        }
        ensure!(
            key.len() == 64,
            "token holder malformed key: token=0x{} key=0x{}",
            hex::encode(hash),
            hex::encode(&key)
        );
        let balance = TokenBalance::from_be_bytes(&value)
            .with_context(|| format!("token holder balance key=0x{}", hex::encode(&key)))?;
        if balance.is_zero() {
            continue;
        }
        let decimal = balance.to_string();
        let bucket = thresholds.partition_point(|bound| {
            (decimal.len(), decimal.as_str()) >= (bound.len(), bound.as_str())
        });
        totals[bucket].0 = totals[bucket]
            .0
            .checked_add(1)
            .context("holder bucket count overflow")?;
        totals[bucket].1 = totals[bucket]
            .1
            .checked_add(&balance)
            .context("holder bucket sum overflow")?;
        total = total
            .checked_add(&balance)
            .context("holder total overflow")?;
        count = count.checked_add(1).context("holder count overflow")?;
        largest.push(balance);
        largest.sort_unstable_by(|a, b| b.cmp(a));
        largest.truncate(10);
    }
    let first = largest.first().cloned().unwrap_or_else(TokenBalance::zero);
    let next = largest
        .iter()
        .skip(1)
        .try_fold(TokenBalance::zero(), |sum, n| {
            sum.checked_add(n).context("top ten balance overflow")
        })?;
    let remaining = total
        .checked_sub(&first)
        .and_then(|n| n.checked_sub(&next))
        .context("holder share underflow")?;
    let buckets = totals
        .into_iter()
        .enumerate()
        .map(|(i, (holders_count, balance))| Bucket {
            lower_bound: if i == 0 {
                "0".into()
            } else {
                thresholds[i - 1].to_string()
            },
            upper_bound: thresholds.get(i).map(ToString::to_string),
            holders_count,
            balance: balance.to_string(),
        })
        .collect();
    Ok(Distribution {
        holders_count: count,
        total_supply: total.to_string(),
        largest_holder_balance: first.to_string(),
        next_nine_balance: next.to_string(),
        remaining_balance: remaining.to_string(),
        unit: if decimals.is_some() { "token" } else { "raw" },
        decimals,
        buckets,
    })
}

/// Count the physical live token cells, including cells with a zero amount.
pub(super) fn live_cells_count(state: &AppState, hash: &[u8]) -> anyhow::Result<i64> {
    let mut count = 0i64;
    for row in state
        .store
        .prefix_iterator_cf(state.store.cf_cell_by_type(), hash)
    {
        let (key, value) = row?;
        if !key.starts_with(hash) {
            break;
        }
        ensure!(
            key.len() == 74 && value.is_empty(),
            "token cell index malformed: token=0x{} key=0x{}",
            hex::encode(hash),
            hex::encode(&key)
        );
        let cell = state
            .store
            .get_live_cell_by_outpoint_key(&key[40..], &state.append_only_store)?
            .with_context(|| {
                format!(
                    "token cell index missing live marker: key=0x{}",
                    hex::encode(&key)
                )
            })?;
        ensure!(
            cell.type_script_hash.as_deref() == Some(hash)
                && keys::decode_block_num(&key[32..40]) == cell.created_at_block,
            "token cell index/payload mismatch: key=0x{}",
            hex::encode(&key)
        );
        count = count
            .checked_add(1)
            .context("token live cell count overflow")?;
    }
    Ok(count)
}
