#![allow(clippy::type_complexity)]

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
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
    cursor: Option<String>,
    network: Option<String>,
    decoder_type: Option<String>,
    search: Option<String>,
}

fn default_limit() -> i64 {
    20
}

#[derive(Debug, FromRow)]
struct ScriptRow {
    #[allow(dead_code)]
    id: i64,
    code_hash: Vec<u8>,
    name: String,
    description: Option<String>,
    rfc: Option<String>,
    website: Option<String>,
    source_url: Option<String>,
    decoder_type: Option<String>,
    network: String,
    hash_type: Option<String>,
    data_hash: Option<Vec<u8>>,
    type_hash: Option<Vec<u8>>,
    tag: Option<String>,
    deprecated: bool,
    is_system: bool,
    script_kind: Option<String>,
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

    let rows: Vec<(
        Vec<u8>,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        i64,
        String,
    )> = sqlx::query_as(
        r#"
        SELECT DISTINCT ON (ks.code_hash) 
            ks.code_hash, 
            ks.name, 
            sus.script_kind, 
            ks.decoder_type, 
            ks.hash_type,
            COALESCE(SUM(sus.live_cells_count) OVER (PARTITION BY ks.code_hash), 0)::BIGINT as live_cells_count,
            COALESCE(SUM(sus.live_capacity_sum) OVER (PARTITION BY ks.code_hash), 0)::TEXT as live_capacity_sum
        FROM known_scripts ks
        LEFT JOIN script_usage_stats sus ON sus.code_hash = ks.code_hash
        WHERE ks.code_hash = ANY($1) AND ks.network = $2
        ORDER BY ks.code_hash, ks.deprecated ASC, ks.is_system DESC
        "#,
    )
    .bind(&code_hash_bytes)
    .bind(&state.ckb_network)
    .fetch_all(&state.read_pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    if rows.is_empty() {
        return ok(HashMap::new());
    }

    let mut code_cell_map: std::collections::HashMap<Vec<u8>, (Vec<u8>, i16)> =
        std::collections::HashMap::new();

    let data_hashes: Vec<Vec<u8>> = rows
        .iter()
        .filter(|r| r.4.as_deref() != Some("type"))
        .map(|r| r.0.clone())
        .collect();

    if !data_hashes.is_empty() {
        let cells: Vec<(Vec<u8>, Vec<u8>, i16)> = sqlx::query_as(
            r#"
            SELECT DISTINCT ON (data_hash) data_hash, tx_hash, output_index
            FROM cells
            WHERE data_hash = ANY($1)
            ORDER BY data_hash, status ASC, created_at_block DESC
            "#,
        )
        .bind(&data_hashes)
        .fetch_all(&state.read_pool)
        .await
        .unwrap_or_default();

        for (hash, tx, idx) in cells {
            code_cell_map.insert(hash, (tx, idx));
        }
    }

    let type_hashes: Vec<Vec<u8>> = rows
        .iter()
        .filter(|r| r.4.as_deref() == Some("type"))
        .map(|r| r.0.clone())
        .collect();

    if !type_hashes.is_empty() {
        let cells: Vec<(Vec<u8>, Vec<u8>, i16)> = sqlx::query_as(
            r#"
            SELECT DISTINCT ON (type_script_hash) type_script_hash, tx_hash, output_index
            FROM cells
            WHERE type_script_hash = ANY($1) AND status = 0
            ORDER BY type_script_hash, created_at_block DESC
            "#,
        )
        .bind(&type_hashes)
        .fetch_all(&state.read_pool)
        .await
        .unwrap_or_default();

        for (hash, tx, idx) in cells {
            code_cell_map.insert(hash, (tx, idx));
        }
    }

    let result: HashMap<String, ScriptLookupInfo> = rows
        .into_iter()
        .map(
            |(
                code_hash,
                name,
                script_kind,
                decoder_type,
                hash_type,
                live_cells_count,
                live_capacity_sum,
            )| {
                let code_hash_hex = format!("0x{}", hex::encode(&code_hash));
                let (tx_hash, output_index) = code_cell_map
                    .get(&code_hash)
                    .map(|(tx, idx)| (Some(format!("0x{}", hex::encode(tx))), Some(*idx as i32)))
                    .unwrap_or((None, None));
                (
                    code_hash_hex.clone(),
                    ScriptLookupInfo {
                        code_hash: code_hash_hex,
                        name,
                        script_kind,
                        decoder_type,
                        hash_type,
                        code_cell_tx_hash: tx_hash,
                        code_cell_output_index: output_index,
                        live_cells_count,
                        live_capacity_sum,
                    },
                )
            },
        )
        .collect();

    ok(result)
}

#[derive(Debug, Deserialize)]
pub struct CodeCellQuery {
    code_hash: String,
    hash_type: String,
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

