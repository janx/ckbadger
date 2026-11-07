//! One canonical projection for every asset's Activities / Transactions tabs.
use std::collections::{HashMap, HashSet};

use anyhow::{ensure, Context};
use ckbadger_store::{CkbadgerStore, ResolvedTx, TxActions, TxIndexEntry};

use super::{
    activities::{build_global_activity_response, GlobalActivityResponse},
    transactions::{transaction_response, TransactionResponse},
};
use crate::{
    response::{ok, ApiError, ApiResult, ApiRouteError, CursorPaginatedResponse},
    utils::parse_optional_block_tx_cursor,
    AppState,
};

pub(crate) const TRANSACTION_BATCH_SIZE: usize = 1024;

#[derive(Clone)]
pub(crate) struct AssetTransaction {
    pub block_number: i64,
    pub tx_index: i32,
    pub entry: TxIndexEntry,
}

/// Resolve one bounded batch in the pinned canonical view. Callers retain these
/// entries instead of locating the same transaction again during projection.
pub(crate) fn canonical_transactions(
    store: &CkbadgerStore,
    hashes: &[Vec<u8>],
) -> anyhow::Result<Vec<AssetTransaction>> {
    ensure!(
        hashes.len() <= TRANSACTION_BATCH_SIZE,
        "asset transaction batch exceeds {TRANSACTION_BATCH_SIZE}: {}",
        hashes.len()
    );
    for hash in hashes {
        ensure!(
            hash.len() == 32,
            "asset feed invalid transaction hash length {}",
            hash.len()
        );
    }
    store
        .get_txs_by_hash_batch(hashes)?
        .into_iter()
        .map(|(hash, row)| {
            let (block_number, tx_index, entry) = row.with_context(|| {
                format!(
                    "asset feed missing canonical transaction 0x{}",
                    hex::encode(&hash)
                )
            })?;
            ensure!(
                block_number >= 0 && tx_index >= 0,
                "asset feed invalid location block={block_number} index={tx_index} tx=0x{}",
                hex::encode(&hash)
            );
            Ok(AssetTransaction {
                block_number,
                tx_index,
                entry,
            })
        })
        .collect()
}

pub(crate) fn sort_scope(scope: &mut [AssetTransaction]) {
    scope.sort_by_key(|row| std::cmp::Reverse((row.block_number, row.tx_index)));
}

/// Discovery owns whether an orphan history outpoint belongs to its scope.
/// Once supplied, each transaction must resolve to the pinned canonical view.
pub(crate) fn canonical_scope(
    store: &CkbadgerStore,
    hashes: impl IntoIterator<Item = Vec<u8>>,
) -> anyhow::Result<Vec<AssetTransaction>> {
    let mut seen = HashSet::new();
    let mut scope = Vec::new();
    let mut batch = Vec::with_capacity(TRANSACTION_BATCH_SIZE);
    for hash in hashes {
        if !seen.insert(hash.clone()) {
            continue;
        }
        batch.push(hash);
        if batch.len() == TRANSACTION_BATCH_SIZE {
            scope.extend(canonical_transactions(store, &batch)?);
            batch.clear();
        }
    }
    scope.extend(canonical_transactions(store, &batch)?);
    sort_scope(&mut scope);
    Ok(scope)
}

/// Page-size bounds every asset feed applies to the requested `limit`.
pub(crate) fn feed_limit(limit: i64) -> i64 {
    limit.clamp(1, 100)
}

/// The `nextCursor` of a page whose last row is `row`.
pub(crate) fn feed_cursor(row: &AssetTransaction) -> String {
    format!("{}:{}", row.block_number, row.tx_index)
}

fn page(
    scope: Vec<AssetTransaction>,
    limit: i64,
    cursor: Option<&str>,
) -> Result<(Vec<AssetTransaction>, i64, i64, Option<String>), ApiRouteError> {
    let limit = feed_limit(limit);
    let cursor = parse_optional_block_tx_cursor(cursor, "asset transaction cursor")?;
    let total = i64::try_from(scope.len())
        .map_err(|_| ApiError::internal("asset transaction count overflow"))?;
    if let Some(location) = cursor {
        if !scope
            .iter()
            .any(|row| (row.block_number, row.tx_index) == location)
        {
            return Err(ApiError::bad_request(
                "asset transaction cursor does not belong to this scope",
            ));
        }
    }
    let mut rows: Vec<_> = scope
        .into_iter()
        .filter(|row| cursor.is_none_or(|key| (row.block_number, row.tx_index) < key))
        .take(limit as usize + 1)
        .collect();
    let more = rows.len() > limit as usize;
    rows.truncate(limit as usize);
    let next = if more {
        rows.last().map(feed_cursor)
    } else {
        None
    };
    Ok((rows, total, limit, next))
}

