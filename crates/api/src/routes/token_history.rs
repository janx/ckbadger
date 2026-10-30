//! Exact read projections of the existing token indexes; no persistent state.
use super::{
    activities::{participant_lock_kind, GlobalActivityResponse},
    asset_feed,
    transactions::TransactionResponse,
};
use crate::utils::address::resolve_lock_address;
use crate::{
    response::{default_limit, ok, ApiError, ApiResult, CursorPaginatedResponse},
    utils::{apply_owned_capacity_delta, parse_hash32, stats_bucket::ChainTime},
    AppState,
};
use anyhow::{ensure, Context};
use axum::extract::{Path, Query, State};
use chrono::{Datelike, Days, NaiveDate};
use ckbadger_common::{stats_bucket::stats_day, TokenBalance};
use ckbadger_store::{keys, CkbadgerStore, TokenTransferRecord};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

#[derive(Deserialize)]
pub(super) struct FeedParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
}

/// A block range `[from, before)` of a token's transfers.
#[derive(Debug, Clone, Copy)]
struct BlockRange {
    from: i64,
    before: Option<i64>,
}

/// One `token_transfers` row of token `hash`, its key and payload checked.
/// The one decode every transfer reader here uses.
fn decode_transfer_row(
    hash: &[u8],
    key: &[u8],
    value: &[u8],
) -> anyhow::Result<(i64, i32, TokenTransferRecord)> {
    ensure!(
        key.len() == 44,
        "token transfer malformed key token=0x{} key=0x{}",
        hex::encode(hash),
        hex::encode(key)
    );
    let (block, sequence) = keys::decode_token_transfer_key(key);
    let row: TokenTransferRecord = bincode::deserialize(value)
        .with_context(|| format!("token transfer decode key=0x{}", hex::encode(key)))?;
    ensure!(
        block >= 0
            && sequence >= 0
            && row.block_number == block
            && row.tx_hash.len() == 32
            && !(row.is_mint && row.is_burn),
        "token transfer invalid identity/flags key=0x{} tx=0x{}",
        hex::encode(key),
        hex::encode(&row.tx_hash)
    );
    Ok((block, sequence, row))
}

