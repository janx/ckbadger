#![allow(clippy::type_complexity)]

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use super::statistics::{StackedAreaChartResponse, StackedAreaDataPoint, StackedAreaSeries};
use crate::response::{
    decode_cursor_single, encode_cursor_single, ok, ApiError, ApiResult, CursorPaginatedResponse,
};
use crate::utils::{
    apply_live_capacity_delta, date_keys_inclusive, deployment_reference_hashes,
    is_known_script_name, merge_script_info_for_reference, parse_chart_date_range,
    related_code_hashes_for_reference,
};
use crate::warmup::CACHE_KEY_SCRIPTS_ALL;
use crate::AppState;

type ApiRouteError = (StatusCode, Json<ApiError>);

fn load_script_infos_cached(
    state: &Arc<AppState>,
) -> Result<Vec<(Vec<u8>, ckbadger_store::ScriptInfo)>, ApiRouteError> {
    state
        .mem_cache
        .get::<Vec<(Vec<u8>, ckbadger_store::ScriptInfo)>>(CACHE_KEY_SCRIPTS_ALL)
        .ok_or_else(|| ApiError::internal("script cache unavailable; warmup in progress"))
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/scripts", get(list_scripts))
        .route("/scripts/lookup", post(lookup_scripts))
        .route("/scripts/code-cell", get(get_code_cell))
        .route(
            "/scripts/charts/capacity-history",
            get(get_script_capacity_history_chart_by_code_hash),
        )
        .route("/scripts/{name}", get(get_script))
        .route("/scripts/{name}/usage", get(get_script_usage))
        .route(
            "/scripts/{name}/charts/capacity-history",
            get(get_script_capacity_history_chart),
        )
}

#[derive(Debug, Deserialize)]
pub struct ListParams {
    #[serde(default = "default_limit")]
    limit: i64,
    #[allow(dead_code)]
    cursor: Option<String>,
    network: Option<String>,
    #[allow(dead_code)]
    decoder_type: Option<String>,
    search: Option<String>,
    #[serde(default = "default_script_sort_key")]
    sort_key: ScriptSortKey,
    #[serde(default = "default_sort_direction")]
    sort_direction: SortDirection,
}

