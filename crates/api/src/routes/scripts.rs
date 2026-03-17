#![allow(clippy::type_complexity)]

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use super::statistics::{StackedAreaChartResponse, StackedAreaDataPoint, StackedAreaSeries};
use crate::response::{
    decode_cursor_single, default_limit, encode_cursor_single, ok, ApiError, ApiResult,
    ApiRouteError, CursorPaginatedResponse,
};
use crate::utils::{
    apply_live_capacity_delta, date_keys_inclusive, deployment_reference_hashes,
    hash_type_to_string, is_known_script_name, list_version_code_cells,
    merge_script_info_for_reference, parse_chart_date_range, related_code_hashes_for_reference,
    resolve_script_by_hash, CurrentScriptVersionResolution, VersionCodeCell,
};
use crate::warmup::{CACHE_KEY_SCRIPTS_ALL, CACHE_KEY_SCRIPT_VERSIONS_ALL};
use crate::AppState;

fn load_script_infos_cached(
    state: &Arc<AppState>,
) -> Result<Vec<(Vec<u8>, ckbadger_store::ScriptInfo)>, ApiRouteError> {
    state
        .mem_cache
        .get::<Vec<(Vec<u8>, ckbadger_store::ScriptInfo)>>(CACHE_KEY_SCRIPTS_ALL)
        .ok_or_else(|| ApiError::warmup_pending("script cache unavailable; warmup in progress"))
}

