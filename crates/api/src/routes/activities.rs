use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use ckbadger_store::CkbadgerStore;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::response::{ok, ApiError, ApiResult, CursorPaginatedResponse};
use crate::tx_block_map::get_block_number_for_tx;
use crate::utils::{address_to_lock_script_hash, is_ckb_address};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/activities", get(list_activities))
        .route("/activities/address/{addr}", get(get_address_activities))
        .route(
            "/activities/transaction/{hash}",
            get(get_transaction_activities),
        )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListActivitiesParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
    activity_type: Option<String>,
    activity_category: Option<String>,
}

fn default_limit() -> i64 {
    20
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityResponse {
    pub activity_id: String,
    pub activity_type: String,
    pub activity_category: String,
    pub block_number: i64,
    pub tx_hash: String,
    pub tx_index: i32,
    pub activity_index: i16,
    pub from_address: Option<String>,
    pub to_address: Option<String>,
    pub from_lock_hash: Option<String>,
    pub to_lock_hash: Option<String>,
    pub amount: String,
    pub asset_id: Option<String>,
    pub metadata: serde_json::Value,
    pub timestamp: String,
}

fn encode_activity_cursor(block_number: i64, tx_index: i32, activity_index: i16) -> String {
    format!("{}:{}:{}", block_number, tx_index, activity_index)
}

fn decode_activity_cursor(cursor: &str) -> Option<(i64, i32, i16)> {
    let parts: Vec<&str> = cursor.split(':').collect();
    if parts.len() != 3 {
        return None;
    }
    let block_number = parts[0].parse().ok()?;
    let tx_index = parts[1].parse().ok()?;
    let activity_index = parts[2].parse().ok()?;
    Some((block_number, tx_index, activity_index))
}

/// Build an activity_id from block_number and activity_index.
fn make_activity_id(block_num: i64, idx: i32) -> String {
    let mut id = Vec::with_capacity(12);
    id.extend_from_slice(&block_num.to_be_bytes());
    id.extend_from_slice(&idx.to_be_bytes());
    format!("0x{}", hex::encode(&id))
}

/// Convert an ActivityEntry (from store) + its block_num and idx into an API response.
fn entry_to_response(
    block_num: i64,
    idx: i32,
    entry: &ckbadger_store::ActivityEntry,
) -> ActivityResponse {
    let timestamp_dt = chrono::DateTime::from_timestamp_millis(entry.timestamp).unwrap_or_default();

    ActivityResponse {
        activity_id: make_activity_id(block_num, idx),
        activity_type: entry.activity_type.clone(),
        activity_category: entry.category.clone(),
        block_number: block_num,
        tx_hash: format!("0x{}", hex::encode(&entry.tx_hash)),
        tx_index: entry.tx_idx,
        activity_index: idx as i16,
        from_address: None,
        to_address: None,
        from_lock_hash: entry
            .from_lock
            .as_ref()
            .map(|h| format!("0x{}", hex::encode(h))),
        to_lock_hash: entry
            .to_lock
            .as_ref()
            .map(|h| format!("0x{}", hex::encode(h))),
        amount: entry
            .amount
            .map(|a| a.to_string())
            .unwrap_or_else(|| "0".to_string()),
        asset_id: entry
            .asset_id
            .as_ref()
            .map(|h| format!("0x{}", hex::encode(h))),
        metadata: entry.metadata.clone().unwrap_or(serde_json::Value::Null),
        timestamp: timestamp_dt.to_rfc3339(),
    }
}

const VALID_CATEGORIES: [&str; 8] = [
    "ckb", "cellbase", "token", "dob", "nft", "dao", "script", "rgbpp",
];

const VALID_TYPES: [&str; 18] = [
    "CKB_TRANSFER",
    "CELLBASE_REWARD",
    "TOKEN_MINT",
    "TOKEN_TRANSFER",
    "TOKEN_BURN",
    "DOB_MINT",
    "DOB_TRANSFER",
    "DOB_BURN",
    "NFT_MINT",
    "NFT_TRANSFER",
    "DAO_DEPOSIT",
    "DAO_WITHDRAW_REQUEST",
    "DAO_WITHDRAW_COMPLETE",
    "SCRIPT_DEPLOY",
    "RGBPP_TRANSFER",
    "RGBPP_LEAP_IN",
    "RGBPP_LEAP_OUT",
    "RGBPP_ISSUANCE",
];

/// Collect recent activities from the store by scanning blocks in reverse order.
/// Applies optional type/category filters and cursor-based pagination.
fn collect_activities_from_store(
    store: &CkbadgerStore,
    limit: usize,
    cursor: Option<(i64, i32, i16)>,
    activity_type: Option<&str>,
    activity_category: Option<&str>,
) -> Result<Vec<(i64, i32, ckbadger_store::ActivityEntry)>, String> {
    // Start scanning from cursor block or from the tip
    let start_block = cursor.map(|(b, _, _)| b).unwrap_or_else(|| {
        store
            .get_sync_status()
            .map(|s| s.tip_block_number)
            .unwrap_or(0)
    });

    let mut results = Vec::new();
    // We need limit+1 to know if there are more results
    let target = limit;

    // Scan blocks in reverse order
    let blocks = store
        .list_blocks_desc(Some(start_block), 10000)
        .map_err(|e| e.to_string())?;

    for (block_num, _header) in &blocks {
        let activities = store
            .list_block_activities(*block_num)
            .map_err(|e| e.to_string())?;

        for (idx, entry) in activities.iter().rev() {
            // Apply cursor filter
            if let Some((cb, ct, ci)) = cursor {
                if (*block_num, entry.tx_idx, *idx as i16) >= (cb, ct, ci) {
                    continue;
                }
            }

            // Apply type filter
            if let Some(typ) = activity_type {
                if entry.activity_type != typ {
                    continue;
                }
            }

            // Apply category filter
            if let Some(cat) = activity_category {
                if entry.category != cat {
                    continue;
                }
            }

            results.push((*block_num, *idx, entry.clone()));
            if results.len() > target {
                return Ok(results);
            }
        }
    }

    Ok(results)
}

async fn list_activities(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListActivitiesParams>,
) -> ApiResult<CursorPaginatedResponse<ActivityResponse>> {
    let limit = params.limit.clamp(1, 100);

    if let Some(ref cat) = params.activity_category {
        if !VALID_CATEGORIES.contains(&cat.as_str()) {
            return Err(ApiError::bad_request(format!(
                "Invalid activity_category '{}'. Valid: {}",
                cat,
                VALID_CATEGORIES.join(", ")
            )));
        }
    }

    if let Some(ref typ) = params.activity_type {
        if !VALID_TYPES.contains(&typ.as_str()) {
            return Err(ApiError::bad_request(format!(
                "Invalid activity_type '{}'. Valid: {}",
                typ,
                VALID_TYPES.join(", ")
            )));
        }
    }

    let cursor = params
        .cursor
        .as_ref()
        .and_then(|c| decode_activity_cursor(c));

    let rows = collect_activities_from_store(
        &state.store,
        (limit + 1) as usize,
        cursor,
        params.activity_type.as_deref(),
        params.activity_category.as_deref(),
    )
    .map_err(ApiError::internal)?;

    let has_more = rows.len() as i64 > limit;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

    let next_cursor = if has_more {
        rows.last()
            .map(|(bn, idx, entry)| encode_activity_cursor(*bn, entry.tx_idx, *idx as i16))
    } else {
        None
    };

    let data: Vec<ActivityResponse> = rows
        .iter()
        .map(|(bn, idx, entry)| entry_to_response(*bn, *idx, entry))
        .collect();

    ok(CursorPaginatedResponse::without_total(
        data,
        limit,
        next_cursor,
    ))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressActivitiesParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
    activity_type: Option<String>,
    activity_category: Option<String>,
    direction: Option<String>,
}

async fn get_address_activities(
    State(state): State<Arc<AppState>>,
    Path(addr): Path<String>,
    Query(params): Query<AddressActivitiesParams>,
) -> ApiResult<CursorPaginatedResponse<ActivityResponse>> {
    let lock_hash = if is_ckb_address(&addr) {
        address_to_lock_script_hash(&addr)
            .map_err(|e| ApiError::bad_request(format!("Invalid CKB address: {}", e)))?
    } else {
        hex::decode(addr.strip_prefix("0x").unwrap_or(&addr))
            .map_err(|_| ApiError::bad_request("Invalid address/lock script hash"))?
    };

    let limit = params.limit.clamp(1, 100);

    if let Some(ref cat) = params.activity_category {
        if !VALID_CATEGORIES.contains(&cat.as_str()) {
            return Err(ApiError::bad_request(format!(
                "Invalid activity_category '{}'. Valid: {}",
                cat,
                VALID_CATEGORIES.join(", ")
            )));
        }
    }

    if let Some(ref typ) = params.activity_type {
        if !VALID_TYPES.contains(&typ.as_str()) {
            return Err(ApiError::bad_request(format!(
                "Invalid activity_type '{}'. Valid: {}",
                typ,
                VALID_TYPES.join(", ")
            )));
        }
    }

    if let Some(ref dir) = params.direction {
        if dir != "in" && dir != "out" && dir != "all" {
            return Err(ApiError::bad_request(
                "Invalid direction. Must be 'in', 'out', or 'all'",
            ));
        }
    }

    let direction = params.direction.as_deref().unwrap_or("all");

    // Fetch more than needed so we can filter and still have enough
    let fetch_limit = (limit as usize + 1) * 4;
    let all_activities = state
        .store
        .list_activities_by_addr(&lock_hash, fetch_limit)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let cursor = params
        .cursor
        .as_ref()
        .and_then(|c| decode_activity_cursor(c));

    // Filter, apply cursor, direction, type, category
    let mut filtered: Vec<(i64, i32, ckbadger_store::ActivityEntry)> = Vec::new();

    for entry in &all_activities {
        // The store returns entries but doesn't include block_num directly.
        // We get the block number from the tx_location.
        let block_num = state
            .store
            .get_tx_location(&entry.tx_hash)
            .ok()
            .flatten()
            .map(|(bn, _)| bn)
            .unwrap_or(0);

        // Compute a synthetic activity_index: use tx_idx as the ordering key
        // since each entry has a unique tx_idx in the block context.
        let idx = entry.tx_idx;

        // Apply cursor
        if let Some((cb, ct, ci)) = cursor {
            if (block_num, entry.tx_idx, idx as i16) >= (cb, ct, ci) {
                continue;
            }
        }

        // Apply direction filter
        match direction {
            "in" => {
                if entry.to_lock.as_deref() != Some(&lock_hash[..]) {
                    continue;
                }
            }
            "out" => {
                if entry.from_lock.as_deref() != Some(&lock_hash[..]) {
                    continue;
                }
            }
            _ => {
                // "all" - either from or to matches (already filtered by store)
            }
        }

        // Apply type filter
        if let Some(ref typ) = params.activity_type {
            if entry.activity_type != *typ {
                continue;
            }
        }

        // Apply category filter
        if let Some(ref cat) = params.activity_category {
            if entry.category != *cat {
                continue;
            }
        }

        filtered.push((block_num, idx, entry.clone()));
        if filtered.len() > limit as usize {
            break;
        }
    }

    let has_more = filtered.len() as i64 > limit;
    let filtered: Vec<_> = filtered.into_iter().take(limit as usize).collect();

    let next_cursor = if has_more {
        filtered
            .last()
            .map(|(bn, idx, entry)| encode_activity_cursor(*bn, entry.tx_idx, *idx as i16))
    } else {
        None
    };

    let data: Vec<ActivityResponse> = filtered
        .iter()
        .map(|(bn, idx, entry)| entry_to_response(*bn, *idx, entry))
        .collect();

    ok(CursorPaginatedResponse::without_total(
        data,
        limit,
        next_cursor,
    ))
}

/// Fetch activities for a transaction, used by both the /activities/transaction endpoint
/// and from transactions.rs.
pub fn fetch_transaction_activities(
    store: &CkbadgerStore,
    tx_hash: &[u8],
) -> Result<Vec<ActivityResponse>, String> {
    let block_number = get_block_number_for_tx(store, tx_hash).ok().flatten();

    if let Some(bn) = block_number {
        let activities = store.list_block_activities(bn).map_err(|e| e.to_string())?;

        let data: Vec<ActivityResponse> = activities
            .iter()
            .filter(|(_, entry)| entry.tx_hash == tx_hash)
            .map(|(idx, entry)| entry_to_response(bn, *idx, entry))
            .collect();

        Ok(data)
    } else {
        // Transaction not found or not indexed yet
        Ok(Vec::new())
    }
}

async fn get_transaction_activities(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> ApiResult<Vec<ActivityResponse>> {
    let tx_hash = hex::decode(hash.strip_prefix("0x").unwrap_or(&hash))
        .map_err(|_| ApiError::bad_request("Invalid transaction hash"))?;

    if tx_hash.len() != 32 {
        return Err(ApiError::bad_request(
            "Transaction hash must be 32 bytes (64 hex chars)",
        ));
    }

    let data = fetch_transaction_activities(&state.store, &tx_hash).map_err(ApiError::internal)?;

    ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_activity_cursor() {
        let cursor = encode_activity_cursor(12345, 5, 3);
        assert_eq!(cursor, "12345:5:3");

        let decoded = decode_activity_cursor(&cursor);
        assert_eq!(decoded, Some((12345, 5, 3)));
    }

    #[test]
    fn test_decode_activity_cursor_invalid() {
        assert_eq!(decode_activity_cursor("invalid"), None);
        assert_eq!(decode_activity_cursor("12345:5"), None);
        assert_eq!(decode_activity_cursor("12345:5:3:extra"), None);
        assert_eq!(decode_activity_cursor("abc:def:ghi"), None);
    }

    #[test]
    fn test_valid_categories() {
        assert!(VALID_CATEGORIES.contains(&"ckb"));
        assert!(VALID_CATEGORIES.contains(&"token"));
        assert!(VALID_CATEGORIES.contains(&"rgbpp"));
        assert!(!VALID_CATEGORIES.contains(&"invalid"));
    }

    #[test]
    fn test_valid_types() {
        assert!(VALID_TYPES.contains(&"CKB_TRANSFER"));
        assert!(VALID_TYPES.contains(&"TOKEN_MINT"));
        assert!(VALID_TYPES.contains(&"RGBPP_LEAP_IN"));
        assert!(!VALID_TYPES.contains(&"INVALID_TYPE"));
    }
}