fn default_limit() -> i64 {
    20
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ScriptSortKey {
    Name,
    Kind,
    Description,
    Used,
    Capacity,
    UsedRatio,
    LiveCells,
    Cells,
}

fn default_script_sort_key() -> ScriptSortKey {
    ScriptSortKey::Name
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum SortDirection {
    Asc,
    Desc,
}

fn default_sort_direction() -> SortDirection {
    SortDirection::Asc
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptResponse {
    pub code_hash: String,
    pub name: String,
    pub description: Option<String>,
    pub script_kind: Option<String>,
    pub rfc: Option<String>,
    pub website: Option<String>,
    pub source_url: Option<String>,
    pub decoder_type: Option<String>,
    pub network: String,
    pub hash_type: Option<String>,
    pub data_hash: Option<String>,
    pub type_hash: Option<String>,
    pub tag: Option<String>,
    pub deprecated: bool,
    pub is_system: bool,
    pub code_cell_tx_hash: Option<String>,
    pub code_cell_output_index: Option<i32>,
    pub deployed_at: Option<i64>,
    pub live_cells_count: i64,
    pub cells_count: i64,
    pub live_capacity_sum: String,
    pub live_used_capacity_sum: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptUsageResponse {
    pub name: String,
    pub cells_count: i64,
    pub live_cells_count: i64,
    pub capacity_sum: String,
    pub live_capacity_sum: String,
    pub used_capacity_sum: String,
    pub live_used_capacity_sum: String,
    pub by_deployment: Vec<DeploymentUsage>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentUsage {
    pub code_hash: String,
    pub script_kind: Option<String>,
    pub cells_count: i64,
    pub live_cells_count: i64,
    pub capacity_sum: String,
    pub live_capacity_sum: String,
    pub used_capacity_sum: String,
    pub live_used_capacity_sum: String,
}

/// Request body for bulk script lookup by code_hash
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LookupScriptsRequest {
    /// List of code_hash values (hex strings with 0x prefix)
    pub code_hashes: Vec<String>,
}

/// Lightweight script info for lookup results
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptLookupInfo {
    pub code_hash: String,
    pub name: String,
    pub script_kind: Option<String>,
    pub decoder_type: Option<String>,
    pub hash_type: Option<String>,
    pub deployment_type_hash: Option<String>,
    pub deployment_data_hash: Option<String>,
    pub code_cell_tx_hash: Option<String>,
    pub code_cell_output_index: Option<i32>,
    pub live_cells_count: i64,
    pub live_capacity_sum: String,
    pub live_used_capacity_sum: String,
}

/// Resolve the deployment code cell outpoint for a script.
fn resolve_code_cell(
    info: &ckbadger_store::ScriptInfo,
    store: &ckbadger_store::CkbadgerStore,
    cells_store: &ckbadger_store::CkbadgerStore,
    ckb_store: Option<&ckb_store_reader::CkbChainReader>,
) -> Result<(Option<String>, Option<i32>), ApiRouteError> {
    let (type_ref, data_ref) = deployment_reference_hashes(info);

    if let Some(type_hash) = type_ref.as_deref() {
        let cells = store
            .list_cells_by_type(type_hash, 1, None, cells_store)
            .map_err(|e| {
                ApiError::internal(format!(
                    "failed to resolve code cell by deployment type hash 0x{}: {}",
                    hex::encode(type_hash),
                    e
                ))
            })?;
        if let Some((tx_hash, idx, _)) = cells.first() {
            let output_index = i32::from(*idx);
            return Ok((
                Some(format!("0x{}", hex::encode(tx_hash))),
                Some(output_index),
            ));
        }
    } else if let Some(data_hash) = data_ref.as_deref() {
        let data_hash_arr: [u8; 32] = data_hash.try_into().map_err(|_| {
            ApiError::internal(format!(
                "deployment data hash must be 32 bytes for code-cell resolution: data_hash=0x{}",
                hex::encode(data_hash)
            ))
        })?;

        if let Some((tx_hash, output_index)) =
            ckb_store.and_then(|reader| reader.find_cell_by_data_hash(&data_hash_arr))
        {
            let output_index = i32::try_from(output_index).map_err(|e| {
                ApiError::internal(format!(
                    "deployment data lookup returned out-of-range output index: data_hash=0x{}, output_index={}, error={}",
                    hex::encode(data_hash),
                    output_index,
                    e
                ))
            })?;
            return Ok((
                Some(format!("0x{}", hex::encode(tx_hash))),
                Some(output_index),
            ));
        }
    }

    // Use the imported outpoint when no live deployment cell lookup is available.
    if let (Some(tx_hash), Some(idx)) = (&info.code_cell_tx_hash, info.code_cell_output_index) {
        if !tx_hash.is_empty() {
            return Ok((
                Some(format!("0x{}", hex::encode(tx_hash))),
                Some(idx as i32),
            ));
        }
    }

    Ok((None, None))
}

fn resolve_deployed_at(
    store: &ckbadger_store::CkbadgerStore,
    cells_store: &ckbadger_store::CkbadgerStore,
    code_cell_tx_hash: Option<&str>,
    code_cell_output_index: Option<i32>,
) -> Option<i64> {
    let tx_hash = code_cell_tx_hash?;
    let output_index = code_cell_output_index?;
    let output_index = i16::try_from(output_index).ok()?;
    let tx_hash_bytes = hex::decode(tx_hash.strip_prefix("0x").unwrap_or(tx_hash)).ok()?;

    let created_at_block = store
        .get_cell(&tx_hash_bytes, output_index, cells_store)
        .ok()
        .flatten()
        .or_else(|| {
            store
                .get_consumed_cell(&tx_hash_bytes, output_index, cells_store)
                .ok()
                .flatten()
        })
        .map(|cell| cell.created_at_block)?;

    store
        .get_block_header(created_at_block)
        .ok()
        .flatten()
        .map(|header| header.timestamp)
}

fn script_display_name(info: &ckbadger_store::ScriptInfo) -> &str {
    info.name.as_deref().unwrap_or("Unknown")
}

fn script_kind_for_sort(info: &ckbadger_store::ScriptInfo) -> &str {
    if info.lock_cells_count > 0 && info.type_cells_count > 0 {
        "lock+type"
    } else if info.lock_cells_count > 0 {
        "lock"
    } else if info.type_cells_count > 0 {
        "type"
    } else {
        ""
    }
}

fn checked_capacity_totals(
    info: &ckbadger_store::ScriptInfo,
    context: &str,
) -> Result<(i128, i128), ApiRouteError> {
    let capacity = info.lock_live_capacity_sum + info.type_live_capacity_sum;
    let used = info.lock_live_used_capacity_sum + info.type_live_used_capacity_sum;
    if capacity < 0 {
        return Err(ApiError::internal(format!(
            "negative live capacity in {}: code_hash=0x{}, capacity={}",
            context,
            hex::encode(&info.code_hash),
            capacity
        )));
    }
    if used < 0 {
        return Err(ApiError::internal(format!(
            "negative live used capacity in {}: code_hash=0x{}, used={}",
            context,
            hex::encode(&info.code_hash),
            used
        )));
    }
    if used > capacity {
        return Err(ApiError::internal(format!(
            "live used capacity exceeds total in {}: code_hash=0x{}, used={}, capacity={}",
            context,
            hex::encode(&info.code_hash),
            used,
            capacity
        )));
    }
    Ok((capacity, used))
}

fn live_capacity_sum_for_sort(info: &ckbadger_store::ScriptInfo) -> i128 {
    info.lock_live_capacity_sum + info.type_live_capacity_sum
}

fn live_used_capacity_sum_for_sort(info: &ckbadger_store::ScriptInfo) -> i128 {
    info.lock_live_used_capacity_sum + info.type_live_used_capacity_sum
}

fn used_ratio_for_sort(info: &ckbadger_store::ScriptInfo) -> Option<(i128, i128)> {
    let capacity = live_capacity_sum_for_sort(info);
    if capacity <= 0 {
        return None;
    }
    Some((live_used_capacity_sum_for_sort(info), capacity))
}

fn apply_direction(ordering: Ordering, direction: SortDirection) -> Ordering {
    match direction {
        SortDirection::Asc => ordering,
        SortDirection::Desc => ordering.reverse(),
    }
}

fn compare_used_ratio(
    left: Option<(i128, i128)>,
    right: Option<(i128, i128)>,
    direction: SortDirection,
) -> Ordering {
    match (left, right) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some((left_used, left_capacity)), Some((right_used, right_capacity))) => {
            let left_side = left_used * right_capacity;
            let right_side = right_used * left_capacity;
            apply_direction(left_side.cmp(&right_side), direction)
        }
    }
}

fn compare_script_entries(
    left: &(Vec<u8>, ckbadger_store::ScriptInfo),
    right: &(Vec<u8>, ckbadger_store::ScriptInfo),
    sort_key: ScriptSortKey,
    direction: SortDirection,
) -> Ordering {
    let compared = match sort_key {
        ScriptSortKey::Name => apply_direction(
            script_display_name(&left.1).cmp(script_display_name(&right.1)),
            direction,
        ),
        ScriptSortKey::Kind => apply_direction(
            script_kind_for_sort(&left.1).cmp(script_kind_for_sort(&right.1)),
            direction,
        ),
        ScriptSortKey::Description => apply_direction(
            left.1
                .description
                .as_deref()
                .unwrap_or("")
                .cmp(right.1.description.as_deref().unwrap_or("")),
            direction,
        ),
        ScriptSortKey::Used => apply_direction(
            live_used_capacity_sum_for_sort(&left.1)
                .cmp(&live_used_capacity_sum_for_sort(&right.1)),
            direction,
        ),
        ScriptSortKey::Capacity => apply_direction(
            live_capacity_sum_for_sort(&left.1).cmp(&live_capacity_sum_for_sort(&right.1)),
            direction,
        ),
        ScriptSortKey::UsedRatio => compare_used_ratio(
            used_ratio_for_sort(&left.1),
            used_ratio_for_sort(&right.1),
            direction,
        ),
        ScriptSortKey::LiveCells => apply_direction(
            (left.1.lock_live_cells_count + left.1.type_live_cells_count)
                .cmp(&(right.1.lock_live_cells_count + right.1.type_live_cells_count)),
            direction,
        ),
        ScriptSortKey::Cells => apply_direction(
            (left.1.lock_cells_count + left.1.type_cells_count)
                .cmp(&(right.1.lock_cells_count + right.1.type_cells_count)),
            direction,
        ),
    };

    if compared != Ordering::Equal {
        return compared;
    }

    script_display_name(&left.1)
        .cmp(script_display_name(&right.1))
        .then_with(|| left.0.cmp(&right.0))
}

/// Convert a store ScriptInfo into an API ScriptResponse.
fn script_info_to_response(
    info: &ckbadger_store::ScriptInfo,
    network: &str,
    state: &AppState,
) -> Result<ScriptResponse, ApiRouteError> {
    let hash_type_str = match info.hash_type {
        0 => Some("data".to_string()),
        1 => Some("type".to_string()),
        2 => Some("data1".to_string()),
        4 => Some("data2".to_string()),
        _ => None,
    };

    // Determine script_kind from usage stats
    let script_kind = if info.lock_cells_count > 0 && info.type_cells_count > 0 {
        Some("lock+type".to_string())
    } else if info.lock_cells_count > 0 {
        Some("lock".to_string())
    } else if info.type_cells_count > 0 {
        Some("type".to_string())
    } else {
        None
    };

    // Resolve deployment code cell outpoint from the deployment-family references.
    // Type-referenced deployments use the type index; data-referenced deployments
    // use the direct CKB cell-data reader by data hash.
    let (code_cell_tx_hash, code_cell_output_index) = resolve_code_cell(
        info,
        &state.store,
        &state.append_only_store,
        state.ckb_store.as_deref(),
    )?;
    let deployed_at = resolve_deployed_at(
        &state.store,
        &state.append_only_store,
        code_cell_tx_hash.as_deref(),
        code_cell_output_index,
    );
    let (live_capacity, live_used) = checked_capacity_totals(info, "script response")?;
    let live_used_capacity_sum = live_used.to_string();
    let live_capacity_sum = live_capacity.to_string();
    let (deployment_type_hash, deployment_data_hash) = deployment_reference_hashes(info);

    Ok(ScriptResponse {
        code_hash: format!("0x{}", hex::encode(&info.code_hash)),
        name: info.name.clone().unwrap_or_else(|| "Unknown".to_string()),
        description: info.description.clone(),
        script_kind,
        rfc: None,
        website: info.website.clone(),
        source_url: None,
        decoder_type: None,
        network: network.to_string(),
        hash_type: hash_type_str,
        data_hash: deployment_data_hash
            .as_ref()
            .map(|h| format!("0x{}", hex::encode(h))),
        type_hash: deployment_type_hash
            .as_ref()
            .map(|h| format!("0x{}", hex::encode(h))),
        tag: None,
        deprecated: false,
        is_system: false,
        code_cell_tx_hash,
        code_cell_output_index,
        deployed_at,
        live_cells_count: info.lock_live_cells_count + info.type_live_cells_count,
        cells_count: info.lock_cells_count + info.type_cells_count,
        live_capacity_sum,
        live_used_capacity_sum,
    })
}

async fn lookup_scripts(
    State(state): State<Arc<AppState>>,
    Json(request): Json<LookupScriptsRequest>,
) -> ApiResult<HashMap<String, ScriptLookupInfo>> {
    if request.code_hashes.is_empty() {
        return ok(HashMap::new());
    }

    if request.code_hashes.len() > 100 {
        return Err(ApiError::bad_request(
            "Too many code_hashes, maximum is 100",
        ));
    }

    let code_hash_bytes: Result<Vec<Vec<u8>>, _> = request
        .code_hashes
        .iter()
        .map(|h| hex::decode(h.strip_prefix("0x").unwrap_or(h)))
        .collect();

    let code_hash_bytes =
        code_hash_bytes.map_err(|_| ApiError::bad_request("Invalid hex in code_hashes"))?;

    let all_script_infos: Vec<ckbadger_store::ScriptInfo> = load_script_infos_cached(&state)?
        .into_iter()
        .map(|(_, info)| info)
        .collect();

    let mut result: HashMap<String, ScriptLookupInfo> = HashMap::new();

    for code_hash in &code_hash_bytes {
        if let Some(info) = merge_script_info_for_reference(&all_script_infos, code_hash) {
            let code_hash_hex = format!("0x{}", hex::encode(code_hash));

            let script_kind = if info.lock_cells_count > 0 && info.type_cells_count > 0 {
                Some("lock+type".to_string())
            } else if info.lock_cells_count > 0 {
                Some("lock".to_string())
            } else if info.type_cells_count > 0 {
                Some("type".to_string())
            } else {
                None
            };

            let hash_type_str = match info.hash_type {
                0 => Some("data".to_string()),
                1 => Some("type".to_string()),
                2 => Some("data1".to_string()),
                4 => Some("data2".to_string()),
                _ => None,
            };

            let live_cells_count = info.lock_live_cells_count + info.type_live_cells_count;
            let live_capacity_sum =
                (info.lock_live_capacity_sum + info.type_live_capacity_sum).to_string();
            let live_used_capacity_sum =
                (info.lock_live_used_capacity_sum + info.type_live_used_capacity_sum).to_string();

            let (code_cell_tx_hash, code_cell_output_index) = resolve_code_cell(
                &info,
                &state.store,
                &state.append_only_store,
                state.ckb_store.as_deref(),
            )?;
            let (deployment_type_hash, deployment_data_hash) = deployment_reference_hashes(&info);

            result.insert(
                code_hash_hex.clone(),
                ScriptLookupInfo {
                    code_hash: code_hash_hex,
                    name: info.name.clone().unwrap_or_else(|| "Unknown".to_string()),
                    script_kind,
                    decoder_type: None,
                    hash_type: hash_type_str,
                    deployment_type_hash: deployment_type_hash
                        .as_ref()
                        .map(|h| format!("0x{}", hex::encode(h))),
                    deployment_data_hash: deployment_data_hash
                        .as_ref()
                        .map(|h| format!("0x{}", hex::encode(h))),
                    code_cell_tx_hash,
                    code_cell_output_index,
                    live_cells_count,
                    live_capacity_sum,
                    live_used_capacity_sum,
                },
            );
        }
    }

    ok(result)
}

#[derive(Debug, Deserialize)]
pub struct CodeCellQuery {
    code_hash: String,
    hash_type: String,
}

#[derive(Debug, Deserialize)]
pub struct ScriptCapacityHistoryQuery {
    code_hash: Option<String>,
    script_kind: Option<String>,
    from: Option<String>,
    to: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ScriptCapacityHistoryByCodeHashQuery {
    code_hash: String,
    script_kind: Option<String>,
    from: Option<String>,
    to: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeCellResponse {
    pub tx_hash: Option<String>,
    pub output_index: Option<i32>,
}

async fn get_code_cell(
    State(state): State<Arc<AppState>>,
    Query(params): Query<CodeCellQuery>,
) -> ApiResult<CodeCellResponse> {
    let code_hash_bytes = hex::decode(
        params
            .code_hash
            .strip_prefix("0x")
            .unwrap_or(&params.code_hash),
    )
    .map_err(|_| ApiError::bad_request("Invalid code_hash hex"))?;

    let all_script_infos: Vec<ckbadger_store::ScriptInfo> = load_script_infos_cached(&state)?
        .into_iter()
        .map(|(_, info)| info)
        .collect();

    // Build a minimal ScriptInfo for resolve_code_cell when this hash is unknown.
    let hash_type = match params.hash_type.as_str() {
        "data" => 0,
        "type" => 1,
        "data1" => 2,
        "data2" => 4,
        _ => 0,
    };
    let script_info = merge_script_info_for_reference(&all_script_infos, &code_hash_bytes)
        .unwrap_or_else(|| ckbadger_store::ScriptInfo {
            code_hash: code_hash_bytes,
            hash_type,
            ..Default::default()
        });

    let (tx_hash, output_index) = resolve_code_cell(
        &script_info,
        &state.store,
        &state.append_only_store,
        state.ckb_store.as_deref(),
    )?;

    ok(CodeCellResponse {
        tx_hash,
        output_index,
    })
}

async fn list_scripts(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<ScriptResponse>> {
    let limit = params.limit.clamp(1, 100) as usize;
    let network = params.network.as_deref().unwrap_or(&state.ckb_network);

    let all_scripts = load_script_infos_cached(&state)?;

    let search_pattern = params.search.as_ref().map(|s| s.to_lowercase());

    let mut filtered: Vec<_> = all_scripts
        .into_iter()
        .filter(|(_, info)| {
            // Filter by search pattern
            if let Some(ref pattern) = search_pattern {
                let name_match = info
                    .name
                    .as_ref()
                    .map(|n| n.to_lowercase().contains(pattern))
                    .unwrap_or(false);
                if !name_match {
                    return false;
                }
            }
            true
        })
        .collect();

    let all_filtered_infos: Vec<ckbadger_store::ScriptInfo> =
        filtered.iter().map(|(_, info)| info.clone()).collect();
    filtered.retain(|(_, info)| {
        if is_known_script_name(info.name.as_deref()) {
            return true;
        }
        let Some(resolved) = merge_script_info_for_reference(&all_filtered_infos, &info.code_hash)
        else {
            return true;
        };
        !is_known_script_name(resolved.name.as_deref())
    });

    // First sort by display name, then code hash to ensure deterministic dedup selection.
    filtered.sort_by(|a, b| {
        script_display_name(&a.1)
            .cmp(script_display_name(&b.1))
            .then_with(|| a.0.cmp(&b.0))
    });

    // Deduplicate by known name (keep one deployment per known script name), but
    // keep all Unknown entries distinct by code hash.
    let mut seen_known_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    let deduped: Vec<_> = filtered
        .into_iter()
        .filter(|(_, info)| {
            let Some(name) = info.name.as_ref() else {
                return true;
            };
            let trimmed = name.trim();
            if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("unknown") {
                return true;
            }
            seen_known_names.insert(trimmed.to_string())
        })
        .collect();

    // Apply requested ordering to the entire result set before pagination.
    let mut deduped = deduped;
    for (_, info) in &deduped {
        checked_capacity_totals(info, "list scripts")?;
    }
    deduped.sort_by(|a, b| compare_script_entries(a, b, params.sort_key, params.sort_direction));

    let total = deduped.len() as i64;

    let start_idx = params
        .cursor
        .as_deref()
        .and_then(decode_cursor_single)
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(0);

    let page: Vec<_> = deduped
        .into_iter()
        .skip(start_idx)
        .take(limit + 1)
        .collect();
    let has_more = page.len() > limit;
    let page: Vec<_> = page.into_iter().take(limit).collect();

    let next_cursor = if has_more {
        Some(encode_cursor_single((start_idx + limit) as i64))
    } else {
        None
    };

    let scripts: Vec<ScriptResponse> = page
        .iter()
        .map(|(_, info)| script_info_to_response(info, network, &state))
        .collect::<Result<Vec<_>, _>>()?;

    ok(CursorPaginatedResponse::new(
        scripts,
        total,
        limit as i64,
        next_cursor,
    ))
}

async fn get_script(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> ApiResult<Vec<ScriptResponse>> {
    let network = &state.ckb_network;

    let all_scripts = load_script_infos_cached(&state)?;

    let matching: Vec<_> = all_scripts
        .into_iter()
        .filter(|(_, info)| info.name.as_deref() == Some(name.as_str()))
        .collect();

    if matching.is_empty() {
        return Err(ApiError::not_found("Script not found"));
    }
    for (_, info) in &matching {
        checked_capacity_totals(info, "get script")?;
    }

    let mut scripts: Vec<ScriptResponse> = matching
        .iter()
        .map(|(_, info)| script_info_to_response(info, network, &state))
        .collect::<Result<Vec<_>, _>>()?;

    // Propagate script_kind from deployments that have usage stats to those that don't.
    // All deployments of the same script serve the same purpose (lock/type).
    let known_kind = scripts.iter().find_map(|s| s.script_kind.clone());
    if let Some(ref kind) = known_kind {
        for s in &mut scripts {
            if s.script_kind.is_none() {
                s.script_kind = Some(kind.clone());
            }
        }
    }

    // Show newest deployments first when deployment timestamp is available.
    scripts.sort_by(|a, b| match (a.deployed_at, b.deployed_at) {
        (Some(a_ts), Some(b_ts)) => b_ts.cmp(&a_ts).then_with(|| a.code_hash.cmp(&b.code_hash)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.code_hash.cmp(&b.code_hash),
    });

    ok(scripts)
}

async fn get_script_usage(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> ApiResult<ScriptUsageResponse> {
    let all_scripts = load_script_infos_cached(&state)?;

    let matching: Vec<_> = all_scripts
        .into_iter()
        .filter(|(_, info)| info.name.as_deref() == Some(name.as_str()))
        .collect();

    if matching.is_empty() {
        return ok(ScriptUsageResponse {
            name,
            cells_count: 0,
            live_cells_count: 0,
            capacity_sum: "0".to_string(),
            live_capacity_sum: "0".to_string(),
            used_capacity_sum: "0".to_string(),
            live_used_capacity_sum: "0".to_string(),
            by_deployment: vec![],
        });
    }

    let mut total_cells: i64 = 0;
    let mut total_live: i64 = 0;
    let mut total_cap: u128 = 0;
    let mut total_live_cap: u128 = 0;
    let mut total_used_cap: u128 = 0;
    let mut total_live_used_cap: u128 = 0;

    let by_deployment: Vec<DeploymentUsage> = matching
        .into_iter()
        .map(|(_, info)| {
            let cells_count = info.lock_cells_count + info.type_cells_count;
            let live_cells_count = info.lock_live_cells_count + info.type_live_cells_count;
            let capacity_sum = (info.lock_capacity_sum + info.type_capacity_sum) as u128;
            let live_capacity_sum =
                (info.lock_live_capacity_sum + info.type_live_capacity_sum) as u128;
            let used_capacity_sum =
                (info.lock_used_capacity_sum + info.type_used_capacity_sum) as u128;
            let live_used_capacity_sum =
                (info.lock_live_used_capacity_sum + info.type_live_used_capacity_sum) as u128;

            total_cells += cells_count;
            total_live += live_cells_count;
            total_cap += capacity_sum;
            total_live_cap += live_capacity_sum;
            total_used_cap += used_capacity_sum;
            total_live_used_cap += live_used_capacity_sum;

            let script_kind = if info.lock_cells_count > 0 && info.type_cells_count > 0 {
                Some("lock+type".to_string())
            } else if info.lock_cells_count > 0 {
                Some("lock".to_string())
            } else if info.type_cells_count > 0 {
                Some("type".to_string())
            } else {
                None
            };

            DeploymentUsage {
                code_hash: format!("0x{}", hex::encode(&info.code_hash)),
                script_kind,
                cells_count,
                live_cells_count,
                capacity_sum: capacity_sum.to_string(),
                live_capacity_sum: live_capacity_sum.to_string(),
                used_capacity_sum: used_capacity_sum.to_string(),
                live_used_capacity_sum: live_used_capacity_sum.to_string(),
            }
        })
        .collect();

    ok(ScriptUsageResponse {
        name,
        cells_count: total_cells,
        live_cells_count: total_live,
        capacity_sum: total_cap.to_string(),
        live_capacity_sum: total_live_cap.to_string(),
        used_capacity_sum: total_used_cap.to_string(),
        live_used_capacity_sum: total_live_used_cap.to_string(),
        by_deployment,
    })
}

fn format_yyyymmdd_for_chart(date_yyyymmdd: u32) -> String {
    let date = format!("{date_yyyymmdd:08}");
    format!("{}-{}-{}", &date[0..4], &date[4..6], &date[6..8])
}

fn parse_script_kind_filter(script_kind: Option<&str>) -> Result<Vec<bool>, ApiRouteError> {
    match script_kind {
        None => Ok(vec![false, true]),
        Some("lock") => Ok(vec![false]),
        Some("type") => Ok(vec![true]),
        Some("both") | Some("lock+type") => Ok(vec![false, true]),
        Some(_) => Err(ApiError::bad_request(
            "Invalid script_kind, expected lock/type/both",
        )),
    }
}

fn parse_code_hash_hex(code_hash: &str) -> Result<Vec<u8>, ApiRouteError> {
    let decoded = hex::decode(code_hash.strip_prefix("0x").unwrap_or(code_hash))
        .map_err(|_| ApiError::bad_request("Invalid code_hash hex"))?;
    if decoded.len() != 32 {
        return Err(ApiError::bad_request("Invalid code_hash length"));
    }
    Ok(decoded)
}

fn apply_script_chart_delta(
    cumulative_capacity: i128,
    cumulative_used: i128,
    cap_delta: i128,
    used_delta: i128,
    context: &str,
) -> Result<(i128, i128), ApiRouteError> {
    apply_live_capacity_delta(
        cumulative_capacity,
        cumulative_used,
        cap_delta,
        used_delta,
        context,
    )
    .map_err(|e| ApiError::internal(e.to_string()))
}

fn latest_complete_script_chart_date_from_tip(tip_timestamp_ms: i64) -> Result<u32, ApiRouteError> {
    let tip_date = ckbadger_common::block_date_from_ms(tip_timestamp_ms);
    let latest_complete = tip_date
        .checked_sub_signed(chrono::Duration::days(1))
        .ok_or_else(|| {
            ApiError::internal(format!(
                "tip timestamp does not have a previous CKB chart day: tip_timestamp_ms={}",
                tip_timestamp_ms
            ))
        })?;

    latest_complete
        .format("%Y%m%d")
        .to_string()
        .parse::<u32>()
        .map_err(|e| {
            ApiError::internal(format!(
                "failed to encode latest complete script chart date: tip_timestamp_ms={}, date={}, error={}",
                tip_timestamp_ms,
                latest_complete,
                e
            ))
        })
}

fn latest_complete_script_chart_date(state: &AppState) -> Result<u32, ApiRouteError> {
    let (_, header) = state
        .store
        .get_sync_tip_block()
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| {
            ApiError::internal(
                "script capacity history requires a sync tip block when script daily deltas exist",
            )
        })?;

    latest_complete_script_chart_date_from_tip(header.timestamp)
}

fn resolve_script_capacity_chart_bounds(
    daily_deltas: &BTreeMap<u32, (i128, i128)>,
    from_date: Option<u32>,
    to_date: Option<u32>,
    default_end_date: Option<u32>,
    has_history: bool,
) -> Option<(u32, u32)> {
    if !has_history {
        return None;
    }

    let first_delta_date = daily_deltas.keys().next().copied();
    let bounds = match (from_date, to_date) {
        (Some(from), Some(to)) => Some((from, to)),
        (Some(from), None) => default_end_date.map(|end| (from, end)),
        (None, Some(to)) => first_delta_date.map(|first| (first, to)),
        (None, None) => first_delta_date.zip(default_end_date),
    };

    bounds.filter(|(start, end)| start <= end)
}

fn build_script_capacity_history_chart(
    state: &AppState,
    targets: Vec<(Vec<u8>, bool)>,
    title: String,
    from_date: Option<u32>,
    to_date: Option<u32>,
) -> Result<StackedAreaChartResponse, ApiRouteError> {
    let series = vec![
        StackedAreaSeries {
            key: "used".to_string(),
            label: "Used".to_string(),
            color: "#f59e0b".to_string(),
        },
        StackedAreaSeries {
            key: "unused".to_string(),
            label: "Unused".to_string(),
            color: "#00c389".to_string(),
        },
    ];

    if targets.is_empty() {
        return Ok(StackedAreaChartResponse {
            data: vec![],
            series,
            title,
        });
    }

    let mut dedup = HashSet::new();
    let unique_targets: Vec<(Vec<u8>, bool)> = targets
        .into_iter()
        .filter(|target| dedup.insert(target.clone()))
        .collect();

    let mut cumulative_capacity: i128 = 0;
    let mut cumulative_used: i128 = 0;
    if let Some(from) = from_date {
        let mut baseline_daily: BTreeMap<u32, (i128, i128)> = BTreeMap::new();
        for (code_hash, is_type) in &unique_targets {
            let baseline = state
                .store
                .list_script_daily_deltas_in_range(
                    code_hash,
                    *is_type,
                    None,
                    Some(from.saturating_sub(1)),
                )
                .map_err(|e| ApiError::internal(e.to_string()))?;
            for (date, delta) in baseline {
                let entry = baseline_daily.entry(date).or_insert((0, 0));
                entry.0 += delta.live_capacity_delta;
                entry.1 += delta.live_used_capacity_delta;
            }
        }
        for (_, (cap_delta, used_delta)) in baseline_daily {
            (cumulative_capacity, cumulative_used) = apply_script_chart_delta(
                cumulative_capacity,
                cumulative_used,
                cap_delta,
                used_delta,
                "building script baseline capacity history chart",
            )?;
        }
    }

    let mut daily_deltas: BTreeMap<u32, (i128, i128)> = BTreeMap::new();
    for (code_hash, is_type) in &unique_targets {
        let deltas = state
            .store
            .list_script_daily_deltas_in_range(code_hash, *is_type, from_date, to_date)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        for (date, delta) in deltas {
            let entry = daily_deltas.entry(date).or_insert((0, 0));
            entry.0 += delta.live_capacity_delta;
            entry.1 += delta.live_used_capacity_delta;
        }
    }

    let has_history = !daily_deltas.is_empty() || cumulative_capacity != 0 || cumulative_used != 0;
    let default_end_date = if to_date.is_none() && has_history {
        Some(latest_complete_script_chart_date(state)?)
    } else {
        None
    };
    let chart_bounds = resolve_script_capacity_chart_bounds(
        &daily_deltas,
        from_date,
        to_date,
        default_end_date,
        has_history,
    );
    let dates = if let Some((start, end)) = chart_bounds {
        date_keys_inclusive(start, end).map_err(ApiError::internal)?
    } else {
        Vec::new()
    };

    let mut data = Vec::with_capacity(dates.len());
    for date in dates {
        let (cap_delta, used_delta) = daily_deltas.get(&date).copied().unwrap_or((0, 0));
        (cumulative_capacity, cumulative_used) = apply_script_chart_delta(
            cumulative_capacity,
            cumulative_used,
            cap_delta,
            used_delta,
            &format!("building script capacity history chart at date {}", date),
        )?;
        let unused = cumulative_capacity - cumulative_used;
        data.push(StackedAreaDataPoint {
            date: format_yyyymmdd_for_chart(date),
            values: HashMap::from([
                ("used".to_string(), cumulative_used.to_string()),
                ("unused".to_string(), unused.to_string()),
            ]),
        });
    }

    Ok(StackedAreaChartResponse {
        data,
        series,
        title,
    })
}

async fn get_script_capacity_history_chart(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Query(params): Query<ScriptCapacityHistoryQuery>,
) -> ApiResult<StackedAreaChartResponse> {
    let (from_date, to_date) = parse_chart_date_range(params.from.as_deref(), params.to.as_deref())
        .map_err(|msg| ApiError::bad_request(&msg))?;

    let all_scripts = load_script_infos_cached(&state)?;

    let code_hash_filter = params
        .code_hash
        .as_deref()
        .map(parse_code_hash_hex)
        .transpose()?;
    let matching: Vec<ckbadger_store::ScriptInfo> = all_scripts
        .into_iter()
        .filter(|(_, info)| info.name.as_deref() == Some(name.as_str()))
        .filter(|(_, info)| {
            code_hash_filter
                .as_ref()
                .map(|filter| &info.code_hash == filter)
                .unwrap_or(true)
        })
        .map(|(_, info)| info)
        .collect();

    let kind_filter = parse_script_kind_filter(params.script_kind.as_deref())?;
    let mut targets = Vec::new();
    for info in matching {
        for is_type in &kind_filter {
            targets.push((info.code_hash.clone(), *is_type));
        }
    }

    ok(build_script_capacity_history_chart(
        &state,
        targets,
        format!("{name} Capacity History"),
        from_date,
        to_date,
    )?)
}

async fn get_script_capacity_history_chart_by_code_hash(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ScriptCapacityHistoryByCodeHashQuery>,
) -> ApiResult<StackedAreaChartResponse> {
    let (from_date, to_date) = parse_chart_date_range(params.from.as_deref(), params.to.as_deref())
        .map_err(|msg| ApiError::bad_request(&msg))?;

    let code_hash = parse_code_hash_hex(&params.code_hash)?;
    let all_script_infos: Vec<ckbadger_store::ScriptInfo> = load_script_infos_cached(&state)?
        .into_iter()
        .map(|(_, info)| info)
        .collect();
    let related_hashes = related_code_hashes_for_reference(&all_script_infos, &code_hash);
    let target_hashes = if related_hashes.is_empty() {
        vec![code_hash.clone()]
    } else {
        related_hashes
    };
    let kind_filter = parse_script_kind_filter(params.script_kind.as_deref())?;
    let mut targets = Vec::new();
    for target_hash in target_hashes {
        for is_type in &kind_filter {
            targets.push((target_hash.clone(), *is_type));
        }
    }

    ok(build_script_capacity_history_chart(
        &state,
        targets,
        format!("0x{} Capacity History", hex::encode(&code_hash)),
        from_date,
        to_date,
    )?)
}

#[cfg(test)]
mod tests {
    use super::{
        apply_script_chart_delta, checked_capacity_totals,
        latest_complete_script_chart_date_from_tip, resolve_script_capacity_chart_bounds,
    };
    use axum::http::StatusCode;
    use ckbadger_store::ScriptInfo;
    use std::collections::BTreeMap;

    #[test]
    fn apply_script_chart_delta_accepts_delta_beyond_i64() {
        let huge = i128::from(i64::MAX) + 1;
        let (capacity, used) = apply_script_chart_delta(0, 0, huge, 0, "script chart").unwrap();
        assert_eq!(capacity, huge);
        assert_eq!(used, 0);
    }

    #[test]
    fn apply_script_chart_delta_errors_on_invariant_violation() {
        let err = apply_script_chart_delta(10, 5, 0, 10, "script chart").unwrap_err();
        assert_eq!(err.0, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(err
            .1
             .0
            .message
            .contains("live used capacity exceeds live capacity"));
    }

    #[test]
    fn checked_capacity_totals_errors_on_negative_capacity() {
        let info = ScriptInfo {
            code_hash: vec![0xAA; 32],
            lock_live_capacity_sum: -1,
            ..Default::default()
        };
        let err = checked_capacity_totals(&info, "test").unwrap_err();
        assert_eq!(err.0, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(err.1 .0.message.contains("negative live capacity"));
    }

    #[test]
    fn checked_capacity_totals_errors_when_occupied_exceeds_capacity() {
        let info = ScriptInfo {
            code_hash: vec![0xBB; 32],
            lock_live_capacity_sum: 100,
            lock_live_used_capacity_sum: 101,
            ..Default::default()
        };
        let err = checked_capacity_totals(&info, "test").unwrap_err();
        assert_eq!(err.0, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(err
            .1
             .0
            .message
            .contains("live used capacity exceeds total"));
    }

    #[test]
    fn latest_complete_script_chart_date_from_tip_uses_ckb_day_boundary() {
        assert_eq!(
            latest_complete_script_chart_date_from_tip(1_705_536_000_000).unwrap(),
            20240117
        );
    }

    #[test]
    fn resolve_script_capacity_chart_bounds_extends_unbounded_chart_to_latest_complete_day() {
        let daily_deltas = BTreeMap::from([(20240115, (100, 40))]);

        assert_eq!(
            resolve_script_capacity_chart_bounds(&daily_deltas, None, None, Some(20240117), true,),
            Some((20240115, 20240117))
        );
    }

    #[test]
    fn resolve_script_capacity_chart_bounds_preserves_from_only_flat_history() {
        let daily_deltas = BTreeMap::new();

        assert_eq!(
            resolve_script_capacity_chart_bounds(
                &daily_deltas,
                Some(20240116),
                None,
                Some(20240117),
                true,
            ),
            Some((20240116, 20240117))
        );
    }
}
