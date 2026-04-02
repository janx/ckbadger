use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use ckbadger_indexer::media_store::MediaBlobStore;
use ckbadger_indexer::parser::{build_dob1_svg, extract_dob1_pattern};

use super::assets::{
    build_nft_item_activities_response, count_nft_collection_activities_cached,
    decode_activity_cursor, decode_item_id, list_canonical_nft_collection_activities_page,
    normalize_activity_action_filter, MnftItemActivitiesParams, MnftItemActivityResponse,
    NftLifecycleStandard,
};
use super::statistics::{StackedAreaChartResponse, StackedAreaDataPoint, StackedAreaSeries};
use crate::response::{
    default_limit, ok, ApiError, ApiResult, ApiRouteError, CursorPaginatedResponse,
};
use crate::utils::{apply_owned_capacity_delta, date_keys_inclusive, parse_chart_date_range};
use crate::warmup::{CachedAssetEntry, SporeCache, CACHE_KEY_ASSETS_OBJECT};
use crate::AppState;
use ckbadger_store::types::SOLE_SPORES_SENTINEL_COLLECTION;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/spore/clusters", get(list_clusters))
        .route("/spore/clusters/{cluster_id}", get(get_cluster))
        .route(
            "/spore/clusters/{cluster_id}/charts/capacity-history",
            get(get_cluster_capacity_chart),
        )
        .route(
            "/spore/clusters/{cluster_id}/holders",
            get(get_cluster_holders),
        )
        .route(
            "/spore/clusters/{cluster_id}/activities",
            get(get_cluster_activities),
        )
        .route(
            "/spore/clusters/{cluster_id}/spores",
            get(get_spores_by_cluster),
        )
        .route("/spore/objects", get(list_spores))
        .route("/spore/objects/{spore_id}", get(get_spore))
        .route(
            "/spore/objects/{spore_id}/activities",
            get(list_spore_item_activities),
        )
        .route("/spore/objects/{spore_id}/decode", get(decode_spore))
        .route("/spore/objects/{spore_id}/media/{hash}", get(serve_media))
        .route("/spore/objects/{spore_id}/render", get(render_spore_svg))
        .route(
            "/spore/objects/{spore_id}/charts/capacity-history",
            get(get_spore_capacity_chart),
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

#[derive(Debug, Deserialize)]
pub struct ClusterHoldersParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ClusterActivitiesParams {
    #[serde(default = "default_limit")]
    limit: i64,
    cursor: Option<String>,
    action: Option<String>,
}

fn decode_cluster_holders_cursor(
    raw: &str,
) -> Result<(i64, Vec<u8>), (axum::http::StatusCode, axum::Json<ApiError>)> {
    let mut parts = raw.split(':');
    let count = parts
        .next()
        .ok_or_else(|| ApiError::bad_request("Invalid cluster holders cursor"))?
        .parse::<i64>()
        .map_err(|_| ApiError::bad_request("Invalid cluster holders cursor"))?;
    let lock_hash_hex = parts
        .next()
        .ok_or_else(|| ApiError::bad_request("Invalid cluster holders cursor"))?;
    if parts.next().is_some() {
        return Err(ApiError::bad_request("Invalid cluster holders cursor"));
    }
    let lock_hash = hex::decode(lock_hash_hex.strip_prefix("0x").unwrap_or(lock_hash_hex))
        .map_err(|_| ApiError::bad_request("Invalid cluster holders cursor"))?;
    if lock_hash.len() != 32 {
        return Err(ApiError::bad_request("Invalid cluster holders cursor"));
    }
    Ok((count, lock_hash))
}

fn decode_cluster_activity_cursor(
    raw: &str,
) -> Result<(i64, i32), (axum::http::StatusCode, axum::Json<ApiError>)> {
    let mut parts = raw.split(':');
    let block = parts
        .next()
        .ok_or_else(|| ApiError::bad_request("Invalid cluster activities cursor"))?
        .parse::<i64>()
        .map_err(|_| ApiError::bad_request("Invalid cluster activities cursor"))?;
    let tx_index = parts
        .next()
        .ok_or_else(|| ApiError::bad_request("Invalid cluster activities cursor"))?
        .parse::<i32>()
        .map_err(|_| ApiError::bad_request("Invalid cluster activities cursor"))?;
    if parts.next().is_some() {
        return Err(ApiError::bad_request("Invalid cluster activities cursor"));
    }
    Ok((block, tx_index))
}

fn normalize_cluster_activity_action_filter(
    raw: Option<&str>,
) -> Result<Option<String>, (axum::http::StatusCode, axum::Json<ApiError>)> {
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
            "Invalid cluster activity action filter. Expected one of: mint, transfer, burn",
        )),
    }
}

fn parse_fixed_len_hex(
    raw: &str,
    expected_len: usize,
    err_msg: &'static str,
) -> Result<Vec<u8>, (axum::http::StatusCode, axum::Json<ApiError>)> {
    let bytes = hex::decode(raw.strip_prefix("0x").unwrap_or(raw))
        .map_err(|_| ApiError::bad_request(err_msg))?;
    if bytes.len() != expected_len {
        return Err(ApiError::bad_request(err_msg));
    }
    Ok(bytes)
}

/// Parse a cluster_id URL parameter. Accepts "sole-spores" alias
/// or a 32-byte hex string.
fn parse_cluster_id_param(
    raw: &str,
) -> Result<Vec<u8>, (axum::http::StatusCode, axum::Json<ApiError>)> {
    if raw.eq_ignore_ascii_case("sole-spores") {
        return Ok(SOLE_SPORES_SENTINEL_COLLECTION.to_vec());
    }
    parse_fixed_len_hex(
        raw,
        32,
        "Invalid cluster ID (expected 32-byte hex or 'sole-spores')",
    )
}