pub(crate) fn activities(
    state: &AppState,
    scope: Vec<AssetTransaction>,
    limit: i64,
    cursor: Option<&str>,
) -> ApiResult<CursorPaginatedResponse<GlobalActivityResponse>> {
    let (rows, total, limit, next) = page(scope, limit, cursor)?;
    ok(CursorPaginatedResponse::new(
        project_activities(state, rows)?,
        total,
        limit,
        next,
    ))
}

/// The one activity projection of already-paged canonical rows, shared by
/// in-memory scopes ([`activities`]) and feeds paged at their source index.
pub(crate) fn project_activities(
    state: &AppState,
    rows: Vec<AssetTransaction>,
) -> Result<Vec<GlobalActivityResponse>, ApiRouteError> {
    let actions =
        canonical_actions(state, &rows).map_err(|e| ApiError::internal(format!("{e:#}")))?;
    project_actions(state, &actions)
}

/// [`project_activities`] of a page whose `TxActions` the caller has read.
pub(crate) fn project_actions(
    state: &AppState,
    actions: &[TxActions],
) -> Result<Vec<GlobalActivityResponse>, ApiRouteError> {
    let mut scripts = HashMap::new();
    let mut addresses = HashMap::new();
    actions
        .iter()
        .map(|actions| {
            build_global_activity_response(
                &state.store,
                &state.append_only_store,
                &state.ckb_network,
                actions,
                &mut scripts,
                &mut addresses,
            )
            .map_err(|e| ApiError::internal(e.to_string()))
        })
        .collect()
}

/// The canonical `TxActions` of a page of resolved rows, in order: one
/// `block_headers` read for the page's blocks and one `activities` read
/// ([`CkbadgerStore::tx_actions_of`]). An asset feed lists no cellbase (CKB
/// cellbase outputs carry no type script); a cellbase row is an error.
pub(crate) fn canonical_actions(
    state: &AppState,
    rows: &[AssetTransaction],
) -> anyhow::Result<Vec<TxActions>> {
    let mut blocks: Vec<i64> = rows.iter().map(|row| row.block_number).collect();
    blocks.sort_unstable();
    blocks.dedup();
    let headers = state.store.get_block_headers_batch(&blocks)?;
    let resolved = rows
        .iter()
        .map(|row| {
            let header = headers
                .get(&row.block_number)
                .with_context(|| format!("asset feed missing header block={}", row.block_number))?;
            Ok(ResolvedTx {
                block_number: row.block_number,
                tx_index: row.tx_index,
                entry: &row.entry,
                block_hash: &header.hash,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    state
        .store
        .tx_actions_of(&resolved)?
        .into_iter()
        .zip(rows)
        .map(|(actions, row)| {
            actions.with_context(|| {
                format!(
                    "asset feed row block={} index={} tx=0x{} is a cellbase",
                    row.block_number,
                    row.tx_index,
                    hex::encode(row.entry.tx_hash)
                )
            })
        })
        .collect()
}

pub(crate) fn transactions(
    state: &AppState,
    scope: Vec<AssetTransaction>,
    limit: i64,
    cursor: Option<&str>,
) -> ApiResult<CursorPaginatedResponse<TransactionResponse>> {
    let (rows, total, limit, next) = page(scope, limit, cursor)?;
    ok(CursorPaginatedResponse::new(
        project_transactions(state, rows)?,
        total,
        limit,
        next,
    ))
}

/// The one transaction-row projection of already-paged canonical rows.
pub(crate) fn project_transactions(
    state: &AppState,
    rows: Vec<AssetTransaction>,
) -> Result<Vec<TransactionResponse>, ApiRouteError> {
    let actions =
        canonical_actions(state, &rows).map_err(|e| ApiError::internal(format!("{e:#}")))?;
    project_transactions_of(state, &rows, &actions)
}

/// [`project_transactions`] of a page whose `TxActions` the caller has read
/// ([`canonical_actions`]); each row's block hash is its actions'.
pub(crate) fn project_transactions_of(
    state: &AppState,
    rows: &[AssetTransaction],
    actions: &[TxActions],
) -> Result<Vec<TransactionResponse>, ApiRouteError> {
    rows.iter()
        .zip(actions)
        .map(|(row, actions)| {
            transaction_response(
                state,
                ResolvedTx {
                    block_number: row.block_number,
                    tx_index: row.tx_index,
                    entry: &row.entry,
                    block_hash: &actions.block_hash,
                },
                Some(actions),
                None,
                // CKB cellbase outputs carry no type script, so no asset feed
                // lists a cellbase ([`canonical_actions`] refuses one).
                None,
            )
        })
        .collect()
}
