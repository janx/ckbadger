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
    ApiRouteError, CursorPaginatedResponse, ScriptFamilyDetailResponse,
    ScriptFamilyListItemResponse, ScriptObservedReferenceResponse, ScriptVersionDeploymentResponse,
    ScriptVersionDetailResponse,
};
use crate::utils::script_resolution::resolve_dep_cells_for_transaction;
use crate::utils::{
    apply_owned_capacity_delta, date_keys_inclusive, deployment_reference_hashes,
    hash_type_to_string, hash_type_to_u8, list_version_code_cells, merge_script_info_for_reference,
    parse_chart_date_range, reference_form_member_version, resolve_script_by_hash,
    CurrentScriptVersionResolution, VersionCodeCell,
};
use crate::warmup::{
    CACHE_KEY_SCRIPTS_ALL, CACHE_KEY_SCRIPT_FAMILIES_ALL, CACHE_KEY_SCRIPT_VERSIONS_ALL,
};
use crate::AppState;

fn load_script_infos_cached(
    state: &Arc<AppState>,
) -> Result<Vec<(Vec<u8>, ckbadger_store::ScriptInfo)>, ApiRouteError> {
    state
        .mem_cache
        .get::<Vec<(Vec<u8>, ckbadger_store::ScriptInfo)>>(CACHE_KEY_SCRIPTS_ALL)
        .ok_or_else(|| ApiError::warmup_pending("script cache unavailable; warmup in progress"))
}

pub(crate) fn load_script_families_cached(
    state: &Arc<AppState>,
) -> Result<Vec<(String, ckbadger_store::types::ScriptFamilyInfo)>, ApiRouteError> {
    state
        .mem_cache
        .get::<Vec<(String, ckbadger_store::types::ScriptFamilyInfo)>>(
            CACHE_KEY_SCRIPT_FAMILIES_ALL,
        )
        .ok_or_else(|| ApiError::warmup_pending("script cache unavailable; warmup in progress"))
}

