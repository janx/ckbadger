use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use ckbadger_store::{types::NftCollectionActivityEntry, CkbadgerStore};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use super::statistics::{StackedAreaChartResponse, StackedAreaDataPoint, StackedAreaSeries};
use crate::cache::InMemoryCache;
use crate::response::{ok, ApiError, ApiResult, CursorPaginatedResponse};
use crate::utils::{
    apply_live_capacity_delta, date_keys_inclusive, ensure_derived_ready, parse_chart_date_range,
    resolve_nft_collection_name,
};
use crate::warmup::{CachedAssetEntry, CACHE_KEY_ASSETS_NFT, CACHE_KEY_ASSETS_TOKEN};
use crate::AppState;

const DOTBIT_SENTINEL_COLLECTION: [u8; 32] = *b"dotbit_collection_______________";
const DID_CKB_SENTINEL_COLLECTION: [u8; 32] = *b"did_ckb_collection______________";
type ApiRouteError = (axum::http::StatusCode, Json<ApiError>);
type LiveOutpointMap = HashMap<Vec<u8>, (Vec<u8>, i16)>;
const NFT_ACTIVITY_SCAN_CHUNK_SIZE: usize = 128;
const NFT_ACTIVITY_COUNT_CACHE_TTL: Duration = Duration::from_secs(30);
const NFT_HOLDER_LIST_CACHE_TTL: Duration = Duration::from_secs(30);

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/assets", get(list_assets))
        .route("/assets/nfts/items/{nft_id}", get(get_nft_item_detail))
        .route(
            "/assets/nfts/dotbit/items/{nft_id}",
            get(get_dotbit_item_detail),
        )
        .route(
            "/assets/nfts/did/items/{nft_id}",
            get(get_did_ckb_item_detail),
        )
        .route(
            "/assets/nfts/dotbit/items/{nft_id}/activities",
            get(list_dotbit_item_activities),
        )
        .route(
            "/assets/nfts/did/items/{nft_id}/activities",
            get(list_did_ckb_item_activities),
        )
        .route(
            "/assets/nfts/items/{nft_id}/activities",
            get(list_mnft_item_activities),
        )
        .route("/assets/nfts/{collection_id}", get(get_nft_collection))
        .route(
            "/assets/nfts/{collection_id}/items",
            get(list_nft_collection_items),
        )
        .route(
            "/assets/nfts/{collection_id}/holders",
            get(list_nft_collection_holders),
        )
        .route(
            "/assets/nfts/{collection_id}/activities",
            get(list_nft_collection_activities),
        )
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
    asset_type: Option<AssetFilterType>,
    standard: Option<String>,
    cursor: Option<String>,
    search: Option<String>,
    #[serde(default = "default_asset_sort_key")]
    sort_key: AssetSortKey,
    #[serde(default = "default_sort_direction")]
    sort_direction: SortDirection,
    storage_tier: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChartRangeParams {
    from: Option<String>,
    to: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct NftItemsParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
    search: Option<String>,
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MnftItemActivitiesParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
    action: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct NftCollectionHoldersParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct NftCollectionActivitiesParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
    action: Option<String>,
}

