use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::response::{ok, ApiError, ApiResult, CursorPaginatedResponse};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/tokens", get(list_tokens))
        .route("/tokens/{type_hash}", get(get_token))
        .route("/tokens/{type_hash}/holders", get(get_token_holders))
        .route("/tokens/{type_hash}/transfers", get(get_token_transfers))
}

#[derive(Debug, Deserialize)]
pub struct ListParams {
    #[serde(default = "default_limit")]
    limit: i64,
    standard: Option<String>,
    cursor: Option<String>,
    search: Option<String>,
}

fn default_limit() -> i64 {
    20
}

#[derive(Debug, Deserialize)]
pub struct HolderParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TransferParams {
    #[serde(default = "default_limit")]
    limit: i64,
    #[allow(dead_code)]
    cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenResponse {
    pub type_script_hash: String,
    pub type_code_hash: String,
    pub type_hash_type: String,
    pub type_args: String,
    pub standard: String,
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub decimals: i16,
    pub description: Option<String>,
    pub icon_url: Option<String>,
    pub published: bool,
    pub famous: bool,
    pub tags: Option<Vec<String>>,
    pub udt_type: Option<String>,
    pub manager: Option<String>,
    pub email: Option<String>,
    pub operator_website: Option<String>,
    pub total_supply: String,
    pub holders_count: i32,
    pub transfers_count: i64,
    pub transfers_24h: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenHolderResponse {
    pub lock_script_hash: String,
    pub address: Option<String>,
    pub balance: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenTransferResponse {
    pub tx_hash: String,
    pub block_number: i64,
    pub from_lock_hash: Option<String>,
    pub from_address: Option<String>,
    pub to_lock_hash: String,
    pub to_address: Option<String>,
    pub amount: String,
    pub is_mint: bool,
    pub is_burn: bool,
    pub timestamp: String,
}

/// Convert a store TokenInfo + key into an API TokenResponse.
fn token_info_to_response(
    type_hash: &[u8],
    info: &ckbadger_store::TokenInfo,
    transfers_count: i64,
    transfers_24h: i64,
) -> TokenResponse {
    TokenResponse {
        type_script_hash: format!("0x{}", hex::encode(type_hash)),
        type_code_hash: format!("0x{}", hex::encode(&info.type_code_hash)),
        type_hash_type: hash_type_to_string(info.hash_type as i16),
        type_args: format!("0x{}", hex::encode(&info.type_args)),
        standard: info.standard.clone(),
        name: info.name.clone(),
        symbol: info.symbol.clone(),
        decimals: info.decimals.unwrap_or(0) as i16,
        description: info.description.clone(),
        icon_url: info.icon_url.clone(),
        published: false,
        famous: false,
        tags: None,
        udt_type: None,
        manager: None,
        email: None,
        operator_website: None,
        total_supply: info
            .total_supply
            .map(|s| s.to_string())
            .unwrap_or_else(|| "0".to_string()),
        holders_count: info.holders_count as i32,
        transfers_count,
        transfers_24h,
    }
}

async fn list_tokens(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<TokenResponse>> {
    let limit = params.limit.clamp(1, 100) as usize;

    let all_tokens = state
        .store
        .list_tokens()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Apply filters
    let search_hash = params.search.as_ref().and_then(|s| {
        let s = s.strip_prefix("0x").unwrap_or(s);
        hex::decode(s).ok()
    });
    let search_lower = params.search.as_ref().map(|s| s.to_lowercase());

    let mut filtered: Vec<_> = all_tokens
        .into_iter()
        .filter(|(type_hash, info)| {
            // Filter by standard
            if let Some(ref standard) = params.standard {
                if &info.standard != standard {
                    return false;
                }
            }
            // Filter by search (hash match or name/symbol match)
            if let Some(ref hash) = search_hash {
                if type_hash != hash {
                    return false;
                }
            } else if let Some(ref pattern) = search_lower {
                let name_match = info
                    .name
                    .as_ref()
                    .map(|n| n.to_lowercase().contains(pattern))
                    .unwrap_or(false);
                let symbol_match = info
                    .symbol
                    .as_ref()
                    .map(|s| s.to_lowercase().contains(pattern))
                    .unwrap_or(false);
                if !name_match && !symbol_match {
                    return false;
                }
            }
            true
        })
        .collect();

    // Sort by holders_count DESC (matching the original ORDER BY transfers_24h DESC, holders_count DESC)
    filtered.sort_by(|a, b| b.1.holders_count.cmp(&a.1.holders_count));

    // Apply cursor-based pagination
    let cursor_holders = params.cursor.as_ref().and_then(|c| {
        let parts: Vec<&str> = c.split(':').collect();
        if parts.len() >= 2 {
            parts[1].parse::<i64>().ok()
        } else {
            c.parse::<i64>().ok()
        }
    });

    let start_idx = if let Some(ch) = cursor_holders {
        filtered
            .iter()
            .position(|(_, info)| info.holders_count < ch)
            .unwrap_or(filtered.len())
    } else {
        0
    };

    let page: Vec<_> = filtered.iter().skip(start_idx).take(limit + 1).collect();

    let has_more = page.len() > limit;
    let page: Vec<_> = page.into_iter().take(limit).collect();

    let next_cursor = if has_more {
        page.last()
            .map(|(_, info)| format!("0:{}:0", info.holders_count))
    } else {
        None
    };

    let now_ms = chrono::Utc::now().timestamp_millis();
    let tokens: Vec<TokenResponse> = page
        .into_iter()
        .map(|(type_hash, info)| {
            let transfers_count = state
                .store
                .get_token_transfers_count(type_hash)
                .unwrap_or(0);
            let transfers_24h = state
                .store
                .get_token_24h_transfers(type_hash, now_ms)
                .unwrap_or(0);
            token_info_to_response(type_hash, info, transfers_count, transfers_24h)
        })
        .collect();

    ok(CursorPaginatedResponse::without_total(
        tokens,
        limit as i64,
        next_cursor,
    ))
}

async fn get_token(
    State(state): State<Arc<AppState>>,
    Path(type_hash): Path<String>,
) -> ApiResult<TokenResponse> {
    let hash = hex::decode(type_hash.strip_prefix("0x").unwrap_or(&type_hash))
        .map_err(|_| ApiError::bad_request("Invalid type script hash"))?;

    let info = state
        .store
        .get_token(&hash)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    match info {
        Some(info) => {
            let now_ms = chrono::Utc::now().timestamp_millis();
            let transfers_count = state.store.get_token_transfers_count(&hash).unwrap_or(0);
            let transfers_24h = state
                .store
                .get_token_24h_transfers(&hash, now_ms)
                .unwrap_or(0);
            ok(token_info_to_response(
                &hash,
                &info,
                transfers_count,
                transfers_24h,
            ))
        }
        None => Err(ApiError::not_found("Token not found")),
    }
}

async fn get_token_holders(
    State(state): State<Arc<AppState>>,
    Path(type_hash): Path<String>,
    Query(params): Query<HolderParams>,
) -> ApiResult<CursorPaginatedResponse<TokenHolderResponse>> {
    let hash = hex::decode(type_hash.strip_prefix("0x").unwrap_or(&type_hash))
        .map_err(|_| ApiError::bad_request("Invalid type script hash"))?;

    // Verify token exists and get holders count
    let token_info = state
        .store
        .get_token(&hash)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let holders_count = match token_info {
        Some(info) => info.holders_count,
        None => return Err(ApiError::not_found("Token not found")),
    };

    let limit = params.limit.clamp(1, 100) as usize;

    // list_token_holders returns (lock_hash, balance) sorted by prefix scan order
    let all_holders = state
        .store
        .list_token_holders(&hash, 10000) // fetch up to 10000
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Sort by balance DESC
    let mut sorted_holders: Vec<_> = all_holders
        .into_iter()
        .filter(|(_, balance)| *balance > 0)
        .collect();
    sorted_holders.sort_by(|a, b| b.1.cmp(&a.1));

    // Apply cursor
    let cursor_balance = params.cursor.as_ref().and_then(|c| {
        let parts: Vec<&str> = c.split(':').collect();
        if parts.len() == 2 {
            parts[0].parse::<i128>().ok()
        } else {
            None
        }
    });

    let start_idx = if let Some(cb) = cursor_balance {
        sorted_holders
            .iter()
            .position(|(_, balance)| *balance < cb)
            .unwrap_or(sorted_holders.len())
    } else {
        0
    };

    let page: Vec<_> = sorted_holders
        .iter()
        .skip(start_idx)
        .take(limit + 1)
        .collect();

    let has_more = page.len() > limit;
    let page: Vec<_> = page.into_iter().take(limit).collect();

    let next_cursor = if has_more {
        page.last()
            .map(|(lock, balance)| format!("{}:{}", balance, hex::encode(lock)))
    } else {
        None
    };

    let holders: Vec<TokenHolderResponse> = page
        .into_iter()
        .map(|(lock_script_hash, balance)| TokenHolderResponse {
            lock_script_hash: format!("0x{}", hex::encode(lock_script_hash)),
            address: None,
            balance: balance.to_string(),
        })
        .collect();

    ok(CursorPaginatedResponse::new(
        holders,
        holders_count,
        limit as i64,
        next_cursor,
    ))
}

async fn get_token_transfers(
    State(state): State<Arc<AppState>>,
    Path(type_hash): Path<String>,
    Query(params): Query<TransferParams>,
) -> ApiResult<CursorPaginatedResponse<TokenTransferResponse>> {
    let hash = hex::decode(type_hash.strip_prefix("0x").unwrap_or(&type_hash))
        .map_err(|_| ApiError::bad_request("Invalid type script hash"))?;

    // Verify token exists
    let token_info = state
        .store
        .get_token(&hash)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    if token_info.is_none() {
        return Err(ApiError::not_found("Token not found"));
    }

    let limit = params.limit.clamp(1, 100);

    // Token transfers (activities) are not directly queryable by asset_id from the
    // current RocksDB store API. Return empty for now.
    let transfers: Vec<TokenTransferResponse> = Vec::new();

    ok(CursorPaginatedResponse::new(transfers, 0, limit, None))
}

fn hash_type_to_string(hash_type: i16) -> String {
    match hash_type {
        0 => "data".to_string(),
        1 => "type".to_string(),
        2 => "data1".to_string(),
        4 => "data2".to_string(),
        _ => format!("unknown({})", hash_type),
    }
}