pub(crate) fn load_script_versions_cached(
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
    cursor: Option<String>,
    network: Option<String>,
    #[serde(rename = "decoder_type")]
    _decoder_type: Option<String>,
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
    UsedAs,
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
pub struct ScriptUsageResponse {
    pub name: String,
    pub cells_count: i64,
    pub live_cells_count: i64,
    pub capacity_sum: String,
    pub owned_capacity_sum: String,
    #[serde(rename = "commonKnowledgeSizeSum")]
    pub used_capacity_sum: String,
    pub owned_knowledge_sum: String,
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
    pub owned_capacity_sum: String,
    #[serde(rename = "commonKnowledgeSizeSum")]
    pub used_capacity_sum: String,
    pub owned_knowledge_sum: String,
}

/// Request body for bulk script lookup by code_hash
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LookupScriptsRequest {
    /// List of code_hash values (hex strings with 0x prefix)
    pub code_hashes: Vec<String>,
    #[serde(default)]
    pub tx_hash: Option<String>,
}

/// Lightweight script info for lookup results
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptLookupInfo {
    pub reference_hash: String,
    pub code_hash: String,
    pub name: String,
    pub deprecated: bool,
    pub script_kind: Option<String>,
    pub decoder_type: Option<String>,
    pub hash_type: Option<String>,
    pub deployment_type_hash: Option<String>,
    pub deployment_data_hash: Option<String>,
    pub code_cell_tx_hash: Option<String>,
    pub code_cell_output_index: Option<i32>,
    pub live_cells_count: i64,
    pub owned_capacity_sum: String,
    pub owned_knowledge_sum: String,
    pub code_cells_live_count: i64,
    pub code_cells_total: i64,
    pub resolution_state: String,
    pub ambiguity: Option<ScriptResolutionAmbiguityResponse>,
}

/// Resolve the deployment code cell outpoint for a script.
///
/// Tries type-ref lookup first, then data-hash lookup as fallback (not mutually exclusive).
/// Both paths use ckbadger's own domain store indexes — no dependency on CKB node RocksDB.
#[cfg_attr(not(test), allow(dead_code))]
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
        version.lock_owned_capacity_sum + version.type_owned_capacity_sum,
        version.lock_used_capacity_sum + version.type_used_capacity_sum,
        version.lock_owned_knowledge_sum + version.type_owned_knowledge_sum,
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
        family_id: None,
        deprecated: fallback.deprecated,
        category: fallback.category,
        website: fallback.website,
        description: fallback.description,
        lock_cells_count: fallback.lock_cells_count,
        lock_live_cells_count: fallback.lock_live_cells_count,
        lock_capacity_sum: fallback.lock_capacity_sum,
        lock_owned_capacity_sum: fallback.lock_owned_capacity_sum,
        lock_used_capacity_sum: fallback.lock_used_capacity_sum,
        lock_owned_knowledge_sum: fallback.lock_owned_knowledge_sum,
        type_cells_count: fallback.type_cells_count,
        type_live_cells_count: fallback.type_live_cells_count,
        type_capacity_sum: fallback.type_capacity_sum,
        type_owned_capacity_sum: fallback.type_owned_capacity_sum,
        type_used_capacity_sum: fallback.type_used_capacity_sum,
        type_owned_knowledge_sum: fallback.type_owned_knowledge_sum,
        associated_code_hash: None,
        canonical_reference_hash: None,
        canonical_hash_type: None,
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

#[cfg_attr(not(test), allow(dead_code))]
fn checked_capacity_totals(
    info: &ckbadger_store::ScriptInfo,
    context: &str,
) -> Result<(i128, i128), ApiRouteError> {
    let capacity = info.lock_owned_capacity_sum + info.type_owned_capacity_sum;
    let used = info.lock_owned_knowledge_sum + info.type_owned_knowledge_sum;
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

/// Resolve capacity totals for a script version by looking up ScriptInfo.
///
/// Tier 1: direct lookup (caller pre-fetched or version_hash == code_hash).
/// Tier 2: lookup by associated_code_hash from label data (type-ref scripts
/// where version_hash is a data_hash, not a code_hash).
/// Tier 3: fall back to ScriptVersionInfo fields (zeros).
fn resolve_version_capacity(
    version: &ckbadger_store::types::ScriptVersionInfo,
    direct_script_info: Option<&ckbadger_store::ScriptInfo>,
    script_infos_cache: &[(Vec<u8>, ckbadger_store::ScriptInfo)],
) -> Result<(i64, i64, i128, i128, i128, i128), ApiRouteError> {
    // Tier 1: use pre-fetched direct lookup (caller may have already loaded it)
    let info = direct_script_info
        .cloned()
        .or_else(|| {
            // Tier 1b: search cache by version_hash (works when version_hash == code_hash)
            script_infos_cache
                .iter()
                .find(|(code_hash, _)| code_hash == &version.version_hash)
                .map(|(_, info)| info.clone())
        })
        .or_else(|| {
            // Tier 2: lookup by associated_code_hash from label data
            let assoc = version.associated_code_hash.as_ref()?;
            script_infos_cache
                .iter()
                .find(|(code_hash, _)| code_hash == assoc)
                .map(|(_, info)| info.clone())
        });

    let Some(info) = info else {
        // Tier 3: no ScriptInfo found
        return Ok(version_totals(version));
    };

    // Compute all 6 return values
    let cells = info.lock_cells_count + info.type_cells_count;
    let live_cells = info.lock_live_cells_count + info.type_live_cells_count;
    let cap = info.lock_capacity_sum + info.type_capacity_sum;
    let live_cap = info.lock_owned_capacity_sum + info.type_owned_capacity_sum;
    let used = info.lock_used_capacity_sum + info.type_used_capacity_sum;
    let live_used = info.lock_owned_knowledge_sum + info.type_owned_knowledge_sum;

    // Validate all capacity values (fail-fast, matches checked_capacity_totals pattern).
    // Total (historical) values are also checked because get_script_usage casts i128→u128.
    if cap < 0 {
        return Err(ApiError::internal(format!(
            "negative capacity in resolve_version_capacity: code_hash=0x{}, value={}",
            hex::encode(&info.code_hash),
            cap
        )));
    }
    if used < 0 {
        return Err(ApiError::internal(format!(
            "negative used capacity in resolve_version_capacity: code_hash=0x{}, value={}",
            hex::encode(&info.code_hash),
            used
        )));
    }
    if live_cap < 0 {
        return Err(ApiError::internal(format!(
            "negative live capacity in resolve_version_capacity: code_hash=0x{}, value={}",
            hex::encode(&info.code_hash),
            live_cap
        )));
    }
    if live_used < 0 {
        return Err(ApiError::internal(format!(
            "negative live used capacity in resolve_version_capacity: code_hash=0x{}, value={}",
            hex::encode(&info.code_hash),
            live_used
        )));
    }
    if live_used > live_cap {
        return Err(ApiError::internal(format!(
            "live used exceeds total in resolve_version_capacity: code_hash=0x{}, used={}, capacity={}",
            hex::encode(&info.code_hash),
            live_used,
            live_cap
        )));
    }

    Ok((cells, live_cells, cap, live_cap, used, live_used))
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

fn checked_family_totals(
    info: &ckbadger_store::types::ScriptFamilyInfo,
    context: &str,
) -> Result<(i128, i128), ApiRouteError> {
    if info.owned_capacity_sum < 0 {
        return Err(ApiError::internal(format!(
            "negative live capacity in {}: family_id={}, capacity={}",
            context, info.family_id, info.owned_capacity_sum
        )));
    }
    if info.owned_knowledge_sum < 0 {
        return Err(ApiError::internal(format!(
            "negative live common knowledge size in {}: family_id={}, used={}",
            context, info.family_id, info.owned_knowledge_sum
        )));
    }
    if info.owned_knowledge_sum > info.owned_capacity_sum {
        return Err(ApiError::internal(format!(
            "live common knowledge size exceeds total in {}: family_id={}, used={}, capacity={}",
            context, info.family_id, info.owned_knowledge_sum, info.owned_capacity_sum
        )));
    }
    Ok((info.owned_capacity_sum, info.owned_knowledge_sum))
}

fn family_display_name(info: &ckbadger_store::types::ScriptFamilyInfo) -> &str {
    info.name.as_str()
}

fn family_used_as_for_sort(info: &ckbadger_store::types::ScriptFamilyInfo) -> &str {
    match (info.lock_cells_count > 0, info.type_cells_count > 0) {
        (true, true) => "lock+type",
        (true, false) => "lock",
        (false, true) => "type",
        (false, false) => "",
    }
}

fn used_ratio_for_family_sort(
    info: &ckbadger_store::types::ScriptFamilyInfo,
) -> Option<(i128, i128)> {
    if info.owned_capacity_sum <= 0 {
        return None;
    }
    Some((info.owned_knowledge_sum, info.owned_capacity_sum))
}

fn compare_script_family_entries(
    left: &(String, ckbadger_store::types::ScriptFamilyInfo),
    right: &(String, ckbadger_store::types::ScriptFamilyInfo),
    sort_key: ScriptSortKey,
    direction: SortDirection,
) -> Ordering {
    let compared = match sort_key {
        ScriptSortKey::Name => apply_direction(
            family_display_name(&left.1).cmp(family_display_name(&right.1)),
            direction,
        ),
        ScriptSortKey::UsedAs => apply_direction(
            family_used_as_for_sort(&left.1).cmp(family_used_as_for_sort(&right.1)),
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
            left.1.owned_knowledge_sum.cmp(&right.1.owned_knowledge_sum),
            direction,
        ),
        ScriptSortKey::Capacity => apply_direction(
            left.1.owned_capacity_sum.cmp(&right.1.owned_capacity_sum),
            direction,
        ),
        ScriptSortKey::UsedRatio => compare_used_ratio(
            used_ratio_for_family_sort(&left.1),
            used_ratio_for_family_sort(&right.1),
            direction,
        ),
        ScriptSortKey::LiveCells => apply_direction(
            left.1.live_cells_count.cmp(&right.1.live_cells_count),
            direction,
        ),
        ScriptSortKey::Cells => {
            apply_direction(left.1.cells_count.cmp(&right.1.cells_count), direction)
        }
    };

    if compared != Ordering::Equal {
        return compared;
    }

    family_display_name(&left.1)
        .cmp(family_display_name(&right.1))
        .then_with(|| left.0.cmp(&right.0))
}

fn script_family_to_response(
    family_id: &str,
    info: &ckbadger_store::types::ScriptFamilyInfo,
) -> Result<ScriptFamilyListItemResponse, ApiRouteError> {
    let (owned_capacity, owned_knowledge) = checked_family_totals(info, "script family response")?;
    Ok(ScriptFamilyListItemResponse {
        family_id: family_id.to_string(),
        name: info.name.clone(),
        description: info.description.clone(),
        script_kind: script_kind_from_counts(info.lock_cells_count, info.type_cells_count),
        deprecated: info.deprecated,
        website: info.website.clone(),
        live_cells_count: info.live_cells_count,
        cells_count: info.cells_count,
        owned_capacity_sum: owned_capacity.to_string(),
        owned_knowledge_sum: owned_knowledge.to_string(),
        versions_count: info.versions_count,
    })
}

fn reference_totals(info: &ckbadger_store::types::ScriptReferenceInfo) -> (i64, i64, i128, i128) {
    (
        info.lock_live_cells_count + info.type_live_cells_count,
        info.lock_cells_count + info.type_cells_count,
        info.lock_owned_capacity_sum + info.type_owned_capacity_sum,
        info.lock_owned_knowledge_sum + info.type_owned_knowledge_sum,
    )
}

fn reference_info_to_response(
    info: &ckbadger_store::types::ScriptReferenceInfo,
) -> Result<ScriptObservedReferenceResponse, ApiRouteError> {
    let (live_cells_count, cells_count, owned_capacity_sum, owned_knowledge_sum) =
        reference_totals(info);
    if owned_capacity_sum < 0 || owned_knowledge_sum < 0 || owned_knowledge_sum > owned_capacity_sum
    {
        return Err(ApiError::internal(format!(
            "invalid script reference totals while building family detail: reference_hash=0x{}, hash_type={}, owned_capacity_sum={}, owned_knowledge_sum={}",
            hex::encode(&info.reference_hash),
            info.hash_type,
            owned_capacity_sum,
            owned_knowledge_sum
        )));
    }
    Ok(ScriptObservedReferenceResponse {
        reference_hash: format!("0x{}", hex::encode(&info.reference_hash)),
        hash_type: hash_type_to_string(info.hash_type)
            .map(str::to_string)
            .unwrap_or_else(|| format!("unknown({})", info.hash_type)),
        live_cells_count,
        cells_count,
        owned_capacity_sum: owned_capacity_sum.to_string(),
        owned_knowledge_sum: owned_knowledge_sum.to_string(),
    })
}

fn checked_version_totals(
    version_hash: &[u8],
    info: &ckbadger_store::types::ScriptVersionInfo,
    context: &str,
) -> Result<(i64, i64, i128, i128, i128, i128), ApiRouteError> {
    let (
        cells_count,
        live_cells_count,
        capacity_sum,
        owned_capacity_sum,
        used_capacity_sum,
        owned_knowledge_sum,
    ) = version_totals(info);

    if capacity_sum < 0 {
        return Err(ApiError::internal(format!(
            "negative capacity in {}: version_hash=0x{}, value={}",
            context,
            hex::encode(version_hash),
            capacity_sum
        )));
    }
    if used_capacity_sum < 0 {
        return Err(ApiError::internal(format!(
            "negative used capacity in {}: version_hash=0x{}, value={}",
            context,
            hex::encode(version_hash),
            used_capacity_sum
        )));
    }
    if used_capacity_sum > capacity_sum {
        return Err(ApiError::internal(format!(
            "used capacity exceeds total in {}: version_hash=0x{}, used={}, capacity={}",
            context,
            hex::encode(version_hash),
            used_capacity_sum,
            capacity_sum
        )));
    }
    if owned_capacity_sum < 0 {
        return Err(ApiError::internal(format!(
            "negative live capacity in {}: version_hash=0x{}, value={}",
            context,
            hex::encode(version_hash),
            owned_capacity_sum
        )));
    }
    if owned_knowledge_sum < 0 {
        return Err(ApiError::internal(format!(
            "negative live common knowledge size in {}: version_hash=0x{}, value={}",
            context,
            hex::encode(version_hash),
            owned_knowledge_sum
        )));
    }
    if owned_knowledge_sum > owned_capacity_sum {
        return Err(ApiError::internal(format!(
            "live common knowledge size exceeds total in {}: version_hash=0x{}, used={}, capacity={}",
            context,
            hex::encode(version_hash),
            owned_knowledge_sum,
            owned_capacity_sum
        )));
    }

    Ok((
        cells_count,
        live_cells_count,
        capacity_sum,
        owned_capacity_sum,
        used_capacity_sum,
        owned_knowledge_sum,
    ))
}

fn code_cell_timestamp(
    state: &AppState,
    version_hash: &[u8],
    created_at_block: i64,
) -> Result<i64, ApiRouteError> {
    let header = state
        .store
        .get_block_header(created_at_block)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| {
            ApiError::internal(format!(
                "missing block header while building script family detail: version_hash=0x{}, created_at_block={}",
                hex::encode(version_hash),
                created_at_block
            ))
        })?;

    Ok(header.timestamp)
}

fn first_code_cell_timestamp(
    state: &AppState,
    version_hash: &[u8],
    code_cells: &[VersionCodeCell],
) -> Result<Option<i64>, ApiRouteError> {
    let mut earliest = None;

    for (_, _, cell, _) in code_cells {
        let timestamp = code_cell_timestamp(state, version_hash, cell.created_at_block)?;
        earliest = Some(earliest.map_or(timestamp, |current: i64| current.min(timestamp)));
    }

    Ok(earliest)
}

fn script_version_deployments(
    state: &AppState,
    version_hash: &[u8],
    info: &ckbadger_store::types::ScriptVersionInfo,
    code_cells: &[VersionCodeCell],
) -> Result<Vec<ScriptVersionDeploymentResponse>, ApiRouteError> {
    code_cells
        .iter()
        .map(|(tx_hash, output_index, cell, _)| {
            let hash_type = if cell.type_script_hash.is_some() {
                "type".to_string()
            } else {
                info.canonical_hash_type
                    .and_then(hash_type_to_string)
                    .unwrap_or("data")
                    .to_string()
            };
            let type_reference_hash = cell
                .type_script_hash
                .as_ref()
                .or_else(|| {
                    if info.canonical_hash_type == Some(1) {
                        info.canonical_reference_hash.as_ref()
                    } else {
                        None
                    }
                })
                .map(|hash| format!("0x{}", hex::encode(hash)));

            Ok(ScriptVersionDeploymentResponse {
                hash_type,
                type_reference_hash,
                data_reference_hash: format!("0x{}", hex::encode(version_hash)),
                code_cell_tx_hash: format!("0x{}", hex::encode(tx_hash)),
                code_cell_output_index: i32::from(*output_index),
                deployed_at: code_cell_timestamp(state, version_hash, cell.created_at_block)?,
            })
        })
        .collect()
}

/// An observed reference form that resolved into a family/version set.
struct FamilyMemberForm {
    reference_hash: Vec<u8>,
    hash_type: u8,
    version_hash: Vec<u8>,
    info: ckbadger_store::types::ScriptReferenceInfo,
}

fn canonical_reference_set(
    versions: &[(Vec<u8>, ckbadger_store::types::ScriptVersionInfo)],
) -> HashSet<(u8, Vec<u8>)> {
    versions
        .iter()
        .filter_map(|(_, info)| {
            Some((
                info.canonical_hash_type?,
                info.canonical_reference_hash.as_ref()?.clone(),
            ))
        })
        .collect()
}

/// Enumerate the observed reference forms belonging to a version set.
///
/// THE single family-membership computation: the family detail response, the
/// family capacity-history chart and the most-utilized grouping all derive
/// their reference sets through [`reference_form_member_version`] over the
/// persisted reference forms, so counters and charts cannot diverge.
///
/// A canonical reference that fails to resolve into the set is an invariant
/// violation and is reported as an error.
fn family_member_reference_forms(
    state: &AppState,
    membership_context: &str,
    allowed_versions: &HashSet<Vec<u8>>,
    canonical_references: &HashSet<(u8, Vec<u8>)>,
) -> Result<Vec<FamilyMemberForm>, ApiRouteError> {
    let mut members = Vec::new();

    for ((reference_hash, hash_type), info) in state
        .store
        .list_script_reference_infos()
        .map_err(|e| ApiError::internal(e.to_string()))?
    {
        let member_version = reference_form_member_version(
            &state.store,
            &state.append_only_store,
            hash_type,
            &reference_hash,
            &|hash: &[u8]| allowed_versions.contains(hash),
        )
        .map_err(|e| ApiError::internal(e.to_string()))?;

        let version_hash = match member_version {
            Some(version_hash) if allowed_versions.contains(&version_hash) => version_hash,
            Some(_) => continue,
            None => {
                if canonical_references.contains(&(hash_type, reference_hash.clone())) {
                    return Err(ApiError::internal(format!(
                        "script family detail is missing reference->version mapping: context={}, reference_hash=0x{}, hash_type={}",
                        membership_context,
                        hex::encode(&reference_hash),
                        hash_type
                    )));
                }
                continue;
            }
        };

        members.push(FamilyMemberForm {
            reference_hash,
            hash_type,
            version_hash,
            info,
        });
    }

    Ok(members)
}

fn observed_references_by_version(
    state: &AppState,
    family_id: &str,
    versions: &[(Vec<u8>, ckbadger_store::types::ScriptVersionInfo)],
) -> Result<HashMap<Vec<u8>, Vec<ScriptObservedReferenceResponse>>, ApiRouteError> {
    let mut by_version: HashMap<Vec<u8>, Vec<ScriptObservedReferenceResponse>> = HashMap::new();
    let allowed_versions: HashSet<Vec<u8>> = versions
        .iter()
        .map(|(version_hash, _)| version_hash.clone())
        .collect();
    let canonical_references = canonical_reference_set(versions);

    for member in
        family_member_reference_forms(state, family_id, &allowed_versions, &canonical_references)?
    {
        by_version
            .entry(member.version_hash)
            .or_default()
            .push(reference_info_to_response(&member.info)?);
    }

    for references in by_version.values_mut() {
        references.sort_by(|left, right| {
            right
                .live_cells_count
                .cmp(&left.live_cells_count)
                .then_with(|| right.cells_count.cmp(&left.cells_count))
                .then_with(|| left.hash_type.cmp(&right.hash_type))
                .then_with(|| left.reference_hash.cmp(&right.reference_hash))
        });
    }

    Ok(by_version)
}

fn load_script_family_by_name(
    state: &AppState,
    name: &str,
) -> Result<(String, ckbadger_store::types::ScriptFamilyInfo), ApiRouteError> {
    let family_id = state
        .store
        .get_script_family_id_by_name(name)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found("Script not found"))?;
    let family = state
        .store
        .get_script_family(&family_id)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| {
            ApiError::internal(format!(
                "script family name index points to missing family: name={}, family_id={}",
                name, family_id
            ))
        })?;

    if family.family_id != family_id {
        return Err(ApiError::internal(format!(
            "script family record mismatch while resolving family by name: name={}, indexed_family_id={}, record_family_id={}",
            name, family_id, family.family_id
        )));
    }

    Ok((family_id, family))
}

