use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use ckbadger_store::{
    types::{ActivityEntry, AssetAction, AssetChange},
    CkbadgerStore,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::response::{ok, ApiError, ApiResult, CursorPaginatedResponse};
use crate::utils::{address::address_to_lock_script_hash, ensure_derived_ready};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/addresses/{addr}/activities", get(get_address_activities))
}

#[derive(Debug, Deserialize)]
pub struct ActivityParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
    filter: Option<String>,
}

fn default_limit() -> i64 {
    20
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityResponse {
    pub tx_hash: String,
    pub block_number: i64,
    pub tx_index: i32,
    pub timestamp: String,
    pub ckb_delta: String,
    pub occupied_delta: String,
    pub is_cellbase: bool,
    pub asset_changes: Vec<AssetChangeResponse>,
    pub peers: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum AssetChangeResponse {
    #[serde(rename = "token")]
    Token {
        type_script_hash: String,
        delta: String,
        symbol: Option<String>,
        decimals: Option<u8>,
    },
    #[serde(rename = "dob")]
    Dob {
        dob_id: String,
        standard: String,
        action: String,
    },
    #[serde(rename = "nft")]
    Nft {
        nft_id: String,
        standard: String,
        action: String,
    },
    #[serde(rename = "daoDeposit")]
    DaoDeposit { capacity: String },
    #[serde(rename = "daoWithdrawRequest")]
    DaoWithdrawRequest {
        capacity: String,
        deposit_block: i64,
    },
    #[serde(rename = "daoWithdrawComplete")]
    DaoWithdrawComplete {
        capacity: String,
        compensation: String,
    },
}

fn action_to_string(action: &AssetAction) -> String {
    match action {
        AssetAction::Mint => "mint".to_string(),
        AssetAction::Transfer => "transfer".to_string(),
        AssetAction::Burn => "burn".to_string(),
        AssetAction::Recycle => "recycle".to_string(),
        AssetAction::Renew => "renew".to_string(),
        AssetAction::Update => "update".to_string(),
    }
}

fn convert_asset_change(change: &AssetChange) -> AssetChangeResponse {
    match change {
        AssetChange::Token {
            type_script_hash,
            delta,
            symbol,
            decimals,
        } => AssetChangeResponse::Token {
            type_script_hash: format!("0x{}", hex::encode(type_script_hash)),
            delta: delta.to_string(),
            symbol: symbol.clone(),
            decimals: *decimals,
        },
        AssetChange::Dob {
            dob_id,
            standard,
            action,
        } => AssetChangeResponse::Dob {
            dob_id: format!("0x{}", hex::encode(dob_id)),
            standard: standard.clone(),
            action: action_to_string(action),
        },
        AssetChange::Nft {
            nft_id,
            standard,
            action,
        } => AssetChangeResponse::Nft {
            nft_id: format!("0x{}", hex::encode(nft_id)),
            standard: standard.clone(),
            action: action_to_string(action),
        },
        AssetChange::DaoDeposit { capacity } => AssetChangeResponse::DaoDeposit {
            capacity: capacity.to_string(),
        },
        AssetChange::DaoWithdrawRequest {
            capacity,
            deposit_block,
        } => AssetChangeResponse::DaoWithdrawRequest {
            capacity: capacity.to_string(),
            deposit_block: *deposit_block,
        },
        AssetChange::DaoWithdrawComplete {
            capacity,
            compensation,
        } => AssetChangeResponse::DaoWithdrawComplete {
            capacity: capacity.to_string(),
            compensation: compensation.to_string(),
        },
    }
}

const ACTIVITY_SCAN_CHUNK_SIZE: usize = 128;

fn is_canonical_activity_entry(
    store: &CkbadgerStore,
    block_num: i64,
    tx_idx: i32,
    entry: &ActivityEntry,
) -> anyhow::Result<bool> {
    if entry.block_number != block_num || entry.tx_index != tx_idx {
        return Ok(false);
    }
    let Some((canonical_block, canonical_tx_idx)) = store.get_tx_location(&entry.tx_hash)? else {
        return Ok(false);
    };
    if canonical_block != block_num || canonical_tx_idx != tx_idx {
        return Ok(false);
    }
    Ok(store
        .get_tx_index(canonical_block, canonical_tx_idx)?
        .is_some())
}

fn list_canonical_activities_page(
    store: &CkbadgerStore,
    append_only_store: &CkbadgerStore,
    lock_hash: &[u8],
    limit: usize,
    cursor: Option<(i64, i32)>,
    filter: Option<&str>,
) -> anyhow::Result<Vec<(i64, i32, ActivityEntry)>> {
    if limit == 0 {
        return Ok(Vec::new());
    }

    let scan_limit = ACTIVITY_SCAN_CHUNK_SIZE.max(limit);
    let mut out = Vec::with_capacity(limit);
    let mut scan_cursor = cursor;

    loop {
        let rows = append_only_store.list_activities(lock_hash, scan_limit, scan_cursor, filter)?;
        if rows.is_empty() {
            break;
        }
        let rows_len = rows.len();
        let mut last_seen = None;
        for (block_num, tx_idx, entry) in rows {
            last_seen = Some((block_num, tx_idx));
            if is_canonical_activity_entry(store, block_num, tx_idx, &entry)? {
                out.push((block_num, tx_idx, entry));
                if out.len() >= limit {
                    return Ok(out);
                }
            }
        }
        if rows_len < scan_limit {
            break;
        }
        let Some(last_seen_cursor) = last_seen else {
            break;
        };
        scan_cursor = Some(last_seen_cursor);
    }

    Ok(out)
}

async fn get_address_activities(
    State(state): State<Arc<AppState>>,
    Path(addr): Path<String>,
    Query(params): Query<ActivityParams>,
) -> ApiResult<CursorPaginatedResponse<ActivityResponse>> {
    ensure_derived_ready(&state)?;
    let lock_hash = if addr.starts_with("ckb1") || addr.starts_with("ckt1") {
        address_to_lock_script_hash(&addr)
            .map_err(|e| ApiError::bad_request(format!("Invalid address: {}", e)))?
    } else {
        hex::decode(addr.strip_prefix("0x").unwrap_or(&addr))
            .map_err(|_| ApiError::bad_request("Invalid lock script hash"))?
    };

    let limit = params.limit.clamp(1, 100) as usize;

    // Parse cursor: "block_num:tx_idx"
    let cursor = params.cursor.as_ref().and_then(|c| {
        let parts: Vec<&str> = c.split(':').collect();
        if parts.len() == 2 {
            let block_num = parts[0].parse::<i64>().ok()?;
            let tx_idx = parts[1].parse::<i32>().ok()?;
            Some((block_num, tx_idx))
        } else {
            None
        }
    });

    let results = list_canonical_activities_page(
        state.store.as_ref(),
        state.append_only_store.as_ref(),
        &lock_hash,
        limit + 1,
        cursor,
        params.filter.as_deref(),
    )
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let has_more = results.len() > limit;
    let page: Vec<_> = results.into_iter().take(limit).collect();

    let next_cursor = if has_more {
        page.last()
            .map(|(block_num, tx_idx, _)| format!("{}:{}", block_num, tx_idx))
    } else {
        None
    };

    let activities: Vec<ActivityResponse> = page
        .into_iter()
        .map(|(_, _, entry)| ActivityResponse {
            tx_hash: format!("0x{}", hex::encode(&entry.tx_hash)),
            block_number: entry.block_number,
            tx_index: entry.tx_index,
            timestamp: entry.timestamp.to_string(),
            ckb_delta: entry.ckb_delta.to_string(),
            occupied_delta: entry.occupied_delta.to_string(),
            is_cellbase: entry.is_cellbase,
            asset_changes: entry
                .asset_changes
                .iter()
                .map(convert_asset_change)
                .collect(),
            peers: entry
                .peers
                .iter()
                .map(|h| format!("0x{}", hex::encode(h)))
                .collect(),
        })
        .collect();

    ok(CursorPaginatedResponse::without_total(
        activities,
        limit as i64,
        next_cursor,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_store::batch::StoreBatch;
    use ckbadger_store::types::TxIndexEntry;

    fn make_activity(tx_hash: &[u8], block_number: i64, tx_index: i32) -> ActivityEntry {
        ActivityEntry {
            tx_hash: tx_hash.to_vec(),
            block_number,
            tx_index,
            timestamp: 1_700_000_000 + block_number,
            ckb_delta: 0,
            occupied_delta: 0,
            is_cellbase: false,
            asset_changes: vec![],
            peers: vec![],
        }
    }

    #[test]
    fn test_list_canonical_activities_page_filters_orphaned_entries() {
        let root = tempfile::tempdir().unwrap();
        let domain_path = root.path().join("domain");
        let append_path = root.path().join("append");
        let domain = CkbadgerStore::open_domain(&domain_path).unwrap();
        let append = CkbadgerStore::open_append_only(&append_path).unwrap();

        let lock_hash = [0xAA; 32];
        let stale_tx = vec![0x30; 32];
        let canonical_tx_new = vec![0x20; 32];
        let canonical_tx_old = vec![0x10; 32];

        let mut append_batch = StoreBatch::new(&append);
        append_batch.put_activity(&lock_hash, 30, 0, &make_activity(&stale_tx, 30, 0));
        append_batch.put_activity(&lock_hash, 20, 0, &make_activity(&canonical_tx_new, 20, 0));
        append_batch.put_activity(&lock_hash, 10, 0, &make_activity(&canonical_tx_old, 10, 0));
        append_batch.commit().unwrap();

        let tx_index = TxIndexEntry {
            is_cellbase: false,
            timestamp: 1_700_000_000_000,
            inputs_count: 1,
            outputs_count: 1,
            fee: 0,
            tx_size: 1,
            cycles: None,
        };

        let mut domain_batch = StoreBatch::new(&domain);
        // Simulate stale/orphan-like mapping without canonical tx_index entry.
        domain_batch.put_tx_hash_map(&stale_tx, 30, 0);
        domain_batch.put_tx_hash_map(&canonical_tx_new, 20, 0);
        domain_batch.put_tx_hash_map(&canonical_tx_old, 10, 0);
        domain_batch.put_tx_index(20, 0, &tx_index);
        domain_batch.put_tx_index(10, 0, &tx_index);
        domain_batch.commit().unwrap();

        let rows =
            list_canonical_activities_page(&domain, &append, &lock_hash, 3, None, None).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, 20);
        assert_eq!(rows[1].0, 10);
        assert_eq!(rows[0].2.tx_hash, canonical_tx_new);
        assert_eq!(rows[1].2.tx_hash, canonical_tx_old);
    }
}
