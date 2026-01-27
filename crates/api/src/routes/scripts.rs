#![allow(clippy::type_complexity)]
#![allow(dead_code)]

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use clickhouse::Row;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::clickhouse::hex_hash;
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

#[derive(Debug, Clone, Row, Deserialize)]
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

    let code_hashes_hex: Vec<String> = request
        .code_hashes
        .iter()
        .map(|h| h.strip_prefix("0x").unwrap_or(h).to_string())
        .collect();

    let placeholders = code_hashes_hex
        .iter()
        .map(|hash| format!("unhex('{}')", hash))
        .collect::<Vec<_>>()
        .join(",");

    let query = format!(
        "SELECT {} as code_hash, name, script_kind, decoder_type, hash_type, live_cells_count, live_capacity_sum
         FROM known_scripts
         WHERE code_hash IN ({})
         AND network = '{}'",
        hex_hash("code_hash"),
        placeholders,
        state.ckb_network
    );

    #[derive(Row, Deserialize)]
    struct ScriptLookupRow {
        code_hash: String,
        name: String,
        script_kind: Option<String>,
        decoder_type: Option<String>,
        hash_type: Option<String>,
        live_cells_count: i64,
        live_capacity_sum: String,
    }

    let rows: Vec<ScriptLookupRow> = state
        .clickhouse
        .client()
        .query(&query)
        .fetch_all::<ScriptLookupRow>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    if rows.is_empty() {
        return ok(HashMap::new());
    }

    let mut code_cell_map: std::collections::HashMap<String, (String, i32)> =
        std::collections::HashMap::new();

    let data_hashes: Vec<String> = rows
        .iter()
        .filter(|r| r.hash_type.as_deref() != Some("type"))
        .map(|r| r.code_hash.clone())
        .collect();

    if !data_hashes.is_empty() {
        let data_placeholders = data_hashes
            .iter()
            .map(|hash| format!("unhex('{}')", hash))
            .collect::<Vec<_>>()
            .join(",");

        let data_query = format!(
            "SELECT {} as data_hash, {} as tx_hash, output_index
             FROM cells
             WHERE data_hash IN ({})
             ORDER BY created_at_block DESC
             LIMIT 1 BY data_hash",
            hex_hash("data_hash"),
            hex_hash("tx_hash"),
            data_placeholders
        );

        #[derive(Row, Deserialize)]
        struct CodeCellRow {
            data_hash: String,
            tx_hash: String,
            output_index: u16,
        }

        let cells: Vec<CodeCellRow> = state
            .clickhouse
            .client()
            .query(&data_query)
            .fetch_all::<CodeCellRow>()
            .await
            .unwrap_or_default();

        for cell in cells {
            code_cell_map.insert(cell.data_hash, (cell.tx_hash, cell.output_index as i32));
        }
    }

    let type_hashes: Vec<String> = rows
        .iter()
        .filter(|r| r.hash_type.as_deref() == Some("type"))
        .map(|r| r.code_hash.clone())
        .collect();

    if !type_hashes.is_empty() {
        let type_placeholders = type_hashes
            .iter()
            .map(|hash| format!("unhex('{}')", hash))
            .collect::<Vec<_>>()
            .join(",");

        let type_query = format!(
            "SELECT {} as type_script_hash, {} as tx_hash, output_index
             FROM cells
             WHERE type_script_hash IN ({})
             AND status = 0
             ORDER BY created_at_block DESC
             LIMIT 1 BY type_script_hash",
            hex_hash("type_script_hash"),
            hex_hash("tx_hash"),
            type_placeholders
        );

        #[derive(Row, Deserialize)]
        struct TypeCellRow {
            type_script_hash: String,
            tx_hash: String,
            output_index: u16,
        }

        let cells: Vec<TypeCellRow> = state
            .clickhouse
            .client()
            .query(&type_query)
            .fetch_all::<TypeCellRow>()
            .await
            .unwrap_or_default();

        for cell in cells {
            code_cell_map.insert(
                cell.type_script_hash,
                (cell.tx_hash, cell.output_index as i32),
            );
        }
    }

    let result: HashMap<String, ScriptLookupInfo> = rows
        .into_iter()
        .map(|row| {
            let (tx_hash, output_index) = code_cell_map
                .get(&row.code_hash)
                .map(|(tx, idx)| (Some(tx.clone()), Some(*idx)))
                .unwrap_or((None, None));
            (
                row.code_hash.clone(),
                ScriptLookupInfo {
                    code_hash: row.code_hash,
                    name: row.name,
                    script_kind: row.script_kind,
                    decoder_type: row.decoder_type,
                    hash_type: row.hash_type,
                    code_cell_tx_hash: tx_hash,
                    code_cell_output_index: output_index,
                    live_cells_count: row.live_cells_count,
                    live_capacity_sum: row.live_capacity_sum,
                },
            )
        })
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
    let code_hash_hex = params
        .code_hash
        .strip_prefix("0x")
        .unwrap_or(&params.code_hash);

    let hash_type = params.hash_type.as_str();

    let query = if hash_type == "type" {
        format!(
            "SELECT {} as tx_hash, output_index
            FROM cells
            WHERE type_script_hash = unhex('{}')
            AND status = 0
            ORDER BY created_at_block DESC
            LIMIT 1",
            hex_hash("tx_hash"),
            code_hash_hex
        )
    } else {
        format!(
            "SELECT {} as tx_hash, output_index
            FROM cells
            WHERE data_hash = unhex('{}')
            ORDER BY created_at_block DESC
            LIMIT 1",
            hex_hash("tx_hash"),
            code_hash_hex
        )
    };

    #[derive(Row, Deserialize)]
    struct CodeCellRow {
        tx_hash: String,
        output_index: u16,
    }

    let result: Option<CodeCellRow> = state
        .clickhouse
        .client()
        .query(&query)
        .fetch_optional::<CodeCellRow>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    ok(CodeCellResponse {
        tx_hash: result.as_ref().map(|r| r.tx_hash.clone()),
        output_index: result.map(|r| r.output_index as i32),
    })
}