fn load_family_versions(
    state: &Arc<AppState>,
    family_id: &str,
) -> Result<Vec<(Vec<u8>, ckbadger_store::types::ScriptVersionInfo)>, ApiRouteError> {
    let mut versions_by_hash: HashMap<Vec<u8>, ckbadger_store::types::ScriptVersionInfo> =
        load_script_versions_cached(state)?.into_iter().collect();
    let version_hashes = state
        .store
        .list_script_version_hashes_by_family(family_id)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let mut versions = Vec::with_capacity(version_hashes.len());
    for version_hash in version_hashes {
        let version_info = if let Some(version_info) = versions_by_hash.remove(&version_hash) {
            version_info
        } else {
            state
                .store
                .get_script_version(&version_hash)
                .map_err(|e| ApiError::internal(e.to_string()))?
                .ok_or_else(|| {
                    ApiError::internal(format!(
                        "script family references missing version: family_id={}, version_hash=0x{}",
                        family_id,
                        hex::encode(&version_hash)
                    ))
                })?
        };

        if version_info.family_id.as_deref() != Some(family_id) {
            return Err(ApiError::internal(format!(
                "script version family mismatch while building family detail: family_id={}, version_hash=0x{}, version_family_id={:?}",
                family_id,
                hex::encode(&version_hash),
                version_info.family_id
            )));
        }

        versions.push((version_hash, version_info));
    }

    Ok(versions)
}

