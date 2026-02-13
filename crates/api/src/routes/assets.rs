use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::response::{ok, ApiError, ApiResult, CursorPaginatedResponse};
use crate::warmup::{
    CachedAssetEntry, CACHE_KEY_ASSETS_DOB, CACHE_KEY_ASSETS_NFT, CACHE_KEY_ASSETS_TOKEN,
};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/assets", get(list_assets))
}

#[derive(Debug, Deserialize)]
pub struct ListParams {
    #[serde(default = "default_limit")]
    limit: i64,
    #[serde(rename = "type")]
    asset_type: Option<String>,
    #[allow(dead_code)]
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

async fn list_assets(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<AssetResponse>> {
    let limit = params.limit.clamp(1, 100);

    let search_lower = params.search.as_ref().map(|s| s.to_lowercase());
    let filter_type = params.asset_type.as_deref();

    let (total, rows) = fetch_assets_cached(&state, filter_type, search_lower.as_deref(), limit)?;

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

/// Read from in-memory cache, apply search filter + limit. Falls back to direct computation.
fn fetch_assets_cached(
    state: &Arc<AppState>,
    filter_type: Option<&str>,
    search: Option<&str>,
    limit: i64,
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

    all_cached.truncate((limit + 1) as usize);

    let assets: Vec<AssetResponse> = all_cached
        .into_iter()
        .map(|e| e.to_asset_response())
        .collect();

    Ok((total, assets))
}

/// Fallback: compute token assets directly (when cache is cold).
fn compute_token_assets(
    state: &Arc<AppState>,
) -> Result<Vec<CachedAssetEntry>, (axum::http::StatusCode, Json<ApiError>)> {
    let tokens = state
        .store
        .list_tokens()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let now_ms = chrono::Utc::now().timestamp_millis();
    let mut result = Vec::with_capacity(tokens.len());

    for (hash, info) in &tokens {
        let transfers_count = info.transfers_count;
        let transfers_24h = state
            .store
            .get_token_24h_transfers(hash, now_ms)
            .unwrap_or(0);

        result.push(CachedAssetEntry {
            id: format!("0x{}", hex::encode(hash)),
            asset_type: "token".to_string(),
            standard: info.standard.clone(),
            name: info.name.clone(),
            symbol: info.symbol.clone(),
            icon_url: info.icon_url.clone(),
            holders_count: info.holders_count,
            transfers_count,
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

/// Fallback: compute DOB assets directly.
fn compute_dob_assets(
    state: &Arc<AppState>,
) -> Result<Vec<CachedAssetEntry>, (axum::http::StatusCode, Json<ApiError>)> {
    let spores = state
        .store
        .list_spores(10_000)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    struct ClusterAgg {
        count: i64,
        owners: std::collections::HashSet<Vec<u8>>,
    }

    let mut cluster_map: std::collections::HashMap<Vec<u8>, ClusterAgg> =
        std::collections::HashMap::new();

    for (id, entry) in &spores {
        if entry.standard.is_cluster() {
            continue;
        }
        let cluster_id_bytes = entry.collection_id.clone().unwrap_or_else(|| id.clone());
        let agg = cluster_map
            .entry(cluster_id_bytes)
            .or_insert_with(|| ClusterAgg {
                count: 0,
                owners: std::collections::HashSet::new(),
            });
        agg.count += 1;
        if entry.is_live {
            if let Some(ref owner) = entry.owner_lock_hash {
                agg.owners.insert(owner.clone());
            }
        }
    }

    let mut result = Vec::new();
    for (cluster_id_bytes, agg) in &cluster_map {
        let cluster_hex = format!("0x{}", hex::encode(cluster_id_bytes));
        let cluster_entry = state.store.get_spore(cluster_id_bytes).ok().flatten();
        let name = cluster_entry.as_ref().and_then(|e| e.name.clone());

        result.push(CachedAssetEntry {
            id: cluster_hex.clone(),
            asset_type: "dob".to_string(),
            standard: "spore".to_string(),
            name: name.clone(),
            symbol: None,
            icon_url: None,
            holders_count: agg.owners.len() as i64,
            transfers_count: agg.count,
            transfers_24h: 0,
            decimals: None,
            total_supply: Some(agg.count.to_string()),
            content_type: None,
            content_size: None,
            cluster_id: Some(cluster_hex),
            cluster_name: name,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            description: None,
        });
    }

    Ok(result)
}

/// Fallback: compute NFT assets directly.
fn compute_nft_assets(
    state: &Arc<AppState>,
) -> Result<Vec<CachedAssetEntry>, (axum::http::StatusCode, Json<ApiError>)> {
    let nfts = state
        .store
        .list_nfts(10_000)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let mut collection_map: std::collections::HashMap<
        String,
        (
            Option<String>,
            i64,
            bool,
            ckbadger_store::types::NftStandard,
        ),
    > = std::collections::HashMap::new();

    for (id, entry) in &nfts {
        let collection_hex = entry
            .collection_id
            .as_ref()
            .map(|c| format!("0x{}", hex::encode(c)))
            .unwrap_or_else(|| format!("0x{}", hex::encode(id)));

        let counter = collection_map
            .entry(collection_hex)
            .or_insert_with(|| (entry.name.clone(), 0, entry.is_live, entry.standard));
        counter.1 += 1;
    }

    let mut result = Vec::new();
    for (collection_hex, (name, count, is_live, standard)) in &collection_map {
        if !is_live {
            continue;
        }
        result.push(CachedAssetEntry {
            id: collection_hex.clone(),
            asset_type: "nft".to_string(),
            standard: standard.asset_standard().to_string(),
            name: name.clone(),
            symbol: None,
            icon_url: None,
            holders_count: *count,
            transfers_count: *count,
            transfers_24h: 0,
            decimals: None,
            total_supply: Some(count.to_string()),
            content_type: None,
            content_size: None,
            cluster_id: Some(collection_hex.clone()),
            cluster_name: name.clone(),
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            description: None,
        });
    }

    Ok(result)
}
