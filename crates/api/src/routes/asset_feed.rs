//! One canonical projection for every asset's Activities / Transactions tabs.
use std::collections::{HashMap, HashSet};

use anyhow::{ensure, Context};
use ckbadger_store::{CkbadgerStore, TxActions, TxIndexEntry};

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
    let mut scripts = HashMap::new();
    let mut addresses = HashMap::new();
    rows.into_iter()
        .map(|row| {
            let result = (|| -> anyhow::Result<_> {
                let actions = canonical_actions(state, &row)?;
                build_global_activity_response(
                    &state.store,
                    &state.append_only_store,
                    &state.ckb_network,
                    &actions,
                    &mut scripts,
                    &mut addresses,
                )
            })();
            result.map_err(|e| ApiError::internal(e.to_string()))
        })
        .collect()
}

/// The canonical `TxActions` of one resolved row, checked against the block
/// header it claims.
pub(crate) fn canonical_actions(
    state: &AppState,
    row: &AssetTransaction,
) -> anyhow::Result<TxActions> {
    let actions = state
        .store
        .get_tx_actions(row.block_number, row.tx_index, &row.entry.tx_hash)?
        .with_context(|| {
            format!(
                "asset feed missing actions block={} index={} tx=0x{}",
                row.block_number,
                row.tx_index,
                hex::encode(row.entry.tx_hash)
            )
        })?;
    let header = state
        .store
        .get_block_header(row.block_number)?
        .with_context(|| format!("asset feed missing header block={}", row.block_number))?;
    ensure!(
        actions.block_hash == header.hash
            && actions.tx_hash == row.entry.tx_hash
            && actions.block_number == row.block_number
            && actions.tx_index == row.tx_index,
        "asset feed action identity mismatch block={} index={} tx=0x{}",
        row.block_number,
        row.tx_index,
        hex::encode(row.entry.tx_hash)
    );
    Ok(actions)
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
    rows.into_iter()
        .map(|row| {
            let header = state
                .store
                .get_block_header(row.block_number)
                .map_err(|e| ApiError::internal(e.to_string()))?
                .ok_or_else(|| {
                    ApiError::internal(format!(
                        "asset feed missing header block={}",
                        row.block_number
                    ))
                })?;
            transaction_response(
                state,
                row.block_number,
                row.tx_index,
                &row.entry,
                &header.hash,
                None,
                // CKB cellbase outputs carry no type script, so no asset feed
                // lists a cellbase; one that did would fail on its `pays`.
                None,
            )
        })
        .collect()
}
