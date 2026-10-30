//! Exact read projections of the existing token indexes; no persistent state.
use super::{
    activities::{participant_lock_kind, GlobalActivityResponse},
    asset_feed,
    transactions::TransactionResponse,
};
use crate::utils::address::resolve_lock_address;
use crate::{
    response::{default_limit, ok, ApiError, ApiResult, CursorPaginatedResponse},
    utils::{
        apply_owned_capacity_delta, parse_hash32,
        stats_bucket::{complete_days_cache_key, ChainTime},
    },
    AppState,
};
use anyhow::{ensure, Context};
use axum::extract::{Path, Query, State};
use chrono::{Datelike, Days, NaiveDate};
use ckbadger_common::{stats_bucket::stats_day, TokenBalance};
use ckbadger_store::{keys, CkbadgerStore, TokenTransferRecord};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
};

#[derive(Deserialize)]
pub(super) struct FeedParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
}

/// Visit every transfer row, validating key, payload and canonical transaction.
/// Buffer at most 1024 transfer rows. The last transaction survives a batch
/// boundary so a transaction with many transfer legs is still located once.
fn visit_transfers(
    store: &CkbadgerStore,
    hash: &[u8],
    visit: impl FnMut(
        i64,
        i32,
        &asset_feed::AssetTransaction,
        TokenTransferRecord,
    ) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    visit_transfers_in(store, hash, BlockRange::ALL, visit)
}

/// A block range `[from, before)` of a token's transfers.
#[derive(Debug, Clone, Copy)]
struct BlockRange {
    from: i64,
    before: Option<i64>,
}

impl BlockRange {
    const ALL: Self = Self {
        from: 0,
        before: None,
    };
}

/// [`visit_transfers`] restricted to the transfers of `range`, newest first.
/// The index is keyed newest first, so the walk seeks to `range.before` and
/// stops at the first row older than `range.from`.
fn visit_transfers_in(
    store: &CkbadgerStore,
    hash: &[u8],
    range: BlockRange,
    mut visit: impl FnMut(
        i64,
        i32,
        &asset_feed::AssetTransaction,
        TokenTransferRecord,
    ) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let start = match range.before {
        // The first row of block `before - 1` (sequences are keyed descending).
        Some(before) if before <= range.from => return Ok(()),
        Some(before) => keys::encode_token_transfer_key(hash, before - 1, i32::MAX),
        None => hash.to_vec(),
    };
    let mut previous = None;
    let mut batch = Vec::with_capacity(asset_feed::TRANSACTION_BATCH_SIZE);
    for item in store.iterator_cf(
        store.cf_token_transfers(),
        rocksdb::IteratorMode::From(&start, rocksdb::Direction::Forward),
    ) {
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
        if block < range.from {
            break;
        }
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
        batch.push((block, sequence, row));
        if batch.len() == asset_feed::TRANSACTION_BATCH_SIZE {
            visit_transfer_batch(store, &mut batch, &mut previous, &mut visit)?;
        }
    }
    if !batch.is_empty() {
        visit_transfer_batch(store, &mut batch, &mut previous, &mut visit)?;
    }
    Ok(())
}

