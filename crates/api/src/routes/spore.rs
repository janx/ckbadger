use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::statistics::{StackedAreaChartResponse, StackedAreaDataPoint, StackedAreaSeries};
use crate::response::{ok, ApiError, ApiResult, CursorPaginatedResponse};
use crate::utils::{apply_live_capacity_delta, parse_chart_date_range};
use crate::warmup::{CachedAssetEntry, CACHE_KEY_ASSETS_DOB};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/spore/clusters", get(list_clusters))
        .route("/spore/clusters/{cluster_id}", get(get_cluster))
        .route(
            "/spore/clusters/{cluster_id}/charts/occupation",
            get(get_cluster_occupation_chart),
        )
        .route(
            "/spore/clusters/{cluster_id}/spores",
            get(get_spores_by_cluster),
        )
        .route("/spore/nfts", get(list_spores))
        .route("/spore/nfts/{spore_id}", get(get_spore))
        .route(
            "/spore/nfts/{spore_id}/charts/occupation",
            get(get_spore_occupation_chart),
        )
        .route("/spore/owner/{lock_hash}", get(get_spores_by_owner))
}

#[derive(Debug, Deserialize)]
pub struct ListParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ChartRangeParams {
    from: Option<String>,
    to: Option<String>,
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
    pub live_capacity: Option<String>,
    pub live_occupied_capacity: Option<String>,
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
    pub live_capacity: Option<String>,
    pub live_occupied_capacity: Option<String>,
}

/// Convert a DobEntry from the store into a SporeResponse.
fn spore_to_response(
    spore_id: &[u8],
    entry: &ckbadger_store::SporeEntry,
    live_capacity: Option<i64>,
    live_occupied_capacity: Option<i64>,
) -> SporeResponse {
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
        live_capacity: live_capacity.map(|v| v.to_string()),
        live_occupied_capacity: live_occupied_capacity.map(|v| v.to_string()),
    }
}

fn format_yyyymmdd_for_chart(date: u32) -> String {
    let s = format!("{date:08}");
    format!("{}-{}-{}", &s[0..4], &s[4..6], &s[6..8])
}

fn build_capacity_occupation_chart(
    deltas: Vec<(u32, i64, i64)>,
    title: String,
) -> anyhow::Result<StackedAreaChartResponse> {
    build_capacity_occupation_chart_with_initial(deltas, title, 0, 0)
}

fn build_capacity_occupation_chart_with_initial(
    deltas: Vec<(u32, i64, i64)>,
    title: String,
    initial_capacity: i128,
    initial_occupied: i128,
) -> anyhow::Result<StackedAreaChartResponse> {
    if initial_capacity < 0 {
        anyhow::bail!(
            "invalid initial capacity for spore chart: {}",
            initial_capacity
        );
    }
    if initial_occupied < 0 {
        anyhow::bail!(
            "invalid initial occupied capacity for spore chart: {}",
            initial_occupied
        );
    }
    if initial_occupied > initial_capacity {
        anyhow::bail!(
            "invalid initial occupied/capacity for spore chart: occupied={}, capacity={}",
            initial_occupied,
            initial_capacity
        );
    }
    let mut running_capacity = initial_capacity;
    let mut running_occupied = initial_occupied;
    let mut data = Vec::with_capacity(deltas.len());

    for (date, capacity_delta, occupied_delta) in deltas {
        (running_capacity, running_occupied) = apply_live_capacity_delta(
            running_capacity,
            running_occupied,
            capacity_delta,
            occupied_delta,
            &format!("building spore occupation chart at date {}", date),
        )?;
        let unoccupied = running_capacity - running_occupied;
        let mut values = std::collections::HashMap::new();
        values.insert("occupied".to_string(), running_occupied.to_string());
        values.insert("unoccupied".to_string(), unoccupied.to_string());

        data.push(StackedAreaDataPoint {
            date: format_yyyymmdd_for_chart(date),
            values,
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
                color: "#06b6d4".to_string(),
            },
        ],
        title,
    })
}

fn latest_capacity_from_chart(
    chart: &StackedAreaChartResponse,
) -> (Option<String>, Option<String>) {
    if let Some(last) = chart.data.last() {
        let occupied = last.values.get("occupied").cloned();
        let unoccupied = last.values.get("unoccupied").cloned();
        let capacity = match (&occupied, &unoccupied) {
            (Some(o), Some(u)) => {
                let total = o.parse::<i128>().unwrap_or(0) + u.parse::<i128>().unwrap_or(0);
                Some(total.to_string())
            }
            _ => None,
        };
        return (capacity, occupied);
    }
    (Some("0".to_string()), Some("0".to_string()))
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
                live_capacity: None,
                live_occupied_capacity: None,
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
                live_capacity: None,
                live_occupied_capacity: None,
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
        .map(|(spore_id, entry)| spore_to_response(spore_id, entry, None, None))
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
    let daily = state
        .store
        .list_cluster_daily_deltas(&id)
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
        format!(
            "{} Capacity Occupation",
            name.clone().unwrap_or_else(|| "Spore Cluster".to_string())
        ),
    )
    .map_err(|e| ApiError::internal(e.to_string()))?;
    let (live_capacity, live_occupied_capacity) = latest_capacity_from_chart(&chart);

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
        live_capacity,
        live_occupied_capacity,
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
        .map(|(spore_id, entry)| spore_to_response(spore_id, entry, None, None))
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
        Some(entry) => {
            let daily = state
                .store
                .list_spore_daily_deltas(&id)
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
                "Spore Capacity Occupation".to_string(),
            )
            .map_err(|e| ApiError::internal(e.to_string()))?;
            let (live_capacity, live_occupied_capacity) = latest_capacity_from_chart(&chart);
            let cap = live_capacity.and_then(|v| v.parse::<i64>().ok());
            let occ = live_occupied_capacity.and_then(|v| v.parse::<i64>().ok());
            ok(spore_to_response(&id, &entry, cap, occ))
        }
        None => Err(ApiError::not_found("Spore not found")),
    }
}

