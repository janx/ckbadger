use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use super::statistics::{StackedAreaChartResponse, StackedAreaDataPoint, StackedAreaSeries};
use crate::response::{ok, ApiError, ApiResult, CursorPaginatedResponse};
use crate::utils::parse_chart_date_range;
use crate::warmup::{CachedAssetEntry, CACHE_KEY_ASSETS_TOKEN};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/tokens", get(list_tokens))
        .route("/tokens/{type_hash}", get(get_token))
        .route("/tokens/{type_hash}/holders", get(get_token_holders))
        .route("/tokens/{type_hash}/transfers", get(get_token_transfers))
        .route(
            "/tokens/{type_hash}/charts/occupation",
            get(get_token_occupation_chart),
        )
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

#[derive(Debug, Deserialize)]
pub struct ChartRangeParams {
    from: Option<String>,
    to: Option<String>,
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
    pub cells_count: Option<i64>,
    pub total_capacity: Option<String>,
    pub total_occupied_capacity: Option<String>,
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
    cell_stats: Option<ckbadger_store::TokenCellStats>,
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
        cells_count: cell_stats.as_ref().map(|s| s.cells_count),
        total_capacity: cell_stats.as_ref().map(|s| s.total_capacity.to_string()),
        total_occupied_capacity: cell_stats
            .as_ref()
            .map(|s| s.total_occupied_capacity.to_string()),
    }
}