    let hash_type = params.hash_type.as_str();

    let result: Option<(Vec<u8>, i16)> = if hash_type == "type" {
        sqlx::query_as(
            r#"
            SELECT tx_hash, output_index
            FROM cells
            WHERE type_script_hash = $1 AND status = 0
            ORDER BY created_at_block DESC
            LIMIT 1
            "#,
        )
        .bind(&code_hash_bytes)
        .fetch_optional(&state.read_pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
    } else {
        sqlx::query_as(
            r#"
            SELECT tx_hash, output_index
            FROM cells
            WHERE data_hash = $1
            ORDER BY status ASC, created_at_block DESC
            LIMIT 1
            "#,
        )
        .bind(&code_hash_bytes)
        .fetch_optional(&state.read_pool)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
    };

    ok(CodeCellResponse {
        tx_hash: result
            .as_ref()
            .map(|(tx, _)| format!("0x{}", hex::encode(tx))),
        output_index: result.map(|(_, idx)| idx as i32),
    })
}

fn row_to_response(r: ScriptRow) -> ScriptResponse {
    row_to_response_with_code_cell(r, None, None)
}

fn row_to_response_with_code_cell(
    r: ScriptRow,
    tx_hash: Option<Vec<u8>>,
    output_index: Option<i16>,
) -> ScriptResponse {
    ScriptResponse {
        code_hash: format!("0x{}", hex::encode(&r.code_hash)),
        name: r.name,
        description: r.description,
        script_kind: r.script_kind,
        rfc: r.rfc,
        website: r.website,
        source_url: r.source_url,
        decoder_type: r.decoder_type,
        network: r.network,
        hash_type: r.hash_type,
        data_hash: r.data_hash.map(|h| format!("0x{}", hex::encode(&h))),
        type_hash: r.type_hash.map(|h| format!("0x{}", hex::encode(&h))),
        tag: r.tag,
        deprecated: r.deprecated,
        is_system: r.is_system,
        code_cell_tx_hash: tx_hash.map(|h| format!("0x{}", hex::encode(&h))),
        code_cell_output_index: output_index.map(|i| i as i32),
    }
}

async fn list_scripts(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<ScriptResponse>> {
    let _limit = params.limit.clamp(1, 100);
    let _cursor_id = params
        .cursor
        .as_ref()
        .and_then(|c| c.parse::<i64>().ok())
        .unwrap_or(i64::MAX);

    let network = params.network.as_deref().unwrap_or(&state.ckb_network);
    let search_pattern = params
        .search
        .as_ref()
        .map(|s| format!("%{}%", s.to_lowercase()));

    // Use LEFT JOIN with script_usage_stats for script_kind instead of slow EXISTS subqueries
    let base_query = r#"
        SELECT DISTINCT ON (ks.name) ks.id, ks.code_hash, ks.name, ks.description, ks.rfc, ks.website, ks.source_url,
               ks.decoder_type, ks.network, ks.hash_type, ks.data_hash, ks.type_hash, ks.tag, ks.deprecated, ks.is_system,
               sus.script_kind
        FROM known_scripts ks
        LEFT JOIN script_usage_stats sus ON sus.code_hash = ks.code_hash
    "#;

    let (total, rows): (i64, Vec<ScriptRow>) = match (&params.decoder_type, &search_pattern) {
        (Some(decoder), Some(pattern)) => {
            let total: (i64,) = sqlx::query_as(
                "SELECT COUNT(DISTINCT name) FROM known_scripts WHERE network = $1 AND decoder_type = $2 AND LOWER(name) LIKE $3",
            )
            .bind(network)
            .bind(decoder)
            .bind(pattern)
            .fetch_one(&state.read_pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

            let query = format!(
                "{} WHERE ks.network = $1 AND ks.decoder_type = $2 AND LOWER(ks.name) LIKE $3 ORDER BY ks.name ASC, ks.is_system DESC, ks.deprecated ASC, ks.id DESC",
                base_query
            );
            let rows = sqlx::query_as::<_, ScriptRow>(&query)
                .bind(network)
                .bind(decoder)
                .bind(pattern)
                .fetch_all(&state.read_pool)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;

            (total.0, rows)
        }
        (Some(decoder), None) => {
            let total: (i64,) = sqlx::query_as(
                "SELECT COUNT(DISTINCT name) FROM known_scripts WHERE network = $1 AND decoder_type = $2",
            )
            .bind(network)
            .bind(decoder)
            .fetch_one(&state.read_pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

            let query = format!(
                "{} WHERE ks.network = $1 AND ks.decoder_type = $2 ORDER BY ks.name ASC, ks.is_system DESC, ks.deprecated ASC, ks.id DESC",
                base_query
            );
            let rows = sqlx::query_as::<_, ScriptRow>(&query)
                .bind(network)
                .bind(decoder)
                .fetch_all(&state.read_pool)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;

            (total.0, rows)
        }
        (None, Some(pattern)) => {
            let total: (i64,) = sqlx::query_as(
                "SELECT COUNT(DISTINCT name) FROM known_scripts WHERE network = $1 AND LOWER(name) LIKE $2",
            )
            .bind(network)
            .bind(pattern)
            .fetch_one(&state.read_pool)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

            let query = format!(
                "{} WHERE ks.network = $1 AND LOWER(ks.name) LIKE $2 ORDER BY ks.name ASC, ks.is_system DESC, ks.deprecated ASC, ks.id DESC",
                base_query
            );
            let rows = sqlx::query_as::<_, ScriptRow>(&query)
                .bind(network)
                .bind(pattern)
                .fetch_all(&state.read_pool)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;

            (total.0, rows)
        }
        (None, None) => {
            let total: (i64,) =
                sqlx::query_as("SELECT COUNT(DISTINCT name) FROM known_scripts WHERE network = $1")
                    .bind(network)
                    .fetch_one(&state.read_pool)
                    .await
                    .map_err(|e| ApiError::internal(e.to_string()))?;

            let query = format!(
                "{} WHERE ks.network = $1 ORDER BY ks.name ASC, ks.is_system DESC, ks.deprecated ASC, ks.id DESC",
                base_query
            );
            let rows = sqlx::query_as::<_, ScriptRow>(&query)
                .bind(network)
                .fetch_all(&state.read_pool)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?;

            (total.0, rows)
        }
    };

    let total_rows = rows.len() as i64;
    let scripts: Vec<ScriptResponse> = rows.into_iter().map(row_to_response).collect();

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
    // Use JOIN with script_usage_stats instead of slow EXISTS subqueries on cells table
    let rows = sqlx::query_as::<_, ScriptRow>(
        r#"
        SELECT ks.id, ks.code_hash, ks.name, ks.description, ks.rfc, ks.website, ks.source_url,
               ks.decoder_type, ks.network, ks.hash_type, ks.data_hash, ks.type_hash, ks.tag, ks.deprecated, ks.is_system,
               sus.script_kind
        FROM known_scripts ks
        LEFT JOIN script_usage_stats sus ON sus.code_hash = ks.code_hash
        WHERE ks.name = $1 AND ks.network = $2
        ORDER BY ks.deprecated ASC, ks.tag DESC
        "#,
    )
    .bind(&name)
    .bind(&state.ckb_network)
    .fetch_all(&state.read_pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    if rows.is_empty() {
        return Err(ApiError::not_found("Script not found"));
    }

    let mut code_cell_map: std::collections::HashMap<Vec<u8>, (Vec<u8>, i16)> =
        std::collections::HashMap::new();

    // hash_type = data/data1/data2: code_hash matches cells.data_hash
    let data_hashes: Vec<Vec<u8>> = rows
        .iter()
        .filter(|r| r.hash_type.as_deref() != Some("type"))
        .map(|r| r.code_hash.clone())
        .collect();

    if !data_hashes.is_empty() {
        let cells: Vec<(Vec<u8>, Vec<u8>, i16)> = sqlx::query_as(
            r#"
            SELECT DISTINCT ON (data_hash) data_hash, tx_hash, output_index
            FROM cells
            WHERE data_hash = ANY($1)
            ORDER BY data_hash, status ASC, created_at_block DESC
            "#,
        )
        .bind(&data_hashes)
        .fetch_all(&state.read_pool)
        .await
        .unwrap_or_default();

        for (hash, tx, idx) in cells {
            code_cell_map.insert(hash, (tx, idx));
        }
    }

    // hash_type = type: code_hash matches cells.type_script_hash
    let type_hashes: Vec<Vec<u8>> = rows
        .iter()
        .filter(|r| r.hash_type.as_deref() == Some("type"))
        .map(|r| r.code_hash.clone())
        .collect();

    if !type_hashes.is_empty() {
        let cells: Vec<(Vec<u8>, Vec<u8>, i16)> = sqlx::query_as(
            r#"
            SELECT DISTINCT ON (type_script_hash) type_script_hash, tx_hash, output_index
            FROM cells
            WHERE type_script_hash = ANY($1) AND status = 0
            ORDER BY type_script_hash, created_at_block DESC
            "#,
        )
        .bind(&type_hashes)
        .fetch_all(&state.read_pool)
        .await
        .unwrap_or_default();

        for (hash, tx, idx) in cells {
            code_cell_map.insert(hash, (tx, idx));
        }
    }

    let scripts: Vec<ScriptResponse> = rows
        .into_iter()
        .map(|row| {
            let (tx_hash, output_index) = code_cell_map
                .get(&row.code_hash)
                .map(|(tx, idx)| (Some(tx.clone()), Some(*idx)))
                .unwrap_or((None, None));

            row_to_response_with_code_cell(row, tx_hash, output_index)
        })
        .collect();

    ok(scripts)
}

async fn get_script_usage(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> ApiResult<ScriptUsageResponse> {
    let code_hashes: Vec<(Vec<u8>,)> = sqlx::query_as(
        "SELECT DISTINCT code_hash FROM known_scripts WHERE name = $1 AND network = $2",
    )
    .bind(&name)
    .bind(&state.ckb_network)
    .fetch_all(&state.read_pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    if code_hashes.is_empty() {
        return ok(ScriptUsageResponse {
            name,
            cells_count: 0,
            live_cells_count: 0,
            capacity_sum: "0".to_string(),
            live_capacity_sum: "0".to_string(),
            by_deployment: vec![],
        });
    }

    let hashes: Vec<Vec<u8>> = code_hashes.into_iter().map(|(h,)| h).collect();

    let per_deployment: Vec<(Vec<u8>, String, i64, i64, String, String)> = sqlx::query_as(
        r#"
        SELECT 
            code_hash,
            script_kind,
            cells_count,
            live_cells_count,
            capacity_sum::text,
            live_capacity_sum::text
        FROM script_usage_stats
        WHERE code_hash = ANY($1)
        "#,
    )
    .bind(&hashes)
    .fetch_all(&state.read_pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let mut total_cells: i64 = 0;
    let mut total_live: i64 = 0;
    let mut total_cap: u128 = 0;
    let mut total_live_cap: u128 = 0;

    let by_deployment: Vec<DeploymentUsage> = per_deployment
        .into_iter()
        .map(
            |(
                code_hash,
                script_kind,
                cells_count,
                live_cells_count,
                capacity_sum,
                live_capacity_sum,
            )| {
                total_cells += cells_count;
                total_live += live_cells_count;
                total_cap += capacity_sum.parse::<u128>().unwrap_or(0);
                total_live_cap += live_capacity_sum.parse::<u128>().unwrap_or(0);

                DeploymentUsage {
                    code_hash: format!("0x{}", hex::encode(&code_hash)),
                    script_kind: Some(script_kind),
                    cells_count,
                    live_cells_count,
                    capacity_sum,
                    live_capacity_sum,
                }
            },
        )
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
