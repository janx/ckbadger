use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::response::{ok, ApiError, ApiResult, CursorPaginatedResponse};
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

    let (total, rows) = fetch_assets(&state, filter_type, search_lower.as_deref(), limit)?;

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

    let assets: Vec<AssetResponse> = rows
        .into_iter()
        .map(|r| AssetResponse {
            id: r.id,
            asset_type: r.asset_type,
            standard: r.standard,
            name: r.name,
            symbol: r.symbol,
            icon_url: r.icon_url,
            published: r.published,
            famous: r.famous,
            tags: r.tags,
            holders_count: r.holders_count,
            transfers_count: r.transfers_count,
            transfers_24h: r.transfers_24h,
            decimals: r.decimals,
            total_supply: r.total_supply,
            content_type: r.content_type,
            content_size: r.content_size,
            cluster_id: r.cluster_id,
            cluster_name: r.cluster_name,
        })
        .collect();

    ok(CursorPaginatedResponse::new(
        assets,
        total,
        limit,
        next_cursor,
    ))
}

#[derive(Debug)]
struct AssetRow {
    id: String,
    asset_type: String,
    standard: String,
    name: Option<String>,
    symbol: Option<String>,
    icon_url: Option<String>,
    published: bool,
    famous: bool,
    tags: Option<Vec<String>>,
    holders_count: i64,
    transfers_count: i64,
    transfers_24h: i64,
    decimals: Option<i16>,
    total_supply: Option<String>,
    content_type: Option<String>,
    content_size: Option<i32>,
    cluster_id: Option<String>,
    cluster_name: Option<String>,
}

fn fetch_assets(
    state: &Arc<AppState>,
    filter_type: Option<&str>,
    search: Option<&str>,
    limit: i64,
) -> Result<(i64, Vec<AssetRow>), (axum::http::StatusCode, Json<ApiError>)> {
    let mut assets: Vec<AssetRow> = Vec::new();

    // -- Tokens --
    if !matches!(filter_type, Some("nft") | Some("dob")) {
        let tokens = state
            .store
            .list_tokens()
            .map_err(|e| ApiError::internal(e.to_string()))?;

        for (hash, info) in &tokens {
            let name_lower = info.name.as_deref().unwrap_or("").to_lowercase();
            let symbol_lower = info.symbol.as_deref().unwrap_or("").to_lowercase();

            if let Some(s) = search {
                if !name_lower.contains(s) && !symbol_lower.contains(s) {
                    continue;
                }
            }

            let now_ms = chrono::Utc::now().timestamp_millis();
            let transfers_count = state.store.get_token_transfers_count(hash).unwrap_or(0);
            let transfers_24h = state
                .store
                .get_token_24h_transfers(hash, now_ms)
                .unwrap_or(0);

            assets.push(AssetRow {
                id: format!("0x{}", hex::encode(hash)),
                asset_type: "token".to_string(),
                standard: info.standard.clone(),
                name: info.name.clone(),
                symbol: info.symbol.clone(),
                icon_url: info.icon_url.clone(),
                published: false,
                famous: false,
                tags: None,
                holders_count: info.holders_count,
                transfers_count,
                transfers_24h,
                decimals: info.decimals.map(|d| d as i16),
                total_supply: info.total_supply.map(|s| s.to_string()),
                content_type: None,
                content_size: None,
                cluster_id: None,
                cluster_name: None,
            });
        }
    }

    // -- Spores (DOB) --
    if !matches!(filter_type, Some("token") | Some("nft")) {
        let spores = state
            .store
            .list_spores(10_000)
            .map_err(|e| ApiError::internal(e.to_string()))?;

        // Group spores by cluster_id to produce DOB asset rows.
        // Track: cluster_id_bytes, spore count, unique owner lock hashes.
        struct ClusterAgg {
            count: i64,
            owners: std::collections::HashSet<Vec<u8>>,
        }

        let mut cluster_map: std::collections::HashMap<Vec<u8>, ClusterAgg> =
            std::collections::HashMap::new();

        for (id, entry) in &spores {
            // Skip cluster entries themselves
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

        for (cluster_id_bytes, agg) in &cluster_map {
            let cluster_hex = format!("0x{}", hex::encode(cluster_id_bytes));

            // Look up the cluster entry to get the real name
            let cluster_entry = state.store.get_spore(cluster_id_bytes).ok().flatten();
            let name = cluster_entry.as_ref().and_then(|e| e.name.clone());

            if let Some(s) = search {
                let n = name.as_deref().unwrap_or("").to_lowercase();
                if !n.contains(s) {
                    continue;
                }
            }

            let holders_count = agg.owners.len() as i64;

            assets.push(AssetRow {
                id: cluster_hex.clone(),
                asset_type: "dob".to_string(),
                standard: "spore".to_string(),
                name: name.clone(),
                symbol: None,
                icon_url: None,
                published: false,
                famous: false,
                tags: None,
                holders_count,
                transfers_count: agg.count,
                transfers_24h: 0,
                decimals: None,
                total_supply: Some(agg.count.to_string()),
                content_type: None,
                content_size: None,
                cluster_id: Some(cluster_hex),
                cluster_name: name,
            });
        }
    }

    // -- NFTs --
    if !matches!(filter_type, Some("token") | Some("dob")) {
        let nfts = state
            .store
            .list_nfts(10_000)
            .map_err(|e| ApiError::internal(e.to_string()))?;

        // Group NFTs by collection_id
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

        for (collection_hex, (name, count, is_live, standard)) in &collection_map {
            if !is_live {
                continue;
            }

            if let Some(s) = search {
                let n = name.as_deref().unwrap_or("").to_lowercase();
                if !n.contains(s) {
                    continue;
                }
            }

            assets.push(AssetRow {
                id: collection_hex.clone(),
                asset_type: "nft".to_string(),
                standard: standard.asset_standard().to_string(),
                name: name.clone(),
                symbol: None,
                icon_url: None,
                published: false,
                famous: false,
                tags: None,
                holders_count: *count,
                transfers_count: *count,
                transfers_24h: 0,
                decimals: None,
                total_supply: Some(count.to_string()),
                content_type: None,
                content_size: None,
                cluster_id: Some(collection_hex.clone()),
                cluster_name: name.clone(),
            });
        }
    }

    let total = assets.len() as i64;

    assets.sort_by(|a, b| {
        b.transfers_24h
            .cmp(&a.transfers_24h)
            .then_with(|| b.holders_count.cmp(&a.holders_count))
            .then_with(|| a.asset_type.cmp(&b.asset_type))
            .then_with(|| b.id.cmp(&a.id))
    });

    assets.truncate((limit + 1) as usize);

    Ok((total, assets))
}