async fn list_tokens(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<TokenResponse>> {
    let limit = params.limit.clamp(1, 100) as usize;

    // Try reading from in-memory cache first
    let cached = state
        .mem_cache
        .get::<Vec<CachedAssetEntry>>(CACHE_KEY_ASSETS_TOKEN);

    // Apply filters and serve from cache if available
    if let Some(cached_tokens) = cached {
        return serve_tokens_from_cache(cached_tokens, &params, limit);
    }

    // Cache cold — fall back to direct store reads
    serve_tokens_from_store(&state, &params, limit)
}

/// Serve token list from pre-computed cache.
fn serve_tokens_from_cache(
    cached: Vec<CachedAssetEntry>,
    params: &ListParams,
    limit: usize,
) -> ApiResult<CursorPaginatedResponse<TokenResponse>> {
    let search_lower = params.search.as_ref().map(|s| s.to_lowercase());

    let mut filtered: Vec<_> = cached
        .into_iter()
        .filter(|entry| {
            if let Some(ref standard) = params.standard {
                if &entry.standard != standard {
                    return false;
                }
            }
            if let Some(ref pattern) = search_lower {
                let name_match = entry
                    .name
                    .as_ref()
                    .map(|n| n.to_lowercase().contains(pattern))
                    .unwrap_or(false);
                let symbol_match = entry
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

    // Already sorted by transfers_24h DESC, holders_count DESC from cache
    // But we need to sort by holders_count for the tokens endpoint
    filtered.sort_by(|a, b| b.holders_count.cmp(&a.holders_count));

    // Apply cursor
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
            .position(|e| e.holders_count < ch)
            .unwrap_or(filtered.len())
    } else {
        0
    };

    let page: Vec<_> = filtered.iter().skip(start_idx).take(limit + 1).collect();
    let has_more = page.len() > limit;
    let page: Vec<_> = page.into_iter().take(limit).collect();

    let next_cursor = if has_more {
        page.last().map(|e| format!("0:{}:0", e.holders_count))
    } else {
        None
    };

    // Build TokenResponse directly from cache — zero DB reads
    let tokens: Vec<TokenResponse> = page
        .into_iter()
        .map(|entry| TokenResponse {
            type_script_hash: entry.id.clone(),
            type_code_hash: entry.type_code_hash.clone().unwrap_or_default(),
            type_hash_type: entry.type_hash_type.clone().unwrap_or_default(),
            type_args: entry.type_args.clone().unwrap_or_default(),
            standard: entry.standard.clone(),
            name: entry.name.clone(),
            symbol: entry.symbol.clone(),
            decimals: entry.decimals.unwrap_or(0),
            description: entry.description.clone(),
            icon_url: entry.icon_url.clone(),
            published: false,
            famous: false,
            tags: None,
            udt_type: None,
            manager: None,
            email: None,
            operator_website: None,
            total_supply: entry.total_supply.clone().unwrap_or_default(),
            holders_count: entry.holders_count as i32,
            transfers_count: entry.transfers_count,
            transfers_24h: entry.transfers_24h,
            cells_count: None,
            total_capacity: None,
            total_occupied_capacity: None,
        })
        .collect();

    ok(CursorPaginatedResponse::without_total(
        tokens,
        limit as i64,
        next_cursor,
    ))
}

/// Fallback: serve token list from direct store reads.
fn serve_tokens_from_store(
    state: &Arc<AppState>,
    params: &ListParams,
    limit: usize,
) -> ApiResult<CursorPaginatedResponse<TokenResponse>> {
    let all_tokens = state
        .store
        .list_tokens()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let search_hash = params.search.as_ref().and_then(|s| {
        let s = s.strip_prefix("0x").unwrap_or(s);
        hex::decode(s).ok()
    });
    let search_lower = params.search.as_ref().map(|s| s.to_lowercase());

    let mut filtered: Vec<_> = all_tokens
        .into_iter()
        .filter(|(type_hash, info)| {
            if let Some(ref standard) = params.standard {
                if &info.standard != standard {
                    return false;
                }
            }
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

    filtered.sort_by(|a, b| b.1.holders_count.cmp(&a.1.holders_count));

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
            // Read transfers_count from TokenInfo (no more N+1 stats lookup)
            let transfers_count = info.transfers_count;
            let transfers_24h = state
                .store
                .get_token_24h_transfers(type_hash, now_ms)
                .unwrap_or(0);
            token_info_to_response(type_hash, info, transfers_count, transfers_24h, None)
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
        Some(mut info) => {
            let now_ms = chrono::Utc::now().timestamp_millis();
            // Read transfers_count from TokenInfo directly
            let transfers_count = info.transfers_count;
            let transfers_24h = state
                .store
                .get_token_24h_transfers(&hash, now_ms)
                .unwrap_or(0);
            let cell_stats = state.store.aggregate_token_cell_stats(&hash).ok();
            // Use real holder count from token_holders CF instead of
            // potentially stale TokenInfo.holders_count.
            if let Ok(real_count) = state.store.count_token_holders(&hash) {
                info.holders_count = real_count;
            }
            ok(token_info_to_response(
                &hash,
                &info,
                transfers_count,
                transfers_24h,
                cell_stats,
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

    // Verify the token exists.
    let token_exists = state
        .store
        .get_token(&hash)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .is_some();
    if !token_exists {
        return Err(ApiError::not_found("Token not found"));
    }

    let limit = params.limit.clamp(1, 100) as usize;

    let all_holders = state
        .store
        .list_token_holders(&hash, 10000)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let mut sorted_holders: Vec<_> = all_holders
        .into_iter()
        .filter(|(_, balance)| *balance > 0)
        .collect();
    sorted_holders.sort_by(|a, b| b.1.cmp(&a.1));

    let holders_count = sorted_holders.len() as i64;

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

    let token_info = state
        .store
        .get_token(&hash)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let info = match token_info {
        Some(info) => info,
        None => return Err(ApiError::not_found("Token not found")),
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

    let results = state
        .store
        .list_token_transfers(&hash, limit + 1, cursor)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let has_more = results.len() > limit;
    let page: Vec<_> = results.into_iter().take(limit).collect();

    let next_cursor = if has_more {
        page.last()
            .map(|(block_num, tx_idx, _)| format!("{}:{}", block_num, tx_idx))
    } else {
        None
    };

    let transfers: Vec<TokenTransferResponse> = page
        .into_iter()
        .map(|(_, _, record)| TokenTransferResponse {
            tx_hash: format!("0x{}", hex::encode(&record.tx_hash)),
            block_number: record.block_number,
            from_lock_hash: record
                .from_lock_hash
                .as_ref()
                .map(|h| format!("0x{}", hex::encode(h))),
            from_address: None,
            to_lock_hash: format!("0x{}", hex::encode(&record.to_lock_hash)),
            to_address: None,
            amount: record.amount.to_string(),
            is_mint: record.is_mint,
            is_burn: record.is_burn,
            timestamp: record.timestamp.to_string(),
        })
        .collect();

    ok(CursorPaginatedResponse::new(
        transfers,
        info.transfers_count,
        limit as i64,
        next_cursor,
    ))
}

fn format_yyyymmdd_for_chart(date_yyyymmdd: u32) -> String {
    let date = format!("{date_yyyymmdd:08}");
    format!("{}-{}-{}", &date[0..4], &date[4..6], &date[6..8])
}

async fn get_token_occupation_chart(
    State(state): State<Arc<AppState>>,
    Path(type_hash): Path<String>,
    Query(params): Query<ChartRangeParams>,
) -> ApiResult<StackedAreaChartResponse> {
    let hash = hex::decode(type_hash.strip_prefix("0x").unwrap_or(&type_hash))
        .map_err(|_| ApiError::bad_request("Invalid type script hash"))?;

    let token = state
        .store
        .get_token(&hash)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let token = match token {
        Some(info) => info,
        None => return Err(ApiError::not_found("Token not found")),
    };

    let (from_date, to_date) = parse_chart_date_range(params.from.as_deref(), params.to.as_deref())
        .map_err(|msg| ApiError::bad_request(&msg))?;

    let mut cumulative_capacity: i128 = 0;
    let mut cumulative_occupied: i128 = 0;
    if let Some(from) = from_date {
        let baseline = state
            .store
            .list_token_daily_deltas_in_range(&hash, None, Some(from.saturating_sub(1)))
            .map_err(|e| ApiError::internal(e.to_string()))?;
        for (_, delta) in baseline {
            cumulative_capacity = (cumulative_capacity + delta.live_capacity_delta as i128).max(0);
            cumulative_occupied =
                (cumulative_occupied + delta.live_occupied_capacity_delta as i128).max(0);
            if cumulative_occupied > cumulative_capacity {
                cumulative_occupied = cumulative_capacity;
            }
        }
    }

    let deltas = state
        .store
        .list_token_daily_deltas_in_range(&hash, from_date, to_date)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let mut data = Vec::with_capacity(deltas.len());
    for (date, delta) in deltas {
        cumulative_capacity = (cumulative_capacity + delta.live_capacity_delta as i128).max(0);
        cumulative_occupied =
            (cumulative_occupied + delta.live_occupied_capacity_delta as i128).max(0);
        if cumulative_occupied > cumulative_capacity {
            cumulative_occupied = cumulative_capacity;
        }
        let unoccupied = cumulative_capacity - cumulative_occupied;

        data.push(StackedAreaDataPoint {
            date: format_yyyymmdd_for_chart(date),
            values: HashMap::from([
                ("occupied".to_string(), cumulative_occupied.to_string()),
                ("unoccupied".to_string(), unoccupied.to_string()),
            ]),
        });
    }

    let title = token
        .symbol
        .or(token.name)
        .unwrap_or_else(|| format!("0x{}", hex::encode(&hash)));

    ok(StackedAreaChartResponse {
        data,
        series: vec![
            StackedAreaSeries {
                key: "occupied".to_string(),
                label: "Occupied".to_string(),
                color: "#f59e0b".to_string(),
            },
            StackedAreaSeries {
                key: "unoccupied".to_string(),
                label: "Unoccupied".to_string(),
                color: "#00c389".to_string(),
            },
        ],
        title: format!("{title} Capacity Occupation"),
    })
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