fn is_sole_spores_sentinel(id: &[u8]) -> bool {
    id == SOLE_SPORES_SENTINEL_COLLECTION
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterCompositionResponse {
    pub tier: String,
    pub onchain_count: i64,
    pub pure_ckb_count: i64,
    pub decentralized_mixture_count: i64,
    pub centralized_mixture_count: i64,
    pub unknown_count: i64,
    pub onchain_ratio: String,
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
    pub holders_count: i64,
    pub activities_count: i64,
    pub created_at_block: i64,
    pub owned_capacity: Option<String>,
    pub owned_knowledge: Option<String>,
    pub composition: ClusterCompositionResponse,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SporeMediaSourceResponse {
    pub uri: String,
    pub scheme: String,
    pub source_location: String,
    pub dependency_tier: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SporeMediaProfileResponse {
    pub tier: String,
    pub sources: Vec<SporeMediaSourceResponse>,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SporeResponse {
    pub spore_id: String,
    pub tx_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_index: Option<i32>,
    pub cluster_id: Option<String>,
    pub content_type: String,
    pub content_size: i32,
    pub owner_lock_hash: String,
    pub owner_address: Option<String>,
    pub is_live: bool,
    pub created_at_block: i64,
    pub owned_capacity: Option<String>,
    pub owned_knowledge: Option<String>,
    pub media_profile: Option<SporeMediaProfileResponse>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterHolderResponse {
    pub lock_script_hash: String,
    pub address: Option<String>,
    pub item_count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterActivityResponse {
    pub tx_hash: String,
    pub block_number: i64,
    pub tx_index: i32,
    pub timestamp: String,
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DobTraitResponse {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecodedMediaResponse {
    pub media_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub size: u64,
    pub hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<u32>,
    pub url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SporeDobDecodeResponse {
    pub status: String,
    pub spore_id: String,
    pub content_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dna_hex: Option<String>,
    pub traits: Vec<DobTraitResponse>,
    #[serde(default)]
    pub media: Vec<DecodedMediaResponse>,
    pub issues: Vec<String>,
}

/// Convert an ObjectEntry from the store into a SporeResponse.
fn spore_to_response(
    spore_id: &[u8],
    entry: &ckbadger_store::ObjectEntry,
    owned_capacity: Option<i128>,
    owned_knowledge: Option<i128>,
) -> SporeResponse {
    let (content_type, content_size, media_profile) = match &entry.extra {
        ckbadger_store::ObjectExtra::Spore {
            content_type,
            content_length,
            media_profile,
        } => (
            content_type.clone(),
            *content_length as i32,
            Some(SporeMediaProfileResponse {
                tier: media_profile.tier.as_str().to_string(),
                sources: media_profile
                    .sources
                    .iter()
                    .map(|source| SporeMediaSourceResponse {
                        uri: source.uri.clone(),
                        scheme: source.scheme.clone(),
                        source_location: source.source_location.clone(),
                        dependency_tier: source.dependency_tier.as_str().to_string(),
                    })
                    .collect(),
                issues: media_profile.issues.clone(),
            }),
        ),
        _ => (String::new(), 0, None),
    };
    SporeResponse {
        spore_id: format!("0x{}", hex::encode(spore_id)),
        tx_hash: format!("0x{}", hex::encode(&entry.created_at_tx)),
        output_index: None, // ObjectEntry does not store output_index; needs schema addition
        cluster_id: entry
            .collection_id
            .as_ref()
            .filter(|c| !is_sole_spores_sentinel(c))
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
        owned_capacity: owned_capacity.map(|v| v.to_string()),
        owned_knowledge: owned_knowledge.map(|v| v.to_string()),
        media_profile,
    }
}

fn format_yyyymmdd_for_chart(date: u32) -> String {
    let s = format!("{date:08}");
    format!("{}-{}-{}", &s[0..4], &s[4..6], &s[6..8])
}

fn build_capacity_history_chart(
    deltas: Vec<(u32, i128, i128)>,
    title: String,
) -> anyhow::Result<StackedAreaChartResponse> {
    build_capacity_history_chart_with_initial(deltas, title, 0, 0, None, None)
}

fn build_capacity_history_chart_with_initial(
    deltas: Vec<(u32, i128, i128)>,
    title: String,
    initial_capacity: i128,
    initial_used: i128,
    from_date: Option<u32>,
    to_date: Option<u32>,
) -> anyhow::Result<StackedAreaChartResponse> {
    if initial_capacity < 0 {
        anyhow::bail!(
            "invalid initial capacity for spore chart: {}",
            initial_capacity
        );
    }
    if initial_used < 0 {
        anyhow::bail!(
            "invalid initial common knowledge size for spore chart: {}",
            initial_used
        );
    }
    if initial_used > initial_capacity {
        anyhow::bail!(
            "invalid initial common knowledge size/capacity for spore chart: used={}, capacity={}",
            initial_used,
            initial_capacity
        );
    }
    let mut daily_deltas: BTreeMap<u32, (i128, i128)> = BTreeMap::new();
    for (date, capacity_delta, used_delta) in deltas {
        let entry = daily_deltas.entry(date).or_insert((0, 0));
        entry.0 = entry.0.checked_add(capacity_delta).ok_or_else(|| {
            anyhow::anyhow!(
                "capacity delta overflow while building spore capacity history chart: date={}",
                date
            )
        })?;
        entry.1 = entry.1.checked_add(used_delta).ok_or_else(|| {
            anyhow::anyhow!(
                "used delta overflow while building spore capacity history chart: date={}",
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

    let mut running_capacity = initial_capacity;
    let mut running_used = initial_used;
    let mut data = Vec::with_capacity(dates.len());

    for date in dates {
        let (capacity_delta, used_delta) = daily_deltas.get(&date).copied().unwrap_or((0, 0));
        (running_capacity, running_used) = apply_owned_capacity_delta(
            running_capacity,
            running_used,
            capacity_delta,
            used_delta,
            &format!("building spore capacity history chart at date {}", date),
        )?;
        let unused = running_capacity - running_used;
        let mut values = std::collections::HashMap::new();
        values.insert("used".to_string(), running_used.to_string());
        values.insert("unused".to_string(), unused.to_string());

        data.push(StackedAreaDataPoint {
            date: format_yyyymmdd_for_chart(date),
            values,
        });
    }

    Ok(StackedAreaChartResponse {
        data,
        series: vec![
            StackedAreaSeries {
                key: "used".to_string(),
                label: "Used".to_string(),
                color: "#f59e0b".to_string(),
            },
            StackedAreaSeries {
                key: "unused".to_string(),
                label: "Unused".to_string(),
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
        let used = last.values.get("used").cloned();
        let unused = last.values.get("unused").cloned();
        let capacity = match (&used, &unused) {
            (Some(o), Some(u)) => {
                let total = o.parse::<i128>().unwrap_or(0) + u.parse::<i128>().unwrap_or(0);
                Some(total.to_string())
            }
            _ => None,
        };
        return (capacity, used);
    }
    (Some("0".to_string()), Some("0".to_string()))
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

fn resolve_composition_tier(
    btc_ckb: i64,
    pure_ckb: i64,
    decentralized_mixture: i64,
    centralized_mixture: i64,
    unknown: i64,
) -> String {
    if centralized_mixture > 0 {
        return "centralized_mixture".to_string();
    }
    if decentralized_mixture > 0 {
        return "decentralized_mixture".to_string();
    }
    let total_onchain = btc_ckb + pure_ckb;
    if total_onchain > 0 && unknown == 0 {
        if btc_ckb > 0 {
            return "btc_ckb".to_string();
        }
        return "pure_ckb".to_string();
    }
    "unknown".to_string()
}

fn cluster_composition_from_aggregate(
    aggregate: Option<&ckbadger_store::types::ClusterAggregate>,
    spores_count: i64,
) -> ClusterCompositionResponse {
    let btc_ckb_count = aggregate.map(|a| a.btc_ckb_count).unwrap_or(0);
    let pure_ckb_count = aggregate.map(|a| a.pure_ckb_count).unwrap_or(0);
    let decentralized_mixture_count = aggregate
        .map(|a| a.decentralized_mixture_count)
        .unwrap_or(0);
    let centralized_mixture_count = aggregate.map(|a| a.centralized_mixture_count).unwrap_or(0);
    let unknown_count = aggregate
        .map(|a| a.unknown_count)
        .unwrap_or(spores_count.max(0));
    let total_onchain = btc_ckb_count + pure_ckb_count;
    ClusterCompositionResponse {
        tier: resolve_composition_tier(
            btc_ckb_count,
            pure_ckb_count,
            decentralized_mixture_count,
            centralized_mixture_count,
            unknown_count,
        ),
        onchain_count: total_onchain,
        pure_ckb_count,
        decentralized_mixture_count,
        centralized_mixture_count,
        unknown_count,
        onchain_ratio: format_ratio_4(total_onchain, spores_count),
    }
}

/// List clusters — use cached object assets (filtered to Spore) when available.
async fn list_clusters(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<ClusterResponse>> {
    let limit = params.limit.clamp(1, 100) as usize;
    let cursor_block = params.cursor.unwrap_or(i64::MAX);

    // Try cached object assets first (Spore entries carry cluster grouping)
    if let Some(cached_objects) = state
        .mem_cache
        .get::<Vec<CachedAssetEntry>>(CACHE_KEY_ASSETS_OBJECT)
    {
        return serve_clusters_from_cache(cached_objects, cursor_block, limit, &state);
    }

    Err(state.asset_cache_unavailable("cluster cache unavailable; warmup in progress"))
}

fn serve_clusters_from_cache(
    cached: Vec<CachedAssetEntry>,
    cursor_block: i64,
    limit: usize,
    state: &Arc<AppState>,
) -> ApiResult<CursorPaginatedResponse<ClusterResponse>> {
    let cluster_ids = unique_cluster_ids_from_cached_entries(&cached);

    let spore_entries = state
        .store
        .get_spores_batch(&cluster_ids)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut spore_map: HashMap<Vec<u8>, ckbadger_store::ObjectEntry> =
        HashMap::with_capacity(spore_entries.len());
    for (cluster_id, entry_opt) in spore_entries {
        if let Some(entry) = entry_opt {
            spore_map.insert(cluster_id, entry);
        }
    }

    let cluster_aggregates = state
        .store
        .get_cluster_aggregates_batch(&cluster_ids)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut cluster_agg_map: HashMap<Vec<u8>, ckbadger_store::ClusterAggregate> =
        HashMap::with_capacity(cluster_aggregates.len());
    for (cluster_id, aggregate_opt) in cluster_aggregates {
        if let Some(aggregate) = aggregate_opt {
            cluster_agg_map.insert(cluster_id, aggregate);
        }
    }

    let clusters =
        build_cluster_responses_from_cached_entries(cached, &spore_map, &cluster_agg_map);

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

fn cluster_id_bytes_from_cached_entry(entry: &CachedAssetEntry) -> Option<Vec<u8>> {
    if entry.standard != "spore" {
        return None;
    }
    let cluster_id_hex = entry.cluster_id.as_ref().unwrap_or(&entry.id);
    hex::decode(cluster_id_hex.strip_prefix("0x").unwrap_or(cluster_id_hex)).ok()
}

fn unique_cluster_ids_from_cached_entries(cached: &[CachedAssetEntry]) -> Vec<Vec<u8>> {
    let mut unique_ids = Vec::new();
    let mut seen: HashSet<Vec<u8>> = HashSet::new();
    for cluster_id in cached.iter().filter_map(cluster_id_bytes_from_cached_entry) {
        if seen.insert(cluster_id.clone()) {
            unique_ids.push(cluster_id);
        }
    }
    unique_ids
}

fn build_cluster_responses_from_cached_entries(
    cached: Vec<CachedAssetEntry>,
    spore_map: &HashMap<Vec<u8>, ckbadger_store::ObjectEntry>,
    cluster_agg_map: &HashMap<Vec<u8>, ckbadger_store::ClusterAggregate>,
) -> Vec<ClusterResponse> {
    let mut clusters = Vec::new();

    for entry in cached {
        let Some(cluster_id) = cluster_id_bytes_from_cached_entry(&entry) else {
            continue;
        };

        let cluster_entry = spore_map.get(&cluster_id);
        let cluster_aggregate = cluster_agg_map.get(&cluster_id);
        let created_at_block = cluster_entry.map(|e| e.created_at_block).unwrap_or(0);
        let description = cluster_entry.and_then(|e| e.description.clone());
        let owner_lock_hash = cluster_entry.and_then(|e| e.owner_lock_hash.clone());

        clusters.push(ClusterResponse {
            cluster_id: entry.id,
            name: entry.name,
            description,
            owner_lock_hash: owner_lock_hash
                .as_ref()
                .map(|h| format!("0x{}", hex::encode(h)))
                .unwrap_or_default(),
            owner_address: None,
            spores_count: entry.transfers_count as i32, // transfers_count holds spore count for DOB
            holders_count: cluster_aggregate.map(|a| a.owner_count).unwrap_or(0),
            activities_count: 0,
            created_at_block,
            owned_capacity: None,
            owned_knowledge: None,
            composition: cluster_composition_from_aggregate(
                cluster_aggregate,
                entry.transfers_count,
            ),
        });
    }

    clusters.sort_by(|a, b| b.created_at_block.cmp(&a.created_at_block));
    clusters
}

fn load_spore_cache(
    state: &AppState,
) -> Result<arc_swap::Guard<Arc<Option<SporeCache>>>, ApiRouteError> {
    let guard = state.spore_cache.load();
    if guard.is_some() {
        Ok(guard)
    } else {
        Err(ApiError::warmup_pending(
            "spore cache unavailable; warmup in progress",
        ))
    }
}

async fn get_cluster_holders(
    State(state): State<Arc<AppState>>,
    Path(cluster_id): Path<String>,
    Query(params): Query<ClusterHoldersParams>,
) -> ApiResult<CursorPaginatedResponse<ClusterHolderResponse>> {
    let id = parse_cluster_id_param(&cluster_id)?;
    let limit = params.limit.clamp(1, 100) as usize;
    let cursor = params
        .cursor
        .as_deref()
        .map(decode_cluster_holders_cursor)
        .transpose()?;

    let store = state.store.clone();
    let id_c = id.clone();
    let owners = tokio::task::spawn_blocking(move || store.list_cluster_owner_counts(&id_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;
    if owners.is_empty() && !is_sole_spores_sentinel(&id) {
        let store = state.store.clone();
        let id_c = id.clone();
        let cluster_exists = tokio::task::spawn_blocking(move || store.get_spore(&id_c))
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .map_err(|e| ApiError::internal(e.to_string()))?
            .is_some();
        if !cluster_exists {
            return Err(ApiError::not_found("Cluster not found"));
        }
    }

    let mut rows = owners;
    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let total = rows.len() as i64;
    let start_idx = if let Some((cursor_count, cursor_lock_hash)) = cursor {
        rows.iter()
            .position(|(lock_hash, count)| *count == cursor_count && *lock_hash == cursor_lock_hash)
            .map(|idx| idx + 1)
            .ok_or_else(|| ApiError::bad_request("Invalid cluster holders cursor"))?
    } else {
        0
    };

    let page: Vec<_> = rows.iter().skip(start_idx).take(limit + 1).collect();
    let has_more = page.len() > limit;
    let page: Vec<_> = page.into_iter().take(limit).collect();
    let next_cursor = if has_more {
        page.last()
            .map(|(lock_hash, count)| format!("{}:{}", count, hex::encode(lock_hash)))
    } else {
        None
    };

    let response_rows: Vec<ClusterHolderResponse> = page
        .into_iter()
        .map(|(lock_hash, count)| ClusterHolderResponse {
            lock_script_hash: format!("0x{}", hex::encode(lock_hash)),
            address: None,
            item_count: *count,
        })
        .collect();

    ok(CursorPaginatedResponse::new(
        response_rows,
        total,
        limit as i64,
        next_cursor,
    ))
}

async fn get_cluster_activities(
    State(state): State<Arc<AppState>>,
    Path(cluster_id): Path<String>,
    Query(params): Query<ClusterActivitiesParams>,
) -> ApiResult<CursorPaginatedResponse<ClusterActivityResponse>> {
    let id = parse_cluster_id_param(&cluster_id)?;
    let limit = params.limit.clamp(1, 100);
    let cursor = params
        .cursor
        .as_deref()
        .map(decode_cluster_activity_cursor)
        .transpose()?;
    let action_filter = normalize_cluster_activity_action_filter(params.action.as_deref())?;

    // Validate cluster exists (sentinel always passes)
    if !is_sole_spores_sentinel(&id) {
        let store = state.store.clone();
        let id_c = id.clone();
        let cluster_exists = tokio::task::spawn_blocking(move || store.get_spore(&id_c))
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .map_err(|e| ApiError::internal(e.to_string()))?
            .is_some();
        if !cluster_exists {
            return Err(ApiError::not_found("Cluster not found"));
        }
    }

    // Use pre-computed collection activity index and drop orphaned history rows.
    let results = list_canonical_nft_collection_activities_page(
        state.store.as_ref(),
        state.store.as_ref(),
        &id,
        (limit as usize) + 1,
        cursor,
        action_filter.as_deref(),
    )
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let has_more = results.len() as i64 > limit;
    let page: Vec<ClusterActivityResponse> = results
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
            ClusterActivityResponse {
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

/// Get spores by cluster — served from in-memory SporeCache (zero RocksDB reads).
async fn get_spores_by_cluster(
    State(state): State<Arc<AppState>>,
    Path(cluster_id): Path<String>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<SporeResponse>> {
    let id = parse_cluster_id_param(&cluster_id)?;

    let limit = params.limit.clamp(1, 100) as usize;
    let cursor_block = params.cursor.unwrap_or(i64::MAX);

    let guard = load_spore_cache(&state)?;
    let cache = guard.as_ref().as_ref().unwrap();

    // by_cluster indices are already sorted by created_at_block desc (insertion order).
    // Filter to live spores with created_at_block < cursor_block.
    let selected: Vec<_> = cache
        .by_cluster
        .get(&id)
        .map(|indices| {
            indices
                .iter()
                .filter(|&&i| {
                    let entry = &cache.all[i].1;
                    entry.is_live && entry.created_at_block < cursor_block
                })
                .take(limit + 1)
                .map(|&i| &cache.all[i])
                .collect()
        })
        .unwrap_or_default();

    let has_more = selected.len() > limit;
    let page: Vec<_> = selected.into_iter().take(limit).collect();

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
    let id = parse_cluster_id_param(&cluster_id)?;

    // Look up the cluster entry directly
    let store = state.store.clone();
    let id_c = id.clone();
    let cluster_entry = tokio::task::spawn_blocking(move || store.get_spore(&id_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let store = state.store.clone();
    let id_c = id.clone();
    let cluster_aggregate = tokio::task::spawn_blocking(move || store.get_cluster_aggregate(&id_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Read counts from pre-computed aggregate (zero RocksDB scans).
    let spores_count = cluster_aggregate
        .as_ref()
        .map(|agg| agg.total_count)
        .unwrap_or(0);

    if spores_count == 0 && cluster_entry.is_none() && !is_sole_spores_sentinel(&id) {
        return Err(ApiError::not_found("Cluster not found"));
    }

    let holders_count = cluster_aggregate
        .as_ref()
        .map(|agg| agg.owner_count)
        .unwrap_or(0);
    let activities_count = count_nft_collection_activities_cached(
        state.store.as_ref(),
        state.store.as_ref(),
        &state.mem_cache,
        &id,
    )
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let (name, description, owner_lock_hash, created_at_block) = if is_sole_spores_sentinel(&id) {
        (
            Some("Sole Spores".to_string()),
            Some("Spores not belonging to any cluster".to_string()),
            None,
            0i64,
        )
    } else {
        let name = cluster_entry.as_ref().and_then(|e| e.name.clone());
        let description = cluster_entry.as_ref().and_then(|e| e.description.clone());
        let owner_lock_hash = cluster_entry
            .as_ref()
            .and_then(|e| e.owner_lock_hash.clone());
        let created_at_block = cluster_entry
            .as_ref()
            .map(|e| e.created_at_block)
            .unwrap_or(0);
        (name, description, owner_lock_hash, created_at_block)
    };
    let owned_capacity = cluster_aggregate
        .as_ref()
        .map(|agg| agg.owned_capacity.to_string());
    let owned_knowledge = cluster_aggregate
        .as_ref()
        .map(|agg| agg.owned_knowledge.to_string());

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
        holders_count,
        activities_count,
        created_at_block,
        owned_capacity,
        owned_knowledge,
        composition: cluster_composition_from_aggregate(cluster_aggregate.as_ref(), spores_count),
    })
}

async fn list_spores(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<SporeResponse>> {
    let limit = params.limit.clamp(1, 100) as usize;
    let cursor_block = params.cursor.unwrap_or(i64::MAX);

    let guard = load_spore_cache(&state)?;
    let cache = guard.as_ref().as_ref().unwrap();

    let start = cache
        .live_indices
        .iter()
        .position(|&i| cache.all[i].1.created_at_block < cursor_block)
        .unwrap_or(cache.live_indices.len());

    let selected: Vec<_> = cache.live_indices[start..]
        .iter()
        .take(limit + 1)
        .map(|&i| &cache.all[i])
        .collect();

    let has_more = selected.len() > limit;
    let page = &selected[..selected.len().min(limit)];

    let next_cursor = if has_more {
        page.last()
            .map(|(_, entry)| entry.created_at_block.to_string())
    } else {
        None
    };

    let spores: Vec<SporeResponse> = page
        .iter()
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

    let store = state.store.clone();
    let id_c = id.clone();
    let entry = tokio::task::spawn_blocking(move || store.get_spore(&id_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;

    match entry {
        Some(entry) if entry.standard.is_cluster() => Err(ApiError::not_found("Spore not found")),
        Some(entry) => {
            let daily = state
                .store
                .list_spore_daily_deltas(&id)
                .map_err(|e| ApiError::internal(e.to_string()))?;
            let chart = build_capacity_history_chart(
                daily
                    .into_iter()
                    .map(|(date, delta)| {
                        (
                            date,
                            delta.owned_capacity_delta,
                            delta.owned_knowledge_delta,
                        )
                    })
                    .collect(),
                "Spore Capacity History".to_string(),
            )
            .map_err(|e| ApiError::internal(e.to_string()))?;
            let (owned_capacity, owned_knowledge) = latest_capacity_from_chart(&chart);
            let cap = owned_capacity.and_then(|v| v.parse::<i128>().ok());
            let occ = owned_knowledge.and_then(|v| v.parse::<i128>().ok());
            ok(spore_to_response(&id, &entry, cap, occ))
        }
        None => Err(ApiError::not_found("Spore not found")),
    }
}

async fn list_spore_item_activities(
    State(state): State<Arc<AppState>>,
    Path(spore_id): Path<String>,
    Query(params): Query<MnftItemActivitiesParams>,
) -> ApiResult<CursorPaginatedResponse<MnftItemActivityResponse>> {
    let limit = params.limit.clamp(1, 100);
    let action_filter = normalize_activity_action_filter(params.action.as_deref())?;
    let spore_id_bytes = decode_item_id(&spore_id)?;

    // Verify the spore exists and is not a cluster
    let entry = state
        .store
        .get_spore(&spore_id_bytes)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Spore not found"))?;
    if entry.standard.is_cluster() {
        return Err(ApiError::not_found("Spore not found"));
    }

    let cursor = params
        .cursor
        .as_deref()
        .map(decode_activity_cursor)
        .transpose()?;
    let response = build_nft_item_activities_response(
        &state,
        &spore_id_bytes,
        NftLifecycleStandard::Spore,
        limit,
        cursor,
        action_filter.as_deref(),
    )?;
    ok(response)
}

async fn decode_spore(
    State(state): State<Arc<AppState>>,
    Path(spore_id): Path<String>,
) -> ApiResult<SporeDobDecodeResponse> {
    let id = hex::decode(spore_id.strip_prefix("0x").unwrap_or(&spore_id))
        .map_err(|_| ApiError::bad_request("Invalid spore ID"))?;

    let store = state.store.clone();
    let id_c = id.clone();
    let entry = tokio::task::spawn_blocking(move || store.get_spore(&id_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Spore not found"))?;

    let content_type = match &entry.extra {
        ckbadger_store::ObjectExtra::Spore { content_type, .. } => content_type.clone(),
        _ => String::new(),
    };

    // Check CF_DOB_DECODED for cached decode result
    let store = state.store.clone();
    let id_for_decode = id.clone();
    let decoded = tokio::task::spawn_blocking(move || store.get_dob_decoded(&id_for_decode))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let spore_id_hex = format!("0x{}", hex::encode(&id));

    match decoded {
        Some(decoded_entry) => {
            // Merge traits across all steps: later steps override same-name traits,
            // earlier steps' unique traits are preserved. Insertion order maintained.
            let mut traits: Vec<DobTraitResponse> = Vec::new();
            for step in &decoded_entry.steps {
                for t in &step.traits {
                    if let Some(existing) = traits.iter_mut().find(|r| r.name == t.name) {
                        existing.value = t.value.clone();
                    } else {
                        traits.push(DobTraitResponse {
                            name: t.name.clone(),
                            value: t.value.clone(),
                        });
                    }
                }
            }

            // One media entry per step
            let mut media: Vec<DecodedMediaResponse> = decoded_entry
                .steps
                .iter()
                .map(|step| DecodedMediaResponse {
                    media_type: step.media_type.clone(),
                    role: None,
                    size: step.size,
                    hash: step.hash.clone(),
                    step: Some(step.step),
                    url: format!("/spore/objects/{}/media/{}", spore_id_hex, step.hash),
                })
                .collect();

            // Check all steps for renderable SVG in trait values or raw output.
            // If found, add a render URL so the frontend can display it.
            let has_image_media = decoded_entry
                .steps
                .iter()
                .any(|s| s.media_type.starts_with("image/"));
            if !has_image_media {
                let all_traits: Vec<_> = decoded_entry
                    .steps
                    .iter()
                    .flat_map(|s| s.traits.iter())
                    .collect();
                let has_svg_in_traits = all_traits.iter().any(|t| {
                    let trimmed = t.value.trim();
                    trimmed
                        .get(..4)
                        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("<svg"))
                });
                let has_svg = if has_svg_in_traits {
                    true
                } else if let Some(cluster_id) = entry.collection_id.as_deref() {
                    let store = state.store.clone();
                    let cid = cluster_id.to_vec();
                    tokio::task::spawn_blocking(move || {
                        store
                            .get_spore(&cid)
                            .ok()
                            .flatten()
                            .and_then(|c| c.description)
                            .and_then(|desc| serde_json::from_str::<serde_json::Value>(&desc).ok())
                            .map(|meta| !extract_dob1_pattern(&meta).is_empty())
                            .unwrap_or(false)
                    })
                    .await
                    .unwrap_or(false)
                } else {
                    false
                };

                if has_svg {
                    media.push(DecodedMediaResponse {
                        media_type: "image/svg+xml".to_string(),
                        role: Some("render".to_string()),
                        size: 0,
                        hash: String::new(),
                        step: None,
                        url: format!("/spore/objects/{}/render", spore_id_hex),
                    });
                }
            }

            ok(SporeDobDecodeResponse {
                status: "decoded".to_string(),
                spore_id: spore_id_hex,
                content_type,
                dna_hex: None,
                traits,
                media,
                issues: Vec::new(),
            })
        }
        None => ok(SporeDobDecodeResponse {
            status: "pending".to_string(),
            spore_id: spore_id_hex,
            content_type,
            dna_hex: None,
            traits: Vec::new(),
            media: vec![],
            issues: vec![
                "DOB decode pending — background worker has not processed this spore yet"
                    .to_string(),
            ],
        }),
    }
}

/// Serve a content-addressed media blob for a decoded DOB spore.
///
/// `GET /spore/objects/{spore_id}/media/{hash}`
///
/// Returns the raw blob bytes with the appropriate Content-Type header.
/// Content-addressed blobs are immutable, so the response sets a long-lived
/// `Cache-Control` header.
async fn serve_media(
    State(state): State<Arc<AppState>>,
    Path((spore_id, hash)): Path<(String, String)>,
) -> Result<Response, (StatusCode, axum::Json<ApiError>)> {
    // 1. Parse spore_id and validate hash parameter
    if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ApiError::bad_request(
            "Invalid media hash: expected exactly 64 hex characters",
        ));
    }

    let id = hex::decode(spore_id.strip_prefix("0x").unwrap_or(&spore_id))
        .map_err(|_| ApiError::bad_request("Invalid spore ID hex"))?;

    // 2. Load decoded entry to validate hash membership
    let store = state.store.clone();
    let id_for_decode = id.clone();
    let decoded_entry = tokio::task::spawn_blocking(move || store.get_dob_decoded(&id_for_decode))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("No decoded data for this spore"))?;

    // 3. Validate that the requested hash exists in a decode step
    let matched_step = decoded_entry
        .steps
        .iter()
        .find(|s| s.hash == hash)
        .ok_or_else(|| ApiError::not_found("Media hash not found for this spore"))?;

    let content_type = matched_step.media_type.clone();

    // 4. Load spore entry to get collection_id for filesystem path
    let store = state.store.clone();
    let id_for_spore = id.clone();
    let spore_entry = tokio::task::spawn_blocking(move || store.get_spore(&id_for_spore))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Spore not found"))?;

    let collection_id = spore_entry.collection_id.as_deref().unwrap_or(&id);

    // 5. Read blob from filesystem
    let blob_store = MediaBlobStore::new(state.dob_decode_dir.clone());
    let blob_hash = hash.clone();
    let collection_id_owned = collection_id.to_vec();
    let blob =
        tokio::task::spawn_blocking(move || blob_store.read(&collection_id_owned, &blob_hash))
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .map_err(|e| ApiError::internal(format!("Failed to read media blob: {e}")))?;

    // 6. Return binary response with security headers.
    //
    // Media blobs are untrusted on-chain data (DOB decoder output). A malicious
    // decoder could emit SVG/HTML containing <script> or event handlers. Without
    // mitigation, navigating directly to this URL would execute attacker JS in
    // the API origin, enabling cookie/localStorage theft (stored XSS).
    //
    // Defence layers:
    // - CSP `default-src 'none'` blocks all script execution, even inline
    //   handlers and <script> tags. `style-src 'unsafe-inline'` permits SVG
    //   presentation attributes. `img-src data:` permits embedded data URIs
    //   within SVG.
    // - `X-Content-Type-Options: nosniff` prevents browsers from MIME-sniffing
    //   a response into a dangerous type.
    // - `Content-Disposition: attachment` on non-image types forces download
    //   instead of inline rendering, preventing untrusted HTML/SVG execution.
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, &content_type)
        .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
        .header(
            header::CONTENT_SECURITY_POLICY,
            "default-src 'none'; style-src 'unsafe-inline'; img-src data:",
        )
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff");

    if !content_type.starts_with("image/") {
        builder = builder.header(header::CONTENT_DISPOSITION, "attachment");
    }

    Ok(builder
        .body(axum::body::Body::from(blob))
        .map_err(|e| ApiError::internal(e.to_string()))?
        .into_response())
}

/// Render DOB SVG on-the-fly from decoded traits or cluster patterns.
///
/// `GET /spore/objects/{spore_id}/render`
///
/// Two SVG sources (tried in order):
/// 1. Inline SVG in decoded trait values (some decoders emit SVG as a trait)
/// 2. SVG built from cluster DOB1 pattern templates + decoded trait values
///
/// Returns `image/svg+xml` with security headers.
async fn render_spore_svg(
    State(state): State<Arc<AppState>>,
    Path(spore_id): Path<String>,
) -> Result<Response, (StatusCode, axum::Json<ApiError>)> {
    let id = hex::decode(spore_id.strip_prefix("0x").unwrap_or(&spore_id))
        .map_err(|_| ApiError::bad_request("Invalid spore ID hex"))?;

    // Load decoded traits
    let store = state.store.clone();
    let id_c = id.clone();
    let decoded = tokio::task::spawn_blocking(move || store.get_dob_decoded(&id_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Spore has not been decoded yet"))?;

    // Collect all traits from all steps
    let all_traits: Vec<&ckbadger_store::DobDecodedTrait> =
        decoded.steps.iter().flat_map(|s| s.traits.iter()).collect();

    // Source 1: extract inline SVG from trait values (fast path — no cluster load needed)
    if let Some(svg) = extract_svg_from_decoded_traits(&all_traits) {
        return Ok(svg_response(svg));
    }

    // Source 2: build SVG from DOB1 pattern templates + decoded trait values
    let store = state.store.clone();
    let id_c = id.clone();
    let spore_entry = tokio::task::spawn_blocking(move || store.get_spore(&id_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Spore not found"))?;

    let cluster_id = spore_entry
        .collection_id
        .as_deref()
        .ok_or_else(|| ApiError::not_found("Spore has no collection"))?
        .to_vec();

    let store = state.store.clone();
    let cluster_entry = tokio::task::spawn_blocking(move || store.get_spore(&cluster_id))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Cluster not found"))?;

    let metadata: serde_json::Value = cluster_entry
        .description
        .as_deref()
        .and_then(|d| serde_json::from_str(d).ok())
        .ok_or_else(|| ApiError::not_found("Cluster has no valid JSON description"))?;

    // Merge traits for pattern rendering (later steps override same-name)
    let mut trait_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for t in &all_traits {
        trait_map.insert(t.name.clone(), t.value.clone());
    }

    let patterns = extract_dob1_pattern(&metadata);
    let svg = build_dob1_svg(&patterns, &trait_map)
        .ok_or_else(|| ApiError::not_found("No SVG can be rendered for this spore"))?;

    Ok(svg_response(svg))
}

fn svg_response(svg: String) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "image/svg+xml")
        .header(header::CACHE_CONTROL, "public, max-age=86400")
        .header(
            header::CONTENT_SECURITY_POLICY,
            "default-src 'none'; style-src 'unsafe-inline'; img-src data:",
        )
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .body(axum::body::Body::from(svg))
        .expect("valid response")
        .into_response()
}

/// Extract SVG markup from decoded trait values across all steps.
fn extract_svg_from_decoded_traits(
    traits: &[&ckbadger_store::types::DobDecodedTrait],
) -> Option<String> {
    for t in traits {
        let trimmed = t.value.trim();
        if trimmed
            .get(..4)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("<svg"))
        {
            return Some(t.value.clone());
        }
    }
    None
}

async fn get_cluster_capacity_chart(
    State(state): State<Arc<AppState>>,
    Path(cluster_id): Path<String>,
    Query(params): Query<ChartRangeParams>,
) -> ApiResult<StackedAreaChartResponse> {
    let (from_date, to_date) = parse_chart_date_range(params.from.as_deref(), params.to.as_deref())
        .map_err(|msg| ApiError::bad_request(&msg))?;

    let id = parse_cluster_id_param(&cluster_id)?;

    let store = state.store.clone();
    let id_c = id.clone();
    let cluster_entry = tokio::task::spawn_blocking(move || store.get_spore(&id_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let store = state.store.clone();
    let id_c = id.clone();
    let spores_count = tokio::task::spawn_blocking(move || store.count_spores_in_cluster(&id_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;
    if spores_count == 0 && cluster_entry.is_none() && !is_sole_spores_sentinel(&id) {
        return Err(ApiError::not_found("Cluster not found"));
    }

    let name = if is_sole_spores_sentinel(&id) {
        "Sole Spores".to_string()
    } else {
        cluster_entry
            .as_ref()
            .and_then(|e| e.name.clone())
            .unwrap_or_else(|| "Spore Cluster".to_string())
    };
    let store = state.store.clone();
    let id_c = id.clone();
    let daily = tokio::task::spawn_blocking(move || {
        store.list_cluster_daily_deltas_in_range(&id_c, from_date, to_date)
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e| ApiError::internal(e.to_string()))?;
    let (initial_capacity, initial_used) = if let Some(from) = from_date {
        let mut base_capacity: i128 = 0;
        let mut base_used: i128 = 0;
        let baseline = state
            .store
            .list_cluster_daily_deltas_in_range(&id, None, Some(from.saturating_sub(1)))
            .map_err(|e| ApiError::internal(e.to_string()))?;
        for (_, delta) in baseline {
            (base_capacity, base_used) = apply_owned_capacity_delta(
                base_capacity,
                base_used,
                delta.owned_capacity_delta,
                delta.owned_knowledge_delta,
                "building cluster baseline capacity history chart",
            )
            .map_err(|e| ApiError::internal(e.to_string()))?;
        }
        (base_capacity, base_used)
    } else {
        (0, 0)
    };

    ok(build_capacity_history_chart_with_initial(
        daily
            .into_iter()
            .map(|(date, delta)| {
                (
                    date,
                    delta.owned_capacity_delta,
                    delta.owned_knowledge_delta,
                )
            })
            .collect(),
        format!("{name} Capacity History"),
        initial_capacity,
        initial_used,
        from_date,
        to_date,
    )
    .map_err(|e| ApiError::internal(e.to_string()))?)
}

async fn get_spore_capacity_chart(
    State(state): State<Arc<AppState>>,
    Path(spore_id): Path<String>,
    Query(params): Query<ChartRangeParams>,
) -> ApiResult<StackedAreaChartResponse> {
    let (from_date, to_date) = parse_chart_date_range(params.from.as_deref(), params.to.as_deref())
        .map_err(|msg| ApiError::bad_request(&msg))?;

    let id = hex::decode(spore_id.strip_prefix("0x").unwrap_or(&spore_id))
        .map_err(|_| ApiError::bad_request("Invalid spore ID"))?;

    let store = state.store.clone();
    let id_c = id.clone();
    let entry = tokio::task::spawn_blocking(move || store.get_spore(&id_c))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;
    if entry.is_none() {
        return Err(ApiError::not_found("Spore not found"));
    }

    let store = state.store.clone();
    let id_c = id.clone();
    let daily = tokio::task::spawn_blocking(move || {
        store.list_spore_daily_deltas_in_range(&id_c, from_date, to_date)
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(|e| ApiError::internal(e.to_string()))?;
    let (initial_capacity, initial_used) = if let Some(from) = from_date {
        let mut base_capacity: i128 = 0;
        let mut base_used: i128 = 0;
        let baseline = state
            .store
            .list_spore_daily_deltas_in_range(&id, None, Some(from.saturating_sub(1)))
            .map_err(|e| ApiError::internal(e.to_string()))?;
        for (_, delta) in baseline {
            (base_capacity, base_used) = apply_owned_capacity_delta(
                base_capacity,
                base_used,
                delta.owned_capacity_delta,
                delta.owned_knowledge_delta,
                "building spore baseline capacity history chart",
            )
            .map_err(|e| ApiError::internal(e.to_string()))?;
        }
        (base_capacity, base_used)
    } else {
        (0, 0)
    };

    ok(build_capacity_history_chart_with_initial(
        daily
            .into_iter()
            .map(|(date, delta)| {
                (
                    date,
                    delta.owned_capacity_delta,
                    delta.owned_knowledge_delta,
                )
            })
            .collect(),
        "Spore Capacity History".to_string(),
        initial_capacity,
        initial_used,
        from_date,
        to_date,
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

    let guard = load_spore_cache(&state)?;
    let cache = guard.as_ref().as_ref().unwrap();

    let selected: Vec<_> = cache
        .by_owner
        .get(&hash)
        .map(|indices| {
            let start = indices
                .iter()
                .position(|&i| cache.all[i].1.created_at_block < cursor_block)
                .unwrap_or(indices.len());

            indices[start..]
                .iter()
                .take(limit + 1)
                .map(|&i| &cache.all[i])
                .collect()
        })
        .unwrap_or_default();

    let has_more = selected.len() > limit;
    let page = &selected[..selected.len().min(limit)];

    let next_cursor = if has_more {
        page.last()
            .map(|(_, entry)| entry.created_at_block.to_string())
    } else {
        None
    };

    let spores: Vec<SporeResponse> = page
        .iter()
        .map(|(spore_id, entry)| spore_to_response(spore_id, entry, None, None))
        .collect();

    ok(CursorPaginatedResponse::without_total(
        spores,
        limit as i64,
        next_cursor,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_spore_entry(
        created_at_block: i64,
        owner_lock_hash: Option<Vec<u8>>,
    ) -> ckbadger_store::ObjectEntry {
        ckbadger_store::ObjectEntry {
            standard: ckbadger_store::ObjectStandard::Spore,
            collection_id: None,
            token_id: None,
            owner_lock_hash,
            name: Some("sample".to_string()),
            description: None,
            is_live: true,
            created_at_block,
            created_at_tx: vec![0x11; 32],
            extra: ckbadger_store::ObjectExtra::Spore {
                content_type: "text/plain".to_string(),
                content_length: 5,
                media_profile: ckbadger_store::SporeMediaProfile::default(),
            },
        }
    }

    #[test]
    fn test_list_spores_pagination_uses_live_indices() {
        use crate::warmup::SporeCache;

        let spores = vec![
            (vec![0x01; 32], make_spore_entry(300, Some(vec![0xAA; 32]))),
            (vec![0x02; 32], {
                let mut e = make_spore_entry(200, Some(vec![0xAA; 32]));
                e.is_live = false;
                e
            }),
            (vec![0x03; 32], make_spore_entry(100, Some(vec![0xAA; 32]))),
        ];
        let cache = SporeCache::build(spores);

        assert_eq!(cache.live_indices, vec![0, 2]);

        let cursor_block = i64::MAX;
        let limit = 1;
        let start = cache
            .live_indices
            .iter()
            .position(|&i| cache.all[i].1.created_at_block < cursor_block)
            .unwrap_or(cache.live_indices.len());
        let selected: Vec<_> = cache.live_indices[start..]
            .iter()
            .take(limit + 1)
            .map(|&i| &cache.all[i])
            .collect();
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].1.created_at_block, 300);
        assert_eq!(selected[1].1.created_at_block, 100);
    }

    #[test]
    fn test_owner_lookup_uses_by_owner_index() {
        use crate::warmup::SporeCache;

        let owner_a = vec![0xAA; 32];
        let owner_b = vec![0xBB; 32];
        let spores = vec![
            (vec![0x01; 32], make_spore_entry(300, Some(owner_a.clone()))),
            (vec![0x02; 32], make_spore_entry(200, Some(owner_b.clone()))),
            (vec![0x03; 32], make_spore_entry(100, Some(owner_a.clone()))),
        ];
        let cache = SporeCache::build(spores);

        let indices_a = cache.by_owner.get(&owner_a).unwrap();
        assert_eq!(indices_a, &vec![0, 2]);
        assert_eq!(cache.all[indices_a[0]].1.created_at_block, 300);
        assert_eq!(cache.all[indices_a[1]].1.created_at_block, 100);

        let indices_b = cache.by_owner.get(&owner_b).unwrap();
        assert_eq!(indices_b, &vec![1]);
    }

    #[test]
    fn test_build_cluster_responses_from_cached_entries_uses_maps_and_sorts() {
        let first_cluster = vec![0x11; 32];
        let second_cluster = vec![0x22; 32];

        let cached = vec![
            CachedAssetEntry {
                id: format!("0x{}", hex::encode(&first_cluster)),
                asset_type: "object".to_string(),
                standard: "spore".to_string(),
                name: Some("First".to_string()),
                symbol: None,
                icon_url: None,
                holders_count: 0,
                transfers_count: 7,
                transfers_24h: 0,
                decimals: None,
                total_supply: None,
                maximum_supply: None,
                content_type: None,
                content_size: None,
                cluster_id: None,
                cluster_name: None,
                owned_capacity: None,
                owned_knowledge: None,
                composition_tier: None,
                onchain_ratio: None,
                onchain_count: None,
                type_code_hash: None,
                type_hash_type: None,
                type_args: None,
                description: None,
            },
            CachedAssetEntry {
                id: format!("0x{}", hex::encode(&second_cluster)),
                asset_type: "object".to_string(),
                standard: "spore".to_string(),
                name: Some("Second".to_string()),
                symbol: None,
                icon_url: None,
                holders_count: 0,
                transfers_count: 3,
                transfers_24h: 0,
                decimals: None,
                total_supply: None,
                maximum_supply: None,
                content_type: None,
                content_size: None,
                cluster_id: None,
                cluster_name: None,
                owned_capacity: None,
                owned_knowledge: None,
                composition_tier: None,
                onchain_ratio: None,
                onchain_count: None,
                type_code_hash: None,
                type_hash_type: None,
                type_args: None,
                description: None,
            },
        ];

        let mut spore_map = HashMap::new();
        spore_map.insert(
            first_cluster.clone(),
            ckbadger_store::ObjectEntry {
                standard: ckbadger_store::ObjectStandard::SporeCluster,
                collection_id: None,
                token_id: None,
                owner_lock_hash: Some(vec![0xAA; 32]),
                name: Some("Cluster A".to_string()),
                description: Some("A".to_string()),
                is_live: true,
                created_at_block: 100,
                created_at_tx: vec![0xAA; 32],
                extra: ckbadger_store::ObjectExtra::SporeCluster,
            },
        );
        spore_map.insert(
            second_cluster.clone(),
            ckbadger_store::ObjectEntry {
                standard: ckbadger_store::ObjectStandard::SporeCluster,
                collection_id: None,
                token_id: None,
                owner_lock_hash: Some(vec![0xBB; 32]),
                name: Some("Cluster B".to_string()),
                description: Some("B".to_string()),
                is_live: true,
                created_at_block: 200,
                created_at_tx: vec![0xBB; 32],
                extra: ckbadger_store::ObjectExtra::SporeCluster,
            },
        );

        let mut cluster_agg_map = HashMap::new();
        cluster_agg_map.insert(
            first_cluster.clone(),
            ckbadger_store::ClusterAggregate {
                owner_count: 12,
                ..Default::default()
            },
        );
        cluster_agg_map.insert(
            second_cluster.clone(),
            ckbadger_store::ClusterAggregate {
                owner_count: 34,
                ..Default::default()
            },
        );

        let clusters =
            build_cluster_responses_from_cached_entries(cached, &spore_map, &cluster_agg_map);
        assert_eq!(clusters.len(), 2);
        assert_eq!(
            clusters[0].cluster_id,
            format!("0x{}", hex::encode(&second_cluster))
        );
        assert_eq!(clusters[0].created_at_block, 200);
        assert_eq!(clusters[0].holders_count, 34);
        assert_eq!(
            clusters[1].cluster_id,
            format!("0x{}", hex::encode(&first_cluster))
        );
        assert_eq!(clusters[1].created_at_block, 100);
        assert_eq!(clusters[1].holders_count, 12);
    }

    #[test]
    fn test_parse_fixed_len_hex_rejects_non_32_bytes() {
        let err = parse_fixed_len_hex("0x1234", 32, "Invalid cluster ID").unwrap_err();
        assert_eq!(err.0, axum::http::StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_parse_cluster_id_param_sole_spores_alias() {
        let result = parse_cluster_id_param("sole-spores").unwrap();
        assert_eq!(result, SOLE_SPORES_SENTINEL_COLLECTION.to_vec());

        let result = parse_cluster_id_param("Sole-Spores").unwrap();
        assert_eq!(result, SOLE_SPORES_SENTINEL_COLLECTION.to_vec());
    }

    #[test]
    fn test_parse_cluster_id_param_hex() {
        let hex_id = "ab".repeat(32);
        let result = parse_cluster_id_param(&hex_id).unwrap();
        assert_eq!(result, vec![0xab; 32]);

        let hex_id_0x = format!("0x{}", "cd".repeat(32));
        let result = parse_cluster_id_param(&hex_id_0x).unwrap();
        assert_eq!(result, vec![0xcd; 32]);
    }

    #[test]
    fn test_is_sole_spores_sentinel() {
        assert!(is_sole_spores_sentinel(&SOLE_SPORES_SENTINEL_COLLECTION));
        assert!(!is_sole_spores_sentinel(&[0xab; 32]));
    }
}