fn load_script_versions_cached(
    state: &Arc<AppState>,
) -> Result<Vec<(Vec<u8>, ckbadger_store::types::ScriptVersionInfo)>, ApiRouteError> {
    state
        .mem_cache
        .get::<Vec<(Vec<u8>, ckbadger_store::types::ScriptVersionInfo)>>(
            CACHE_KEY_SCRIPT_VERSIONS_ALL,
        )
        .ok_or_else(|| {
            ApiError::warmup_pending("script version cache unavailable; warmup in progress")
        })
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/scripts", get(list_scripts))
        .route("/scripts/lookup", post(lookup_scripts))
        .route("/scripts/code-cell", get(get_code_cell))
        .route("/scripts/code-cells", get(get_code_cells))
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
pub struct ScriptResolutionAmbiguityResponse {
    pub version_hashes: Vec<String>,
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
    #[serde(rename = "liveCommonKnowledgeSizeSum")]
    pub live_used_capacity_sum: String,
    pub code_cells_live_count: i64,
    pub code_cells_total: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptUsageResponse {
    pub name: String,
    pub cells_count: i64,
    pub live_cells_count: i64,
    pub capacity_sum: String,
    pub live_capacity_sum: String,
    #[serde(rename = "commonKnowledgeSizeSum")]
    pub used_capacity_sum: String,
    #[serde(rename = "liveCommonKnowledgeSizeSum")]
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
    #[serde(rename = "commonKnowledgeSizeSum")]
    pub used_capacity_sum: String,
    #[serde(rename = "liveCommonKnowledgeSizeSum")]
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
    pub reference_hash: String,
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
    #[serde(rename = "liveCommonKnowledgeSizeSum")]
    pub live_used_capacity_sum: String,
    pub code_cells_live_count: i64,
    pub code_cells_total: i64,
    pub resolution_state: String,
    pub ambiguity: Option<ScriptResolutionAmbiguityResponse>,
}

/// Resolve the deployment code cell outpoint for a script.
///
/// Tries type-ref lookup first, then data-hash lookup as fallback (not mutually exclusive).
/// Both paths use ckbadger's own domain store indexes — no dependency on CKB node RocksDB.
fn resolve_code_cell(
    info: &ckbadger_store::ScriptInfo,
    store: &ckbadger_store::CkbadgerStore,
    cells_store: &ckbadger_store::CkbadgerStore,
) -> Result<(Option<String>, Option<i32>), ApiRouteError> {
    let (type_ref, data_ref) = deployment_reference_hashes(info);

    // Step 1: try type-ref lookup (CF_CELL_BY_TYPE)
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
    }

    // Step 2: try data-hash lookup (CF_CELL_BY_DATA_HASH) — runs even if type-ref was attempted.
    // Uses find_any_cell_by_data_hash which checks both live and consumed cells,
    // since code cells may have been consumed while the script is still in use.
    if let Some(data_hash) = data_ref.as_deref() {
        if let Some((tx_hash, idx, _)) = store
            .find_any_cell_by_data_hash(data_hash, cells_store)
            .map_err(|e| {
                ApiError::internal(format!(
                    "failed to resolve code cell by deployment data hash 0x{}: {}",
                    hex::encode(data_hash),
                    e
                ))
            })?
        {
            let output_index = i32::from(idx);
            return Ok((
                Some(format!("0x{}", hex::encode(tx_hash))),
                Some(output_index),
            ));
        }
    }

    // Step 3: use the imported outpoint when no live deployment cell found.
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

fn count_code_cells(
    info: &ckbadger_store::ScriptInfo,
    store: &ckbadger_store::CkbadgerStore,
    cells_store: &ckbadger_store::CkbadgerStore,
) -> Result<(i64, i64), ApiRouteError> {
    let (type_ref, data_ref) = deployment_reference_hashes(info);

    let mut seen = std::collections::HashSet::new();
    let mut live_count: i64 = 0;
    let mut total_count: i64 = 0;

    if let Some(type_hash) = type_ref.as_deref() {
        let cells = store
            .list_all_cells_by_type(type_hash, cells_store)
            .map_err(|e| ApiError::internal(format!("count_code_cells type failed: {}", e)))?;
        for (tx_hash, idx, _, is_live) in cells {
            if seen.insert((tx_hash, idx)) {
                total_count += 1;
                if is_live {
                    live_count += 1;
                }
            }
        }
    }

    if let Some(data_hash) = data_ref.as_deref() {
        let cells = store
            .list_all_cells_by_data_hash(data_hash, cells_store)
            .map_err(|e| ApiError::internal(format!("count_code_cells data failed: {}", e)))?;
        for (tx_hash, idx, _, is_live) in cells {
            if seen.insert((tx_hash, idx)) {
                total_count += 1;
                if is_live {
                    live_count += 1;
                }
            }
        }
    }

    Ok((live_count, total_count))
}

fn script_kind_from_counts(lock_cells_count: i64, type_cells_count: i64) -> Option<String> {
    match (lock_cells_count > 0, type_cells_count > 0) {
        (true, true) => Some("lock+type".to_string()),
        (true, false) => Some("lock".to_string()),
        (false, true) => Some("type".to_string()),
        (false, false) => None,
    }
}

fn version_totals(
    version: &ckbadger_store::types::ScriptVersionInfo,
) -> (i64, i64, i128, i128, i128, i128) {
    (
        version.lock_cells_count + version.type_cells_count,
        version.lock_live_cells_count + version.type_live_cells_count,
        version.lock_capacity_sum + version.type_capacity_sum,
        version.lock_live_capacity_sum + version.type_live_capacity_sum,
        version.lock_used_capacity_sum + version.type_used_capacity_sum,
        version.lock_live_used_capacity_sum + version.type_live_used_capacity_sum,
    )
}

fn version_script_kind(info: &ckbadger_store::types::ScriptVersionInfo) -> Option<String> {
    script_kind_from_counts(info.lock_cells_count, info.type_cells_count)
}

fn sort_code_cells(code_cells: &mut [VersionCodeCell]) {
    code_cells.sort_by(|left, right| {
        right
            .3
            .cmp(&left.3)
            .then_with(|| left.2.created_at_block.cmp(&right.2.created_at_block))
            .then_with(|| left.0.cmp(&right.0))
            .then_with(|| left.1.cmp(&right.1))
    });
}

struct ResolvedScriptIdentifier {
    version_hash: Vec<u8>,
    version_info: ckbadger_store::types::ScriptVersionInfo,
    code_cells: Vec<VersionCodeCell>,
}

struct AmbiguousScriptIdentifier {
    version_hashes: Vec<Vec<u8>>,
}

enum ScriptIdentifierResolution {
    Resolved(Box<ResolvedScriptIdentifier>),
    Ambiguous(AmbiguousScriptIdentifier),
    NotFound,
}

fn fallback_script_version_info(
    state: &AppState,
    reference_hash: &[u8],
    version_hash: &[u8],
) -> Result<ckbadger_store::types::ScriptVersionInfo, ApiRouteError> {
    let direct_info = state
        .store
        .get_script_info(reference_hash)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let version_info = if reference_hash == version_hash {
        None
    } else {
        state
            .store
            .get_script_info(version_hash)
            .map_err(|e| ApiError::internal(e.to_string()))?
    };

    let fallback = match (direct_info, version_info) {
        (Some(direct), Some(version)) => {
            let candidates = vec![direct.clone(), version];
            merge_script_info_for_reference(&candidates, reference_hash).unwrap_or(direct)
        }
        (Some(direct), None) => direct,
        (None, Some(version)) => version,
        (None, None) => {
            return Ok(ckbadger_store::types::ScriptVersionInfo {
                version_hash: version_hash.to_vec(),
                ..Default::default()
            });
        }
    };

    Ok(ckbadger_store::types::ScriptVersionInfo {
        version_hash: version_hash.to_vec(),
        name: fallback.name,
        category: fallback.category,
        website: fallback.website,
        description: fallback.description,
        lock_cells_count: fallback.lock_cells_count,
        lock_live_cells_count: fallback.lock_live_cells_count,
        lock_capacity_sum: fallback.lock_capacity_sum,
        lock_live_capacity_sum: fallback.lock_live_capacity_sum,
        lock_used_capacity_sum: fallback.lock_used_capacity_sum,
        lock_live_used_capacity_sum: fallback.lock_live_used_capacity_sum,
        type_cells_count: fallback.type_cells_count,
        type_live_cells_count: fallback.type_live_cells_count,
        type_capacity_sum: fallback.type_capacity_sum,
        type_live_capacity_sum: fallback.type_live_capacity_sum,
        type_used_capacity_sum: fallback.type_used_capacity_sum,
        type_live_used_capacity_sum: fallback.type_live_used_capacity_sum,
    })
}

fn resolve_script_identifier(
    state: &AppState,
    hash_bytes: &[u8],
) -> Result<ScriptIdentifierResolution, ApiRouteError> {
    match resolve_script_by_hash(&state.store, &state.append_only_store, hash_bytes)
        .map_err(|e| ApiError::internal(e.to_string()))?
    {
        CurrentScriptVersionResolution::Resolved(resolved) => {
            let resolved = *resolved;
            let version_info = match resolved.version_info {
                Some(version_info) => version_info,
                None => fallback_script_version_info(state, hash_bytes, &resolved.version_hash)?,
            };
            let code_cells = list_version_code_cells(
                &state.store,
                &state.append_only_store,
                &resolved.version_hash,
            )
            .map_err(|e| ApiError::internal(e.to_string()))?;
            Ok(ScriptIdentifierResolution::Resolved(Box::new(
                ResolvedScriptIdentifier {
                    version_hash: resolved.version_hash,
                    version_info,
                    code_cells,
                },
            )))
        }
        CurrentScriptVersionResolution::Ambiguous(ambiguous) => {
            let ambiguous = *ambiguous;
            Ok(ScriptIdentifierResolution::Ambiguous(
                AmbiguousScriptIdentifier {
                    version_hashes: ambiguous.version_hashes,
                },
            ))
        }
        CurrentScriptVersionResolution::NotFound => Ok(ScriptIdentifierResolution::NotFound),
    }
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
            "negative live common knowledge size in {}: code_hash=0x{}, used={}",
            context,
            hex::encode(&info.code_hash),
            used
        )));
    }
    if used > capacity {
        return Err(ApiError::internal(format!(
            "live common knowledge size exceeds total in {}: code_hash=0x{}, used={}, capacity={}",
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
    let (code_cell_tx_hash, code_cell_output_index) =
        resolve_code_cell(info, &state.store, &state.append_only_store)?;
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
    let (code_cells_live_count, code_cells_total) =
        count_code_cells(info, &state.store, &state.append_only_store)?;

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
        code_cells_live_count,
        code_cells_total,
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

    let mut result: HashMap<String, ScriptLookupInfo> = HashMap::new();

    for code_hash in &code_hash_bytes {
        let reference_hash_hex = format!("0x{}", hex::encode(code_hash));
        match resolve_script_identifier(&state, code_hash)? {
            ScriptIdentifierResolution::Resolved(resolved) => {
                let ResolvedScriptIdentifier {
                    version_hash,
                    version_info,
                    mut code_cells,
                } = *resolved;
                sort_code_cells(&mut code_cells);
                let (
                    _cells_count,
                    live_cells_count,
                    _capacity_sum,
                    live_capacity_sum,
                    _used_sum,
                    live_used_sum,
                ) = version_totals(&version_info);
                let code_cell = code_cells.first();

                // Derive hash_type from ScriptInfo if available
                let hash_type = state
                    .store
                    .get_script_info(code_hash)
                    .ok()
                    .flatten()
                    .and_then(|info| hash_type_to_string(info.hash_type).map(|s| s.to_string()));

                // Derive deployment hashes from ScriptInfo
                let (deployment_type_hash, deployment_data_hash) = state
                    .store
                    .get_script_info(code_hash)
                    .ok()
                    .flatten()
                    .map(|info| {
                        let (type_ref, data_ref) = deployment_reference_hashes(&info);
                        (
                            type_ref.map(|h| format!("0x{}", hex::encode(h))),
                            data_ref.map(|h| format!("0x{}", hex::encode(h))),
                        )
                    })
                    .unwrap_or((None, None));

                result.insert(
                    reference_hash_hex.clone(),
                    ScriptLookupInfo {
                        reference_hash: reference_hash_hex.clone(),
                        code_hash: format!("0x{}", hex::encode(&version_hash)),
                        name: version_info
                            .name
                            .clone()
                            .unwrap_or_else(|| "Unknown".to_string()),
                        script_kind: version_script_kind(&version_info),
                        decoder_type: version_info.category.clone(),
                        hash_type,
                        deployment_type_hash,
                        deployment_data_hash,
                        code_cell_tx_hash: code_cell
                            .map(|(tx_hash, _, _, _)| format!("0x{}", hex::encode(tx_hash))),
                        code_cell_output_index: code_cell
                            .map(|(_, output_index, _, _)| i32::from(*output_index)),
                        live_cells_count,
                        live_capacity_sum: live_capacity_sum.to_string(),
                        live_used_capacity_sum: live_used_sum.to_string(),
                        code_cells_live_count: code_cells
                            .iter()
                            .filter(|(_, _, _, is_live)| *is_live)
                            .count() as i64,
                        code_cells_total: code_cells.len() as i64,
                        resolution_state: "resolved".to_string(),
                        ambiguity: None,
                    },
                );
            }
            ScriptIdentifierResolution::Ambiguous(ambiguous) => {
                let AmbiguousScriptIdentifier { version_hashes } = ambiguous;
                result.insert(
                    reference_hash_hex.clone(),
                    ScriptLookupInfo {
                        reference_hash: reference_hash_hex.clone(),
                        code_hash: reference_hash_hex.clone(),
                        name: "Ambiguous Script Reference".to_string(),
                        script_kind: None,
                        decoder_type: None,
                        hash_type: None,
                        deployment_type_hash: None,
                        deployment_data_hash: None,
                        code_cell_tx_hash: None,
                        code_cell_output_index: None,
                        live_cells_count: 0,
                        live_capacity_sum: "0".to_string(),
                        live_used_capacity_sum: "0".to_string(),
                        code_cells_live_count: 0,
                        code_cells_total: 0,
                        resolution_state: "ambiguous".to_string(),
                        ambiguity: Some(ScriptResolutionAmbiguityResponse {
                            version_hashes: version_hashes
                                .into_iter()
                                .map(|hash| format!("0x{}", hex::encode(hash)))
                                .collect(),
                        }),
                    },
                );
            }
            ScriptIdentifierResolution::NotFound => {}
        }
    }

    ok(result)
}

#[derive(Debug, Deserialize)]
pub struct CodeCellQuery {
    code_hash: String,
    #[allow(dead_code)]
    hash_type: Option<String>,
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeCellEntry {
    pub tx_hash: String,
    pub output_index: i32,
    pub status: &'static str,
    pub created_at_block: i64,
    pub capacity: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeCellsResponse {
    pub code_cells: Vec<CodeCellEntry>,
    pub live_count: i64,
    pub total_count: i64,
    pub resolved_version_hash: Option<String>,
    pub ambiguity: Option<ScriptResolutionAmbiguityResponse>,
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

    match resolve_script_identifier(&state, &code_hash_bytes)? {
        ScriptIdentifierResolution::Resolved(resolved) => {
            let ResolvedScriptIdentifier { mut code_cells, .. } = *resolved;
            sort_code_cells(&mut code_cells);
            let tx_hash = code_cells
                .first()
                .map(|(tx_hash, _, _, _)| format!("0x{}", hex::encode(tx_hash)));
            let output_index = code_cells
                .first()
                .map(|(_, output_index, _, _)| i32::from(*output_index));
            ok(CodeCellResponse {
                tx_hash,
                output_index,
            })
        }
        ScriptIdentifierResolution::Ambiguous(_) | ScriptIdentifierResolution::NotFound => {
            ok(CodeCellResponse {
                tx_hash: None,
                output_index: None,
            })
        }
    }
}

async fn get_code_cells(
    State(state): State<Arc<AppState>>,
    Query(params): Query<CodeCellQuery>,
) -> ApiResult<CodeCellsResponse> {
    let code_hash_bytes = hex::decode(
        params
            .code_hash
            .strip_prefix("0x")
            .unwrap_or(&params.code_hash),
    )
    .map_err(|_| ApiError::bad_request("Invalid code_hash hex"))?;

    match resolve_script_identifier(&state, &code_hash_bytes)? {
        ScriptIdentifierResolution::Resolved(resolved) => {
            let ResolvedScriptIdentifier {
                version_hash,
                mut code_cells,
                ..
            } = *resolved;
            sort_code_cells(&mut code_cells);
            let live_count = code_cells
                .iter()
                .filter(|(_, _, _, is_live)| *is_live)
                .count() as i64;
            let total_count = code_cells.len() as i64;
            let code_cells = code_cells
                .into_iter()
                .map(|(tx_hash, output_index, cell, is_live)| CodeCellEntry {
                    tx_hash: format!("0x{}", hex::encode(&tx_hash)),
                    output_index: i32::from(output_index),
                    status: if is_live { "live" } else { "consumed" },
                    created_at_block: cell.created_at_block,
                    capacity: cell.cell.capacity.to_string(),
                })
                .collect();

            ok(CodeCellsResponse {
                code_cells,
                live_count,
                total_count,
                resolved_version_hash: Some(format!("0x{}", hex::encode(version_hash))),
                ambiguity: None,
            })
        }
        ScriptIdentifierResolution::Ambiguous(ambiguous) => ok(CodeCellsResponse {
            code_cells: vec![],
            live_count: 0,
            total_count: 0,
            resolved_version_hash: None,
            ambiguity: Some(ScriptResolutionAmbiguityResponse {
                version_hashes: ambiguous
                    .version_hashes
                    .into_iter()
                    .map(|hash| format!("0x{}", hex::encode(hash)))
                    .collect(),
            }),
        }),
        ScriptIdentifierResolution::NotFound => ok(CodeCellsResponse {
            code_cells: vec![],
            live_count: 0,
            total_count: 0,
            resolved_version_hash: None,
            ambiguity: None,
        }),
    }
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

    let matching: Vec<_> = load_script_versions_cached(&state)?
        .into_iter()
        .filter(|(_, info)| info.name.as_deref() == Some(name.as_str()))
        .collect();

    if matching.is_empty() {
        return Err(ApiError::not_found("Script not found"));
    }
    let mut scripts = Vec::new();

    for (version_hash, version_info) in matching {
        let mut code_cells =
            list_version_code_cells(&state.store, &state.append_only_store, &version_hash)
                .map_err(|e| ApiError::internal(e.to_string()))?;
        sort_code_cells(&mut code_cells);

        let (
            cells_count,
            live_cells_count,
            _capacity_sum,
            live_capacity_sum,
            _used_sum,
            live_used_sum,
        ) = version_totals(&version_info);
        let code_cells_live_count = code_cells
            .iter()
            .filter(|(_, _, _, is_live)| *is_live)
            .count() as i64;
        let code_cells_total = code_cells.len() as i64;

        // Derive hash_type, type_hash, data_hash from ScriptInfo if available
        let script_info = state.store.get_script_info(&version_hash).ok().flatten();
        let hash_type = script_info
            .as_ref()
            .and_then(|info| hash_type_to_string(info.hash_type).map(|s| s.to_string()));
        let (type_hash, data_hash) = script_info
            .as_ref()
            .map(|info| {
                let (type_ref, data_ref) = deployment_reference_hashes(info);
                (
                    type_ref.map(|h| format!("0x{}", hex::encode(h))),
                    data_ref.map(|h| format!("0x{}", hex::encode(h))),
                )
            })
            .unwrap_or((None, None));
        // If no data_hash from ScriptInfo, use the version_hash itself
        let data_hash = data_hash.or_else(|| Some(format!("0x{}", hex::encode(&version_hash))));

        if code_cells.is_empty() {
            scripts.push(ScriptResponse {
                code_hash: format!("0x{}", hex::encode(&version_hash)),
                name: version_info
                    .name
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
                description: version_info.description.clone(),
                script_kind: version_script_kind(&version_info),
                rfc: None,
                website: version_info.website.clone(),
                source_url: None,
                decoder_type: version_info.category.clone(),
                network: network.to_string(),
                hash_type: hash_type.clone(),
                data_hash: data_hash.clone(),
                type_hash: type_hash.clone(),
                tag: None,
                deprecated: false,
                is_system: false,
                code_cell_tx_hash: None,
                code_cell_output_index: None,
                deployed_at: None,
                live_cells_count,
                cells_count,
                live_capacity_sum: live_capacity_sum.to_string(),
                live_used_capacity_sum: live_used_sum.to_string(),
                code_cells_live_count,
                code_cells_total,
            });
            continue;
        }

        for (tx_hash, output_index, cell, _is_live) in &code_cells {
            let deployed_at = state
                .store
                .get_block_header(cell.created_at_block)
                .map_err(|e| ApiError::internal(e.to_string()))?
                .map(|header| header.timestamp);
            scripts.push(ScriptResponse {
                code_hash: format!("0x{}", hex::encode(&version_hash)),
                name: version_info
                    .name
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
                description: version_info.description.clone(),
                script_kind: version_script_kind(&version_info),
                rfc: None,
                website: version_info.website.clone(),
                source_url: None,
                decoder_type: version_info.category.clone(),
                network: network.to_string(),
                hash_type: hash_type.clone(),
                data_hash: data_hash.clone(),
                type_hash: type_hash.clone(),
                tag: None,
                deprecated: false,
                is_system: false,
                code_cell_tx_hash: Some(format!("0x{}", hex::encode(tx_hash))),
                code_cell_output_index: Some(i32::from(*output_index)),
                deployed_at,
                live_cells_count,
                cells_count,
                live_capacity_sum: live_capacity_sum.to_string(),
                live_used_capacity_sum: live_used_sum.to_string(),
                code_cells_live_count,
                code_cells_total,
            });
        }
    }

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
    let matching: Vec<_> = load_script_versions_cached(&state)?
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
        .map(|(version_hash, info)| {
            let (
                cells_count,
                live_cells_count,
                capacity_sum,
                live_capacity_sum,
                used_capacity_sum,
                live_used_capacity_sum,
            ) = version_totals(&info);
            let capacity_sum = capacity_sum as u128;
            let live_capacity_sum = live_capacity_sum as u128;
            let used_capacity_sum = used_capacity_sum as u128;
            let live_used_capacity_sum = live_used_capacity_sum as u128;

            total_cells += cells_count;
            total_live += live_cells_count;
            total_cap += capacity_sum;
            total_live_cap += live_capacity_sum;
            total_used_cap += used_capacity_sum;
            total_live_used_cap += live_used_capacity_sum;

            DeploymentUsage {
                code_hash: format!("0x{}", hex::encode(&version_hash)),
                script_kind: version_script_kind(&info),
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
    let kind_filter = parse_script_kind_filter(params.script_kind.as_deref())?;
    let mut targets = Vec::new();

    match resolve_script_identifier(&state, &code_hash)? {
        ScriptIdentifierResolution::Resolved(resolved) => {
            let ResolvedScriptIdentifier { version_hash, .. } = *resolved;
            // Use version_hash and look up related code hashes from ScriptInfo
            let all_script_infos: Vec<ckbadger_store::ScriptInfo> =
                load_script_infos_cached(&state)?
                    .into_iter()
                    .map(|(_, info)| info)
                    .collect();
            let related_hashes =
                related_code_hashes_for_reference(&all_script_infos, &version_hash);
            let target_hashes = if related_hashes.is_empty() {
                vec![version_hash]
            } else {
                related_hashes
            };
            for target_hash in target_hashes {
                for is_type in &kind_filter {
                    targets.push((target_hash.clone(), *is_type));
                }
            }
        }
        ScriptIdentifierResolution::Ambiguous(_) => {
            return Err(ApiError::bad_request(
                "script capacity history requires an unambiguous script reference",
            ));
        }
        ScriptIdentifierResolution::NotFound => {
            let all_script_infos: Vec<ckbadger_store::ScriptInfo> =
                load_script_infos_cached(&state)?
                    .into_iter()
                    .map(|(_, info)| info)
                    .collect();
            let related_hashes = related_code_hashes_for_reference(&all_script_infos, &code_hash);
            let target_hashes = if related_hashes.is_empty() {
                vec![code_hash.clone()]
            } else {
                related_hashes
            };
            for target_hash in target_hashes {
                for is_type in &kind_filter {
                    targets.push((target_hash.clone(), *is_type));
                }
            }
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
        latest_complete_script_chart_date_from_tip, resolve_code_cell,
        resolve_script_capacity_chart_bounds,
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
            .contains("live common knowledge size exceeds live capacity"));
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
            .contains("live common knowledge size exceeds total"));
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

    /// When type-ref lookup fails (dep_type_hash set but no matching live cell),
    /// resolve_code_cell must fall through to the data-hash lookup instead of
    /// returning (None, None).
    #[test]
    fn resolve_code_cell_falls_through_type_to_data_hash() {
        use ckbadger_store::{CkbadgerStore, LiveCellInfo, StoreBatch};
        use std::sync::Arc;

        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());

        let tx_hash = vec![0xCA; 32];
        let output_index: i16 = 0;
        let block_num: i64 = 5;
        let data_hash = vec![0xDD; 32];

        // Write a live cell that is findable by data_hash.
        let outpoint_key = ckbadger_store::keys::encode_outpoint(&tx_hash, output_index);
        let cell_info = LiveCellInfo {
            capacity: 200_00000000,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![0x33; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 100,
            occupied_capacity: 100_00000000,
            udt_amount: None,
            data_hash: None,
        };
        let marker = ckbadger_store::types::encode_live_cell_marker(block_num);
        let payload = bincode::serialize(&cell_info).unwrap();
        store
            .put_cf(store.cf_live_cells(), &outpoint_key, &marker)
            .unwrap();
        store
            .put_cf(store.cf_cells(), &outpoint_key, &payload)
            .unwrap();

        // Write CF_CELL_BY_DATA_HASH index entry.
        let mut batch = StoreBatch::new(&store);
        batch.put_cell_by_data_hash(&data_hash, block_num, &tx_hash, output_index);
        batch.commit().unwrap();

        // ScriptInfo with dep_type_hash set (triggers type-ref path) but no matching cell,
        // AND data_ref available via hash_type=0.
        let info = ScriptInfo {
            code_hash: data_hash.clone(),
            hash_type: 0,
            dep_type_hash: Some(vec![0xFF; 32]), // no live cell with this type hash
            ..Default::default()
        };

        let (resolved_tx, resolved_idx) = resolve_code_cell(&info, &store, &store).unwrap();
        assert_eq!(
            resolved_tx.as_deref(),
            Some(format!("0x{}", hex::encode(&tx_hash)).as_str()),
            "should resolve code cell via data_hash fallback"
        );
        assert_eq!(resolved_idx, Some(0));
    }

    /// When the code cell has been consumed, resolve_code_cell must still find it
    /// via the preserved CF_CELL_BY_DATA_HASH index entry + consumed cell lookup.
    #[test]
    fn resolve_code_cell_finds_consumed_code_cell_by_data_hash() {
        use ckbadger_store::{CkbadgerStore, LiveCellInfo, StoreBatch};
        use std::sync::Arc;

        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(CkbadgerStore::open_test_unified(dir.path()).unwrap());

        let tx_hash = vec![0xCB; 32];
        let output_index: i16 = 0;
        let block_num: i64 = 5;
        let data_hash = vec![0xEE; 32];

        // Write cell payload (append-only — never deleted).
        let outpoint_key = ckbadger_store::keys::encode_outpoint(&tx_hash, output_index);
        let cell_info = LiveCellInfo {
            capacity: 200_00000000,
            lock_script_hash: vec![0x11; 32],
            lock_code_hash: vec![0x22; 32],
            lock_hash_type: 1,
            lock_args: vec![0x33; 20],
            type_script_hash: None,
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data_size: 100,
            occupied_capacity: 100_00000000,
            udt_amount: None,
            data_hash: None,
        };
        let payload = bincode::serialize(&cell_info).unwrap();
        store
            .put_cf(store.cf_cells(), &outpoint_key, &payload)
            .unwrap();

        // Write consumed cell metadata (not live — no CF_LIVE_CELLS entry).
        let mut batch = StoreBatch::new(&store);
        batch.put_consumed_cell(&tx_hash, output_index, &cell_info, block_num, 10);
        batch.commit().unwrap();

        // Write CF_CELL_BY_DATA_HASH index entry (preserved on consumption).
        let mut batch = StoreBatch::new(&store);
        batch.put_cell_by_data_hash(&data_hash, block_num, &tx_hash, output_index);
        batch.commit().unwrap();

        let info = ScriptInfo {
            code_hash: data_hash.clone(),
            hash_type: 0, // data hash type
            ..Default::default()
        };

        let (resolved_tx, resolved_idx) = resolve_code_cell(&info, &store, &store).unwrap();
        assert_eq!(
            resolved_tx.as_deref(),
            Some(format!("0x{}", hex::encode(&tx_hash)).as_str()),
            "should resolve consumed code cell via data_hash index"
        );
        assert_eq!(resolved_idx, Some(0));
    }
}
