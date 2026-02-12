use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::response::{ok, ApiError, ApiResult, CursorPaginatedResponse};
use crate::warmup::{CachedAssetEntry, CACHE_KEY_ASSETS_DOB};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/spore/clusters", get(list_clusters))
        .route("/spore/clusters/{cluster_id}", get(get_cluster))
        .route(
            "/spore/clusters/{cluster_id}/spores",
            get(get_spores_by_cluster),
        )
        .route("/spore/nfts", get(list_spores))
        .route("/spore/nfts/{spore_id}", get(get_spore))
        .route("/spore/owner/{lock_hash}", get(get_spores_by_owner))
}

#[derive(Debug, Deserialize)]
pub struct ListParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<i64>,
}

fn default_limit() -> i64 {
    20
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterResponse {
    pub cluster_id: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub owner_lock_hash: String,
    pub owner_address: Option<String>,
    pub spores_count: i32,
    pub created_at_block: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SporeResponse {
    pub spore_id: String,
    pub tx_hash: String,
    pub output_index: i32,
    pub cluster_id: Option<String>,
    pub content_type: String,
    pub content_size: i32,
    pub owner_lock_hash: String,
    pub owner_address: Option<String>,
    pub is_live: bool,
    pub created_at_block: i64,
}

/// Convert a DobEntry from the store into a SporeResponse.
fn spore_to_response(spore_id: &[u8], entry: &ckbadger_store::SporeEntry) -> SporeResponse {
    let (content_type, content_size) = match &entry.extra {
        ckbadger_store::DobExtra::Spore {
            content_type,
            content_length,
        } => (content_type.clone(), *content_length as i32),
        _ => (String::new(), 0),
    };
    SporeResponse {
        spore_id: format!("0x{}", hex::encode(spore_id)),
        tx_hash: format!("0x{}", hex::encode(&entry.created_at_tx)),
        output_index: 0,
        cluster_id: entry
            .collection_id
            .as_ref()
            .map(|c| format!("0x{}", hex::encode(c))),
        content_type,
        content_size,
        owner_lock_hash: entry
            .owner_lock_hash
            .as_ref()
            .map(|h| format!("0x{}", hex::encode(h)))
            .unwrap_or_default(),
        owner_address: None,
        is_live: entry.is_live,
        created_at_block: entry.created_at_block,
    }
}

/// List clusters — use cached DOB assets list when available.
async fn list_clusters(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<ClusterResponse>> {
    let limit = params.limit.clamp(1, 100) as usize;
    let cursor_block = params.cursor.unwrap_or(i64::MAX);

    // Try cached DOB assets first — they already have cluster grouping
    if let Some(cached_dobs) = state
        .mem_cache
        .get::<Vec<CachedAssetEntry>>(CACHE_KEY_ASSETS_DOB)
    {
        return serve_clusters_from_cache(cached_dobs, cursor_block, limit, &state);
    }

    // Fallback: derive from spores scan
    serve_clusters_from_store(&state, cursor_block, limit)
}

fn serve_clusters_from_cache(
    cached: Vec<CachedAssetEntry>,
    cursor_block: i64,
    limit: usize,
    state: &Arc<AppState>,
) -> ApiResult<CursorPaginatedResponse<ClusterResponse>> {
    // Build cluster responses from cached entries
    let mut clusters: Vec<ClusterResponse> = cached
        .into_iter()
        .filter_map(|entry| {
            let cluster_id_hex = entry.cluster_id.as_ref().unwrap_or(&entry.id);
            let cluster_id_bytes =
                hex::decode(cluster_id_hex.strip_prefix("0x").unwrap_or(cluster_id_hex)).ok()?;

            let cluster_entry = state.store.get_spore(&cluster_id_bytes).ok().flatten();
            let created_at_block = cluster_entry
                .as_ref()
                .map(|e| e.created_at_block)
                .unwrap_or(0);
            let description = cluster_entry.as_ref().and_then(|e| e.description.clone());
            let owner_lock_hash = cluster_entry
                .as_ref()
                .and_then(|e| e.owner_lock_hash.clone());

            Some(ClusterResponse {
                cluster_id: entry.id,
                name: entry.name,
                description,
                owner_lock_hash: owner_lock_hash
                    .as_ref()
                    .map(|h| format!("0x{}", hex::encode(h)))
                    .unwrap_or_default(),
                owner_address: None,
                spores_count: entry.transfers_count as i32, // transfers_count holds spore count for DOB
                created_at_block,
            })
        })
        .collect();

    clusters.sort_by(|a, b| b.created_at_block.cmp(&a.created_at_block));

    let filtered: Vec<_> = clusters
        .iter()
        .filter(|c| c.created_at_block < cursor_block)
        .take(limit + 1)
        .cloned()
        .collect();

    let has_more = filtered.len() > limit;
    let page: Vec<_> = filtered.into_iter().take(limit).collect();

    let next_cursor = if has_more {
        page.last().map(|c| c.created_at_block.to_string())
    } else {
        None
    };

    ok(CursorPaginatedResponse::without_total(
        page,
        limit as i64,
        next_cursor,
    ))
}

fn serve_clusters_from_store(
    state: &Arc<AppState>,
    cursor_block: i64,
    limit: usize,
) -> ApiResult<CursorPaginatedResponse<ClusterResponse>> {
    let all_spores = state
        .store
        .list_spores(100_000)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    type ClusterInfo = (Option<Vec<u8>>, i32, i64);

    let mut cluster_map: std::collections::HashMap<Vec<u8>, ClusterInfo> =
        std::collections::HashMap::new();

    for (_, entry) in &all_spores {
        if let Some(ref cluster_id) = entry.collection_id {
            let e = cluster_map.entry(cluster_id.clone()).or_insert((
                entry.owner_lock_hash.clone(),
                0,
                entry.created_at_block,
            ));
            e.1 += 1;
            if entry.created_at_block < e.2 {
                e.2 = entry.created_at_block;
            }
        }
    }

    let mut clusters: Vec<_> = cluster_map.into_iter().collect();
    clusters.sort_by(|a, b| b.1 .2.cmp(&a.1 .2));

    let filtered: Vec<_> = clusters
        .iter()
        .filter(|(_, (_, _, created))| *created < cursor_block)
        .take(limit + 1)
        .collect();

    let has_more = filtered.len() > limit;
    let page: Vec<_> = filtered.into_iter().take(limit).collect();

    let next_cursor = if has_more {
        page.last().map(|(_, (_, _, created))| created.to_string())
    } else {
        None
    };

    let result: Vec<ClusterResponse> = page
        .into_iter()
        .map(|(cluster_id, (owner, spores_count, created_at_block))| {
            let cluster_entry = state.store.get_spore(cluster_id).ok().flatten();
            let name = cluster_entry.as_ref().and_then(|e| e.name.clone());
            let description = cluster_entry.as_ref().and_then(|e| e.description.clone());
            ClusterResponse {
                cluster_id: format!("0x{}", hex::encode(cluster_id)),
                name,
                description,
                owner_lock_hash: owner
                    .as_ref()
                    .map(|h| format!("0x{}", hex::encode(h)))
                    .unwrap_or_default(),
                owner_address: None,
                spores_count: *spores_count,
                created_at_block: *created_at_block,
            }
        })
        .collect();

    ok(CursorPaginatedResponse::without_total(
        result,
        limit as i64,
        next_cursor,
    ))
}

/// Get spores by cluster — use secondary index instead of full scan.
async fn get_spores_by_cluster(
    State(state): State<Arc<AppState>>,
    Path(cluster_id): Path<String>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<SporeResponse>> {
    let id = hex::decode(cluster_id.strip_prefix("0x").unwrap_or(&cluster_id))
        .map_err(|_| ApiError::bad_request("Invalid cluster ID"))?;

    let limit = params.limit.clamp(1, 100) as usize;
    let cursor_block = params.cursor.unwrap_or(i64::MAX);

    // Use secondary index for efficient lookup
    let cluster_spores = state
        .store
        .list_spores_by_cluster(&id, 10_000)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let mut filtered: Vec<_> = cluster_spores
        .into_iter()
        .filter(|(_, entry)| entry.is_live && entry.created_at_block < cursor_block)
        .collect();

    filtered.sort_by(|a, b| b.1.created_at_block.cmp(&a.1.created_at_block));

    let page: Vec<_> = filtered.iter().take(limit + 1).collect();
    let has_more = page.len() > limit;
    let page: Vec<_> = page.into_iter().take(limit).collect();

    let next_cursor = if has_more {
        page.last()
            .map(|(_, entry)| entry.created_at_block.to_string())
    } else {
        None
    };

    let spores: Vec<SporeResponse> = page
        .into_iter()
        .map(|(spore_id, entry)| spore_to_response(spore_id, entry))
        .collect();

    ok(CursorPaginatedResponse::without_total(
        spores,
        limit as i64,
        next_cursor,
    ))
}

/// Get cluster — point lookup + count from secondary index (no full scan).
async fn get_cluster(
    State(state): State<Arc<AppState>>,
    Path(cluster_id): Path<String>,
) -> ApiResult<ClusterResponse> {
    let id = hex::decode(cluster_id.strip_prefix("0x").unwrap_or(&cluster_id))
        .map_err(|_| ApiError::bad_request("Invalid cluster ID"))?;

    // Look up the cluster entry directly
    let cluster_entry = state
        .store
        .get_spore(&id)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Count spores in cluster using secondary index
    let spores_count = state
        .store
        .count_spores_in_cluster(&id)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    if spores_count == 0 && cluster_entry.is_none() {
        return Err(ApiError::not_found("Cluster not found"));
    }

    let name = cluster_entry.as_ref().and_then(|e| e.name.clone());
    let description = cluster_entry.as_ref().and_then(|e| e.description.clone());
    let created_at_block = cluster_entry
        .as_ref()
        .map(|e| e.created_at_block)
        .unwrap_or(0);
    let owner_lock_hash = cluster_entry
        .as_ref()
        .and_then(|e| e.owner_lock_hash.clone());

    ok(ClusterResponse {
        cluster_id: format!("0x{}", hex::encode(&id)),
        name,
        description,
        owner_lock_hash: owner_lock_hash
            .as_ref()
            .map(|h| format!("0x{}", hex::encode(h)))
            .unwrap_or_default(),
        owner_address: None,
        spores_count: spores_count as i32,
        created_at_block,
    })
}

async fn list_spores(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<SporeResponse>> {
    let limit = params.limit.clamp(1, 100) as usize;
    let cursor_block = params.cursor.unwrap_or(i64::MAX);

    let all_spores = state
        .store
        .list_spores(100_000)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let mut filtered: Vec<_> = all_spores
        .into_iter()
        .filter(|(_, entry)| entry.is_live && entry.created_at_block < cursor_block)
        .collect();

    filtered.sort_by(|a, b| b.1.created_at_block.cmp(&a.1.created_at_block));

    let page: Vec<_> = filtered.iter().take(limit + 1).collect();
    let has_more = page.len() > limit;
    let page: Vec<_> = page.into_iter().take(limit).collect();

    let next_cursor = if has_more {
        page.last()
            .map(|(_, entry)| entry.created_at_block.to_string())
    } else {
        None
    };

    let spores: Vec<SporeResponse> = page
        .into_iter()
        .map(|(spore_id, entry)| spore_to_response(spore_id, entry))
        .collect();

    ok(CursorPaginatedResponse::without_total(
        spores,
        limit as i64,
        next_cursor,
    ))
}

async fn get_spore(
    State(state): State<Arc<AppState>>,
    Path(spore_id): Path<String>,
) -> ApiResult<SporeResponse> {
    let id = hex::decode(spore_id.strip_prefix("0x").unwrap_or(&spore_id))
        .map_err(|_| ApiError::bad_request("Invalid spore ID"))?;

    let entry = state
        .store
        .get_spore(&id)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    match entry {
        Some(entry) => ok(spore_to_response(&id, &entry)),
        None => Err(ApiError::not_found("Spore not found")),
    }
}

async fn get_spores_by_owner(
    State(state): State<Arc<AppState>>,
    Path(lock_hash): Path<String>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<SporeResponse>> {
    let hash = hex::decode(lock_hash.strip_prefix("0x").unwrap_or(&lock_hash))
        .map_err(|_| ApiError::bad_request("Invalid lock script hash"))?;

    let limit = params.limit.clamp(1, 100) as usize;
    let cursor_block = params.cursor.unwrap_or(i64::MAX);

    let all_spores = state
        .store
        .list_spores(100_000)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let mut filtered: Vec<_> = all_spores
        .into_iter()
        .filter(|(_, entry)| {
            entry.is_live
                && entry.owner_lock_hash.as_ref() == Some(&hash)
                && entry.created_at_block < cursor_block
        })
        .collect();

    filtered.sort_by(|a, b| b.1.created_at_block.cmp(&a.1.created_at_block));

    let page: Vec<_> = filtered.iter().take(limit + 1).collect();
    let has_more = page.len() > limit;
    let page: Vec<_> = page.into_iter().take(limit).collect();

    let next_cursor = if has_more {
        page.last()
            .map(|(_, entry)| entry.created_at_block.to_string())
    } else {
        None
    };

    let spores: Vec<SporeResponse> = page
        .into_iter()
        .map(|(spore_id, entry)| spore_to_response(spore_id, entry))
        .collect();

    ok(CursorPaginatedResponse::without_total(
        spores,
        limit as i64,
        next_cursor,
    ))
}
