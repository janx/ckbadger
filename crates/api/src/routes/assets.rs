use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use super::statistics::{StackedAreaChartResponse, StackedAreaDataPoint, StackedAreaSeries};
use crate::response::{ok, ApiError, ApiResult, CursorPaginatedResponse};
use crate::utils::{resolve_dob_collection_name, resolve_nft_collection_name};
use crate::warmup::{
    CachedAssetEntry, CACHE_KEY_ASSETS_DOB, CACHE_KEY_ASSETS_NFT, CACHE_KEY_ASSETS_TOKEN,
};
use crate::AppState;

const DOTBIT_SENTINEL_COLLECTION: [u8; 32] = *b"dotbit_collection_______________";

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/assets", get(list_assets))
        .route("/assets/nfts/{collection_id}", get(get_nft_collection))
        .route(
            "/assets/nfts/{collection_id}/charts/occupation",
            get(get_nft_collection_occupation_chart),
        )
}

#[derive(Debug, Deserialize)]
pub struct ListParams {
    #[serde(default = "default_limit")]
    limit: i64,
    #[serde(rename = "type")]
    asset_type: Option<String>,
    cursor: Option<String>,
    search: Option<String>,
}

fn default_limit() -> i64 {
    20
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetResponse {
    pub id: String,
    pub asset_type: String,
    pub standard: String,
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub icon_url: Option<String>,
    pub published: bool,
    pub famous: bool,
    pub tags: Option<Vec<String>>,
    pub holders_count: i64,
    pub transfers_count: i64,
    pub transfers_24h: i64,
    pub decimals: Option<i16>,
    pub total_supply: Option<String>,
    pub content_type: Option<String>,
    pub content_size: Option<i32>,
    pub cluster_id: Option<String>,
    pub cluster_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NftCollectionDetailResponse {
    pub collection_id: String,
    pub standard: String,
    pub name: Option<String>,
    pub total_count: i64,
    pub live_count: i64,
    pub live_capacity: String,
    pub live_occupied_capacity: String,
}

async fn list_assets(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<AssetResponse>> {
    let limit = params.limit.clamp(1, 100);

    let search_lower = params.search.as_ref().map(|s| s.to_lowercase());
    let filter_type = params.asset_type.as_deref();

    let (total, rows) = fetch_assets_cached(
        &state,
        filter_type,
        search_lower.as_deref(),
        limit,
        params.cursor.as_deref(),
    )?;

    let has_more = rows.len() as i64 > limit;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

    let next_cursor = if has_more {
        rows.last().map(|r| {
            format!(
                "{}:{}:{}:{}",
                r.transfers_24h, r.holders_count, r.id, r.asset_type
            )
        })
    } else {
        None
    };

    ok(CursorPaginatedResponse::new(
        rows,
        total,
        limit,
        next_cursor,
    ))
}

/// Read from in-memory cache, apply search filter + cursor-based pagination.
/// Falls back to direct computation when cache is cold.
fn fetch_assets_cached(
    state: &Arc<AppState>,
    filter_type: Option<&str>,
    search: Option<&str>,
    limit: i64,
    cursor: Option<&str>,
) -> Result<(i64, Vec<AssetResponse>), (axum::http::StatusCode, Json<ApiError>)> {
    let mut all_cached: Vec<CachedAssetEntry> = Vec::new();

    // Collect from cache based on type filter
    if !matches!(filter_type, Some("nft") | Some("dob")) {
        if let Some(tokens) = state
            .mem_cache
            .get::<Vec<CachedAssetEntry>>(CACHE_KEY_ASSETS_TOKEN)
        {
            all_cached.extend(tokens);
        } else {
            // Cache cold — fall back to direct computation for tokens
            all_cached.extend(compute_token_assets(state)?);
        }
    }

    if !matches!(filter_type, Some("token") | Some("nft")) {
        if let Some(dobs) = state
            .mem_cache
            .get::<Vec<CachedAssetEntry>>(CACHE_KEY_ASSETS_DOB)
        {
            all_cached.extend(dobs);
        } else {
            all_cached.extend(compute_dob_assets(state)?);
        }
    }

    if !matches!(filter_type, Some("token") | Some("dob")) {
        if let Some(nfts) = state
            .mem_cache
            .get::<Vec<CachedAssetEntry>>(CACHE_KEY_ASSETS_NFT)
        {
            all_cached.extend(nfts);
        } else {
            all_cached.extend(compute_nft_assets(state)?);
        }
    }

    // Apply search filter
    if let Some(s) = search {
        all_cached.retain(|entry| {
            let name_match = entry
                .name
                .as_ref()
                .map(|n| n.to_lowercase().contains(s))
                .unwrap_or(false);
            let symbol_match = entry
                .symbol
                .as_ref()
                .map(|sym| sym.to_lowercase().contains(s))
                .unwrap_or(false);
            name_match || symbol_match
        });
    }

    let total = all_cached.len() as i64;

    // Sort: transfers_24h DESC, holders_count DESC, asset_type ASC, id DESC
    all_cached.sort_by(|a, b| {
        b.transfers_24h
            .cmp(&a.transfers_24h)
            .then_with(|| b.holders_count.cmp(&a.holders_count))
            .then_with(|| a.asset_type.cmp(&b.asset_type))
            .then_with(|| b.id.cmp(&a.id))
    });

    // Apply cursor-based pagination: skip items up to and including the cursor item
    if let Some(cursor_str) = cursor {
        if let Some((_c_transfers, _c_holders, c_id, c_type)) = parse_asset_cursor(cursor_str) {
            // Find the cursor item by its unique (id, asset_type) and skip past it
            if let Some(pos) = all_cached
                .iter()
                .position(|e| e.id == c_id && e.asset_type == c_type)
            {
                all_cached = all_cached.split_off(pos + 1);
            }
        }
    }

    all_cached.truncate((limit + 1) as usize);

    let assets: Vec<AssetResponse> = all_cached
        .into_iter()
        .map(|e| e.to_asset_response())
        .collect();

    Ok((total, assets))
}

/// Parse cursor string: "transfers_24h:holders_count:id:asset_type"
fn parse_asset_cursor(cursor: &str) -> Option<(i64, i64, String, String)> {
    let parts: Vec<&str> = cursor.splitn(4, ':').collect();
    if parts.len() == 4 {
        let transfers = parts[0].parse::<i64>().ok()?;
        let holders = parts[1].parse::<i64>().ok()?;
        let id = parts[2].to_string();
        let asset_type = parts[3].to_string();
        Some((transfers, holders, id, asset_type))
    } else {
        None
    }
}

fn format_yyyymmdd_for_chart(date_yyyymmdd: u32) -> String {
    let date = format!("{date_yyyymmdd:08}");
    format!("{}-{}-{}", &date[0..4], &date[4..6], &date[6..8])
}

fn decode_nft_collection_id(
    raw: &str,
) -> Result<Vec<u8>, (axum::http::StatusCode, Json<ApiError>)> {
    let normalized = raw.to_ascii_lowercase();
    if normalized == "dotbit" || normalized == ".bit" {
        return Ok(DOTBIT_SENTINEL_COLLECTION.to_vec());
    }
    hex::decode(raw.strip_prefix("0x").unwrap_or(raw))
        .map_err(|_| ApiError::bad_request("Invalid NFT collection ID"))
}

fn build_capacity_occupation_chart(
    deltas: Vec<(u32, i64, i64)>,
    title: String,
) -> StackedAreaChartResponse {
    let mut cumulative_capacity: i128 = 0;
    let mut cumulative_occupied: i128 = 0;
    let mut data = Vec::with_capacity(deltas.len());

    for (date, cap_delta, occupied_delta) in deltas {
        cumulative_capacity = (cumulative_capacity + cap_delta as i128).max(0);
        cumulative_occupied = (cumulative_occupied + occupied_delta as i128).max(0);
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

    StackedAreaChartResponse {
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
        title,
    }
}

fn latest_capacity_from_chart(chart: &StackedAreaChartResponse) -> (String, String) {
    if let Some(last) = chart.data.last() {
        let occupied = last
            .values
            .get("occupied")
            .cloned()
            .unwrap_or_else(|| "0".to_string());
        let unoccupied = last
            .values
            .get("unoccupied")
            .cloned()
            .unwrap_or_else(|| "0".to_string());
        let total = occupied.parse::<i128>().unwrap_or(0) + unoccupied.parse::<i128>().unwrap_or(0);
        return (total.to_string(), occupied);
    }
    ("0".to_string(), "0".to_string())
}

async fn get_nft_collection(
    State(state): State<Arc<AppState>>,
    Path(collection_id): Path<String>,
) -> ApiResult<NftCollectionDetailResponse> {
    let collection_id_bytes = decode_nft_collection_id(&collection_id)?;

    let agg = state
        .store
        .get_nft_collection_aggregate(&collection_id_bytes)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let agg = agg.ok_or_else(|| ApiError::not_found("NFT collection not found"))?;

    let daily = state
        .store
        .list_nft_daily_deltas(&collection_id_bytes)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let chart = build_capacity_occupation_chart(
        daily
            .into_iter()
            .map(|(date, delta)| {
                (
                    date,
                    delta.live_capacity_delta,
                    delta.live_occupied_capacity_delta,
                )
            })
            .collect(),
        "NFT Collection Capacity Occupation".to_string(),
    );
    let (live_capacity, live_occupied_capacity) = latest_capacity_from_chart(&chart);

    let standard = agg.standard.asset_standard().to_string();
    let name = resolve_nft_collection_name(&standard, agg.name.as_deref());

    ok(NftCollectionDetailResponse {
        collection_id: format!("0x{}", hex::encode(&collection_id_bytes)),
        standard,
        name,
        total_count: agg.total_count,
        live_count: agg.live_count,
        live_capacity,
        live_occupied_capacity,
    })
}

async fn get_nft_collection_occupation_chart(
    State(state): State<Arc<AppState>>,
    Path(collection_id): Path<String>,
) -> ApiResult<StackedAreaChartResponse> {
    let collection_id_bytes = decode_nft_collection_id(&collection_id)?;

    let agg = state
        .store
        .get_nft_collection_aggregate(&collection_id_bytes)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let agg = agg.ok_or_else(|| ApiError::not_found("NFT collection not found"))?;

    let daily = state
        .store
        .list_nft_daily_deltas(&collection_id_bytes)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let standard = agg.standard.asset_standard().to_string();
    let title = resolve_nft_collection_name(&standard, agg.name.as_deref())
        .unwrap_or_else(|| format!("0x{}", hex::encode(&collection_id_bytes)));

    ok(build_capacity_occupation_chart(
        daily
            .into_iter()
            .map(|(date, delta)| {
                (
                    date,
                    delta.live_capacity_delta,
                    delta.live_occupied_capacity_delta,
                )
            })
            .collect(),
        format!("{title} Capacity Occupation"),
    ))
}

/// Fallback: compute token assets directly using batch 24h scan (when cache is cold).
fn compute_token_assets(
    state: &Arc<AppState>,
) -> Result<Vec<CachedAssetEntry>, (axum::http::StatusCode, Json<ApiError>)> {
    let tokens = state
        .store
        .list_tokens()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let now_ms = chrono::Utc::now().timestamp_millis();
    let transfers_24h_map = state
        .store
        .scan_all_token_24h_transfers(now_ms)
        .unwrap_or_default();
    let mut result = Vec::with_capacity(tokens.len());

    for (hash, info) in &tokens {
        // Skip noise tokens: no name/symbol and no holders
        if info.name.is_none() && info.symbol.is_none() && info.holders_count == 0 {
            continue;
        }

        let transfers_24h = transfers_24h_map.get(hash.as_slice()).copied().unwrap_or(0);

        result.push(CachedAssetEntry {
            id: format!("0x{}", hex::encode(hash)),
            asset_type: "token".to_string(),
            standard: info.standard.clone(),
            name: info.name.clone(),
            symbol: info.symbol.clone(),
            icon_url: info.icon_url.clone(),
            holders_count: info.holders_count,
            transfers_count: info.transfers_count,
            transfers_24h,
            decimals: info.decimals.map(|d| d as i16),
            total_supply: info.total_supply.map(|s| s.to_string()),
            content_type: None,
            content_size: None,
            cluster_id: None,
            cluster_name: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            description: None,
        });
    }

    Ok(result)
}

/// Fallback: compute DOB assets from pre-aggregated cluster_agg CF.
fn compute_dob_assets(
    state: &Arc<AppState>,
) -> Result<Vec<CachedAssetEntry>, (axum::http::StatusCode, Json<ApiError>)> {
    let cluster_aggs = state
        .store
        .list_cluster_aggregates()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let mut result = Vec::new();
    for (cluster_id_bytes, agg) in &cluster_aggs {
        if agg.total_count == 0 {
            continue;
        }
        let cluster_hex = format!("0x{}", hex::encode(cluster_id_bytes));
        let display_name = resolve_dob_collection_name(
            state.store.as_ref(),
            cluster_id_bytes,
            agg.name.as_deref(),
        );

        result.push(CachedAssetEntry {
            id: cluster_hex.clone(),
            asset_type: "dob".to_string(),
            standard: "spore".to_string(),
            name: display_name.clone(),
            symbol: None,
            icon_url: None,
            holders_count: agg.owner_count,
            transfers_count: agg.total_count,
            transfers_24h: 0,
            decimals: None,
            total_supply: Some(agg.total_count.to_string()),
            content_type: None,
            content_size: None,
            cluster_id: Some(cluster_hex),
            cluster_name: display_name,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            description: None,
        });
    }

    Ok(result)
}

/// Fallback: compute NFT assets from pre-aggregated nft_collection_agg CF.
fn compute_nft_assets(
    state: &Arc<AppState>,
) -> Result<Vec<CachedAssetEntry>, (axum::http::StatusCode, Json<ApiError>)> {
    let nft_aggs = state
        .store
        .list_nft_collection_aggregates()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let mut result = Vec::new();
    for (collection_id_bytes, agg) in &nft_aggs {
        if agg.total_count == 0 {
            continue;
        }
        let collection_hex = format!("0x{}", hex::encode(collection_id_bytes));
        let standard = agg.standard.asset_standard().to_string();
        let display_name = resolve_nft_collection_name(&standard, agg.name.as_deref());

        result.push(CachedAssetEntry {
            id: collection_hex.clone(),
            asset_type: "nft".to_string(),
            standard,
            name: display_name.clone(),
            symbol: None,
            icon_url: None,
            holders_count: agg.live_count,
            transfers_count: agg.total_count,
            transfers_24h: 0,
            decimals: None,
            total_supply: Some(agg.total_count.to_string()),
            content_type: None,
            content_size: None,
            cluster_id: Some(collection_hex.clone()),
            cluster_name: display_name,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            description: None,
        });
    }

    Ok(result)
}
