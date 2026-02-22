use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::Arc;

use super::statistics::{StackedAreaChartResponse, StackedAreaDataPoint, StackedAreaSeries};
use crate::response::{ok, ApiError, ApiResult, CursorPaginatedResponse};
use crate::utils::{
    accumulate_live_capacity, apply_live_capacity_delta, parse_chart_date_range,
    resolve_dob_collection_name, resolve_nft_collection_name,
};
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
    #[serde(default = "default_asset_sort_key")]
    sort_key: AssetSortKey,
    #[serde(default = "default_sort_direction")]
    sort_direction: SortDirection,
}

#[derive(Debug, Deserialize)]
pub struct ChartRangeParams {
    from: Option<String>,
    to: Option<String>,
}

fn default_limit() -> i64 {
    20
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AssetSortKey {
    Name,
    Type,
    Supply,
    Transfers24h,
    Holders,
    Transfers,
    Occupied,
    Capacity,
}

fn default_asset_sort_key() -> AssetSortKey {
    AssetSortKey::Capacity
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum SortDirection {
    Asc,
    Desc,
}

fn default_sort_direction() -> SortDirection {
    SortDirection::Desc
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
    pub live_capacity: Option<String>,
    pub live_occupied_capacity: Option<String>,
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
        params.sort_key,
        params.sort_direction,
    )?;

    let has_more = rows.len() as i64 > limit;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

    let next_cursor = if has_more {
        rows.last().map(|r| format!("{}:{}", r.asset_type, r.id))
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
    sort_key: AssetSortKey,
    sort_direction: SortDirection,
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

    all_cached.sort_by(|a, b| compare_asset_entries(a, b, sort_key, sort_direction));

    // Apply cursor-based pagination: skip items up to and including the cursor item
    if let Some(cursor_str) = cursor {
        if let Some((c_type, c_id)) = parse_asset_cursor(cursor_str) {
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

/// Parse cursor string.
/// New format: "asset_type:id"
/// Legacy format: "transfers_24h:holders_count:id:asset_type"
fn parse_asset_cursor(cursor: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = cursor.splitn(2, ':').collect();
    if parts.len() == 2 && matches!(parts[0], "token" | "nft" | "dob") {
        return Some((parts[0].to_string(), parts[1].to_string()));
    }

    let legacy_parts: Vec<&str> = cursor.splitn(4, ':').collect();
    if legacy_parts.len() == 4 {
        return Some((legacy_parts[3].to_string(), legacy_parts[2].to_string()));
    }

    None
}

fn apply_direction(ordering: Ordering, direction: SortDirection) -> Ordering {
    match direction {
        SortDirection::Asc => ordering,
        SortDirection::Desc => ordering.reverse(),
    }
}

fn parse_i128_opt(value: Option<&str>) -> Option<i128> {
    value?.parse::<i128>().ok()
}

fn compare_optional_i128(
    left: Option<i128>,
    right: Option<i128>,
    direction: SortDirection,
) -> Ordering {
    match (left, right) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(l), Some(r)) => apply_direction(l.cmp(&r), direction),
    }
}

fn asset_display_name(entry: &CachedAssetEntry) -> String {
    if entry.asset_type == "token" {
        entry
            .symbol
            .clone()
            .or_else(|| entry.name.clone())
            .unwrap_or_else(|| "Unknown Token".to_string())
    } else {
        entry
            .name
            .clone()
            .unwrap_or_else(|| "Unnamed Collection".to_string())
    }
}

fn compare_asset_entries(
    left: &CachedAssetEntry,
    right: &CachedAssetEntry,
    sort_key: AssetSortKey,
    direction: SortDirection,
) -> Ordering {
    let compared = match sort_key {
        AssetSortKey::Name => apply_direction(
            asset_display_name(left).cmp(&asset_display_name(right)),
            direction,
        ),
        AssetSortKey::Type => apply_direction(left.standard.cmp(&right.standard), direction),
        AssetSortKey::Supply => compare_optional_i128(
            parse_i128_opt(left.total_supply.as_deref()),
            parse_i128_opt(right.total_supply.as_deref()),
            direction,
        ),
        AssetSortKey::Transfers24h => {
            apply_direction(left.transfers_24h.cmp(&right.transfers_24h), direction)
        }
        AssetSortKey::Holders => {
            apply_direction(left.holders_count.cmp(&right.holders_count), direction)
        }
        AssetSortKey::Transfers => {
            apply_direction(left.transfers_count.cmp(&right.transfers_count), direction)
        }
        AssetSortKey::Occupied => compare_optional_i128(
            parse_i128_opt(left.live_occupied_capacity.as_deref()),
            parse_i128_opt(right.live_occupied_capacity.as_deref()),
            direction,
        ),
        AssetSortKey::Capacity => compare_optional_i128(
            parse_i128_opt(left.live_capacity.as_deref()),
            parse_i128_opt(right.live_capacity.as_deref()),
            direction,
        ),
    };

    if compared != Ordering::Equal {
        return compared;
    }

    asset_display_name(left)
        .cmp(&asset_display_name(right))
        .then_with(|| left.asset_type.cmp(&right.asset_type))
        .then_with(|| left.id.cmp(&right.id))
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
    deltas: Vec<(u32, i128, i128)>,
    title: String,
) -> anyhow::Result<StackedAreaChartResponse> {
    build_capacity_occupation_chart_with_initial(deltas, title, 0, 0)
}

fn build_capacity_occupation_chart_with_initial(
    deltas: Vec<(u32, i128, i128)>,
    title: String,
    initial_capacity: i128,
    initial_occupied: i128,
) -> anyhow::Result<StackedAreaChartResponse> {
    if initial_capacity < 0 {
        anyhow::bail!(
            "invalid initial capacity for occupation chart: {}",
            initial_capacity
        );
    }
    if initial_occupied < 0 {
        anyhow::bail!(
            "invalid initial occupied capacity for occupation chart: {}",
            initial_occupied
        );
    }
    if initial_occupied > initial_capacity {
        anyhow::bail!(
            "invalid initial occupied/capacity for occupation chart: occupied={}, capacity={}",
            initial_occupied,
            initial_capacity
        );
    }
    let mut cumulative_capacity = initial_capacity;
    let mut cumulative_occupied = initial_occupied;
    let mut data = Vec::with_capacity(deltas.len());

    for (date, cap_delta, occupied_delta) in deltas {
        (cumulative_capacity, cumulative_occupied) = apply_live_capacity_delta(
            cumulative_capacity,
            cumulative_occupied,
            cap_delta,
            occupied_delta,
            &format!("building occupation chart at date {}", date),
        )?;
        let unoccupied = cumulative_capacity - cumulative_occupied;

        data.push(StackedAreaDataPoint {
            date: format_yyyymmdd_for_chart(date),
            values: HashMap::from([
                ("occupied".to_string(), cumulative_occupied.to_string()),
                ("unoccupied".to_string(), unoccupied.to_string()),
            ]),
        });
    }

    Ok(StackedAreaChartResponse {
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
    })
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
    )
    .map_err(|e| ApiError::internal(e.to_string()))?;
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
    Query(params): Query<ChartRangeParams>,
) -> ApiResult<StackedAreaChartResponse> {
    let (from_date, to_date) = parse_chart_date_range(params.from.as_deref(), params.to.as_deref())
        .map_err(|msg| ApiError::bad_request(&msg))?;

    let collection_id_bytes = decode_nft_collection_id(&collection_id)?;

    let agg = state
        .store
        .get_nft_collection_aggregate(&collection_id_bytes)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let agg = agg.ok_or_else(|| ApiError::not_found("NFT collection not found"))?;

    let daily = state
        .store
        .list_nft_daily_deltas_in_range(&collection_id_bytes, from_date, to_date)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let (initial_capacity, initial_occupied) = if let Some(from) = from_date {
        let mut base_capacity: i128 = 0;
        let mut base_occupied: i128 = 0;
        let baseline = state
            .store
            .list_nft_daily_deltas_in_range(
                &collection_id_bytes,
                None,
                Some(from.saturating_sub(1)),
            )
            .map_err(|e| ApiError::internal(e.to_string()))?;
        for (_, delta) in baseline {
            (base_capacity, base_occupied) = apply_live_capacity_delta(
                base_capacity,
                base_occupied,
                delta.live_capacity_delta,
                delta.live_occupied_capacity_delta,
                "building NFT baseline occupation chart",
            )
            .map_err(|e| ApiError::internal(e.to_string()))?;
        }
        (base_capacity, base_occupied)
    } else {
        (0, 0)
    };
    let standard = agg.standard.asset_standard().to_string();
    let title = resolve_nft_collection_name(&standard, agg.name.as_deref())
        .unwrap_or_else(|| format!("0x{}", hex::encode(&collection_id_bytes)));

    ok(build_capacity_occupation_chart_with_initial(
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
        initial_capacity,
        initial_occupied,
    )
    .map_err(|e| ApiError::internal(e.to_string()))?)
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
        let token_daily = state
            .store
            .list_token_daily_deltas(hash)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let (live_capacity, live_occupied_capacity) =
            accumulate_live_capacity(token_daily.into_iter().map(|(_, delta)| {
                (
                    delta.live_capacity_delta,
                    delta.live_occupied_capacity_delta,
                )
            }))
            .map_err(|e| ApiError::internal(e.to_string()))?;

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
            maximum_supply: info.max_supply.map(|s| s.to_string()),
            content_type: None,
            content_size: None,
            cluster_id: None,
            cluster_name: None,
            live_capacity: Some(live_capacity.to_string()),
            live_occupied_capacity: Some(live_occupied_capacity.to_string()),
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
        let cluster_daily = state
            .store
            .list_cluster_daily_deltas(cluster_id_bytes)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let (live_capacity, live_occupied_capacity) =
            accumulate_live_capacity(cluster_daily.into_iter().map(|(_, delta)| {
                (
                    delta.live_capacity_delta,
                    delta.live_occupied_capacity_delta,
                )
            }))
            .map_err(|e| ApiError::internal(e.to_string()))?;

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
            maximum_supply: None,
            content_type: None,
            content_size: None,
            cluster_id: Some(cluster_hex),
            cluster_name: display_name,
            live_capacity: Some(live_capacity.to_string()),
            live_occupied_capacity: Some(live_occupied_capacity.to_string()),
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
        let nft_daily = state
            .store
            .list_nft_daily_deltas(collection_id_bytes)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let (live_capacity, live_occupied_capacity) =
            accumulate_live_capacity(nft_daily.into_iter().map(|(_, delta)| {
                (
                    delta.live_capacity_delta,
                    delta.live_occupied_capacity_delta,
                )
            }))
            .map_err(|e| ApiError::internal(e.to_string()))?;

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
            maximum_supply: None,
            content_type: None,
            content_size: None,
            cluster_id: Some(collection_hex.clone()),
            cluster_name: display_name,
            live_capacity: Some(live_capacity.to_string()),
            live_occupied_capacity: Some(live_occupied_capacity.to_string()),
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            description: None,
        });
    }

    Ok(result)
}