fn row_to_response(r: ScriptRow) -> ScriptResponse {
    row_to_response_with_code_cell(r, None, None)
}

fn row_to_response_with_code_cell(
    r: ScriptRow,
    tx_hash: Option<String>,
    output_index: Option<i32>,
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
        code_cell_tx_hash: tx_hash,
        code_cell_output_index: output_index,
    }
}

async fn list_scripts(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<ScriptResponse>> {
    let limit = params.limit.clamp(1, 100);
    let network = params.network.as_deref().unwrap_or(&state.ckb_network);

    let mut where_clauses = vec![format!("network = '{}'", network)];

    if let Some(decoder) = &params.decoder_type {
        where_clauses.push(format!("decoder_type = '{}'", decoder));
    }

    if let Some(search) = &params.search {
        let search_lower = search.to_lowercase();
        where_clauses.push(format!("lower(name) LIKE '%{}%'", search_lower));
    }

    let where_clause = where_clauses.join(" AND ");

    let count_query = format!(
        "SELECT COUNT(DISTINCT name) as cnt FROM known_scripts WHERE {}",
        where_clause
    );

    #[derive(Row, Deserialize)]
    struct CountRow {
        cnt: i64,
    }

    let count_result: Vec<CountRow> = state
        .clickhouse
        .client()
        .query(&count_query)
        .fetch_all::<CountRow>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let total = count_result.first().map(|r| r.cnt).unwrap_or(0);

    let query = format!(
        "SELECT id, {} as code_hash, name, description, rfc, website, source_url, decoder_type, network, hash_type, 
                {} as data_hash, {} as type_hash, tag, deprecated, is_system, script_kind
         FROM known_scripts
         WHERE {}
         ORDER BY name ASC, is_system DESC, deprecated ASC, id DESC
         LIMIT {}",
        hex_hash("code_hash"),
        hex_hash("data_hash"),
        hex_hash("type_hash"),
        where_clause,
        limit
    );

    let rows: Vec<ScriptRow> = state
        .clickhouse
        .client()
        .query(&query)
        .fetch_all::<ScriptRow>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

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
    let query = format!(
        "SELECT id, {} as code_hash, name, description, rfc, website, source_url, decoder_type, network, hash_type,
                {} as data_hash, {} as type_hash, tag, deprecated, is_system, script_kind
         FROM known_scripts
         WHERE name = '{}' AND network = '{}'
         ORDER BY deprecated ASC, tag DESC",
        hex_hash("code_hash"),
        hex_hash("data_hash"),
        hex_hash("type_hash"),
        name,
        state.ckb_network
    );

    let rows: Vec<ScriptRow> = state
        .clickhouse
        .client()
        .query(&query)
        .fetch_all::<ScriptRow>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    if rows.is_empty() {
        return Err(ApiError::not_found("Script not found"));
    }

    let mut code_cell_map: std::collections::HashMap<String, (String, i32)> =
        std::collections::HashMap::new();

    // hash_type = data/data1/data2: code_hash matches cells.data_hash
    let data_hashes: Vec<String> = rows
        .iter()
        .filter(|r| r.hash_type.as_deref() != Some("type"))
        .map(|r| format!("0x{}", hex::encode(&r.code_hash)))
        .collect();

    if !data_hashes.is_empty() {
        let data_placeholders = data_hashes
            .iter()
            .map(|hash| {
                let hash_clean = hash.strip_prefix("0x").unwrap_or(hash);
                format!("unhex('{}')", hash_clean)
            })
            .collect::<Vec<_>>()
            .join(",");

        let data_query = format!(
            "SELECT {} as data_hash, {} as tx_hash, output_index
             FROM cells
             WHERE data_hash IN ({})
             ORDER BY created_at_block DESC
             LIMIT 1 BY data_hash",
            hex_hash("data_hash"),
            hex_hash("tx_hash"),
            data_placeholders
        );

        #[derive(Row, Deserialize)]
        struct DataCellRow {
            data_hash: String,
            tx_hash: String,
            output_index: u16,
        }

        let cells: Vec<DataCellRow> = state
            .clickhouse
            .client()
            .query(&data_query)
            .fetch_all::<DataCellRow>()
            .await
            .unwrap_or_default();

        for cell in cells {
            code_cell_map.insert(cell.data_hash, (cell.tx_hash, cell.output_index as i32));
        }
    }

    // hash_type = type: code_hash matches cells.type_script_hash
    let type_hashes: Vec<String> = rows
        .iter()
        .filter(|r| r.hash_type.as_deref() == Some("type"))
        .map(|r| format!("0x{}", hex::encode(&r.code_hash)))
        .collect();

    if !type_hashes.is_empty() {
        let type_placeholders = type_hashes
            .iter()
            .map(|hash| {
                let hash_clean = hash.strip_prefix("0x").unwrap_or(hash);
                format!("unhex('{}')", hash_clean)
            })
            .collect::<Vec<_>>()
            .join(",");

        let type_query = format!(
            "SELECT {} as type_script_hash, {} as tx_hash, output_index
             FROM cells
             WHERE type_script_hash IN ({})
             AND status = 0
             ORDER BY created_at_block DESC
             LIMIT 1 BY type_script_hash",
            hex_hash("type_script_hash"),
            hex_hash("tx_hash"),
            type_placeholders
        );

        #[derive(Row, Deserialize)]
        struct TypeCellRow {
            type_script_hash: String,
            tx_hash: String,
            output_index: u16,
        }

        let cells: Vec<TypeCellRow> = state
            .clickhouse
            .client()
            .query(&type_query)
            .fetch_all::<TypeCellRow>()
            .await
            .unwrap_or_default();

        for cell in cells {
            code_cell_map.insert(
                cell.type_script_hash,
                (cell.tx_hash, cell.output_index as i32),
            );
        }
    }

    let scripts: Vec<ScriptResponse> = rows
        .into_iter()
        .map(|row| {
            let code_hash_hex = format!("0x{}", hex::encode(&row.code_hash));
            let (tx_hash, output_index) = code_cell_map
                .get(&code_hash_hex)
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
    let code_hashes_query = format!(
        "SELECT DISTINCT {} as code_hash FROM known_scripts WHERE name = '{}' AND network = '{}'",
        hex_hash("code_hash"),
        name,
        state.ckb_network
    );

    #[derive(Row, Deserialize)]
    struct CodeHashRow {
        code_hash: String,
    }

    let code_hashes: Vec<CodeHashRow> = state
        .clickhouse
        .client()
        .query(&code_hashes_query)
        .fetch_all::<CodeHashRow>()
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

    let hashes_hex: Vec<String> = code_hashes.iter().map(|r| r.code_hash.clone()).collect();
    let placeholders = hashes_hex
        .iter()
        .map(|hash| {
            let hash_clean = hash.strip_prefix("0x").unwrap_or(hash);
            format!("unhex('{}')", hash_clean)
        })
        .collect::<Vec<_>>()
        .join(",");

    let per_deployment_query = format!(
        "SELECT {} as code_hash, script_kind, cells_count, live_cells_count, toString(capacity_sum) as capacity_sum, toString(live_capacity_sum) as live_capacity_sum
         FROM script_usage_stats
         WHERE code_hash IN ({})",
        hex_hash("code_hash"),
        placeholders
    );

    #[derive(Row, Deserialize)]
    struct DeploymentRow {
        code_hash: String,
        script_kind: String,
        cells_count: i64,
        live_cells_count: i64,
        capacity_sum: String,
        live_capacity_sum: String,
    }

    let per_deployment: Vec<DeploymentRow> = state
        .clickhouse
        .client()
        .query(&per_deployment_query)
        .fetch_all::<DeploymentRow>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let mut total_cells: i64 = 0;
    let mut total_live: i64 = 0;
    let mut total_cap: u128 = 0;
    let mut total_live_cap: u128 = 0;

    let by_deployment: Vec<DeploymentUsage> = per_deployment
        .into_iter()
        .map(|row| {
            total_cells += row.cells_count;
            total_live += row.live_cells_count;
            total_cap += row.capacity_sum.parse::<u128>().unwrap_or(0);
            total_live_cap += row.live_capacity_sum.parse::<u128>().unwrap_or(0);

            DeploymentUsage {
                code_hash: row.code_hash,
                script_kind: Some(row.script_kind),
                cells_count: row.cells_count,
                live_cells_count: row.live_cells_count,
                capacity_sum: row.capacity_sum,
                live_capacity_sum: row.live_capacity_sum,
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