async fn get_cluster_occupation_chart(
    State(state): State<Arc<AppState>>,
    Path(cluster_id): Path<String>,
    Query(params): Query<ChartRangeParams>,
) -> ApiResult<StackedAreaChartResponse> {
    let (from_date, to_date) = parse_chart_date_range(params.from.as_deref(), params.to.as_deref())
        .map_err(|msg| ApiError::bad_request(&msg))?;

    let id = hex::decode(cluster_id.strip_prefix("0x").unwrap_or(&cluster_id))
        .map_err(|_| ApiError::bad_request("Invalid cluster ID"))?;

    let cluster_entry = state
        .store
        .get_spore(&id)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let spores_count = state
        .store
        .count_spores_in_cluster(&id)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    if spores_count == 0 && cluster_entry.is_none() {
        return Err(ApiError::not_found("Cluster not found"));
    }

    let name = cluster_entry
        .as_ref()
        .and_then(|e| e.name.clone())
        .unwrap_or_else(|| "Spore Cluster".to_string());
    let daily = state
        .store
        .list_cluster_daily_deltas_in_range(&id, from_date, to_date)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let (initial_capacity, initial_occupied) = if let Some(from) = from_date {
        let mut base_capacity: i128 = 0;
        let mut base_occupied: i128 = 0;
        let baseline = state
            .store
            .list_cluster_daily_deltas_in_range(&id, None, Some(from.saturating_sub(1)))
            .map_err(|e| ApiError::internal(e.to_string()))?;
        for (_, delta) in baseline {
            (base_capacity, base_occupied) = apply_live_capacity_delta(
                base_capacity,
                base_occupied,
                delta.live_capacity_delta,
                delta.live_occupied_capacity_delta,
                "building cluster baseline occupation chart",
            )
            .map_err(|e| ApiError::internal(e.to_string()))?;
        }
        (base_capacity, base_occupied)
    } else {
        (0, 0)
    };

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
        format!("{name} Capacity Occupation"),
        initial_capacity,
        initial_occupied,
    )
    .map_err(|e| ApiError::internal(e.to_string()))?)
}

async fn get_spore_occupation_chart(
    State(state): State<Arc<AppState>>,
    Path(spore_id): Path<String>,
    Query(params): Query<ChartRangeParams>,
) -> ApiResult<StackedAreaChartResponse> {
    let (from_date, to_date) = parse_chart_date_range(params.from.as_deref(), params.to.as_deref())
        .map_err(|msg| ApiError::bad_request(&msg))?;

    let id = hex::decode(spore_id.strip_prefix("0x").unwrap_or(&spore_id))
        .map_err(|_| ApiError::bad_request("Invalid spore ID"))?;

    let entry = state
        .store
        .get_spore(&id)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    if entry.is_none() {
        return Err(ApiError::not_found("Spore not found"));
    }

    let daily = state
        .store
        .list_spore_daily_deltas_in_range(&id, from_date, to_date)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let (initial_capacity, initial_occupied) = if let Some(from) = from_date {
        let mut base_capacity: i128 = 0;
        let mut base_occupied: i128 = 0;
        let baseline = state
            .store
            .list_spore_daily_deltas_in_range(&id, None, Some(from.saturating_sub(1)))
            .map_err(|e| ApiError::internal(e.to_string()))?;
        for (_, delta) in baseline {
            (base_capacity, base_occupied) = apply_live_capacity_delta(
                base_capacity,
                base_occupied,
                delta.live_capacity_delta,
                delta.live_occupied_capacity_delta,
                "building spore baseline occupation chart",
            )
            .map_err(|e| ApiError::internal(e.to_string()))?;
        }
        (base_capacity, base_occupied)
    } else {
        (0, 0)
    };

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
        "Spore Capacity Occupation".to_string(),
        initial_capacity,
        initial_occupied,
    )
    .map_err(|e| ApiError::internal(e.to_string()))?)
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
        .map(|(spore_id, entry)| spore_to_response(spore_id, entry, None, None))
        .collect();

    ok(CursorPaginatedResponse::without_total(
        spores,
        limit as i64,
        next_cursor,
    ))
}