fn visit_transfer_batch(
    store: &CkbadgerStore,
    batch: &mut Vec<(i64, i32, TokenTransferRecord)>,
    previous: &mut Option<asset_feed::AssetTransaction>,
    visit: &mut impl FnMut(
        i64,
        i32,
        &asset_feed::AssetTransaction,
        TokenTransferRecord,
    ) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let last_hash = batch
        .last()
        .context("empty token transfer batch")?
        .2
        .tx_hash
        .clone();
    let hashes = batch
        .iter()
        .filter(|(_, _, row)| {
            !previous
                .as_ref()
                .is_some_and(|tx| tx.entry.tx_hash.as_slice() == row.tx_hash)
        })
        .map(|(_, _, row)| row.tx_hash.clone());
    let mut transactions: HashMap<_, _> = asset_feed::canonical_scope(store, hashes)?
        .into_iter()
        .map(|tx| (tx.entry.tx_hash.to_vec(), tx))
        .collect();
    if let Some(tx) = previous.take() {
        transactions.insert(tx.entry.tx_hash.to_vec(), tx);
    }
    for (block, sequence, row) in batch.drain(..) {
        let tx = transactions.get(&row.tx_hash).with_context(|| {
            format!(
                "token transfer batch missing validated tx=0x{} block={block}",
                hex::encode(&row.tx_hash)
            )
        })?;
        ensure!(
            tx.entry.timestamp == row.timestamp,
            "token transfer timestamp mismatch tx=0x{} block={block}",
            hex::encode(&row.tx_hash)
        );
        ensure!(
            tx.block_number == block,
            "token transfer canonical block mismatch tx=0x{} transfer={block} canonical={}",
            hex::encode(&row.tx_hash),
            tx.block_number
        );
        visit(block, sequence, tx, row)?;
    }
    *previous = Some(transactions.remove(&last_hash).with_context(|| {
        format!(
            "token transfer batch missing last validated tx=0x{}",
            hex::encode(&last_hash)
        )
    })?);
    Ok(())
}

fn token_scope(
    store: &CkbadgerStore,
    hash: &[u8],
) -> anyhow::Result<Vec<asset_feed::AssetTransaction>> {
    let mut seen = HashSet::new();
    let mut scope = Vec::new();
    visit_transfers(store, hash, |_, _, tx, _| {
        if seen.insert(tx.entry.tx_hash) {
            scope.push(tx.clone());
        }
        Ok(())
    })?;
    asset_feed::sort_scope(&mut scope);
    Ok(scope)
}