/// A transfer leg agrees with the canonical transaction it names.
fn check_leg(
    tx: &asset_feed::AssetTransaction,
    block: i64,
    row: &TokenTransferRecord,
) -> anyhow::Result<()> {
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
    Ok(())
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
        if key.len() == 44 && keys::decode_token_transfer_key(&key).0 < range.from {
            break;
        }
        let (block, sequence, row) = decode_transfer_row(hash, &key, &value)?;
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
        check_leg(tx, block, &row)?;
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

/// One page of the token's transfer transactions — the transactions with at
/// least one `token_transfers` row, the scope of its Activities /
/// Transactions tabs — newest first, read from the transfer index at
/// `cursor`. Only the legs of the transactions up to the page's `limit + 1`
/// (plus the cursor block's newer ones) are read, and they are resolved in
/// bounded batches; `total` is the persisted count (`Σ transfer_tx_count`).
/// A cursor that names no transaction of this token is a 400.
fn token_feed_page(
    store: &CkbadgerStore,
    hash: &[u8],
    limit: i64,
    cursor: Option<&str>,
) -> Result<
    (Vec<asset_feed::AssetTransaction>, i64, i64, Option<String>),
    crate::response::ApiRouteError,
> {
    let limit = asset_feed::feed_limit(limit);
    let cursor = crate::utils::parse_optional_block_tx_cursor(cursor, "asset transaction cursor")?;
    let need = usize::try_from(limit).map_err(internal)? + 1;
    let total = TokenDays::load(store, hash)
        .and_then(|days| {
            days.days.iter().try_fold(0i64, |sum, (_, delta, ..)| {
                sum.checked_add(i64::try_from(delta.transfer_tx_count)?)
                    .context("token transfer transaction count overflow")
            })
        })
        .map_err(internal)?;
    let start = match cursor {
        // The first row of the cursor's block (sequences are keyed descending).
        Some((block, _)) => keys::encode_token_transfer_key(hash, block, i32::MAX),
        None => hash.to_vec(),
    };
    let mut rows: Vec<asset_feed::AssetTransaction> = Vec::with_capacity(need);
    let mut cursor_seen = false;
    // Complete transaction groups not yet resolved, and the open one.
    let mut groups: Vec<Vec<(i64, i32, TokenTransferRecord)>> = Vec::new();
    let mut open: Vec<(i64, i32, TokenTransferRecord)> = Vec::new();
    let mut resolve = |groups: &mut Vec<Vec<(i64, i32, TokenTransferRecord)>>,
                       rows: &mut Vec<asset_feed::AssetTransaction>|
     -> anyhow::Result<()> {
        let hashes: Vec<Vec<u8>> = groups
            .iter()
            .map(|legs| legs[0].2.tx_hash.clone())
            .collect();
        let resolved = asset_feed::canonical_transactions(store, &hashes)?;
        for (legs, tx) in groups.drain(..).zip(resolved) {
            ensure!(
                tx.entry.tx_hash.as_slice() == legs[0].2.tx_hash.as_slice(),
                "token feed resolved tx=0x{} for 0x{}",
                hex::encode(tx.entry.tx_hash),
                hex::encode(&legs[0].2.tx_hash)
            );
            for (block, _, row) in &legs {
                check_leg(&tx, *block, row)?;
            }
            let location = (tx.block_number, tx.tx_index);
            if let Some(last) = rows.last() {
                ensure!(
                    location < (last.block_number, last.tx_index),
                    "token transfer index out of transaction order: tx=0x{} at {location:?} after {:?}",
                    hex::encode(tx.entry.tx_hash),
                    (last.block_number, last.tx_index)
                );
            }
            match cursor {
                Some(key) if location > key => {}
                Some(key) if location == key => cursor_seen = true,
                _ => rows.push(tx),
            }
        }
        Ok(())
    };
    (|| -> anyhow::Result<()> {
        for item in store.iterator_cf(
            store.cf_token_transfers(),
            rocksdb::IteratorMode::From(&start, rocksdb::Direction::Forward),
        ) {
            let (key, value) = item?;
            if !key.starts_with(hash) {
                break;
            }
            let leg = decode_transfer_row(hash, &key, &value)?;
            if open
                .first()
                .is_some_and(|first| first.2.tx_hash != leg.2.tx_hash)
            {
                groups.push(std::mem::take(&mut open));
                if groups.len() + rows.len() >= need {
                    resolve(&mut groups, &mut rows)?;
                    if rows.len() >= need {
                        return Ok(());
                    }
                }
            }
            open.push(leg);
        }
        if !open.is_empty() {
            groups.push(std::mem::take(&mut open));
        }
        if !groups.is_empty() {
            resolve(&mut groups, &mut rows)?;
        }
        Ok(())
    })()
    .map_err(internal)?;
    if cursor.is_some() && !cursor_seen {
        return Err(ApiError::bad_request(
            "asset transaction cursor does not belong to this scope",
        ));
    }
    if !rows.is_empty() && total == 0 {
        return Err(internal(format!(
            "token 0x{} has transfer transactions but no persisted transfer_tx_count",
            hex::encode(hash)
        )));
    }
    let more = rows.len() > usize::try_from(limit).map_err(internal)?;
    rows.truncate(usize::try_from(limit).map_err(internal)?);
    let next = if more {
        rows.last().map(asset_feed::feed_cursor)
    } else {
        None
    };
    Ok((rows, total, limit, next))
}

pub(super) async fn activity_feed(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
    Query(params): Query<FeedParams>,
) -> ApiResult<CursorPaginatedResponse<GlobalActivityResponse>> {
    let hash = parse_hash32(&hash, "type_hash")?;
    require_token(&state.store, &hash)?;
    tokio::task::spawn_blocking(move || {
        let (rows, total, limit, next) =
            token_feed_page(&state.store, &hash, params.limit, params.cursor.as_deref())?;
        ok(CursorPaginatedResponse::new(
            asset_feed::project_activities(&state, rows)?,
            total,
            limit,
            next,
        ))
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
        let (rows, total, limit, next) =
            token_feed_page(&state.store, &hash, params.limit, params.cursor.as_deref())?;
        ok(CursorPaginatedResponse::new(
            asset_feed::project_transactions(&state, rows)?,
            total,
            limit,
            next,
        ))
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

/// A token's history, built from its persisted daily rows
/// (`TokenDailyDelta`): capacity, supply (`minted - burned`), transactions
/// and mint / burn record counts per UTC+8 chain-time day. Complete days are
/// rows the indexer sealed; the tip (incomplete) day's row is the indexer's
/// running row. No transfer is read and no block header per transfer block.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct History {
    source: &'static str,
    complete_through: String,
    anchor: Anchor,
    /// One point per complete day from the token's first day.
    daily: Vec<Day>,
    /// Transactions per complete Monday-start week (`TokenDailyDelta.tx_count`:
    /// transactions that created or consumed a cell of the token).
    weekly_transactions: Vec<Week>,
    /// Mint / burn transfer records through the tip.
    mint_count: u64,
    burn_count: u64,
    /// Every transfer record through the tip (the token's persisted counter).
    transfers_count: i64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Anchor {
    block_number: i64,
    block_hash: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Day {
    date: String,
    supply: String,
    capacity: String,
    occupied_capacity: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Week {
    week_start: String,
    transactions_count: u64,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SupplyEvent {
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
        .map_err(internal)??;
    ok(result)
}

/// A token's daily rows in date order, each with the supply and owned
/// capacity after it. The one fold of `TokenDailyDelta` rows every token
/// read uses (history, supply events).
struct TokenDays {
    days: Vec<(
        NaiveDate,
        ckbadger_store::TokenDailyDelta,
        TokenBalance,
        i128,
        i128,
    )>,
}

impl TokenDays {
    fn load(store: &CkbadgerStore, hash: &[u8]) -> anyhow::Result<Self> {
        let mut days = Vec::new();
        let (mut supply, mut capacity, mut occupied) = (TokenBalance::zero(), 0i128, 0i128);
        for (key, delta) in store.list_token_daily_deltas(hash)? {
            let date = NaiveDate::parse_from_str(&key.to_string(), "%Y%m%d")
                .with_context(|| format!("token daily key {key}"))?;
            let context = || format!("token=0x{} date={date}", hex::encode(hash));
            ensure!(
                delta.tx_count > 0,
                "token day without a transaction: {}",
                context()
            );
            supply = delta
                .apply_supply(&supply)?
                .with_context(|| format!("token day burns more than its supply: {}", context()))?;
            (capacity, occupied) = apply_owned_capacity_delta(
                capacity,
                occupied,
                delta.owned_capacity_delta,
                delta.owned_knowledge_delta,
                &format!("token history {}", context()),
            )?;
            ensure_day_has_capacity(hash, date, &supply, capacity, occupied)?;
            days.push((date, delta, supply.clone(), capacity, occupied));
        }
        Ok(Self { days })
    }

    /// The supply after the last row (the token's current supply).
    fn supply(&self) -> TokenBalance {
        self.days
            .last()
            .map(|(_, _, supply, _, _)| supply.clone())
            .unwrap_or_else(TokenBalance::zero)
    }
}

/// Positive end-of-day supply requires cells, so it cannot coexist with zero
/// indexed capacity.
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

fn week_start(date: NaiveDate) -> anyhow::Result<NaiveDate> {
    date.checked_sub_days(Days::new(u64::from(date.weekday().num_days_from_monday())))
        .context("token week underflow")
}

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
    (|| -> anyhow::Result<History> {
        let days = TokenDays::load(store, hash)?;
        let (_, holders) = store.aggregate_token_holder_stats(hash)?;
        ensure!(
            days.supply() == holders,
            "token supply history disagrees with current holders: token=0x{} history={} holders={holders}",
            hex::encode(hash),
            days.supply()
        );
        let (mut mint_count, mut burn_count) = (0u64, 0u64);
        let mut weeks: BTreeMap<NaiveDate, u64> = BTreeMap::new();
        let mut daily = Vec::new();
        let mut cursor: Option<(NaiveDate, String, i128, i128)> = None;
        for (date, delta, supply, capacity, occupied) in &days.days {
            ensure!(
                *date <= tip_day,
                "token day after the indexed tip: token=0x{} date={date} tip={tip_day}",
                hex::encode(hash)
            );
            mint_count = mint_count
                .checked_add(delta.mint_count)
                .context("mint count overflow")?;
            burn_count = burn_count
                .checked_add(delta.burn_count)
                .context("burn count overflow")?;
            if *date > last {
                continue;
            }
            let week = weeks.entry(week_start(*date)?).or_default();
            *week = week
                .checked_add(delta.tx_count)
                .context("weekly token transaction count overflow")?;
            // Fill the days without a row with the previous day's values.
            if let Some((mut fill, ref fill_supply, fill_capacity, fill_occupied)) = cursor {
                fill = fill.succ_opt().context("token history date overflow")?;
                while fill < *date {
                    daily.push(Day {
                        date: fill.to_string(),
                        supply: fill_supply.clone(),
                        capacity: fill_capacity.to_string(),
                        occupied_capacity: fill_occupied.to_string(),
                    });
                    fill = fill.succ_opt().context("token history date overflow")?;
                }
            }
            daily.push(Day {
                date: date.to_string(),
                supply: supply.to_string(),
                capacity: capacity.to_string(),
                occupied_capacity: occupied.to_string(),
            });
            cursor = Some((*date, supply.to_string(), *capacity, *occupied));
        }
        if let Some((mut fill, fill_supply, fill_capacity, fill_occupied)) = cursor {
            fill = fill.succ_opt().context("token history date overflow")?;
            while fill <= last {
                daily.push(Day {
                    date: fill.to_string(),
                    supply: fill_supply.clone(),
                    capacity: fill_capacity.to_string(),
                    occupied_capacity: fill_occupied.to_string(),
                });
                fill = fill.succ_opt().context("token history date overflow")?;
            }
        }
        // Complete weeks only: a week whose end is past `last` holds the
        // incomplete day.
        let mut weekly_transactions = Vec::new();
        if let Some(mut week) = weeks.keys().next().copied() {
            loop {
                let end = week
                    .checked_add_days(Days::new(7))
                    .context("token week overflow")?;
                if end > tip_day {
                    break;
                }
                weekly_transactions.push(Week {
                    week_start: week.to_string(),
                    transactions_count: weeks.get(&week).copied().unwrap_or(0),
                });
                week = end;
            }
        }
        Ok(History {
            source: "indexed_chain_time",
            complete_through: last.to_string(),
            anchor: Anchor {
                block_number: tip,
                block_hash: format!("0x{}", hex::encode(&header.hash)),
            },
            daily,
            weekly_transactions,
            mint_count,
            burn_count,
            transfers_count: store.get_token_transfers_count(hash)?,
        })
    })()
    .map_err(internal)
}

/// Every mint and burn of a token, newest first, paged (IV-27: no longer part
/// of `/history`). `total` is the persisted record count (`Σ mint_count +
/// burn_count`). A page reads transfers only inside the days whose daily row
/// records a mint or burn, and `supplyAfter` comes from that day's persisted
/// end-of-day supply, walked back through the day's own events.
pub(super) async fn supply_events(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
    Query(params): Query<FeedParams>,
) -> ApiResult<CursorPaginatedResponse<SupplyEvent>> {
    let hash = parse_hash32(&hash, "type_hash")?;
    require_token(&state.store, &hash)?;
    let limit = params.limit.clamp(1, 100);
    let cursor = crate::utils::parse_optional_block_tx_cursor(
        params.cursor.as_deref(),
        "supply event cursor",
    )?;
    let page = tokio::task::spawn_blocking(move || {
        build_supply_events(&state, &hash, limit, cursor).map_err(internal)
    })
    .await
    .map_err(internal)??;
    ok(page)
}

fn build_supply_events(
    state: &AppState,
    hash: &[u8],
    limit: i64,
    cursor: Option<(i64, i32)>,
) -> anyhow::Result<CursorPaginatedResponse<SupplyEvent>> {
    let store = &state.store;
    let days = TokenDays::load(store, hash)?;
    let total = days.days.iter().try_fold(0u64, |sum, (_, delta, ..)| {
        sum.checked_add(delta.mint_count)
            .and_then(|sum| sum.checked_add(delta.burn_count))
            .context("supply event count overflow")
    })?;
    let cursor_day = cursor
        .map(|(block, _)| -> anyhow::Result<NaiveDate> {
            let header = store
                .get_block_header(block)?
                .with_context(|| format!("supply event cursor names unindexed block {block}"))?;
            stats_day(header.chain_time_ms)
        })
        .transpose()?;
    let wanted = usize::try_from(limit)? + 1;
    let mut events: Vec<SupplyEvent> = Vec::new();
    let mut previous_supply = TokenBalance::zero();
    let mut day_supply_before = Vec::with_capacity(days.days.len());
    for (_, _, supply, _, _) in &days.days {
        day_supply_before.push(previous_supply.clone());
        previous_supply = supply.clone();
    }
    for (index, (date, delta, end_supply, _, _)) in days.days.iter().enumerate().rev() {
        if events.len() >= wanted {
            break;
        }
        if delta.mint_count == 0 && delta.burn_count == 0 {
            continue;
        }
        if cursor_day.is_some_and(|cursor_day| *date > cursor_day) {
            continue;
        }
        let context = || format!("token=0x{} date={date}", hex::encode(hash));
        let start_ms = ckbadger_common::stats_bucket::utc8_day_end_ms(
            date.pred_opt().context("supply event day underflow")?,
        )?;
        let end_ms = ckbadger_common::stats_bucket::utc8_day_end_ms(*date)?;
        let from = store
            .find_first_block_at_or_after_chain_time_ms(start_ms)?
            .with_context(|| {
                format!("no block on a token day with supply events: {}", context())
            })?;
        let before = store.find_first_block_at_or_after_chain_time_ms(end_ms)?;
        // Walk the day newest first from its end-of-day supply; every mint /
        // burn of the day is seen, so the walk must land on the supply before
        // the day and on the row's record counts.
        let mut running = end_supply.clone();
        let (mut mints, mut burns) = (0u64, 0u64);
        let mut day_events = Vec::new();
        visit_transfers_in(
            store,
            hash,
            BlockRange { from, before },
            |block, seq, tx, row| {
                if !(row.is_mint || row.is_burn) {
                    return Ok(());
                }
                let amount = TokenBalance::from(row.amount);
                let supply_after = running.clone();
                running = if row.is_mint {
                    mints += 1;
                    running.checked_sub(&amount)
                } else {
                    burns += 1;
                    running.checked_add(&amount)
                }
                .with_context(|| {
                    format!(
                        "supply walk out of range block={block} sequence={seq}: {}",
                        context()
                    )
                })?;
                if cursor.is_some_and(|(cursor_block, cursor_seq)| {
                    (block, seq) >= (cursor_block, cursor_seq)
                }) {
                    return Ok(());
                }
                day_events.push((block, seq, tx.tx_index, row, amount, supply_after));
                Ok(())
            },
        )?;
        ensure!(
            (mints, burns) == (delta.mint_count, delta.burn_count),
            "token day's mint/burn records disagree with its row: rows=({mints}, {burns}) counts=({}, {}) {}",
            delta.mint_count,
            delta.burn_count,
            context()
        );
        ensure!(
            running == day_supply_before[index],
            "token day's supply walk does not land on the previous day's supply: walked={running} previous={} {}",
            day_supply_before[index],
            context()
        );
        for (block, seq, tx_index, row, amount, supply_after) in day_events {
            if events.len() >= wanted {
                break;
            }
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
            events.push(SupplyEvent {
                tx_hash: format!("0x{}", hex::encode(&row.tx_hash)),
                block_number: block,
                tx_index,
                transfer_sequence: seq,
                timestamp: row.timestamp.to_string(),
                kind: if row.is_mint { "mint" } else { "burn" },
                amount: amount.to_string(),
                supply_after: supply_after.to_string(),
                lock_hash: format!("0x{}", hex::encode(lock)),
                address: resolve_lock_address(store, lock, &state.ckb_network)?
                    .map(|resolved| resolved.address),
                lock_kind: participant_lock_kind(store, &lock32, None)?,
            });
        }
    }
    let has_more = events.len() >= wanted;
    events.truncate(usize::try_from(limit)?);
    // `CursorPaginatedResponse::new` derives `hasMore` from the cursor.
    let next_cursor = if has_more {
        events
            .last()
            .map(|event| format!("{}:{}", event.block_number, event.transfer_sequence))
    } else {
        None
    };
    Ok(CursorPaginatedResponse::new(
        events,
        i64::try_from(total)?,
        limit,
        next_cursor,
    ))
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

/// Index rows [`live_cells_count`] validates per batch read: two `multi_get`s
/// (live markers, then payloads) per this many cells instead of two point
/// reads per cell.
const LIVE_CELL_COUNT_BATCH: usize = 1024;

/// Count the physical live token cells, including cells with a zero amount.
///
/// The count is the token's `cell_by_type` rows; every row is validated
/// against its live marker (a stale row is an error, never a count) and its
/// immutable payload (type hash and creation height), a batch at a time.
pub(super) fn live_cells_count(state: &AppState, hash: &[u8]) -> anyhow::Result<i64> {
    let mut count = 0i64;
    let mut outpoints: Vec<(Vec<u8>, i16, i64)> = Vec::with_capacity(LIVE_CELL_COUNT_BATCH);
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
        let (tx_hash, index) = keys::decode_outpoint(&key[40..]);
        outpoints.push((tx_hash, index, keys::decode_block_num(&key[32..40])));
        if outpoints.len() == LIVE_CELL_COUNT_BATCH {
            count = validated_live_token_cells(state, hash, &outpoints, count)?;
            outpoints.clear();
        }
    }
    validated_live_token_cells(state, hash, &outpoints, count)
}

/// `count` plus the index rows `outpoints` (`(tx hash, index, height the
/// index key names)`), each validated against its live marker and payload.
fn validated_live_token_cells(
    state: &AppState,
    hash: &[u8],
    outpoints: &[(Vec<u8>, i16, i64)],
    count: i64,
) -> anyhow::Result<i64> {
    if outpoints.is_empty() {
        return Ok(count);
    }
    let refs: Vec<(&[u8], i16)> = outpoints
        .iter()
        .map(|(tx_hash, index, _)| (tx_hash.as_slice(), *index))
        .collect();
    let cells = state
        .store
        .get_cells_batch(&refs, &state.append_only_store)?;
    for (tx_hash, index, height) in outpoints {
        let key = || hex::encode(keys::encode_cell_index_key(hash, *height, tx_hash, *index));
        let cell = cells
            .get(&(tx_hash.clone(), *index))
            .with_context(|| format!("token cell index missing live marker: key=0x{}", key()))?;
        ensure!(
            cell.type_script_hash.as_deref() == Some(hash) && *height == cell.created_at_block,
            "token cell index/payload mismatch: key=0x{}",
            key()
        );
    }
    let batch = i64::try_from(outpoints.len()).context("token live cell batch overflow")?;
    count
        .checked_add(batch)
        .context("token live cell count overflow")
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

    // 1026 transfer transactions; the one at height 2 has two legs.
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
        // The day row the indexer writes for these 1026 transfer transactions.
        store
            .put_token_daily_delta(
                &TOKEN,
                19700101,
                &ckbadger_store::TokenDailyDelta {
                    tx_count: 1026,
                    transfer_tx_count: 1026,
                    ..Default::default()
                },
            )
            .unwrap();
        (dir, store)
    }

    /// Every page of the token's transactions, walked by cursor.
    fn walk(
        store: &CkbadgerStore,
        limit: i64,
    ) -> anyhow::Result<Vec<asset_feed::AssetTransaction>> {
        let mut all = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let (rows, total, _, next) = token_feed_page(store, &TOKEN, limit, cursor.as_deref())
                .map_err(|e| anyhow::anyhow!("{e:?}"))?;
            assert_eq!(total, 1026);
            all.extend(rows);
            match next {
                Some(next) => cursor = Some(next),
                None => return Ok(all),
            }
        }
    }

    #[test]
    fn token_feed_page_resolves_one_page_in_one_bounded_batch() {
        let (_dir, store) = fixture();
        store.reset_read_call_counters();
        let (rows, total, limit, next) = token_feed_page(&store, &TOKEN, 100, None).unwrap();
        assert_eq!((total, limit, rows.len()), (1026, 100, 100));
        for (row, block) in rows.iter().zip((0..1026).rev()) {
            assert_eq!((row.block_number, row.tx_index), (block, 3));
            assert_eq!(row.entry.tx_hash, tx_hash(block));
            assert_eq!(row.entry.cycles, Some(42));
        }
        assert_eq!(next.as_deref(), Some("926:3"));
        assert_eq!(
            store.read_call_counts(),
            (0, 2),
            "one bounded batch: one location and one index multi-get"
        );
        let all = walk(&store, 100).unwrap();
        assert_eq!(all.len(), 1026, "the two-leg transaction is listed once");
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
    fn token_feed_rejects_missing_and_mismatched_canonical_rows_in_a_later_page() {
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
            let error = walk(&store, 100).err().unwrap().to_string();
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
    fn token_feed_validates_every_transfer_leg() {
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
        let error = walk(&store, 100).err().unwrap().to_string();
        assert!(error.contains("timestamp mismatch"), "{error}");
        assert!(error.contains(&hex::encode(tx_hash(2))), "{error}");
    }
}