fn resolved_version_name(
    state: &AppState,
    version_hash: &[u8],
    info: &ckbadger_store::types::ScriptVersionInfo,
    unknown_fallback: &str,
) -> Result<String, ApiRouteError> {
    if let Some(name) = info.name.clone() {
        return Ok(name);
    }

    let Some(family_id) = info.family_id.as_deref() else {
        return Ok(unknown_fallback.to_string());
    };

    let family = state
        .store
        .get_script_family(family_id)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| {
            ApiError::internal(format!(
                "script version points to missing family while resolving name: version_hash=0x{}, family_id={}",
                hex::encode(version_hash),
                family_id
            ))
        })?;

    Ok(family.name)
}

fn script_version_to_detail_response(
    state: &AppState,
    version_hash: &[u8],
    info: &ckbadger_store::types::ScriptVersionInfo,
    observed_references: Vec<ScriptObservedReferenceResponse>,
) -> Result<ScriptVersionDetailResponse, ApiRouteError> {
    let (cells_count, live_cells_count, _, owned_capacity_sum, _, owned_knowledge_sum) =
        checked_version_totals(version_hash, info, "script family detail")?;
    let mut code_cells =
        list_version_code_cells(&state.store, &state.append_only_store, version_hash)
            .map_err(|e| ApiError::internal(e.to_string()))?;
    sort_code_cells(&mut code_cells);

    let deployed_at = first_code_cell_timestamp(state, version_hash, &code_cells)?;
    let code_cells_live_count = code_cells
        .iter()
        .filter(|(_, _, _, is_live)| *is_live)
        .count() as i64;
    let code_cells_total = code_cells.len() as i64;
    let deployments = script_version_deployments(state, version_hash, info, &code_cells)?;

    Ok(ScriptVersionDetailResponse {
        version_hash: format!("0x{}", hex::encode(version_hash)),
        name: resolved_version_name(
            state,
            version_hash,
            info,
            &format!("0x{}", hex::encode(version_hash)),
        )?,
        description: info.description.clone(),
        script_kind: version_script_kind(info),
        website: info.website.clone(),
        deprecated: info.deprecated,
        canonical_reference_hash: info
            .canonical_reference_hash
            .as_ref()
            .map(|hash| format!("0x{}", hex::encode(hash))),
        canonical_hash_type: info
            .canonical_hash_type
            .and_then(hash_type_to_string)
            .map(str::to_string),
        deployed_at,
        live_cells_count,
        cells_count,
        owned_capacity_sum: owned_capacity_sum.to_string(),
        owned_knowledge_sum: owned_knowledge_sum.to_string(),
        code_cells_live_count,
        code_cells_total,
        deployments,
        references: observed_references,
    })
}