fn default_limit() -> i64 {
    20
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum AssetFilterType {
    Token,
    Nft,
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
    OnchainRatio,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NftItemStatusFilter {
    All,
    Live,
    Recycled,
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
    pub storage_tier: Option<String>,
    pub fully_onchain_ratio: Option<String>,
    pub fully_onchain_count: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionStorageProfileResponse {
    pub tier: String,
    pub fully_onchain_count: i64,
    pub decentralized_external_count: i64,
    pub centralized_dependent_count: i64,
    pub unknown_count: i64,
    pub fully_onchain_ratio: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NftCollectionDetailResponse {
    pub collection_id: String,
    pub standard: String,
    pub name: Option<String>,
    pub total_count: i64,
    pub live_count: i64,
    pub holders_count: i64,
    pub activities_count: i64,
    pub live_capacity: String,
    pub live_occupied_capacity: String,
    pub storage_profile: CollectionStorageProfileResponse,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NftCollectionItemResponse {
    pub nft_id: String,
    pub name: Option<String>,
    pub standard: String,
    pub owner_lock_hash: Option<String>,
    pub is_live: bool,
    pub created_at_block: i64,
    pub expired_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registered_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u8>,
    pub tx_hash: Option<String>,
    pub output_index: Option<i16>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MnftClassSummaryResponse {
    pub class_id: String,
    pub issuer_id: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub renderer: Option<String>,
    pub total: u32,
    pub issued: u32,
    pub configure: u8,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MnftIssuerSummaryResponse {
    pub issuer_id: String,
    pub name: Option<String>,
    pub class_count: u32,
    pub set_count: u32,
    pub info_hex: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MnftLifecycleEventResponse {
    pub event: String,
    pub block_number: Option<i64>,
    pub tx_hash: Option<String>,
    pub output_index: Option<i16>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MnftItemDetailResponse {
    pub nft_id: String,
    pub standard: String,
    pub is_live: bool,
    pub owner_lock_hash: Option<String>,
    pub created_at_block: i64,
    pub token_index: u32,
    pub characteristic_hex: String,
    pub configure: u8,
    pub state: u8,
    pub tx_hash: Option<String>,
    pub output_index: Option<i16>,
    pub class: MnftClassSummaryResponse,
    pub issuer: MnftIssuerSummaryResponse,
    pub lifecycle: Vec<MnftLifecycleEventResponse>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MnftItemActivityResponse {
    pub tx_hash: String,
    pub block_number: i64,
    pub tx_index: i32,
    pub timestamp: String,
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NftCollectionHolderResponse {
    pub lock_script_hash: String,
    pub address: Option<String>,
    pub item_count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NftCollectionActivityResponse {
    pub tx_hash: String,
    pub block_number: i64,
    pub tx_index: i32,
    pub timestamp: String,
    pub actions: Vec<String>,
}

async fn list_assets(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<AssetResponse>> {
    ensure_derived_ready(state.as_ref())?;

    let limit = params.limit.clamp(1, 100);

    let search_lower = params.search.as_ref().map(|s| s.to_lowercase());
    let filter_type = params.asset_type;
    let filter_standard = normalize_assets_standard(params.standard.as_deref());
    let filter_storage_tier = normalize_assets_storage_tier(params.storage_tier.as_deref())?;

    let request = CachedAssetsRequest {
        standard: filter_standard.as_deref(),
        storage_tier: filter_storage_tier.as_deref(),
        search: search_lower.as_deref(),
        limit,
        cursor: params.cursor.as_deref(),
        sort_key: params.sort_key,
        sort_direction: params.sort_direction,
    };

    let (total, rows) = fetch_assets_cached(&state, filter_type, request)?;

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
/// Returns an explicit error when cache is unavailable.
struct CachedAssetsRequest<'a> {
    standard: Option<&'a str>,
    storage_tier: Option<&'a str>,
    search: Option<&'a str>,
    limit: i64,
    cursor: Option<&'a str>,
    sort_key: AssetSortKey,
    sort_direction: SortDirection,
}

fn fetch_assets_cached(
    state: &Arc<AppState>,
    filter_type: Option<AssetFilterType>,
    request: CachedAssetsRequest<'_>,
) -> Result<(i64, Vec<AssetResponse>), (axum::http::StatusCode, Json<ApiError>)> {
    let mut all_cached: Vec<CachedAssetEntry> = Vec::new();

    // Collect from cache based on type filter
    match filter_type {
        Some(AssetFilterType::Token) => {
            if let Some(tokens) = state
                .mem_cache
                .get::<Vec<CachedAssetEntry>>(CACHE_KEY_ASSETS_TOKEN)
            {
                all_cached.extend(tokens);
            } else {
                return Err(ApiError::internal(
                    "token asset cache unavailable; warmup in progress",
                ));
            }
        }
        Some(AssetFilterType::Nft) => {
            if let Some(nfts) = state
                .mem_cache
                .get::<Vec<CachedAssetEntry>>(CACHE_KEY_ASSETS_NFT)
            {
                all_cached.extend(nfts);
            } else {
                return Err(ApiError::internal(
                    "nft asset cache unavailable; warmup in progress",
                ));
            }
        }
        None => {
            if let Some(tokens) = state
                .mem_cache
                .get::<Vec<CachedAssetEntry>>(CACHE_KEY_ASSETS_TOKEN)
            {
                all_cached.extend(tokens);
            } else {
                return Err(ApiError::internal(
                    "token asset cache unavailable; warmup in progress",
                ));
            }

            if let Some(nfts) = state
                .mem_cache
                .get::<Vec<CachedAssetEntry>>(CACHE_KEY_ASSETS_NFT)
            {
                all_cached.extend(nfts);
            } else {
                return Err(ApiError::internal(
                    "nft asset cache unavailable; warmup in progress",
                ));
            }
        }
    }

    if let Some(standard_filter) = request.standard {
        all_cached.retain(|entry| entry.standard.eq_ignore_ascii_case(standard_filter));
    }
    if let Some(storage_tier_filter) = request.storage_tier {
        all_cached.retain(|entry| {
            if entry.asset_type != "nft" {
                return false;
            }
            let Some(tier) = entry.storage_tier.as_deref() else {
                return false;
            };
            if storage_tier_filter == "offchain_dependent" {
                return matches!(
                    tier,
                    "offchain_dependent" | "decentralized_external" | "centralized_dependent"
                );
            }
            tier == storage_tier_filter
        });
    }

    // Apply search filter
    if let Some(s) = request.search {
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

    all_cached
        .sort_by(|a, b| compare_asset_entries(a, b, request.sort_key, request.sort_direction));

    // Apply cursor-based pagination: skip items up to and including the cursor item
    if let Some(cursor_str) = request.cursor {
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

    all_cached.truncate((request.limit + 1) as usize);

    let assets: Vec<AssetResponse> = all_cached
        .into_iter()
        .map(|e| e.to_asset_response())
        .collect();

    Ok((total, assets))
}

fn normalize_assets_standard(value: Option<&str>) -> Option<String> {
    value.and_then(|raw| {
        let trimmed = raw.trim().to_ascii_lowercase();
        if trimmed.is_empty() {
            None
        } else if trimmed == "did:ckb" {
            Some("did_ckb".to_string())
        } else {
            Some(trimmed)
        }
    })
}

fn normalize_assets_storage_tier(
    value: Option<&str>,
) -> Result<Option<String>, (axum::http::StatusCode, Json<ApiError>)> {
    let Some(raw) = value else {
        return Ok(None);
    };
    let normalized = raw.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Ok(None);
    }
    match normalized.as_str() {
        "fully_onchain"
        | "offchain_dependent"
        | "decentralized_external"
        | "centralized_dependent"
        | "unknown" => Ok(Some(normalized)),
        _ => Err(ApiError::bad_request(
            "Invalid storage_tier. Expected one of: fully_onchain, offchain_dependent, decentralized_external, centralized_dependent, unknown",
        )),
    }
}

/// Parse cursor string.
/// Current format: "asset_type:id"
fn parse_asset_cursor(cursor: &str) -> Option<(String, String)> {
    let normalize_type = |asset_type: &str| match asset_type {
        "token" => Some("token"),
        "nft" => Some("nft"),
        _ => None,
    };

    let parts: Vec<&str> = cursor.splitn(2, ':').collect();
    if parts.len() == 2 {
        if let Some(asset_type) = normalize_type(parts[0]) {
            return Some((asset_type.to_string(), parts[1].to_string()));
        }
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

fn compare_optional_i64(
    left: Option<i64>,
    right: Option<i64>,
    direction: SortDirection,
) -> Ordering {
    match (left, right) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(l), Some(r)) => apply_direction(l.cmp(&r), direction),
    }
}

fn parse_ratio_1e4(value: Option<&str>) -> Option<i64> {
    let raw = value?.trim();
    if raw.is_empty() {
        return None;
    }
    let mut split = raw.split('.');
    let whole = split.next()?.parse::<i64>().ok()?;
    let frac = split.next().unwrap_or("0");
    if split.next().is_some() {
        return None;
    }
    let mut frac_buf = String::with_capacity(4);
    for ch in frac.chars().take(4) {
        if !ch.is_ascii_digit() {
            return None;
        }
        frac_buf.push(ch);
    }
    while frac_buf.len() < 4 {
        frac_buf.push('0');
    }
    let frac_num = frac_buf.parse::<i64>().ok()?;
    whole.checked_mul(10_000)?.checked_add(frac_num)
}

fn format_ratio_4(numerator: i64, denominator: i64) -> String {
    if denominator <= 0 {
        return "0.0000".to_string();
    }
    let scaled = numerator
        .saturating_mul(10_000)
        .checked_div(denominator)
        .unwrap_or(0);
    let whole = scaled / 10_000;
    let frac = (scaled % 10_000).abs();
    format!("{whole}.{frac:04}")
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
        AssetSortKey::OnchainRatio => compare_optional_i64(
            parse_ratio_1e4(left.fully_onchain_ratio.as_deref()),
            parse_ratio_1e4(right.fully_onchain_ratio.as_deref()),
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
    if normalized == "did:ckb" || normalized == "did_ckb" {
        return Ok(DID_CKB_SENTINEL_COLLECTION.to_vec());
    }
    hex::decode(raw.strip_prefix("0x").unwrap_or(raw))
        .map_err(|_| ApiError::bad_request("Invalid NFT collection ID"))
}

fn decode_nft_item_cursor(raw: &str) -> Result<Vec<u8>, (axum::http::StatusCode, Json<ApiError>)> {
    hex::decode(raw.strip_prefix("0x").unwrap_or(raw))
        .map_err(|_| ApiError::bad_request("Invalid NFT items cursor"))
}

fn decode_nft_item_id(raw: &str) -> Result<Vec<u8>, (axum::http::StatusCode, Json<ApiError>)> {
    hex::decode(raw.strip_prefix("0x").unwrap_or(raw))
        .map_err(|_| ApiError::bad_request("Invalid NFT item ID"))
}

fn decode_activity_cursor(
    raw: &str,
) -> Result<(i64, i32), (axum::http::StatusCode, Json<ApiError>)> {
    let mut parts = raw.split(':');
    let block = parts
        .next()
        .ok_or_else(|| ApiError::bad_request("Invalid activity cursor"))?
        .parse::<i64>()
        .map_err(|_| ApiError::bad_request("Invalid activity cursor"))?;
    let tx_index = parts
        .next()
        .ok_or_else(|| ApiError::bad_request("Invalid activity cursor"))?
        .parse::<i32>()
        .map_err(|_| ApiError::bad_request("Invalid activity cursor"))?;
    if parts.next().is_some() {
        return Err(ApiError::bad_request("Invalid activity cursor"));
    }
    Ok((block, tx_index))
}

fn decode_nft_collection_holders_cursor(
    raw: &str,
) -> Result<(i64, Vec<u8>), (axum::http::StatusCode, Json<ApiError>)> {
    let mut parts = raw.split(':');
    let count = parts
        .next()
        .ok_or_else(|| ApiError::bad_request("Invalid NFT collection holders cursor"))?
        .parse::<i64>()
        .map_err(|_| ApiError::bad_request("Invalid NFT collection holders cursor"))?;
    let lock_hash_hex = parts
        .next()
        .ok_or_else(|| ApiError::bad_request("Invalid NFT collection holders cursor"))?;
    if parts.next().is_some() {
        return Err(ApiError::bad_request(
            "Invalid NFT collection holders cursor",
        ));
    }
    let lock_hash = hex::decode(lock_hash_hex.strip_prefix("0x").unwrap_or(lock_hash_hex))
        .map_err(|_| ApiError::bad_request("Invalid NFT collection holders cursor"))?;
    if lock_hash.len() != 32 {
        return Err(ApiError::bad_request(
            "Invalid NFT collection holders cursor",
        ));
    }
    Ok((count, lock_hash))
}

fn normalize_nft_item_activity_action_filter(
    raw: Option<&str>,
) -> Result<Option<String>, (axum::http::StatusCode, Json<ApiError>)> {
    let Some(raw_value) = raw else {
        return Ok(None);
    };
    let normalized = raw_value.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Ok(None);
    }
    match normalized.as_str() {
        "mint" | "transfer" | "burn" => Ok(Some(normalized)),
        _ => Err(ApiError::bad_request(
            "Invalid nft item activity action filter. Expected one of: mint, transfer, burn",
        )),
    }
}

fn normalize_nft_collection_activity_action_filter(
    raw: Option<&str>,
) -> Result<Option<String>, (axum::http::StatusCode, Json<ApiError>)> {
    let Some(raw_value) = raw else {
        return Ok(None);
    };
    let normalized = raw_value.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Ok(None);
    }
    match normalized.as_str() {
        "mint" | "transfer" | "burn" | "recycle" | "renew" | "update" => {
            Ok(Some(normalized))
        }
        _ => Err(ApiError::bad_request(
            "Invalid nft activity action filter. Expected one of: mint, transfer, burn, recycle, renew, update",
        )),
    }
}

fn canonical_nft_collection_activity_locations(
    store: &CkbadgerStore,
    rows: &[(i64, i32, NftCollectionActivityEntry)],
) -> anyhow::Result<HashMap<Vec<u8>, (i64, i32)>> {
    if rows.is_empty() {
        return Ok(HashMap::new());
    }
    let tx_hashes: Vec<Vec<u8>> = rows
        .iter()
        .map(|(_, _, entry)| entry.tx_hash.clone())
        .collect();
    let tx_batch = store.get_txs_by_hash_batch(&tx_hashes)?;
    let mut out = HashMap::with_capacity(tx_batch.len());
    for (tx_hash, tx_row_opt) in tx_batch {
        if let Some((block_number, tx_index, _)) = tx_row_opt {
            out.insert(tx_hash, (block_number, tx_index));
        }
    }
    Ok(out)
}

pub(crate) fn list_canonical_nft_collection_activities_page(
    store: &CkbadgerStore,
    append_only_store: &CkbadgerStore,
    collection_id: &[u8],
    limit: usize,
    cursor: Option<(i64, i32)>,
    action_filter: Option<&str>,
) -> anyhow::Result<Vec<(i64, i32, NftCollectionActivityEntry)>> {
    if limit == 0 {
        return Ok(Vec::new());
    }

    let scan_limit = NFT_ACTIVITY_SCAN_CHUNK_SIZE.max(limit);
    let mut out = Vec::with_capacity(limit);
    let mut scan_cursor = cursor;

    loop {
        let rows = append_only_store.list_nft_collection_activities(
            collection_id,
            scan_limit,
            scan_cursor,
            action_filter,
        )?;
        if rows.is_empty() {
            break;
        }
        let rows_len = rows.len();
        let canonical_locations = canonical_nft_collection_activity_locations(store, &rows)?;
        let mut last_seen = None;
        for (block_number, tx_index, entry) in rows {
            last_seen = Some((block_number, tx_index));
            if canonical_locations.get(&entry.tx_hash) == Some(&(block_number, tx_index)) {
                out.push((block_number, tx_index, entry));
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

pub(crate) fn count_canonical_nft_collection_activities(
    store: &CkbadgerStore,
    append_only_store: &CkbadgerStore,
    collection_id: &[u8],
) -> anyhow::Result<i64> {
    let mut total = 0i64;
    let mut cursor = None;

    loop {
        let rows = append_only_store.list_nft_collection_activities(
            collection_id,
            NFT_ACTIVITY_SCAN_CHUNK_SIZE,
            cursor,
            None,
        )?;
        if rows.is_empty() {
            break;
        }
        let rows_len = rows.len();
        let canonical_locations = canonical_nft_collection_activity_locations(store, &rows)?;
        let mut last_seen = None;
        for (block_number, tx_index, entry) in rows {
            last_seen = Some((block_number, tx_index));
            if canonical_locations.get(&entry.tx_hash) == Some(&(block_number, tx_index)) {
                total = total.checked_add(1).ok_or_else(|| {
                    anyhow::anyhow!(
                        "canonical nft collection activity count overflow: collection_id=0x{}",
                        hex::encode(collection_id)
                    )
                })?;
            }
        }
        if rows_len < NFT_ACTIVITY_SCAN_CHUNK_SIZE {
            break;
        }
        let Some(last_seen_cursor) = last_seen else {
            break;
        };
        cursor = Some(last_seen_cursor);
    }

    Ok(total)
}

pub(crate) fn count_nft_collection_activities_cached(
    store: &CkbadgerStore,
    append_only_store: &CkbadgerStore,
    mem_cache: &InMemoryCache,
    collection_id: &[u8],
) -> anyhow::Result<i64> {
    let cache_key = format!(
        "assets:nft_collection_activities_count:0x{}",
        hex::encode(collection_id)
    );
    if let Some(cached) = mem_cache.get::<i64>(&cache_key) {
        return Ok(cached);
    }

    let total = count_canonical_nft_collection_activities(store, append_only_store, collection_id)?;
    mem_cache.set(&cache_key, &total, NFT_ACTIVITY_COUNT_CACHE_TTL);
    Ok(total)
}

fn normalize_nft_items_search(search: Option<&str>) -> Option<String> {
    search.and_then(|value| {
        let trimmed = value.trim().to_ascii_lowercase();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

fn normalize_nft_items_status(
    status: Option<&str>,
) -> Result<NftItemStatusFilter, (axum::http::StatusCode, Json<ApiError>)> {
    let Some(raw_status) = status else {
        return Ok(NftItemStatusFilter::All);
    };
    let normalized = raw_status.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Ok(NftItemStatusFilter::All);
    }
    match normalized.as_str() {
        "all" => Ok(NftItemStatusFilter::All),
        "live" => Ok(NftItemStatusFilter::Live),
        "recycled" => Ok(NftItemStatusFilter::Recycled),
        _ => Err(ApiError::bad_request(
            "Invalid nft item status filter. Expected one of: all, live, recycled",
        )),
    }
}

fn nft_item_matches_status(
    status_filter: NftItemStatusFilter,
    entry: &ckbadger_store::types::NftEntry,
) -> bool {
    match status_filter {
        NftItemStatusFilter::All => true,
        NftItemStatusFilter::Live => entry.is_live,
        NftItemStatusFilter::Recycled => !entry.is_live,
    }
}

fn nft_item_matches_search(
    search_lower: Option<&str>,
    nft_id: &[u8],
    entry: &ckbadger_store::types::NftEntry,
) -> bool {
    let Some(search) = search_lower else {
        return true;
    };

    if entry
        .name
        .as_deref()
        .map(|name| name.to_ascii_lowercase().contains(search))
        .unwrap_or(false)
    {
        return true;
    }

    let nft_id_hex = hex::encode(nft_id);
    nft_id_hex.contains(search) || format!("0x{nft_id_hex}").contains(search)
}

fn pad_collection_id_32(id: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    let len = id.len().min(32);
    out[..len].copy_from_slice(&id[..len]);
    out
}

fn nft_entry_matches_collection(
    collection_id: &[u8],
    entry: &ckbadger_store::types::NftEntry,
) -> bool {
    match entry.standard {
        ckbadger_store::types::NftStandard::DotBit => {
            collection_id == DOTBIT_SENTINEL_COLLECTION.as_slice()
        }
        _ => entry
            .collection_id
            .as_deref()
            .map(|id| pad_collection_id_32(id) == pad_collection_id_32(collection_id))
            .unwrap_or(false),
    }
}

fn build_capacity_occupation_chart(
    deltas: Vec<(u32, i128, i128)>,
    title: String,
) -> anyhow::Result<StackedAreaChartResponse> {
    build_capacity_occupation_chart_with_initial(deltas, title, 0, 0, None, None)
}

fn build_capacity_occupation_chart_with_initial(
    deltas: Vec<(u32, i128, i128)>,
    title: String,
    initial_capacity: i128,
    initial_occupied: i128,
    from_date: Option<u32>,
    to_date: Option<u32>,
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
    let mut daily_deltas: std::collections::BTreeMap<u32, (i128, i128)> =
        std::collections::BTreeMap::new();
    for (date, cap_delta, occupied_delta) in deltas {
        let entry = daily_deltas.entry(date).or_insert((0, 0));
        entry.0 = entry.0.checked_add(cap_delta).ok_or_else(|| {
            anyhow::anyhow!(
                "capacity delta overflow while building occupation chart: date={}",
                date
            )
        })?;
        entry.1 = entry.1.checked_add(occupied_delta).ok_or_else(|| {
            anyhow::anyhow!(
                "occupied delta overflow while building occupation chart: date={}",
                date
            )
        })?;
    }

    let chart_bounds = match (from_date, to_date) {
        (Some(from), Some(to)) => Some((from, to)),
        (Some(from), None) => daily_deltas
            .keys()
            .next_back()
            .copied()
            .map(|last| (from, last)),
        (None, Some(to)) => daily_deltas.keys().next().copied().map(|first| (first, to)),
        (None, None) => {
            let first = daily_deltas.keys().next().copied();
            let last = daily_deltas.keys().next_back().copied();
            first.zip(last)
        }
    };
    let dates = if let Some((start, end)) = chart_bounds {
        date_keys_inclusive(start, end).map_err(|e| anyhow::anyhow!(e))?
    } else {
        Vec::new()
    };

    let mut cumulative_capacity = initial_capacity;
    let mut cumulative_occupied = initial_occupied;
    let mut data = Vec::with_capacity(dates.len());

    for date in dates {
        let (cap_delta, occupied_delta) = daily_deltas.get(&date).copied().unwrap_or((0, 0));
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

async fn get_nft_item_detail(
    State(state): State<Arc<AppState>>,
    Path(nft_id): Path<String>,
) -> ApiResult<MnftItemDetailResponse> {
    let nft_id_bytes = decode_nft_item_id(&nft_id)?;
    let entry = state
        .store
        .get_nft(&nft_id_bytes)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("NFT item not found"))?;

    if !matches!(
        entry.standard,
        ckbadger_store::types::NftStandard::MnftToken
    ) {
        return Err(ApiError::bad_request(
            "NFT item detail currently supports mNFT token only",
        ));
    }

    let (token_index, characteristic, token_configure, token_state) = match &entry.extra {
        ckbadger_store::types::NftExtra::MnftToken {
            token_index,
            characteristic,
            configure,
            state,
        } => (*token_index, characteristic.clone(), *configure, *state),
        _ => {
            return Err(ApiError::internal(format!(
                "invalid NFT entry extra type for mNFT token: nft_id=0x{}",
                hex::encode(&nft_id_bytes)
            )))
        }
    };

    let class_id = entry.collection_id.clone().ok_or_else(|| {
        ApiError::internal(format!(
            "mNFT token missing class_id: nft_id=0x{}",
            hex::encode(&nft_id_bytes)
        ))
    })?;
    let class_entry = state
        .store
        .get_nft(&class_id)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| {
            ApiError::internal(format!(
                "mNFT class entry missing: class_id=0x{}, nft_id=0x{}",
                hex::encode(&class_id),
                hex::encode(&nft_id_bytes)
            ))
        })?;
    let (class_description, class_renderer, class_total, class_issued, class_configure) =
        match &class_entry.extra {
            ckbadger_store::types::NftExtra::MnftClass {
                description,
                renderer,
                total,
                issued,
                configure,
            } => (
                description.clone(),
                renderer.clone(),
                *total,
                *issued,
                *configure,
            ),
            _ => {
                return Err(ApiError::internal(format!(
                    "invalid class extra type for mNFT token: class_id=0x{}, nft_id=0x{}",
                    hex::encode(&class_id),
                    hex::encode(&nft_id_bytes)
                )))
            }
        };

    let issuer_id = class_entry.collection_id.clone().ok_or_else(|| {
        ApiError::internal(format!(
            "mNFT class missing issuer_id: class_id=0x{}, nft_id=0x{}",
            hex::encode(&class_id),
            hex::encode(&nft_id_bytes)
        ))
    })?;
    let issuer_entry = state
        .store
        .get_nft(&issuer_id)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| {
            ApiError::internal(format!(
                "mNFT issuer entry missing: issuer_id=0x{}, class_id=0x{}, nft_id=0x{}",
                hex::encode(&issuer_id),
                hex::encode(&class_id),
                hex::encode(&nft_id_bytes)
            ))
        })?;
    let (issuer_class_count, issuer_set_count, issuer_info) = match &issuer_entry.extra {
        ckbadger_store::types::NftExtra::MnftIssuer {
            class_count,
            set_count,
            info,
        } => (*class_count, *set_count, info.clone()),
        _ => {
            return Err(ApiError::internal(format!(
            "invalid issuer extra type for mNFT token: issuer_id=0x{}, class_id=0x{}, nft_id=0x{}",
            hex::encode(&issuer_id),
            hex::encode(&class_id),
            hex::encode(&nft_id_bytes)
        )))
        }
    };

    let live_outpoint = if entry.is_live {
        let map = resolve_live_mnft_outpoints(
            state.store.as_ref(),
            state.append_only_store.as_ref(),
            std::slice::from_ref(&nft_id_bytes),
        )?;
        let (tx_hash, output_index) = map.get(&nft_id_bytes).ok_or_else(|| {
            ApiError::internal(format!(
                "live mNFT token missing outpoint index: nft_id=0x{}",
                hex::encode(&nft_id_bytes)
            ))
        })?;
        Some((format!("0x{}", hex::encode(tx_hash)), *output_index))
    } else {
        None
    };

    let mut lifecycle = vec![MnftLifecycleEventResponse {
        event: "mint".to_string(),
        block_number: Some(entry.created_at_block),
        tx_hash: None,
        output_index: None,
        note: Some("Minted at the first observed block for this token.".to_string()),
    }];
    if let Some((tx_hash, output_index)) = &live_outpoint {
        lifecycle.push(MnftLifecycleEventResponse {
            event: "live".to_string(),
            block_number: None,
            tx_hash: Some(tx_hash.clone()),
            output_index: Some(*output_index),
            note: Some("Current live outpoint resolved from mNFT outpoint index.".to_string()),
        });
    } else {
        lifecycle.push(MnftLifecycleEventResponse {
            event: "burned".to_string(),
            block_number: None,
            tx_hash: None,
            output_index: None,
            note: Some("Token is currently not live.".to_string()),
        });
    }

    ok(MnftItemDetailResponse {
        nft_id: format!("0x{}", hex::encode(&nft_id_bytes)),
        standard: "m-nft".to_string(),
        is_live: entry.is_live,
        owner_lock_hash: entry
            .owner_lock_hash
            .as_ref()
            .map(|h| format!("0x{}", hex::encode(h))),
        created_at_block: entry.created_at_block,
        token_index,
        characteristic_hex: format!("0x{}", hex::encode(characteristic)),
        configure: token_configure,
        state: token_state,
        tx_hash: live_outpoint.as_ref().map(|(tx_hash, _)| tx_hash.clone()),
        output_index: live_outpoint
            .as_ref()
            .map(|(_, output_index)| *output_index),
        class: MnftClassSummaryResponse {
            class_id: format!("0x{}", hex::encode(&class_id)),
            issuer_id: format!("0x{}", hex::encode(&issuer_id)),
            name: class_entry.name,
            description: class_description,
            renderer: class_renderer,
            total: class_total,
            issued: class_issued,
            configure: class_configure,
        },
        issuer: MnftIssuerSummaryResponse {
            issuer_id: format!("0x{}", hex::encode(&issuer_id)),
            name: issuer_entry.name,
            class_count: issuer_class_count,
            set_count: issuer_set_count,
            info_hex: issuer_info.map(|v| format!("0x{}", hex::encode(v))),
        },
        lifecycle,
    })
}

#[derive(Debug, Clone, Copy)]
enum NftLifecycleStandard {
    MnftToken,
    DotBit,
    DidCkb,
}

fn collect_nft_item_lifecycle_actions(
    state: &Arc<AppState>,
    nft_id_bytes: &[u8],
    standard: NftLifecycleStandard,
) -> Result<Vec<(Vec<u8>, String)>, ApiRouteError> {
    let outpoints = match standard {
        NftLifecycleStandard::MnftToken => state
            .store
            .list_mnft_token_outpoints_by_token_id(nft_id_bytes)
            .map_err(|e| ApiError::internal(e.to_string()))?,
        NftLifecycleStandard::DotBit => state
            .store
            .list_dotbit_account_outpoints_by_account_id(nft_id_bytes)
            .map_err(|e| ApiError::internal(e.to_string()))?,
        NftLifecycleStandard::DidCkb => state
            .store
            .list_spore_outpoints_by_spore_id(nft_id_bytes)
            .map_err(|e| ApiError::internal(e.to_string()))?,
    };

    let mut created_txs: HashSet<Vec<u8>> = HashSet::new();
    let mut consumed_txs: HashSet<Vec<u8>> = HashSet::new();

    let outpoint_refs: Vec<(&[u8], i16)> = outpoints
        .iter()
        .map(|(tx_hash, output_index)| (tx_hash.as_slice(), *output_index))
        .collect();
    let consumed_meta = state
        .store
        .get_consumed_cell_meta_batch_with_payload_store(
            state.append_only_store.as_ref(),
            &outpoint_refs,
        )
        .map_err(|e| ApiError::internal(e.to_string()))?;

    for (tx_hash, output_index) in outpoints {
        created_txs.insert(tx_hash.clone());

        if let Some(meta) = consumed_meta.get(&(tx_hash.clone(), output_index)) {
            let consumed_by_tx = meta.consumed_by_tx.clone().ok_or_else(|| {
                ApiError::internal(format!(
                    "consumed nft outpoint missing consumer tx: nft_id=0x{}, tx_hash=0x{}, output_index={}",
                    hex::encode(nft_id_bytes),
                    hex::encode(&tx_hash),
                    output_index
                ))
            })?;
            consumed_txs.insert(consumed_by_tx);
        }
    }

    let mut lifecycle_txs = created_txs.clone();
    lifecycle_txs.extend(consumed_txs.iter().cloned());

    let mut rows = Vec::with_capacity(lifecycle_txs.len());
    for tx_hash in lifecycle_txs {
        let action = match (
            created_txs.contains(&tx_hash),
            consumed_txs.contains(&tx_hash),
        ) {
            (true, true) => "transfer",
            (true, false) => "mint",
            (false, true) => "burn",
            (false, false) => {
                return Err(ApiError::internal(format!(
                    "invalid nft lifecycle action state: nft_id=0x{}, tx_hash=0x{}",
                    hex::encode(nft_id_bytes),
                    hex::encode(&tx_hash)
                )))
            }
        };
        rows.push((tx_hash, action.to_string()));
    }

    Ok(rows)
}

fn build_nft_item_activities_response(
    state: &Arc<AppState>,
    nft_id_bytes: &[u8],
    standard: ckbadger_store::types::NftStandard,
    limit: i64,
    cursor: Option<(i64, i32)>,
    action_filter: Option<&str>,
) -> Result<CursorPaginatedResponse<MnftItemActivityResponse>, ApiRouteError> {
    ensure_derived_ready(state.as_ref())?;
    let lifecycle_standard = match standard {
        ckbadger_store::types::NftStandard::MnftToken => NftLifecycleStandard::MnftToken,
        ckbadger_store::types::NftStandard::DotBit => NftLifecycleStandard::DotBit,
        ckbadger_store::types::NftStandard::DidCkb => NftLifecycleStandard::DidCkb,
        _ => {
            return Err(ApiError::internal(format!(
                "unsupported nft standard for item activities: standard={}",
                standard.asset_standard()
            )))
        }
    };
    let lifecycle_rows =
        collect_nft_item_lifecycle_actions(state, nft_id_bytes, lifecycle_standard)?;
    let tx_hashes: Vec<Vec<u8>> = lifecycle_rows
        .iter()
        .map(|(tx_hash, _)| tx_hash.clone())
        .collect();
    let tx_batch_rows = state
        .store
        .get_txs_by_hash_batch(&tx_hashes)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut tx_rows_by_hash: HashMap<Vec<u8>, (i64, i32, ckbadger_store::types::TxIndexEntry)> =
        HashMap::with_capacity(tx_batch_rows.len());
    for (tx_hash, tx_row_opt) in tx_batch_rows {
        let tx_row = tx_row_opt.ok_or_else(|| {
            ApiError::internal(format!(
                "nft lifecycle tx not found in tx index: nft_id=0x{}, tx_hash=0x{}",
                hex::encode(nft_id_bytes),
                hex::encode(&tx_hash)
            ))
        })?;
        tx_rows_by_hash.insert(tx_hash, tx_row);
    }

    let mut rows = Vec::with_capacity(lifecycle_rows.len());
    for (tx_hash, action) in lifecycle_rows {
        if action_filter
            .map(|filter| filter != action)
            .unwrap_or(false)
        {
            continue;
        }
        let (block_number, tx_index, tx_entry) =
            tx_rows_by_hash.get(&tx_hash).cloned().ok_or_else(|| {
                ApiError::internal(format!(
                    "nft lifecycle tx lookup missing from batch result: nft_id=0x{}, tx_hash=0x{}",
                    hex::encode(nft_id_bytes),
                    hex::encode(&tx_hash)
                ))
            })?;

        rows.push(MnftItemActivityResponse {
            tx_hash: format!("0x{}", hex::encode(&tx_hash)),
            block_number,
            tx_index,
            timestamp: tx_entry.timestamp.to_string(),
            actions: vec![action],
        });
    }

    rows.sort_by(|a, b| {
        b.block_number
            .cmp(&a.block_number)
            .then_with(|| b.tx_index.cmp(&a.tx_index))
            .then_with(|| b.tx_hash.cmp(&a.tx_hash))
    });

    if let Some((cursor_block, cursor_tx_index)) = cursor {
        rows.retain(|row| {
            row.block_number < cursor_block
                || (row.block_number == cursor_block && row.tx_index < cursor_tx_index)
        });
    }

    let has_more = rows.len() as i64 > limit;
    let page: Vec<_> = rows.into_iter().take(limit as usize).collect();
    let next_cursor = if has_more {
        page.last()
            .map(|row| format!("{}:{}", row.block_number, row.tx_index))
    } else {
        None
    };

    Ok(CursorPaginatedResponse::without_total(
        page,
        limit,
        next_cursor,
    ))
}

async fn list_mnft_item_activities(
    State(state): State<Arc<AppState>>,
    Path(nft_id): Path<String>,
    Query(params): Query<MnftItemActivitiesParams>,
) -> ApiResult<CursorPaginatedResponse<MnftItemActivityResponse>> {
    let limit = params.limit.clamp(1, 100);
    let action_filter = normalize_nft_item_activity_action_filter(params.action.as_deref())?;
    let nft_id_bytes = decode_nft_item_id(&nft_id)?;
    let entry = state
        .store
        .get_nft(&nft_id_bytes)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("NFT item not found"))?;
    if !matches!(
        entry.standard,
        ckbadger_store::types::NftStandard::MnftToken
    ) {
        return Err(ApiError::bad_request(
            "NFT item activities currently support mNFT token only",
        ));
    }

    let cursor = params
        .cursor
        .as_deref()
        .map(decode_activity_cursor)
        .transpose()?;
    let response = build_nft_item_activities_response(
        &state,
        &nft_id_bytes,
        entry.standard,
        limit,
        cursor,
        action_filter.as_deref(),
    )?;
    ok(response)
}

async fn list_dotbit_item_activities(
    State(state): State<Arc<AppState>>,
    Path(nft_id): Path<String>,
    Query(params): Query<MnftItemActivitiesParams>,
) -> ApiResult<CursorPaginatedResponse<MnftItemActivityResponse>> {
    let limit = params.limit.clamp(1, 100);
    let action_filter = normalize_nft_item_activity_action_filter(params.action.as_deref())?;
    let nft_id_bytes = decode_nft_item_id(&nft_id)?;
    let entry = state
        .store
        .get_nft(&nft_id_bytes)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found(".bit item not found"))?;
    if !matches!(entry.standard, ckbadger_store::types::NftStandard::DotBit) {
        return Err(ApiError::bad_request("NFT item is not a .bit account"));
    }

    let cursor = params
        .cursor
        .as_deref()
        .map(decode_activity_cursor)
        .transpose()?;
    let response = build_nft_item_activities_response(
        &state,
        &nft_id_bytes,
        entry.standard,
        limit,
        cursor,
        action_filter.as_deref(),
    )?;
    ok(response)
}

async fn list_did_ckb_item_activities(
    State(state): State<Arc<AppState>>,
    Path(nft_id): Path<String>,
    Query(params): Query<MnftItemActivitiesParams>,
) -> ApiResult<CursorPaginatedResponse<MnftItemActivityResponse>> {
    let limit = params.limit.clamp(1, 100);
    let action_filter = normalize_nft_item_activity_action_filter(params.action.as_deref())?;
    let nft_id_bytes = decode_nft_item_id(&nft_id)?;
    let entry = state
        .store
        .get_spore(&nft_id_bytes)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("did:ckb item not found"))?;
    if entry.standard != ckbadger_store::types::DobStandard::DidCkb {
        return Err(ApiError::bad_request("NFT item is not a did:ckb identity"));
    }

    let cursor = params
        .cursor
        .as_deref()
        .map(decode_activity_cursor)
        .transpose()?;
    let response = build_nft_item_activities_response(
        &state,
        &nft_id_bytes,
        ckbadger_store::types::NftStandard::DidCkb,
        limit,
        cursor,
        action_filter.as_deref(),
    )?;
    ok(response)
}

async fn get_nft_collection(
    State(state): State<Arc<AppState>>,
    Path(collection_id): Path<String>,
) -> ApiResult<NftCollectionDetailResponse> {
    ensure_derived_ready(state.as_ref())?;

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
    let storage_profile = CollectionStorageProfileResponse {
        tier: "unknown".to_string(),
        fully_onchain_count: 0,
        decentralized_external_count: 0,
        centralized_dependent_count: 0,
        unknown_count: agg.live_count,
        fully_onchain_ratio: format_ratio_4(0, agg.live_count),
    };

    if agg.holders_count < 0 {
        return Err(ApiError::internal(format!(
            "invalid nft collection aggregate holders_count: collection_id=0x{}, holders_count={}",
            hex::encode(&collection_id_bytes),
            agg.holders_count
        )));
    }
    if agg.activities_count < 0 {
        return Err(ApiError::internal(format!(
            "invalid nft collection aggregate activities_count: collection_id=0x{}, activities_count={}",
            hex::encode(&collection_id_bytes),
            agg.activities_count
        )));
    }
    let holders_count = agg.holders_count;
    let activities_count = agg.activities_count;

    ok(NftCollectionDetailResponse {
        collection_id: format!("0x{}", hex::encode(&collection_id_bytes)),
        standard,
        name,
        total_count: agg.total_count,
        live_count: agg.live_count,
        holders_count,
        activities_count,
        live_capacity,
        live_occupied_capacity,
        storage_profile,
    })
}

fn fetch_did_collection_entries_by_ids(
    store: &CkbadgerStore,
    collection_id_bytes: &[u8],
    nft_ids: &[Vec<u8>],
) -> Result<Vec<(Vec<u8>, ckbadger_store::types::DobEntry)>, ApiRouteError> {
    let entries = store
        .get_spores_batch(nft_ids)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut out = Vec::with_capacity(entries.len());

    for (nft_id, entry_opt) in entries {
        let entry = entry_opt.ok_or_else(|| {
            ApiError::internal(format!(
                "nft_by_collection index points to missing spore_data did:ckb entry: collection_id=0x{}, nft_id=0x{}",
                hex::encode(collection_id_bytes),
                hex::encode(&nft_id)
            ))
        })?;
        if entry.standard != ckbadger_store::types::DobStandard::DidCkb {
            return Err(ApiError::internal(format!(
                "did:ckb collection index mismatch: collection_id=0x{}, nft_id=0x{}, entry_standard={}",
                hex::encode(collection_id_bytes),
                hex::encode(&nft_id),
                entry.standard.as_str()
            )));
        }
        out.push((nft_id, entry));
    }

    Ok(out)
}

fn fetch_nft_collection_entries_by_ids(
    store: &CkbadgerStore,
    collection_id_bytes: &[u8],
    nft_ids: &[Vec<u8>],
) -> Result<Vec<(Vec<u8>, ckbadger_store::types::NftEntry)>, ApiRouteError> {
    let entries = store
        .get_nfts_batch(nft_ids)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut out = Vec::with_capacity(entries.len());

    for (nft_id, entry_opt) in entries {
        let entry = entry_opt.ok_or_else(|| {
            ApiError::internal(format!(
                "nft_by_collection index points to missing nft_data entry: collection_id=0x{}, nft_id=0x{}",
                hex::encode(collection_id_bytes),
                hex::encode(&nft_id)
            ))
        })?;
        if !nft_entry_matches_collection(collection_id_bytes, &entry) {
            return Err(ApiError::internal(format!(
                "nft_by_collection index mismatch: collection_id=0x{}, nft_id=0x{}, entry_standard={}, entry_collection_id={}",
                hex::encode(collection_id_bytes),
                hex::encode(&nft_id),
                entry.standard.as_str(),
                entry
                    .collection_id
                    .as_ref()
                    .map(|id| format!("0x{}", hex::encode(id)))
                    .unwrap_or_else(|| "null".to_string())
            )));
        }
        out.push((nft_id, entry));
    }

    Ok(out)
}

fn resolve_live_dotbit_outpoints(
    store: &CkbadgerStore,
    append_only_store: &CkbadgerStore,
    account_ids: &[Vec<u8>],
) -> Result<LiveOutpointMap, ApiRouteError> {
    store
        .get_live_dotbit_outpoints_by_account_ids_with_payload_store(append_only_store, account_ids)
        .map_err(|e| ApiError::internal(e.to_string()))
}

fn resolve_live_mnft_outpoints(
    store: &CkbadgerStore,
    append_only_store: &CkbadgerStore,
    token_ids: &[Vec<u8>],
) -> Result<LiveOutpointMap, ApiRouteError> {
    store
        .get_live_mnft_token_outpoints_by_token_ids_with_payload_store(append_only_store, token_ids)
        .map_err(|e| ApiError::internal(e.to_string()))
}

async fn list_nft_collection_items(
    State(state): State<Arc<AppState>>,
    Path(collection_id): Path<String>,
    Query(params): Query<NftItemsParams>,
) -> ApiResult<CursorPaginatedResponse<NftCollectionItemResponse>> {
    let limit = params.limit.clamp(1, 100);
    let collection_id_bytes = decode_nft_collection_id(&collection_id)?;
    let search_lower = normalize_nft_items_search(params.search.as_deref());
    let status_filter = normalize_nft_items_status(params.status.as_deref())?;
    let cursor_bytes = params
        .cursor
        .as_deref()
        .map(decode_nft_item_cursor)
        .transpose()?;

    let agg = state
        .store
        .get_nft_collection_aggregate(&collection_id_bytes)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let agg = agg.ok_or_else(|| ApiError::not_found("NFT collection not found"))?;

    if matches!(agg.standard, ckbadger_store::types::NftStandard::DidCkb) {
        if collection_id_bytes != DID_CKB_SENTINEL_COLLECTION {
            return Err(ApiError::internal(format!(
                "did:ckb collection id mismatch: expected=0x{}, got=0x{}",
                hex::encode(DID_CKB_SENTINEL_COLLECTION),
                hex::encode(&collection_id_bytes)
            )));
        }

        let mut matched_items: Vec<(Vec<u8>, ckbadger_store::types::DobEntry)> =
            Vec::with_capacity((limit + 1) as usize);

        if search_lower.is_some() || !matches!(status_filter, NftItemStatusFilter::All) {
            let scan_batch_size = (limit * 4).clamp(64, 400) as usize;
            let mut scan_cursor = cursor_bytes;

            loop {
                let nft_ids = state
                    .store
                    .list_nft_ids_by_collection(
                        &collection_id_bytes,
                        scan_cursor.as_deref(),
                        scan_batch_size,
                    )
                    .map_err(|e| ApiError::internal(e.to_string()))?;

                if nft_ids.is_empty() {
                    break;
                }

                for (nft_id, entry) in fetch_did_collection_entries_by_ids(
                    state.store.as_ref(),
                    &collection_id_bytes,
                    &nft_ids,
                )? {
                    let status_match = match status_filter {
                        NftItemStatusFilter::All => true,
                        NftItemStatusFilter::Live => entry.is_live,
                        NftItemStatusFilter::Recycled => !entry.is_live,
                    };
                    if !status_match {
                        continue;
                    }

                    if let Some(search) = search_lower.as_deref() {
                        let name_match = entry
                            .name
                            .as_deref()
                            .map(|name| name.to_ascii_lowercase().contains(search))
                            .unwrap_or(false);
                        let nft_id_hex = hex::encode(&nft_id);
                        let id_match = nft_id_hex.contains(search)
                            || format!("0x{nft_id_hex}").contains(search);
                        if !name_match && !id_match {
                            continue;
                        }
                    }

                    matched_items.push((nft_id, entry));
                    if matched_items.len() > limit as usize {
                        break;
                    }
                }

                if matched_items.len() > limit as usize || nft_ids.len() < scan_batch_size {
                    break;
                }

                scan_cursor = nft_ids.last().cloned();
            }
        } else {
            let nft_ids = state
                .store
                .list_nft_ids_by_collection(
                    &collection_id_bytes,
                    cursor_bytes.as_deref(),
                    (limit + 1) as usize,
                )
                .map_err(|e| ApiError::internal(e.to_string()))?;

            matched_items.extend(fetch_did_collection_entries_by_ids(
                state.store.as_ref(),
                &collection_id_bytes,
                &nft_ids,
            )?);
        }

        let has_more = matched_items.len() as i64 > limit;
        let page_items: Vec<(Vec<u8>, ckbadger_store::types::DobEntry)> =
            matched_items.into_iter().take(limit as usize).collect();

        let rows = page_items
            .iter()
            .map(|(nft_id, entry)| NftCollectionItemResponse {
                nft_id: format!("0x{}", hex::encode(nft_id)),
                name: entry.name.clone(),
                standard: "did_ckb".to_string(),
                owner_lock_hash: entry
                    .owner_lock_hash
                    .as_ref()
                    .map(|h| format!("0x{}", hex::encode(h))),
                is_live: entry.is_live,
                created_at_block: entry.created_at_block,
                expired_at: None,
                registered_at: None,
                status: None,
                tx_hash: None,
                output_index: None,
            })
            .collect();

        let next_cursor = if has_more {
            page_items
                .last()
                .map(|(id, _)| format!("0x{}", hex::encode(id)))
        } else {
            None
        };

        return ok(CursorPaginatedResponse::new(
            rows,
            agg.total_count,
            limit,
            next_cursor,
        ));
    }

    let mut matched_items: Vec<(Vec<u8>, ckbadger_store::types::NftEntry)> =
        Vec::with_capacity((limit + 1) as usize);

    if search_lower.is_some() || !matches!(status_filter, NftItemStatusFilter::All) {
        let scan_batch_size = (limit * 4).clamp(64, 400) as usize;
        let mut scan_cursor = cursor_bytes;

        loop {
            let nft_ids = state
                .store
                .list_nft_ids_by_collection(
                    &collection_id_bytes,
                    scan_cursor.as_deref(),
                    scan_batch_size,
                )
                .map_err(|e| ApiError::internal(e.to_string()))?;

            if nft_ids.is_empty() {
                break;
            }

            for (nft_id, entry) in fetch_nft_collection_entries_by_ids(
                state.store.as_ref(),
                &collection_id_bytes,
                &nft_ids,
            )? {
                if !nft_item_matches_status(status_filter, &entry) {
                    continue;
                }

                if !nft_item_matches_search(search_lower.as_deref(), &nft_id, &entry) {
                    continue;
                }

                matched_items.push((nft_id, entry));
                if matched_items.len() > limit as usize {
                    break;
                }
            }

            if matched_items.len() > limit as usize || nft_ids.len() < scan_batch_size {
                break;
            }

            scan_cursor = nft_ids.last().cloned();
        }
    } else {
        let nft_ids = state
            .store
            .list_nft_ids_by_collection(
                &collection_id_bytes,
                cursor_bytes.as_deref(),
                (limit + 1) as usize,
            )
            .map_err(|e| ApiError::internal(e.to_string()))?;

        matched_items.extend(fetch_nft_collection_entries_by_ids(
            state.store.as_ref(),
            &collection_id_bytes,
            &nft_ids,
        )?);
    }

    let has_more = matched_items.len() as i64 > limit;
    let page_items: Vec<(Vec<u8>, ckbadger_store::types::NftEntry)> =
        matched_items.into_iter().take(limit as usize).collect();

    let dotbit_live_account_ids: Vec<Vec<u8>> = page_items
        .iter()
        .filter_map(|(nft_id, entry)| {
            (matches!(entry.standard, ckbadger_store::types::NftStandard::DotBit) && entry.is_live)
                .then_some(nft_id.clone())
        })
        .collect();
    let mnft_live_token_ids: Vec<Vec<u8>> = page_items
        .iter()
        .filter_map(|(nft_id, entry)| {
            (matches!(
                entry.standard,
                ckbadger_store::types::NftStandard::MnftToken
            ) && entry.is_live)
                .then_some(nft_id.clone())
        })
        .collect();
    let dotbit_live_outpoints = resolve_live_dotbit_outpoints(
        state.store.as_ref(),
        state.append_only_store.as_ref(),
        &dotbit_live_account_ids,
    )?;
    let mnft_live_outpoints = resolve_live_mnft_outpoints(
        state.store.as_ref(),
        state.append_only_store.as_ref(),
        &mnft_live_token_ids,
    )?;

    let mut rows = Vec::with_capacity(page_items.len());
    for (nft_id, entry) in &page_items {
        let (expired_at, registered_at, status) = match &entry.extra {
            ckbadger_store::types::NftExtra::DotBit {
                expired_at,
                registered_at,
                status,
            } => (*expired_at, *registered_at, *status),
            _ => (None, None, None),
        };

        let (tx_hash, output_index) = match entry.standard {
            ckbadger_store::types::NftStandard::DotBit => {
                if entry.is_live {
                    let (tx_hash, output_index) = dotbit_live_outpoints.get(nft_id).ok_or_else(|| {
                        ApiError::internal(format!(
                            "live dotbit account missing outpoint index: collection_id=0x{}, nft_id=0x{}",
                            hex::encode(&collection_id_bytes),
                            hex::encode(nft_id)
                        ))
                    })?;
                    (
                        Some(format!("0x{}", hex::encode(tx_hash))),
                        Some(*output_index),
                    )
                } else {
                    (None, None)
                }
            }
            ckbadger_store::types::NftStandard::MnftToken => {
                if entry.is_live {
                    let (tx_hash, output_index) = mnft_live_outpoints.get(nft_id).ok_or_else(|| {
                        ApiError::internal(format!(
                            "live mNFT token missing outpoint index: collection_id=0x{}, nft_id=0x{}",
                            hex::encode(&collection_id_bytes),
                            hex::encode(nft_id)
                        ))
                    })?;
                    (
                        Some(format!("0x{}", hex::encode(tx_hash))),
                        Some(*output_index),
                    )
                } else {
                    (None, None)
                }
            }
            _ => (None, None),
        };

        rows.push(NftCollectionItemResponse {
            nft_id: format!("0x{}", hex::encode(nft_id)),
            name: entry.name.clone(),
            standard: entry.standard.asset_standard().to_string(),
            owner_lock_hash: entry
                .owner_lock_hash
                .as_ref()
                .map(|h| format!("0x{}", hex::encode(h))),
            is_live: entry.is_live,
            created_at_block: entry.created_at_block,
            expired_at,
            registered_at,
            status,
            tx_hash,
            output_index,
        });
    }

    let next_cursor = if has_more {
        page_items
            .last()
            .map(|(id, _)| format!("0x{}", hex::encode(id)))
    } else {
        None
    };

    if search_lower.is_some() {
        ok(CursorPaginatedResponse::without_total(
            rows,
            limit,
            next_cursor,
        ))
    } else {
        let total = match status_filter {
            NftItemStatusFilter::All => agg.total_count,
            NftItemStatusFilter::Live => agg.live_count,
            NftItemStatusFilter::Recycled => {
                agg.total_count
                    .checked_sub(agg.live_count)
                    .ok_or_else(|| {
                        ApiError::internal(format!(
                            "invalid nft collection aggregate counts: collection_id=0x{}, total_count={}, live_count={}",
                            hex::encode(&collection_id_bytes),
                            agg.total_count,
                            agg.live_count
                        ))
                    })?
            }
        };
        ok(CursorPaginatedResponse::new(
            rows,
            total,
            limit,
            next_cursor,
        ))
    }
}

fn collect_nft_collection_holder_counts(
    store: &CkbadgerStore,
    collection_id_bytes: &[u8],
) -> Result<HashMap<Vec<u8>, i64>, ApiRouteError> {
    let rows = store
        .list_nft_collection_owner_counts(collection_id_bytes)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut holder_counts: HashMap<Vec<u8>, i64> = HashMap::with_capacity(rows.len());
    for (lock_hash, count) in rows {
        if count <= 0 {
            continue;
        }
        if holder_counts.insert(lock_hash.clone(), count).is_some() {
            return Err(ApiError::internal(format!(
                "duplicate nft collection owner counter key while collecting holders: collection_id=0x{}, lock_hash=0x{}",
                hex::encode(collection_id_bytes),
                hex::encode(&lock_hash)
            )));
        }
    }

    Ok(holder_counts)
}

fn list_nft_collection_holders_ranked(
    store: &CkbadgerStore,
    collection_id_bytes: &[u8],
    _agg: &ckbadger_store::types::NftCollectionAggregate,
) -> Result<Vec<(Vec<u8>, i64)>, ApiRouteError> {
    let mut holders: Vec<(Vec<u8>, i64)> =
        collect_nft_collection_holder_counts(store, collection_id_bytes)?
            .into_iter()
            .collect();
    holders.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    Ok(holders)
}

fn list_nft_collection_holders_ranked_cached(
    store: &CkbadgerStore,
    mem_cache: &InMemoryCache,
    collection_id_bytes: &[u8],
    agg: &ckbadger_store::types::NftCollectionAggregate,
) -> Result<Vec<(Vec<u8>, i64)>, ApiRouteError> {
    let cache_key = format!(
        "assets:nft_collection_holders_ranked:0x{}",
        hex::encode(collection_id_bytes)
    );
    if let Some(cached) = mem_cache.get::<Vec<(Vec<u8>, i64)>>(&cache_key) {
        return Ok(cached);
    }

    let holders = list_nft_collection_holders_ranked(store, collection_id_bytes, agg)?;
    mem_cache.set(&cache_key, &holders, NFT_HOLDER_LIST_CACHE_TTL);
    Ok(holders)
}

async fn list_nft_collection_holders(
    State(state): State<Arc<AppState>>,
    Path(collection_id): Path<String>,
    Query(params): Query<NftCollectionHoldersParams>,
) -> ApiResult<CursorPaginatedResponse<NftCollectionHolderResponse>> {
    ensure_derived_ready(state.as_ref())?;

    let limit = params.limit.clamp(1, 100) as usize;
    let collection_id_bytes = decode_nft_collection_id(&collection_id)?;
    let cursor = params
        .cursor
        .as_deref()
        .map(decode_nft_collection_holders_cursor)
        .transpose()?;

    let agg = state
        .store
        .get_nft_collection_aggregate(&collection_id_bytes)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("NFT collection not found"))?;
    if agg.holders_count < 0 {
        return Err(ApiError::internal(format!(
            "invalid nft collection aggregate holders_count: collection_id=0x{}, holders_count={}",
            hex::encode(&collection_id_bytes),
            agg.holders_count
        )));
    }

    let holders = list_nft_collection_holders_ranked_cached(
        state.store.as_ref(),
        &state.mem_cache,
        &collection_id_bytes,
        &agg,
    )?;

    let total = agg.holders_count;
    let start_idx = if let Some((cursor_count, cursor_lock_hash)) = cursor {
        holders
            .iter()
            .position(|(lock_hash, count)| *count == cursor_count && *lock_hash == cursor_lock_hash)
            .map(|idx| idx + 1)
            .ok_or_else(|| ApiError::bad_request("Invalid NFT collection holders cursor"))?
    } else {
        0
    };

    let page: Vec<_> = holders.iter().skip(start_idx).take(limit + 1).collect();
    let has_more = page.len() > limit;
    let page: Vec<_> = page.into_iter().take(limit).collect();

    let next_cursor = if has_more {
        page.last()
            .map(|(lock_hash, count)| format!("{}:{}", count, hex::encode(lock_hash)))
    } else {
        None
    };

    let rows: Vec<NftCollectionHolderResponse> = page
        .into_iter()
        .map(|(lock_hash, count)| NftCollectionHolderResponse {
            lock_script_hash: format!("0x{}", hex::encode(lock_hash)),
            address: None,
            item_count: *count,
        })
        .collect();

    ok(CursorPaginatedResponse::new(
        rows,
        total,
        limit as i64,
        next_cursor,
    ))
}

async fn list_nft_collection_activities(
    State(state): State<Arc<AppState>>,
    Path(collection_id): Path<String>,
    Query(params): Query<NftCollectionActivitiesParams>,
) -> ApiResult<CursorPaginatedResponse<NftCollectionActivityResponse>> {
    ensure_derived_ready(state.as_ref())?;

    let limit = params.limit.clamp(1, 100);
    let collection_id_bytes = decode_nft_collection_id(&collection_id)?;
    let cursor = params
        .cursor
        .as_deref()
        .map(decode_activity_cursor)
        .transpose()?;
    let action_filter = normalize_nft_collection_activity_action_filter(params.action.as_deref())?;

    // Validate collection exists
    let _agg = state
        .store
        .get_nft_collection_aggregate(&collection_id_bytes)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("NFT collection not found"))?;

    // Fetch canonical rows only; skip orphaned append-only history entries.
    let results = list_canonical_nft_collection_activities_page(
        state.store.as_ref(),
        state.append_only_store.as_ref(),
        &collection_id_bytes,
        (limit as usize) + 1,
        cursor,
        action_filter.as_deref(),
    )
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let has_more = results.len() as i64 > limit;
    let page: Vec<NftCollectionActivityResponse> = results
        .into_iter()
        .take(limit as usize)
        .map(|(block_number, tx_index, entry)| {
            let actions: Vec<String> = entry
                .actions
                .iter()
                .map(|a| match a {
                    ckbadger_store::AssetAction::Mint => "mint".to_string(),
                    ckbadger_store::AssetAction::Transfer => "transfer".to_string(),
                    ckbadger_store::AssetAction::Burn => "burn".to_string(),
                    ckbadger_store::AssetAction::Recycle => "recycle".to_string(),
                    ckbadger_store::AssetAction::Renew => "renew".to_string(),
                    ckbadger_store::AssetAction::Update => "update".to_string(),
                })
                .collect();
            NftCollectionActivityResponse {
                tx_hash: format!("0x{}", hex::encode(&entry.tx_hash)),
                block_number,
                tx_index,
                timestamp: entry.timestamp_ms.to_string(),
                actions,
            }
        })
        .collect();

    let next_cursor = if has_more {
        page.last()
            .map(|row| format!("{}:{}", row.block_number, row.tx_index))
    } else {
        None
    };

    ok(CursorPaginatedResponse::without_total(
        page,
        limit,
        next_cursor,
    ))
}

async fn get_dotbit_item_detail(
    State(state): State<Arc<AppState>>,
    Path(nft_id): Path<String>,
) -> ApiResult<NftCollectionItemResponse> {
    let nft_id_bytes = decode_nft_item_id(&nft_id)?;
    let entry = state
        .store
        .get_nft(&nft_id_bytes)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found(".bit item not found"))?;

    if !matches!(entry.standard, ckbadger_store::types::NftStandard::DotBit) {
        return Err(ApiError::bad_request("NFT item is not a .bit account"));
    }

    let (expired_at, registered_at, status) = match &entry.extra {
        ckbadger_store::types::NftExtra::DotBit {
            expired_at,
            registered_at,
            status,
        } => (*expired_at, *registered_at, *status),
        _ => {
            return Err(ApiError::internal(format!(
                "invalid NFT entry extra type for .bit account: nft_id=0x{}",
                hex::encode(&nft_id_bytes)
            )))
        }
    };

    let (tx_hash, output_index) = if entry.is_live {
        let outpoint_map = resolve_live_dotbit_outpoints(
            state.store.as_ref(),
            state.append_only_store.as_ref(),
            std::slice::from_ref(&nft_id_bytes),
        )?;
        let (tx_hash, output_index) = outpoint_map.get(&nft_id_bytes).ok_or_else(|| {
            ApiError::internal(format!(
                "live dotbit account missing outpoint index: nft_id=0x{}",
                hex::encode(&nft_id_bytes)
            ))
        })?;
        (
            Some(format!("0x{}", hex::encode(tx_hash))),
            Some(*output_index),
        )
    } else {
        (None, None)
    };

    ok(NftCollectionItemResponse {
        nft_id: format!("0x{}", hex::encode(&nft_id_bytes)),
        name: entry.name,
        standard: entry.standard.asset_standard().to_string(),
        owner_lock_hash: entry
            .owner_lock_hash
            .as_ref()
            .map(|h| format!("0x{}", hex::encode(h))),
        is_live: entry.is_live,
        created_at_block: entry.created_at_block,
        expired_at,
        registered_at,
        status,
        tx_hash,
        output_index,
    })
}

async fn get_did_ckb_item_detail(
    State(state): State<Arc<AppState>>,
    Path(nft_id): Path<String>,
) -> ApiResult<NftCollectionItemResponse> {
    let nft_id_bytes = decode_nft_item_id(&nft_id)?;
    let entry = state
        .store
        .get_spore(&nft_id_bytes)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("did:ckb item not found"))?;

    if entry.standard != ckbadger_store::types::DobStandard::DidCkb {
        return Err(ApiError::bad_request("NFT item is not a did:ckb identity"));
    }

    ok(NftCollectionItemResponse {
        nft_id: format!("0x{}", hex::encode(&nft_id_bytes)),
        name: entry.name,
        standard: "did_ckb".to_string(),
        owner_lock_hash: entry
            .owner_lock_hash
            .as_ref()
            .map(|h| format!("0x{}", hex::encode(h))),
        is_live: entry.is_live,
        created_at_block: entry.created_at_block,
        expired_at: None,
        registered_at: None,
        status: None,
        tx_hash: None,
        output_index: None,
    })
}

async fn get_nft_collection_occupation_chart(
    State(state): State<Arc<AppState>>,
    Path(collection_id): Path<String>,
    Query(params): Query<ChartRangeParams>,
) -> ApiResult<StackedAreaChartResponse> {
    ensure_derived_ready(state.as_ref())?;

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
        from_date,
        to_date,
    )
    .map_err(|e| ApiError::internal(e.to_string()))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ckbadger_store::batch::StoreBatch;
    use ckbadger_store::types::{
        AssetAction, LiveCellInfo, NftCollectionAggregate, NftEntry, NftExtra, NftStandard,
        TxIndexEntry,
    };

    fn make_collection_activity(
        tx_hash: &[u8],
        timestamp_ms: i64,
        action: AssetAction,
    ) -> NftCollectionActivityEntry {
        NftCollectionActivityEntry {
            tx_hash: tx_hash.to_vec(),
            timestamp_ms,
            actions: vec![action],
        }
    }

    #[test]
    fn test_parse_asset_cursor_accepts_only_current_format() {
        assert_eq!(
            parse_asset_cursor("token:0xabc"),
            Some(("token".to_string(), "0xabc".to_string()))
        );
        assert_eq!(
            parse_asset_cursor("nft:0xdef"),
            Some(("nft".to_string(), "0xdef".to_string()))
        );
        assert_eq!(parse_asset_cursor("dob:0xdef"), None);
        assert_eq!(parse_asset_cursor("1:2:3:nft"), None);
    }

    #[test]
    fn test_collection_activity_helpers_filter_orphaned_append_history() {
        let root = tempfile::tempdir().unwrap();
        let domain = CkbadgerStore::open_domain(root.path().join("domain")).unwrap();
        let append = CkbadgerStore::open_append_only(root.path().join("append")).unwrap();
        let collection_id = [0xAB; 32];

        let stale_tx = vec![0x30; 32];
        let canonical_tx_new = vec![0x20; 32];
        let canonical_tx_old = vec![0x10; 32];

        let mut append_batch = StoreBatch::new(&append);
        append_batch.put_nft_collection_activity(
            &collection_id,
            30,
            0,
            &make_collection_activity(&stale_tx, 1_700_000_030_000, AssetAction::Transfer),
        );
        append_batch.put_nft_collection_activity(
            &collection_id,
            20,
            0,
            &make_collection_activity(&canonical_tx_new, 1_700_000_020_000, AssetAction::Transfer),
        );
        append_batch.put_nft_collection_activity(
            &collection_id,
            10,
            0,
            &make_collection_activity(&canonical_tx_old, 1_700_000_010_000, AssetAction::Mint),
        );
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
        domain_batch.put_tx_hash_map(&stale_tx, 30, 0);
        domain_batch.put_tx_hash_map(&canonical_tx_new, 20, 0);
        domain_batch.put_tx_hash_map(&canonical_tx_old, 10, 0);
        domain_batch.put_tx_index(20, 0, &tx_index);
        domain_batch.put_tx_index(10, 0, &tx_index);
        domain_batch.commit().unwrap();

        let listed = list_canonical_nft_collection_activities_page(
            &domain,
            &append,
            &collection_id,
            3,
            None,
            None,
        )
        .unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].0, 20);
        assert_eq!(listed[1].0, 10);
        assert_eq!(listed[0].2.tx_hash, canonical_tx_new);
        assert_eq!(listed[1].2.tx_hash, canonical_tx_old);

        let count =
            count_canonical_nft_collection_activities(&domain, &append, &collection_id).unwrap();
        assert_eq!(count, 2);

        let mem_cache = InMemoryCache::new();
        let canonical_cached =
            count_nft_collection_activities_cached(&domain, &append, &mem_cache, &collection_id)
                .unwrap();
        assert_eq!(canonical_cached, 2);

        let mut append_batch = StoreBatch::new(&append);
        append_batch.put_nft_collection_activity(
            &collection_id,
            40,
            0,
            &make_collection_activity(&[0x40; 32], 1_700_000_040_000, AssetAction::Mint),
        );
        append_batch.commit().unwrap();
        let cached_count =
            count_nft_collection_activities_cached(&domain, &append, &mem_cache, &collection_id)
                .unwrap();
        assert_eq!(cached_count, 2);
    }

    #[test]
    fn test_list_nft_collection_holders_ranked_cached_uses_ttl_cache() {
        let root = tempfile::tempdir().unwrap();
        let domain = CkbadgerStore::open_domain(root.path().join("domain")).unwrap();
        let collection_id = [0xBD; 32];

        let aggregate = ckbadger_store::types::NftCollectionAggregate {
            name: Some("sample".to_string()),
            standard: ckbadger_store::types::NftStandard::MnftClass,
            total_count: 2,
            live_count: 2,
            holders_count: 2,
            activities_count: 0,
        };

        let mut batch = StoreBatch::new(&domain);
        batch.put_nft_collection_aggregate(&collection_id, &aggregate);
        batch.put_nft_collection_owner_count(&collection_id, &[0xA1; 32], 1);
        batch.put_nft_collection_owner_count(&collection_id, &[0xA2; 32], 1);
        batch.commit().unwrap();

        let mem_cache = InMemoryCache::new();
        let first = list_nft_collection_holders_ranked_cached(
            &domain,
            &mem_cache,
            &collection_id,
            &aggregate,
        )
        .unwrap();
        assert_eq!(first.len(), 2);
        assert_eq!(
            first
                .iter()
                .find(|(lock_hash, _)| lock_hash == &[0xA1; 32])
                .unwrap()
                .1,
            1
        );

        // Add another live NFT for holder A1; cached ranking should remain stale until TTL refresh.
        let mut overwrite_batch = StoreBatch::new(&domain);
        overwrite_batch.put_nft_collection_owner_count(&collection_id, &[0xA1; 32], 2);
        overwrite_batch.commit().unwrap();

        let cached_after_write = list_nft_collection_holders_ranked_cached(
            &domain,
            &mem_cache,
            &collection_id,
            &aggregate,
        )
        .unwrap();
        assert_eq!(
            cached_after_write
                .iter()
                .find(|(lock_hash, _)| lock_hash == &[0xA1; 32])
                .unwrap()
                .1,
            1
        );

        let uncached_after_write =
            list_nft_collection_holders_ranked(&domain, &collection_id, &aggregate).unwrap();
        assert_eq!(
            uncached_after_write
                .iter()
                .find(|(lock_hash, _)| lock_hash == &[0xA1; 32])
                .unwrap()
                .1,
            2
        );
    }

    fn make_live_nft_cell(type_args: Vec<u8>) -> LiveCellInfo {
        LiveCellInfo {
            capacity: 100_00000000,
            created_at_block: 10,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![],
            type_script_hash: Some(vec![0x33; 32]),
            type_code_hash: Some(vec![0x44; 32]),
            type_args: Some(type_args),
            data_size: 0,
            occupied_capacity: 61_00000000,
            udt_amount: None,
        }
    }

    #[test]
    fn test_resolve_live_dotbit_outpoints_reads_split_layout() {
        let root = tempfile::tempdir().unwrap();
        let domain = CkbadgerStore::open_domain(root.path().join("domain")).unwrap();
        let append = CkbadgerStore::open_append_only(root.path().join("append")).unwrap();
        let collection_id = [0xD1; 32];
        let account_id = vec![0x61; 20];
        let live_tx = vec![0x71; 32];
        let live_idx = 3i16;
        let entry = NftEntry {
            standard: NftStandard::DotBit,
            collection_id: Some(collection_id.to_vec()),
            token_id: Some(account_id.clone()),
            owner_lock_hash: Some(vec![0x55; 32]),
            name: Some("alice.bit".to_string()),
            is_live: true,
            created_at_block: 10,
            extra: NftExtra::DotBit {
                expired_at: None,
                registered_at: None,
                status: Some(0),
            },
        };

        let mut domain_batch = StoreBatch::new(&domain);
        domain_batch.put_nft_collection_aggregate(
            &collection_id,
            &NftCollectionAggregate {
                name: Some(".bit".to_string()),
                standard: NftStandard::DotBit,
                total_count: 1,
                live_count: 1,
                holders_count: 1,
                activities_count: 0,
            },
        );
        domain_batch.put_nft(&account_id, &entry);
        domain_batch.put_nft_by_collection(&collection_id, &account_id);
        domain_batch.put_dotbit_account_outpoint(&live_tx, live_idx, &account_id);
        domain_batch.put_cell(&live_tx, live_idx, &make_live_nft_cell(account_id.clone()));
        domain_batch.commit().unwrap();

        let mut append_batch = StoreBatch::new(&append);
        append_batch.put_cell(&live_tx, live_idx, &make_live_nft_cell(account_id.clone()));
        append_batch.commit().unwrap();

        let outpoints =
            resolve_live_dotbit_outpoints(&domain, &append, std::slice::from_ref(&account_id))
                .unwrap();
        let (tx_hash, output_index) = outpoints.get(&account_id).unwrap();
        assert_eq!(tx_hash, &live_tx);
        assert_eq!(*output_index, live_idx);
    }

    #[test]
    fn test_resolve_live_mnft_outpoints_reads_split_layout() {
        let root = tempfile::tempdir().unwrap();
        let domain = CkbadgerStore::open_domain(root.path().join("domain")).unwrap();
        let append = CkbadgerStore::open_append_only(root.path().join("append")).unwrap();
        let collection_id = [0xE1; 32];
        let token_id = vec![0x81; 28];
        let live_tx = vec![0x91; 32];
        let live_idx = 4i16;
        let entry = NftEntry {
            standard: NftStandard::MnftToken,
            collection_id: Some(collection_id.to_vec()),
            token_id: Some(token_id.clone()),
            owner_lock_hash: Some(vec![0x66; 32]),
            name: Some("mnft".to_string()),
            is_live: true,
            created_at_block: 10,
            extra: NftExtra::MnftToken {
                token_index: 1,
                characteristic: vec![],
                configure: 0,
                state: 0,
            },
        };

        let mut domain_batch = StoreBatch::new(&domain);
        domain_batch.put_nft_collection_aggregate(
            &collection_id,
            &NftCollectionAggregate {
                name: Some("class".to_string()),
                standard: NftStandard::MnftClass,
                total_count: 1,
                live_count: 1,
                holders_count: 1,
                activities_count: 0,
            },
        );
        domain_batch.put_nft(&token_id, &entry);
        domain_batch.put_nft_by_collection(&collection_id, &token_id);
        domain_batch.put_mnft_token_outpoint(&live_tx, live_idx, &token_id);
        domain_batch.put_cell(&live_tx, live_idx, &make_live_nft_cell(token_id.clone()));
        domain_batch.commit().unwrap();

        let mut append_batch = StoreBatch::new(&append);
        append_batch.put_cell(&live_tx, live_idx, &make_live_nft_cell(token_id.clone()));
        append_batch.commit().unwrap();

        let outpoints =
            resolve_live_mnft_outpoints(&domain, &append, std::slice::from_ref(&token_id)).unwrap();
        let (tx_hash, output_index) = outpoints.get(&token_id).unwrap();
        assert_eq!(tx_hash, &live_tx);
        assert_eq!(*output_index, live_idx);
    }
}