pub(super) async fn activity_feed(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
    Query(params): Query<FeedParams>,
) -> ApiResult<CursorPaginatedResponse<GlobalActivityResponse>> {
    let hash = parse_hash32(&hash, "type_hash")?;
    require_token(&state.store, &hash)?;
    tokio::task::spawn_blocking(move || {
        let scope = token_scope(&state.store, &hash).map_err(internal)?;
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
        let scope = token_scope(&state.store, &hash).map_err(internal)?;
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
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Day {
    date: String,
    supply: String,
    capacity: String,
    occupied_capacity: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Week {
    week_start: String,
    transfers_count: i64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SupplyEvent {
    tx_hash: String,
    block_number: i64,
    tx_index: i32,
    transfer_sequence: i32,
    timestamp: String,
    kind: String,
    amount: String,
    supply_after: String,
    lock_hash: String,
    address: Option<String>,
    lock_kind: Option<String>,
}

/// A token's history through its last complete day: everything its transfers
/// in blocks before the incomplete day's first block contribute. Complete
/// days never change without a fork, so this is built once per complete-day
/// boundary and kept in memory (keyed like every complete-day response).
#[derive(Clone, Serialize, Deserialize)]
struct CompleteHistory {
    daily: Vec<Day>,
    weekly: Vec<Week>,
    supply_events: Vec<SupplyEvent>,
    mint_count: i64,
    burn_count: i64,
    transfers_count: i64,
    /// Supply after the last complete day.
    supply: String,
    /// Owned capacity and occupied capacity after the last complete day.
    capacity: i128,
    occupied: i128,
}

/// Complete days of a token history stay valid until the complete-day
/// boundary moves (the key carries it), so the TTL only bounds memory.
const TOKEN_HISTORY_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(24 * 3600);

pub(super) async fn history(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> ApiResult<History> {
    let hash = parse_hash32(&hash, "type_hash")?;
    require_token(&state.store, &hash)?;
    let result = tokio::task::spawn_blocking(move || build_history(&state, &hash))
        .await
        .map_err(internal)??;
    ok(result)
}

/// What the transfers of one block range contribute: weekly transfer counts,
/// mint/burn events with their stats day, and the transfer count.
#[derive(Default)]
struct TransferTally {
    weeks: BTreeMap<NaiveDate, i64>,
    events: Vec<(i64, i32, i32, NaiveDate, TokenTransferRecord)>,
    count: i64,
}

/// Walk the transfers of `range`, dating each by its block's chain time.
/// Headers are read in one batch per 1024 transfer rows, never one point
/// read per block.
fn tally_transfers(
    store: &CkbadgerStore,
    hash: &[u8],
    range: BlockRange,
    tip: i64,
) -> anyhow::Result<TransferTally> {
    let mut tally = TransferTally::default();
    let mut pending = Vec::with_capacity(asset_feed::TRANSACTION_BATCH_SIZE);
    visit_transfers_in(store, hash, range, |block, seq, tx, row| {
        ensure!(
            block <= tip,
            "token history transfer after tip: block={block} tip={tip}"
        );
        pending.push((block, seq, tx.tx_index, row));
        if pending.len() == asset_feed::TRANSACTION_BATCH_SIZE {
            tally_batch(store, &mut pending, &mut tally)?;
        }
        Ok(())
    })?;
    tally_batch(store, &mut pending, &mut tally)?;
    Ok(tally)
}

fn tally_batch(
    store: &CkbadgerStore,
    pending: &mut Vec<(i64, i32, i32, TokenTransferRecord)>,
    tally: &mut TransferTally,
) -> anyhow::Result<()> {
    if pending.is_empty() {
        return Ok(());
    }
    let mut blocks: Vec<i64> = pending.iter().map(|(block, ..)| *block).collect();
    blocks.dedup();
    let headers = store.get_block_headers_batch(&blocks)?;
    for (block, seq, index, row) in pending.drain(..) {
        let header = headers
            .get(&block)
            .with_context(|| format!("token history missing header block={block}"))?;
        ensure!(
            header.chain_time_ms >= header.timestamp,
            "token history invalid chain time block={block}"
        );
        let date = stats_day(header.chain_time_ms)?;
        let monday = date
            .checked_sub_days(Days::new(u64::from(date.weekday().num_days_from_monday())))
            .context("token week underflow")?;
        let total = tally.weeks.entry(monday).or_default();
        *total = total
            .checked_add(1)
            .context("weekly token transfer count overflow")?;
        tally.count = tally
            .count
            .checked_add(1)
            .context("token transfer count overflow")?;
        if row.is_mint || row.is_burn {
            tally.events.push((block, seq, index, date, row));
        }
    }
    Ok(())
}

/// Apply mint/burn events in chain order on top of `supply`, recording each
/// as a [`SupplyEvent`] and the end-of-day supply of each date.
#[allow(clippy::type_complexity)]
fn apply_supply_events(
    state: &AppState,
    hash: &[u8],
    mut events: Vec<(i64, i32, i32, NaiveDate, TokenTransferRecord)>,
    mut supply: TokenBalance,
) -> anyhow::Result<(
    TokenBalance,
    BTreeMap<NaiveDate, TokenBalance>,
    Vec<SupplyEvent>,
    i64,
    i64,
)> {
    let store = &state.store;
    events.sort_by_key(|(block, seq, index, _, _)| (*block, *index, *seq));
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
            kind: if row.is_mint { "mint" } else { "burn" }.to_string(),
            amount: amount.to_string(),
            supply_after: supply.to_string(),
            lock_hash: format!("0x{}", hex::encode(lock)),
            address: resolve_lock_address(store, lock, &state.ckb_network)?
                .map(|resolved| resolved.address),
            lock_kind: participant_lock_kind(store, &lock32, None)?,
        });
    }
    Ok((supply, daily_supply, supply_events, mint_count, burn_count))
}

/// Zero-net daily rows are legitimately absent after mint/burn or
/// create/consume within one day. Positive end-of-day supply still requires
/// cells, so it cannot coexist with zero indexed capacity.
fn ensure_day_has_capacity(
    hash: &[u8],
    date: NaiveDate,
    supply: &TokenBalance,
    capacity: i128,
    occupied: i128,
) -> anyhow::Result<()> {
    ensure!(
        supply.is_zero() || (capacity > 0 && occupied > 0),
        "token history missing capacity history for positive supply: token=0x{} date={date} supply={supply} capacity={capacity} occupied={occupied}",
        hex::encode(hash)
    );
    Ok(())
}

/// The history of a token's complete days: transfers in blocks before
/// `boundary` (the incomplete day's first block) and daily capacity rows
/// through `last`.
fn build_complete_history(
    state: &AppState,
    hash: &[u8],
    boundary: i64,
    last: NaiveDate,
    tip: i64,
) -> anyhow::Result<CompleteHistory> {
    let store = &state.store;
    let tally = tally_transfers(
        store,
        hash,
        BlockRange {
            from: 0,
            before: Some(boundary),
        },
        tip,
    )?;
    let (supply, daily_supply, supply_events, mint_count, burn_count) =
        apply_supply_events(state, hash, tally.events, TokenBalance::zero())?;
    let mut capacities = BTreeMap::new();
    for (key, delta) in store.list_token_daily_deltas(hash)? {
        let date = NaiveDate::parse_from_str(&key.to_string(), "%Y%m%d")?;
        if date <= last {
            capacities.insert(date, delta);
        }
    }
    let first = daily_supply.keys().chain(capacities.keys()).min().copied();
    let mut daily = Vec::new();
    let (mut capacity, mut occupied) = (0, 0);
    let mut cumulative = TokenBalance::zero();
    if let Some(mut date) = first {
        while date <= last {
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
            ensure_day_has_capacity(hash, date, &cumulative, capacity, occupied)?;
            daily.push(Day {
                date: date.to_string(),
                supply: cumulative.to_string(),
                capacity: capacity.to_string(),
                occupied_capacity: occupied.to_string(),
            });
            date = date.succ_opt().context("token history date overflow")?;
        }
    }
    // Complete weeks only: a week whose end is past `last` holds the
    // incomplete day.
    let tip_day = last.succ_opt().context("token history date overflow")?;
    let mut weekly = Vec::new();
    if let Some(mut week) = tally.weeks.keys().next().copied() {
        loop {
            let end = week
                .checked_add_days(Days::new(7))
                .context("token week overflow")?;
            if end > tip_day {
                break;
            }
            weekly.push(Week {
                week_start: week.to_string(),
                transfers_count: tally.weeks.get(&week).copied().unwrap_or(0),
            });
            week = end;
        }
    }
    Ok(CompleteHistory {
        daily,
        weekly,
        supply_events,
        mint_count,
        burn_count,
        transfers_count: tally.count,
        supply: supply.to_string(),
        capacity,
        occupied,
    })
}

/// A token's history: its complete days (built once per complete-day
/// boundary, see [`CompleteHistory`]) plus the incomplete day, read live from
/// the transfers at and after the boundary block.
fn build_history(state: &AppState, hash: &[u8]) -> Result<History, crate::response::ApiRouteError> {
    let store = &state.store;
    let (tip, header) = store
        .get_sync_tip_block()
        .map_err(internal)?
        .ok_or_else(|| ApiError::internal("token history requires indexed tip header"))?;
    let chain = ChainTime::load(store)?
        .ok_or_else(|| ApiError::internal("token history requires an indexed chain"))?;
    if chain.tip_number() != tip {
        return Err(ApiError::internal(format!(
            "token history tip disagrees with the chain clock: tip={tip} clock={}",
            chain.tip_number()
        )));
    }
    let tip_day = chain.tip_day()?;
    let last = chain.last_complete_day()?;
    let boundary = chain.complete_day_boundary_block()?;
    let key = complete_days_cache_key(
        &format!("token-history:v1:0x{}", hex::encode(hash)),
        Some(chain),
    )?;
    let complete = match state.mem_cache.get::<CompleteHistory>(&key) {
        Some(complete) => complete,
        None => {
            let complete =
                build_complete_history(state, hash, boundary, last, tip).map_err(internal)?;
            state
                .mem_cache
                .set(&key, &complete, TOKEN_HISTORY_CACHE_TTL);
            complete
        }
    };
    let today = (|| -> anyhow::Result<_> {
        let tally = tally_transfers(
            store,
            hash,
            BlockRange {
                from: boundary,
                before: None,
            },
            tip,
        )?;
        for (block, seq, _, date, _) in &tally.events {
            ensure!(
                *date == tip_day,
                "token history transfer at or after the incomplete day's first block {boundary} is dated {date}, not {tip_day}: block={block} sequence={seq}"
            );
        }
        let supply = complete
            .supply
            .parse::<TokenBalance>()
            .map_err(|e| anyhow::anyhow!("cached token supply: {e}"))?;
        let (supply, _, events, mints, burns) =
            apply_supply_events(state, hash, tally.events, supply)?;
        let (mut capacity, mut occupied) = (complete.capacity, complete.occupied);
        for (key, delta) in store.list_token_daily_deltas_in_range(
            hash,
            Some(stats_day_key(tip_day)?),
            None,
        )? {
            ensure!(
                key == stats_day_key(tip_day)?,
                "token capacity delta after indexed tip: date={key} tip={tip_day}"
            );
            (capacity, occupied) = apply_owned_capacity_delta(
                capacity,
                occupied,
                delta.owned_capacity_delta,
                delta.owned_knowledge_delta,
                &format!("token history token=0x{} date={tip_day}", hex::encode(hash)),
            )?;
        }
        ensure_day_has_capacity(hash, tip_day, &supply, capacity, occupied)?;
        let (_, current) = store.aggregate_token_holder_stats(hash)?;
        ensure!(supply==current,"token supply history disagrees with current holders: token=0x{} history={supply} holders={current}",hex::encode(hash));
        Ok((tally.count, events, mints, burns))
    })()
    .map_err(internal)?;
    let (count, events, mints, burns) = today;
    let mut supply_events = complete.supply_events;
    supply_events.extend(events);
    Ok(History {
        source: "indexed_chain_time",
        complete_through: last.to_string(),
        anchor: Anchor {
            block_number: tip,
            block_hash: format!("0x{}", hex::encode(header.hash)),
        },
        daily: complete.daily,
        weekly: complete.weekly,
        supply_events,
        mint_count: complete
            .mint_count
            .checked_add(mints)
            .ok_or_else(|| ApiError::internal("mint count overflow"))?,
        burn_count: complete
            .burn_count
            .checked_add(burns)
            .ok_or_else(|| ApiError::internal("burn count overflow"))?,
        transfers_count: complete
            .transfers_count
            .checked_add(count)
            .ok_or_else(|| ApiError::internal("token transfer count overflow"))?,
    })
}

/// The `YYYYMMDD` key of a stats day.
fn stats_day_key(date: NaiveDate) -> anyhow::Result<u32> {
    date.format("%Y%m%d")
        .to_string()
        .parse()
        .context("stats day key")
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

#[cfg(test)]
mod batch_tests {
    use super::*;
    use ckbadger_store::{StoreBatch, TxIndexEntry};

    const TOKEN: [u8; 32] = [0x71; 32];

    fn tx_hash(block: i64) -> [u8; 32] {
        let mut hash = [0x81; 32];
        hash[..8].copy_from_slice(&block.to_be_bytes());
        hash
    }

    // Descending scan: the two legs at height 2 straddle the 1024-row boundary.
    fn fixture() -> (tempfile::TempDir, CkbadgerStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        let mut batch = StoreBatch::new(&store);
        for block in 0..1026 {
            let hash = tx_hash(block);
            batch.put_tx_hash_map(&hash, block, 3);
            batch.put_tx_index(
                block,
                3,
                &TxIndexEntry {
                    tx_hash: hash,
                    is_cellbase: false,
                    timestamp: block * 1000,
                    inputs_count: 1,
                    outputs_count: 2,
                    fee: 10,
                    tx_size: 100,
                    cycles: Some(42),
                    semantic_tags: 0,
                },
            );
            for sequence in 0..if block == 2 { 2 } else { 1 } {
                batch.put_token_transfer(
                    &TOKEN,
                    block,
                    sequence,
                    &TokenTransferRecord {
                        tx_hash: hash.to_vec(),
                        block_number: block,
                        from_lock_hash: Some(vec![0x31; 32]),
                        to_lock_hash: vec![0x32; 32],
                        amount: 10,
                        is_mint: false,
                        is_burn: false,
                        timestamp: block * 1000,
                    },
                );
            }
        }
        batch.commit().unwrap();
        (dir, store)
    }

    #[test]
    fn token_scope_reuses_validated_transactions_in_bounded_batches() {
        let (_dir, store) = fixture();
        store.reset_read_call_counters();
        let scope = token_scope(&store, &TOKEN).unwrap();
        assert_eq!(scope.len(), 1026);
        for (row, block) in scope.iter().zip((0..1026).rev()) {
            assert_eq!((row.block_number, row.tx_index), (block, 3));
            assert_eq!(row.entry.tx_hash, tx_hash(block));
            assert_eq!(row.entry.cycles, Some(42));
        }
        assert_eq!(store.read_call_counts(), (0, 4),
            "two bounded batches, each with one location and one index multi-get; no second localization");
    }

    #[test]
    fn canonical_scope_batches_unique_hashes_without_changing_order_or_entries() {
        let (_dir, store) = fixture();
        store.reset_read_call_counters();
        let hashes = (0..1026).flat_map(|block| [tx_hash(block).to_vec(), tx_hash(block).to_vec()]);
        let scope = asset_feed::canonical_scope(&store, hashes).unwrap();
        assert_eq!(scope.len(), 1026);
        assert_eq!(scope[0].entry.tx_hash, tx_hash(1025));
        assert_eq!(scope.last().unwrap().entry.tx_hash, tx_hash(0));
        assert!(scope
            .iter()
            .all(|row| row.tx_index == 3 && row.entry.cycles == Some(42)));
        assert_eq!(
            store.read_call_counts(),
            (0, 4),
            "duplicate hashes must not create additional point lookups or unbounded batches"
        );
    }

    #[test]
    fn token_scope_rejects_missing_and_mismatched_canonical_rows_in_later_batch() {
        for corrupt_hash in [false, true] {
            let (_dir, store) = fixture();
            if corrupt_hash {
                let mut entry = store.get_tx_index(0, 3).unwrap().unwrap();
                entry.tx_hash = [0xff; 32];
                let mut batch = StoreBatch::new(&store);
                batch.put_tx_index(0, 3, &entry);
                batch.commit().unwrap();
            } else {
                store
                    .delete_cf(store.cf_tx_hash_map(), &tx_hash(0))
                    .unwrap();
            }
            let error = token_scope(&store, &TOKEN).err().unwrap().to_string();
            assert!(error.contains(&hex::encode(tx_hash(0))), "{error}");
            assert!(
                error.contains(if corrupt_hash {
                    "hash mismatch"
                } else {
                    "missing canonical"
                }),
                "{error}"
            );
        }
    }

    #[test]
    fn asset_batch_rejects_invalid_hashes_and_oversized_requests_before_reading() {
        let dir = tempfile::tempdir().unwrap();
        let store = CkbadgerStore::open_domain(dir.path()).unwrap();
        for (hashes, expected) in [
            (vec![vec![0; 31]], "invalid transaction hash length"),
            (vec![vec![0; 32]; 1025], "batch exceeds 1024"),
        ] {
            let error = asset_feed::canonical_transactions(&store, &hashes)
                .err()
                .unwrap()
                .to_string();
            assert!(error.contains(expected), "{error}");
        }
        assert_eq!(store.read_call_counts(), (0, 0));
    }

    #[test]
    fn token_scope_validates_every_transfer_leg_across_batch_boundary() {
        let (_dir, store) = fixture();
        let key = keys::encode_token_transfer_key(&TOKEN, 2, 0);
        let mut row: TokenTransferRecord = bincode::deserialize(
            &store
                .get_cf(store.cf_token_transfers(), &key)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        row.timestamp += 1;
        store
            .put_cf(
                store.cf_token_transfers(),
                &key,
                &bincode::serialize(&row).unwrap(),
            )
            .unwrap();
        let error = token_scope(&store, &TOKEN).err().unwrap().to_string();
        assert!(error.contains("timestamp mismatch"), "{error}");
        assert!(error.contains(&hex::encode(tx_hash(2))), "{error}");
    }
}