/// Build a ScriptLookupInfo from a resolved script identifier.
///
/// Shared by both per-tx and global resolution paths.
fn build_lookup_info(
    state: &AppState,
    reference_hash: &[u8],
    reference_hash_hex: &str,
    version_hash: &[u8],
    version_info: &ckbadger_store::types::ScriptVersionInfo,
    mut code_cells: Vec<VersionCodeCell>,
    all_script_infos: &[(Vec<u8>, ckbadger_store::ScriptInfo)],
) -> Result<ScriptLookupInfo, ApiRouteError> {
    sort_code_cells(&mut code_cells);
    let script_info = state
        .store
        .get_script_info(reference_hash)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let direct_info_for_version = if reference_hash == version_hash {
        script_info.as_ref()
    } else {
        None
    };
    let (
        _cells_count,
        live_cells_count,
        _capacity_sum,
        owned_capacity_sum,
        _used_sum,
        owned_knowledge,
    ) = resolve_version_capacity(version_info, direct_info_for_version, all_script_infos)?;
    let code_cell = code_cells.first();
    let hash_type = script_info
        .as_ref()
        .and_then(|info| hash_type_to_string(info.hash_type).map(|s| s.to_string()));
    let (deployment_type_hash, deployment_data_hash) = script_info
        .as_ref()
        .map(|info| {
            let (type_ref, data_ref) = deployment_reference_hashes(info);
            (
                type_ref.map(|h| format!("0x{}", hex::encode(h))),
                data_ref.map(|h| format!("0x{}", hex::encode(h))),
            )
        })
        .unwrap_or((None, None));

    Ok(ScriptLookupInfo {
        reference_hash: reference_hash_hex.to_string(),
        code_hash: format!("0x{}", hex::encode(version_hash)),
        name: resolved_version_name(state, version_hash, version_info, "Unknown")?,
        deprecated: version_info.deprecated
            || script_info
                .as_ref()
                .map(|info| info.deprecated)
                .unwrap_or(false),
        script_kind: version_script_kind(version_info),
        decoder_type: version_info.category.clone(),
        hash_type,
        deployment_type_hash,
        deployment_data_hash,
        code_cell_tx_hash: code_cell
            .map(|(tx_hash, _, _, _)| format!("0x{}", hex::encode(tx_hash))),
        code_cell_output_index: code_cell.map(|(_, output_index, _, _)| i32::from(*output_index)),
        live_cells_count,
        owned_capacity_sum: owned_capacity_sum.to_string(),
        owned_knowledge_sum: owned_knowledge.to_string(),
        code_cells_live_count: code_cells
            .iter()
            .filter(|(_, _, _, is_live)| *is_live)
            .count() as i64,
        code_cells_total: code_cells.len() as i64,
        resolution_state: "resolved".to_string(),
        ambiguity: None,
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
    let all_script_infos = load_script_infos_cached(&state)?;
    let per_tx_mappings = request
        .tx_hash
        .as_deref()
        .and_then(|tx_hash| resolve_dep_cells_for_transaction(&state, tx_hash));

    for code_hash in &code_hash_bytes {
        let reference_hash_hex = format!("0x{}", hex::encode(code_hash));

        // Per-tx resolution: if cell_deps provide a version_hash, use it directly
        if let Some(version_hash) = per_tx_mappings
            .as_ref()
            .and_then(|m| m.get(code_hash.as_slice()))
        {
            let version_info = state
                .store
                .get_script_version(version_hash)
                .map_err(|e| ApiError::internal(e.to_string()))?
                .unwrap_or_else(|| {
                    fallback_script_version_info(&state, code_hash, version_hash)
                        .unwrap_or_default()
                });

            let code_cells =
                list_version_code_cells(&state.store, &state.append_only_store, version_hash)
                    .unwrap_or_default();

            let info = build_lookup_info(
                &state,
                code_hash,
                &reference_hash_hex,
                version_hash,
                &version_info,
                code_cells,
                &all_script_infos,
            )?;
            result.insert(reference_hash_hex.clone(), info);
            continue;
        }

        match resolve_script_identifier(&state, code_hash)? {
            ScriptIdentifierResolution::Resolved(resolved) => {
                let ResolvedScriptIdentifier {
                    version_hash,
                    version_info,
                    code_cells,
                } = *resolved;
                let info = build_lookup_info(
                    &state,
                    code_hash,
                    &reference_hash_hex,
                    &version_hash,
                    &version_info,
                    code_cells,
                    &all_script_infos,
                )?;
                result.insert(reference_hash_hex.clone(), info);
            }
            ScriptIdentifierResolution::Ambiguous(ambiguous) => {
                let AmbiguousScriptIdentifier { version_hashes } = ambiguous;
                let infos_bare: Vec<ckbadger_store::ScriptInfo> = all_script_infos
                    .iter()
                    .map(|(_, info)| info.clone())
                    .collect();
                let merged = merge_script_info_for_reference(&infos_bare, code_hash);
                // Try to get a known name: first from merged ScriptInfo, then from any version
                let merged_name = merged
                    .as_ref()
                    .and_then(|m| m.name.clone())
                    .filter(|n| crate::utils::is_known_script_name(Some(n.as_str())));
                let name = if let Some(n) = merged_name {
                    n
                } else {
                    // Try each version_hash for a name
                    let mut found_name = None;
                    for vh in &version_hashes {
                        if let Some(vi) = state
                            .store
                            .get_script_version(vh)
                            .map_err(|e| ApiError::internal(e.to_string()))?
                        {
                            let n = resolved_version_name(&state, vh, &vi, "")?;
                            if !n.is_empty() {
                                found_name = Some(n);
                                break;
                            }
                        }
                    }
                    found_name.unwrap_or_else(|| "Ambiguous Script Reference".to_string())
                };
                let script_kind = merged
                    .as_ref()
                    .and_then(|m| script_kind_from_counts(m.lock_cells_count, m.type_cells_count));
                let hash_type = merged
                    .as_ref()
                    .and_then(|m| hash_type_to_string(m.hash_type).map(|s| s.to_string()));
                result.insert(
                    reference_hash_hex.clone(),
                    ScriptLookupInfo {
                        reference_hash: reference_hash_hex.clone(),
                        code_hash: reference_hash_hex.clone(),
                        name,
                        deprecated: false,
                        script_kind,
                        decoder_type: None,
                        hash_type,
                        deployment_type_hash: None,
                        deployment_data_hash: None,
                        code_cell_tx_hash: None,
                        code_cell_output_index: None,
                        live_cells_count: 0,
                        owned_capacity_sum: "0".to_string(),
                        owned_knowledge_sum: "0".to_string(),
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
    #[serde(rename = "hash_type")]
    _hash_type: Option<String>,
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
    hash_type: Option<String>,
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
) -> ApiResult<CursorPaginatedResponse<ScriptFamilyListItemResponse>> {
    let limit = params.limit.clamp(1, 100) as usize;
    let _network = params.network.as_deref().unwrap_or(&state.ckb_network);
    let all_families = load_script_families_cached(&state)?;

    let search_pattern = params.search.as_ref().map(|s| s.to_lowercase());

    let mut filtered: Vec<_> = all_families
        .into_iter()
        .filter(|(_, info)| {
            // Exclude families with zero versions (e.g. other-network-only scripts
            // that leaked through a prior label import).
            if info.versions_count <= 0 {
                return false;
            }
            if let Some(ref pattern) = search_pattern {
                if !info.name.to_lowercase().contains(pattern) {
                    return false;
                }
            }
            true
        })
        .collect();

    for (_, info) in &filtered {
        checked_family_totals(info, "list script families")?;
    }
    filtered.sort_by(|a, b| {
        compare_script_family_entries(a, b, params.sort_key, params.sort_direction)
    });

    let total = filtered.len() as i64;

    let start_idx = params
        .cursor
        .as_deref()
        .and_then(decode_cursor_single)
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(0);

    let page: Vec<_> = filtered
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

    let scripts: Vec<ScriptFamilyListItemResponse> = page
        .iter()
        .map(|(family_id, info)| script_family_to_response(family_id, info))
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
) -> ApiResult<ScriptFamilyDetailResponse> {
    let (family_id, family) = load_script_family_by_name(&state, &name)?;
    let family_versions = load_family_versions(&state, &family_id)?;
    let mut observed_references =
        observed_references_by_version(&state, &family_id, &family_versions)?;
    let mut versions = family_versions
        .into_iter()
        .map(|(version_hash, version_info)| {
            let refs = observed_references
                .remove(&version_hash)
                .unwrap_or_default();
            script_version_to_detail_response(&state, &version_hash, &version_info, refs)
        })
        .collect::<Result<Vec<_>, _>>()?;

    versions.sort_by(|left, right| {
        match (left.deployed_at, right.deployed_at) {
            (Some(left_ts), Some(right_ts)) => right_ts.cmp(&left_ts),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
        .then_with(|| left.version_hash.cmp(&right.version_hash))
    });

    let (owned_capacity_sum, owned_knowledge_sum) =
        checked_family_totals(&family, "script family detail")?;

    ok(ScriptFamilyDetailResponse {
        family_id,
        name: family.name,
        description: family.description,
        script_kind: script_kind_from_counts(family.lock_cells_count, family.type_cells_count),
        website: family.website,
        live_cells_count: family.live_cells_count,
        cells_count: family.cells_count,
        owned_capacity_sum: owned_capacity_sum.to_string(),
        owned_knowledge_sum: owned_knowledge_sum.to_string(),
        versions_count: family.versions_count,
        versions,
    })
}

async fn get_script_usage(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> ApiResult<ScriptUsageResponse> {
    let (family_id, _family) = load_script_family_by_name(&state, &name)?;

    let mut total_cells: i64 = 0;
    let mut total_live: i64 = 0;
    let mut total_cap: u128 = 0;
    let mut total_live_cap: u128 = 0;
    let mut total_used_cap: u128 = 0;
    let mut total_owned_knowledge: u128 = 0;

    let mut by_deployment: Vec<DeploymentUsage> = load_family_versions(&state, &family_id)?
        .into_iter()
        .map(|(version_hash, info)| {
            let (
                cells_count,
                live_cells_count,
                capacity_sum,
                owned_capacity_sum,
                used_capacity_sum,
                owned_knowledge_sum,
            ) = checked_version_totals(&version_hash, &info, "script family usage")?;
            let capacity_sum = capacity_sum as u128;
            let owned_capacity_sum = owned_capacity_sum as u128;
            let used_capacity_sum = used_capacity_sum as u128;
            let owned_knowledge_sum = owned_knowledge_sum as u128;

            total_cells += cells_count;
            total_live += live_cells_count;
            total_cap += capacity_sum;
            total_live_cap += owned_capacity_sum;
            total_used_cap += used_capacity_sum;
            total_owned_knowledge += owned_knowledge_sum;

            Ok(DeploymentUsage {
                code_hash: format!("0x{}", hex::encode(&version_hash)),
                script_kind: version_script_kind(&info),
                cells_count,
                live_cells_count,
                capacity_sum: capacity_sum.to_string(),
                owned_capacity_sum: owned_capacity_sum.to_string(),
                used_capacity_sum: used_capacity_sum.to_string(),
                owned_knowledge_sum: owned_knowledge_sum.to_string(),
            })
        })
        .collect::<Result<Vec<_>, ApiRouteError>>()?;
    by_deployment.sort_by(|left, right| {
        right
            .live_cells_count
            .cmp(&left.live_cells_count)
            .then_with(|| left.code_hash.cmp(&right.code_hash))
    });

    ok(ScriptUsageResponse {
        name,
        cells_count: total_cells,
        live_cells_count: total_live,
        capacity_sum: total_cap.to_string(),
        owned_capacity_sum: total_live_cap.to_string(),
        used_capacity_sum: total_used_cap.to_string(),
        owned_knowledge_sum: total_owned_knowledge.to_string(),
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
    apply_owned_capacity_delta(
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
    targets: Vec<(Vec<u8>, u8, bool)>,
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
    let unique_targets: Vec<(Vec<u8>, u8, bool)> = targets
        .into_iter()
        .filter(|target| dedup.insert(target.clone()))
        .collect();

    let mut cumulative_capacity: i128 = 0;
    let mut cumulative_used: i128 = 0;
    if let Some(from) = from_date {
        let mut baseline_daily: BTreeMap<u32, (i128, i128)> = BTreeMap::new();
        for (code_hash, hash_type, is_type) in &unique_targets {
            let baseline = state
                .store
                .list_script_daily_deltas_in_range(
                    code_hash,
                    *hash_type,
                    *is_type,
                    None,
                    Some(from.saturating_sub(1)),
                )
                .map_err(|e| ApiError::internal(e.to_string()))?;
            for (date, delta) in baseline {
                let entry = baseline_daily.entry(date).or_insert((0, 0));
                entry.0 += delta.owned_capacity_delta;
                entry.1 += delta.owned_knowledge_delta;
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
    for (code_hash, hash_type, is_type) in &unique_targets {
        let deltas = state
            .store
            .list_script_daily_deltas_in_range(code_hash, *hash_type, *is_type, from_date, to_date)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        for (date, delta) in deltas {
            let entry = daily_deltas.entry(date).or_insert((0, 0));
            entry.0 += delta.owned_capacity_delta;
            entry.1 += delta.owned_knowledge_delta;
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

    let code_hash_filter = params
        .code_hash
        .as_deref()
        .map(parse_code_hash_hex)
        .transpose()?;
    let kind_filter = parse_script_kind_filter(params.script_kind.as_deref())?;

    // The chart aggregates exactly the family's member reference forms — the
    // same membership computation the usage counters and family detail use.
    let (family_id, _family) = load_script_family_by_name(&state, &name)?;
    let family_versions = load_family_versions(&state, &family_id)?;
    let allowed_versions: HashSet<Vec<u8>> = family_versions
        .iter()
        .map(|(version_hash, _)| version_hash.clone())
        .collect();
    let canonical_references = canonical_reference_set(&family_versions);
    let member_forms = family_member_reference_forms(
        &state,
        &family_id,
        &allowed_versions,
        &canonical_references,
    )?;

    let mut targets = Vec::new();
    for member in member_forms {
        if code_hash_filter
            .as_ref()
            .map(|filter| &member.reference_hash == filter)
            .unwrap_or(true)
        {
            for is_type in &kind_filter {
                targets.push((member.reference_hash.clone(), member.hash_type, *is_type));
            }
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

/// Capacity-history chart addressed by code_hash.
///
/// With an explicit `hash_type` parameter the chart covers exactly the single
/// observed reference form `(code_hash, hash_type)` — no resolution.
///
/// Without `hash_type` the reference is resolved and the chart covers the
/// member reference set of the resolved version's family — the same set the
/// `/scripts/{name}/charts/capacity-history` path aggregates, so both
/// endpoints and the family counters share one membership computation. A
/// resolved version without a family charts the forms observed for that
/// version alone; an unknown reference charts every form recorded under the
/// code_hash bytes.
async fn get_script_capacity_history_chart_by_code_hash(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ScriptCapacityHistoryByCodeHashQuery>,
) -> ApiResult<StackedAreaChartResponse> {
    let (from_date, to_date) = parse_chart_date_range(params.from.as_deref(), params.to.as_deref())
        .map_err(|msg| ApiError::bad_request(&msg))?;

    let code_hash = parse_code_hash_hex(&params.code_hash)?;
    let kind_filter = parse_script_kind_filter(params.script_kind.as_deref())?;
    let title = format!("0x{} Capacity History", hex::encode(&code_hash));

    if let Some(hash_type_str) = params.hash_type.as_deref() {
        let hash_type = hash_type_to_u8(hash_type_str).ok_or_else(|| {
            ApiError::bad_request("Invalid hash_type, expected data/type/data1/data2")
        })?;
        let targets = kind_filter
            .iter()
            .map(|is_type| (code_hash.clone(), hash_type, *is_type))
            .collect();
        return ok(build_script_capacity_history_chart(
            &state, targets, title, from_date, to_date,
        )?);
    }

    let mut targets = Vec::new();
    match resolve_script_identifier(&state, &code_hash)? {
        ScriptIdentifierResolution::Resolved(resolved) => {
            let ResolvedScriptIdentifier {
                version_hash,
                version_info,
                ..
            } = *resolved;
            let (allowed_versions, canonical_references, membership_context) =
                if let Some(family_id) = version_info.family_id.as_deref() {
                    let family_versions = load_family_versions(&state, family_id)?;
                    let allowed: HashSet<Vec<u8>> = family_versions
                        .iter()
                        .map(|(version_hash, _)| version_hash.clone())
                        .collect();
                    let canonical = canonical_reference_set(&family_versions);
                    (allowed, canonical, family_id.to_string())
                } else {
                    (
                        HashSet::from([version_hash.clone()]),
                        HashSet::new(),
                        format!("version 0x{}", hex::encode(&version_hash)),
                    )
                };
            let member_forms = family_member_reference_forms(
                &state,
                &membership_context,
                &allowed_versions,
                &canonical_references,
            )?;
            for member in member_forms {
                for is_type in &kind_filter {
                    targets.push((member.reference_hash.clone(), member.hash_type, *is_type));
                }
            }
        }
        ScriptIdentifierResolution::Ambiguous(_) => {
            return Err(ApiError::bad_request(
                "script capacity history requires an unambiguous script reference",
            ));
        }
        ScriptIdentifierResolution::NotFound => {
            for hash_type in [0u8, 1u8, 2u8, 4u8] {
                for is_type in &kind_filter {
                    targets.push((code_hash.clone(), hash_type, *is_type));
                }
            }
        }
    }

    ok(build_script_capacity_history_chart(
        &state, targets, title, from_date, to_date,
    )?)
}

#[cfg(test)]
mod tests {
    use super::{
        apply_script_chart_delta, checked_capacity_totals,
        latest_complete_script_chart_date_from_tip, resolve_code_cell,
        resolve_script_capacity_chart_bounds, resolve_version_capacity, CodeCellQuery, ListParams,
    };
    use axum::extract::Query;
    use axum::http::StatusCode;
    use axum::http::Uri;
    use ckbadger_store::types::ScriptVersionInfo;
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
            .contains("owned knowledge exceeds owned capacity"));
    }

    #[test]
    fn checked_capacity_totals_errors_on_negative_capacity() {
        let info = ScriptInfo {
            code_hash: vec![0xAA; 32],
            lock_owned_capacity_sum: -1,
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
            lock_owned_capacity_sum: 100,
            lock_owned_knowledge_sum: 101,
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

    #[test]
    fn list_params_deserializes_unused_query_schema_fields() {
        let uri: Uri = "/scripts?decoder_type=typeid&cursor=cursor-token&limit=25"
            .parse()
            .unwrap();
        let Query(params) = Query::<ListParams>::try_from_uri(&uri).unwrap();

        assert_eq!(params.cursor.as_deref(), Some("cursor-token"));
        assert_eq!(params.limit, 25);
    }

    #[test]
    fn code_cell_query_deserializes_legacy_hash_type_field() {
        let uri: Uri = "/scripts/code-cell?code_hash=0x1234&hash_type=data"
            .parse()
            .unwrap();
        let Query(params) = Query::<CodeCellQuery>::try_from_uri(&uri).unwrap();

        assert_eq!(params.code_hash, "0x1234");
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

    #[test]
    fn resolve_version_capacity_uses_direct_script_info_parameter() {
        let version = ScriptVersionInfo {
            version_hash: vec![0xAA; 32],
            name: Some("test_script".to_string()),
            ..Default::default()
        };
        let script_info = ScriptInfo {
            code_hash: vec![0xAA; 32],
            lock_owned_capacity_sum: 100_00000000,
            lock_owned_knowledge_sum: 61_00000000,
            lock_cells_count: 5,
            lock_live_cells_count: 3,
            lock_capacity_sum: 200_00000000,
            lock_used_capacity_sum: 122_00000000,
            ..Default::default()
        };
        let cache: Vec<(Vec<u8>, ScriptInfo)> = vec![];
        let (cells, live, cap, live_cap, used, live_used) =
            resolve_version_capacity(&version, Some(&script_info), &cache).unwrap();
        assert_eq!(cells, 5);
        assert_eq!(live, 3);
        assert_eq!(cap, 200_00000000);
        assert_eq!(live_cap, 100_00000000);
        assert_eq!(used, 122_00000000);
        assert_eq!(live_used, 61_00000000);
    }

    #[test]
    fn resolve_version_capacity_finds_by_version_hash_in_cache() {
        let version = ScriptVersionInfo {
            version_hash: vec![0xAA; 32],
            name: Some("test_script".to_string()),
            ..Default::default()
        };
        let script_info = ScriptInfo {
            code_hash: vec![0xAA; 32],
            lock_owned_capacity_sum: 100_00000000,
            lock_owned_knowledge_sum: 61_00000000,
            lock_cells_count: 5,
            lock_live_cells_count: 3,
            lock_capacity_sum: 200_00000000,
            lock_used_capacity_sum: 122_00000000,
            ..Default::default()
        };
        let cache = vec![(vec![0xAA; 32], script_info)];
        let (cells, live, cap, live_cap, used, live_used) =
            resolve_version_capacity(&version, None, &cache).unwrap();
        assert_eq!(cells, 5);
        assert_eq!(live, 3);
        assert_eq!(cap, 200_00000000);
        assert_eq!(live_cap, 100_00000000);
        assert_eq!(used, 122_00000000);
        assert_eq!(live_used, 61_00000000);
    }

    #[test]
    fn resolve_version_capacity_uses_associated_code_hash_for_type_hash_scripts() {
        // Type-hash script: version_hash (data_hash) differs from code_hash.
        // associated_code_hash bridges the gap.
        let code_hash = vec![0xCC; 32];
        let data_hash = vec![0xBB; 32]; // different from code_hash
        let version = ScriptVersionInfo {
            version_hash: data_hash,
            name: Some("secp256k1_blake160".to_string()),
            associated_code_hash: Some(code_hash.clone()),
            ..Default::default()
        };
        let script_info = ScriptInfo {
            code_hash: code_hash.clone(),
            name: Some("secp256k1_blake160".to_string()),
            lock_owned_capacity_sum: 500_00000000,
            lock_owned_knowledge_sum: 200_00000000,
            lock_cells_count: 10,
            lock_live_cells_count: 8,
            lock_capacity_sum: 800_00000000,
            lock_used_capacity_sum: 400_00000000,
            ..Default::default()
        };
        let cache = vec![(code_hash, script_info)];
        let (cells, live, _cap, live_cap, _used, live_used) =
            resolve_version_capacity(&version, None, &cache).unwrap();
        assert_eq!(cells, 10);
        assert_eq!(live, 8);
        assert_eq!(live_cap, 500_00000000);
        assert_eq!(live_used, 200_00000000);
    }

    #[test]
    fn resolve_version_capacity_multi_version_uses_correct_per_version_stats() {
        // Simulates a multi-version script where each version has different stats.
        // This is the exact bug scenario: without associated_code_hash, all versions
        // would return the same stats.
        let code_hash_v1 = vec![0xA1; 32];
        let data_hash_v1 = vec![0xD1; 32];
        let code_hash_v2 = vec![0xA2; 32];
        let data_hash_v2 = vec![0xD2; 32];

        let version_v1 = ScriptVersionInfo {
            version_hash: data_hash_v1,
            name: Some("Multisig".to_string()),
            associated_code_hash: Some(code_hash_v1.clone()),
            ..Default::default()
        };
        let version_v2 = ScriptVersionInfo {
            version_hash: data_hash_v2,
            name: Some("Multisig".to_string()),
            associated_code_hash: Some(code_hash_v2.clone()),
            ..Default::default()
        };
        let cache = vec![
            (
                code_hash_v1.clone(),
                ScriptInfo {
                    code_hash: code_hash_v1,
                    name: Some("Multisig".to_string()),
                    lock_live_cells_count: 100,
                    lock_owned_capacity_sum: 1000,
                    ..Default::default()
                },
            ),
            (
                code_hash_v2.clone(),
                ScriptInfo {
                    code_hash: code_hash_v2,
                    name: Some("Multisig".to_string()),
                    lock_live_cells_count: 5,
                    lock_owned_capacity_sum: 50,
                    ..Default::default()
                },
            ),
        ];

        let (_, live_v1, _, live_cap_v1, _, _) =
            resolve_version_capacity(&version_v1, None, &cache).unwrap();
        let (_, live_v2, _, live_cap_v2, _, _) =
            resolve_version_capacity(&version_v2, None, &cache).unwrap();

        assert_eq!(live_v1, 100);
        assert_eq!(live_cap_v1, 1000);
        assert_eq!(live_v2, 5);
        assert_eq!(live_cap_v2, 50);
        assert_ne!(live_v1, live_v2, "each version must have distinct stats");
    }

    #[test]
    fn resolve_version_capacity_returns_zeros_when_no_script_info() {
        let version = ScriptVersionInfo {
            version_hash: vec![0xDD; 32],
            name: Some("unknown_script".to_string()),
            ..Default::default()
        };
        let cache: Vec<(Vec<u8>, ScriptInfo)> = vec![];
        let (cells, live, cap, live_cap, used, live_used) =
            resolve_version_capacity(&version, None, &cache).unwrap();
        assert_eq!(cells, 0);
        assert_eq!(live, 0);
        assert_eq!(cap, 0);
        assert_eq!(live_cap, 0);
        assert_eq!(used, 0);
        assert_eq!(live_used, 0);
    }

    #[test]
    fn resolve_version_capacity_rejects_negative_owned_capacity() {
        let version = ScriptVersionInfo {
            version_hash: vec![0xEE; 32],
            name: Some("bad_script".to_string()),
            ..Default::default()
        };
        let bad_info = ScriptInfo {
            code_hash: vec![0xEE; 32],
            lock_owned_capacity_sum: -1,
            ..Default::default()
        };
        let cache = vec![(vec![0xEE; 32], bad_info)];
        let err = resolve_version_capacity(&version, None, &cache).unwrap_err();
        assert_eq!(err.0, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(err.1 .0.message.contains("negative live capacity"));
    }

    #[test]
    fn resolve_version_capacity_rejects_used_exceeds_total() {
        let version = ScriptVersionInfo {
            version_hash: vec![0xFF; 32],
            name: Some("bad_script2".to_string()),
            ..Default::default()
        };
        let bad_info = ScriptInfo {
            code_hash: vec![0xFF; 32],
            lock_owned_capacity_sum: 100,
            lock_owned_knowledge_sum: 101,
            ..Default::default()
        };
        let cache = vec![(vec![0xFF; 32], bad_info)];
        let err = resolve_version_capacity(&version, None, &cache).unwrap_err();
        assert_eq!(err.0, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(err.1 .0.message.contains("live used exceeds total"));
    }
}
