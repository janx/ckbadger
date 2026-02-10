#![allow(clippy::type_complexity)]

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::response::{ok, ApiError, ApiResult, CursorPaginatedResponse};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/scripts", get(list_scripts))
        .route("/scripts/lookup", post(lookup_scripts))
        .route("/scripts/code-cell", get(get_code_cell))
        .route("/scripts/{name}", get(get_script))
        .route("/scripts/{name}/usage", get(get_script_usage))
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
}

fn default_limit() -> i64 {
    20
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
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptUsageResponse {
    pub name: String,
    pub cells_count: i64,
    pub live_cells_count: i64,
    pub capacity_sum: String,
    pub live_capacity_sum: String,
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
    pub code_cell_tx_hash: Option<String>,
    pub code_cell_output_index: Option<i32>,
    pub live_cells_count: i64,
    pub live_capacity_sum: String,
}

/// Convert a store ScriptInfo into an API ScriptResponse.
fn script_info_to_response(info: &ckbadger_store::ScriptInfo, network: &str) -> ScriptResponse {
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

    ScriptResponse {
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
        data_hash: None,
        type_hash: None,
        tag: None,
        deprecated: false,
        is_system: false,
        code_cell_tx_hash: None,
        code_cell_output_index: None,
    }
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
        let info = state
            .store
            .get_script_info(code_hash)
            .map_err(|e| ApiError::internal(e.to_string()))?;

        if let Some(info) = info {
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
            let live_capacity_sum = (info.lock_live_capacity_sum as i128
                + info.type_live_capacity_sum as i128)
                .to_string();

            result.insert(
                code_hash_hex.clone(),
                ScriptLookupInfo {
                    code_hash: code_hash_hex,
                    name: info.name.clone().unwrap_or_else(|| "Unknown".to_string()),
                    script_kind,
                    decoder_type: None,
                    hash_type: hash_type_str,
                    code_cell_tx_hash: None,
                    code_cell_output_index: None,
                    live_cells_count,
                    live_capacity_sum,
                },
            );
        }
    }

    ok(result)
}

#[derive(Debug, Deserialize)]
pub struct CodeCellQuery {
    code_hash: String,
    #[allow(dead_code)]
    hash_type: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeCellResponse {
    pub tx_hash: Option<String>,
    pub output_index: Option<i32>,
}

async fn get_code_cell(
    State(_state): State<Arc<AppState>>,
    Query(params): Query<CodeCellQuery>,
) -> ApiResult<CodeCellResponse> {
    let _code_hash_bytes = hex::decode(
        params
            .code_hash
            .strip_prefix("0x")
            .unwrap_or(&params.code_hash),
    )
    .map_err(|_| ApiError::bad_request("Invalid code_hash hex"))?;

    // Code cell lookup requires cell table data which is in RocksDB live_cells CF,
    // but not indexed by data_hash or type_script_hash in the current store API.
    // Return empty for now.
    ok(CodeCellResponse {
        tx_hash: None,
        output_index: None,
    })
}

async fn list_scripts(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<ScriptResponse>> {
    let _limit = params.limit.clamp(1, 100);
    let network = params.network.as_deref().unwrap_or(&state.ckb_network);

    let all_scripts = state
        .store
        .list_script_infos()
        .map_err(|e| ApiError::internal(e.to_string()))?;

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

    // Sort by name
    filtered.sort_by(|a, b| {
        let name_a = a.1.name.as_deref().unwrap_or("");
        let name_b = b.1.name.as_deref().unwrap_or("");
        name_a.cmp(name_b)
    });

    // Deduplicate by name (take first occurrence)
    let mut seen_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    let deduped: Vec<_> = filtered
        .into_iter()
        .filter(|(_, info)| {
            let name = info.name.clone().unwrap_or_default();
            seen_names.insert(name)
        })
        .collect();

    let total = deduped.len() as i64;

    let scripts: Vec<ScriptResponse> = deduped
        .iter()
        .map(|(_, info)| script_info_to_response(info, network))
        .collect();

    let total_rows = scripts.len() as i64;

    ok(CursorPaginatedResponse::new(
        scripts,
        total.max(total_rows),
        total_rows,
        None,
    ))
}

async fn get_script(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> ApiResult<Vec<ScriptResponse>> {
    let network = &state.ckb_network;

    let all_scripts = state
        .store
        .list_script_infos()
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let matching: Vec<_> = all_scripts
        .into_iter()
        .filter(|(_, info)| info.name.as_deref() == Some(name.as_str()))
        .collect();

    if matching.is_empty() {
        return Err(ApiError::not_found("Script not found"));
    }

    let scripts: Vec<ScriptResponse> = matching
        .iter()
        .map(|(_, info)| script_info_to_response(info, network))
        .collect();

    ok(scripts)
}

async fn get_script_usage(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> ApiResult<ScriptUsageResponse> {
    let all_scripts = state
        .store
        .list_script_infos()
        .map_err(|e| ApiError::internal(e.to_string()))?;

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
            by_deployment: vec![],
        });
    }

    let mut total_cells: i64 = 0;
    let mut total_live: i64 = 0;
    let mut total_cap: u128 = 0;
    let mut total_live_cap: u128 = 0;

    let by_deployment: Vec<DeploymentUsage> = matching
        .into_iter()
        .map(|(_, info)| {
            let cells_count = info.lock_cells_count + info.type_cells_count;
            let live_cells_count = info.lock_live_cells_count + info.type_live_cells_count;
            let capacity_sum =
                (info.lock_capacity_sum as i128 + info.type_capacity_sum as i128) as u128;
            let live_capacity_sum =
                (info.lock_live_capacity_sum as i128 + info.type_live_capacity_sum as i128) as u128;

            total_cells += cells_count;
            total_live += live_cells_count;
            total_cap += capacity_sum;
            total_live_cap += live_capacity_sum;

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
            }
        })
        .collect();

    ok(ScriptUsageResponse {
        name,
        cells_count: total_cells,
        live_cells_count: total_live,
        capacity_sum: total_cap.to_string(),
        live_capacity_sum: total_live_cap.to_string(),
        by_deployment,
    })
}
