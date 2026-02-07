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
#[serde(rename_all = "camelCase")]
struct ListScriptsParams {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownScript {
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
    pub code_cell_output_index: Option<i16>,
}

#[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
struct KnownScriptRow {
    code_hash: [u8; 32],
    name: String,
    description: String,
    script_kind: String,
    rfc: String,
    website: String,
    source_url: String,
    decoder_type: String,
    network: String,
    hash_type: String,
    data_hash: [u8; 32],
    type_hash: [u8; 32],
    tag: String,
    deprecated: u8,
    is_system: u8,
    code_cell_tx_hash: [u8; 32],
    code_cell_output_index: i16,
}

impl From<KnownScriptRow> for KnownScript {
    fn from(row: KnownScriptRow) -> Self {
        let empty_hash = [0u8; 32];
        Self {
            code_hash: format!("0x{}", hex::encode(row.code_hash)),
            name: row.name,
            description: if row.description.is_empty() {
                None
            } else {
                Some(row.description)
            },
            script_kind: if row.script_kind.is_empty() {
                None
            } else {
                Some(row.script_kind)
            },
            rfc: if row.rfc.is_empty() {
                None
            } else {
                Some(row.rfc)
            },
            website: if row.website.is_empty() {
                None
            } else {
                Some(row.website)
            },
            source_url: if row.source_url.is_empty() {
                None
            } else {
                Some(row.source_url)
            },
            decoder_type: if row.decoder_type.is_empty() {
                None
            } else {
                Some(row.decoder_type)
            },
            network: row.network,
            hash_type: if row.hash_type.is_empty() {
                None
            } else {
                Some(row.hash_type)
            },
            data_hash: if row.data_hash == empty_hash {
                None
            } else {
                Some(format!("0x{}", hex::encode(row.data_hash)))
            },
            type_hash: if row.type_hash == empty_hash {
                None
            } else {
                Some(format!("0x{}", hex::encode(row.type_hash)))
            },
            tag: if row.tag.is_empty() {
                None
            } else {
                Some(row.tag)
            },
            deprecated: row.deprecated != 0,
            is_system: row.is_system != 0,
            code_cell_tx_hash: if row.code_cell_tx_hash == empty_hash {
                None
            } else {
                Some(format!("0x{}", hex::encode(row.code_cell_tx_hash)))
            },
            code_cell_output_index: if row.code_cell_output_index < 0 {
                None
            } else {
                Some(row.code_cell_output_index)
            },
        }
    }
}

async fn list_scripts(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListScriptsParams>,
) -> ApiResult<CursorPaginatedResponse<KnownScript>> {
    let network = params.network.as_deref().unwrap_or("mainnet");

    let mut conditions = vec![format!("network = '{}'", network)];

    if let Some(ref decoder_type) = params.decoder_type {
        conditions.push(format!("decoder_type = '{}'", decoder_type));
    }

    if let Some(ref search) = params.search {
        let search_escaped = search.replace('\'', "''");
        conditions.push(format!(
            "(name ILIKE '%{}%' OR hex(code_hash) ILIKE '%{}%')",
            search_escaped, search_escaped
        ));
    }

    if let Some(ref cursor) = params.cursor {
        let cursor_escaped = cursor.replace('\'', "''");
        conditions.push(format!("name > '{}'", cursor_escaped));
    }

    let where_clause = conditions.join(" AND ");

    let query = format!(
        "SELECT 
            code_hash, name, description, script_kind, rfc, website, source_url,
            decoder_type, network, hash_type, data_hash, type_hash, tag,
            deprecated, is_system, code_cell_tx_hash, code_cell_output_index
         FROM known_scripts FINAL
         WHERE {}
         ORDER BY name ASC
         LIMIT {}",
        where_clause,
        params.limit + 1
    );

    let mut rows: Vec<KnownScriptRow> = state
        .pool
        .query_all(&query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to query scripts: {}", e)))?;

    let has_more = rows.len() as i64 > params.limit;
    if has_more {
        rows.pop();
    }

    let count_query = format!(
        "SELECT count() as total FROM known_scripts FINAL WHERE network = '{}'",
        network
    );

    #[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
    struct CountRow {
        total: u64,
    }

    let count_row: Option<CountRow> = state
        .pool
        .query_one(&count_query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to count scripts: {}", e)))?;

    let total = count_row.map(|r| r.total as i64).unwrap_or(0);
    let next_cursor = if has_more {
        rows.last().map(|r| r.name.clone())
    } else {
        None
    };

    let scripts: Vec<KnownScript> = rows.into_iter().map(|r| r.into()).collect();
    ok(CursorPaginatedResponse::new(
        scripts,
        total,
        params.limit,
        next_cursor,
    ))
}

async fn get_script(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> ApiResult<Vec<KnownScript>> {
    let name_escaped = name.replace('\'', "''");

    let query = format!(
        "SELECT 
            code_hash, name, description, script_kind, rfc, website, source_url,
            decoder_type, network, hash_type, data_hash, type_hash, tag,
            deprecated, is_system, code_cell_tx_hash, code_cell_output_index
         FROM known_scripts FINAL
         WHERE name = '{}'
         ORDER BY network ASC, tag ASC",
        name_escaped
    );

    let rows: Vec<KnownScriptRow> = state
        .pool
        .query_all(&query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to query script: {}", e)))?;

    if rows.is_empty() {
        return Err(ApiError::not_found("Script not found"));
    }

    let scripts: Vec<KnownScript> = rows.into_iter().map(|r| r.into()).collect();
    ok(scripts)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentUsage {
    pub code_hash: String,
    pub script_kind: Option<String>,
    pub cells_count: u64,
    pub live_cells_count: u64,
    pub capacity_sum: String,
    pub live_capacity_sum: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptUsage {
    pub name: String,
    pub cells_count: u64,
    pub live_cells_count: u64,
    pub capacity_sum: String,
    pub live_capacity_sum: String,
    pub by_deployment: Vec<DeploymentUsage>,
}

async fn get_script_usage(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> ApiResult<ScriptUsage> {
    let name_escaped = name.replace('\'', "''");

    let query = format!(
        "SELECT hex(code_hash) as code_hash, script_kind 
         FROM known_scripts FINAL 
         WHERE name = '{}' AND network = 'mainnet'",
        name_escaped
    );

    #[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
    struct CodeHashRow {
        code_hash: String,
        script_kind: String,
    }

    let code_hashes: Vec<CodeHashRow> = state
        .pool
        .query_all(&query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to query script: {}", e)))?;

    if code_hashes.is_empty() {
        return Err(ApiError::not_found("Script not found"));
    }

    let mut deployments = Vec::new();
    let mut total_cells = 0u64;
    let mut total_live_cells = 0u64;
    let mut total_capacity = 0u128;
    let mut total_live_capacity = 0u128;

    for ch in &code_hashes {
        #[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
        struct UsageRow {
            cells_count: u64,
            live_cells_count: u64,
            capacity_sum: String,
            live_capacity_sum: String,
        }

        let usage_query = format!(
            "SELECT 
                count() as cells_count,
                countIf(status = 0) as live_cells_count,
                toString(sum(capacity)) as capacity_sum,
                toString(sumIf(capacity, status = 0)) as live_capacity_sum
             FROM cells
             WHERE hex(lock_code_hash) = '{}' OR hex(type_code_hash) = '{}'",
            ch.code_hash, ch.code_hash
        );

        let usage: Option<UsageRow> = state.pool.query_one(&usage_query).await.unwrap_or(None);

        if let Some(u) = usage {
            let cells_count = u.cells_count;
            let live_cells_count = u.live_cells_count;
            let capacity: u128 = u.capacity_sum.parse().unwrap_or(0);
            let live_capacity: u128 = u.live_capacity_sum.parse().unwrap_or(0);

            total_cells += cells_count;
            total_live_cells += live_cells_count;
            total_capacity += capacity;
            total_live_capacity += live_capacity;

            deployments.push(DeploymentUsage {
                code_hash: format!("0x{}", ch.code_hash.to_lowercase()),
                script_kind: if ch.script_kind.is_empty() {
                    None
                } else {
                    Some(ch.script_kind.clone())
                },
                cells_count,
                live_cells_count,
                capacity_sum: capacity.to_string(),
                live_capacity_sum: live_capacity.to_string(),
            });
        }
    }

    ok(ScriptUsage {
        name,
        cells_count: total_cells,
        live_cells_count: total_live_cells,
        capacity_sum: total_capacity.to_string(),
        live_capacity_sum: total_live_capacity.to_string(),
        by_deployment: deployments,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LookupScriptsRequest {
    code_hashes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptLookupInfo {
    pub code_hash: String,
    pub name: String,
    pub script_kind: Option<String>,
    pub decoder_type: Option<String>,
    pub hash_type: Option<String>,
    pub code_cell_tx_hash: Option<String>,
    pub code_cell_output_index: Option<i16>,
    pub live_cells_count: u64,
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
        return Err(ApiError::bad_request("Maximum 100 code hashes allowed"));
    }

    let hex_list: Vec<String> = request
        .code_hashes
        .iter()
        .map(|h| format!("'{}'", h.trim_start_matches("0x").to_uppercase()))
        .collect();

    let query = format!(
        "SELECT 
            code_hash, name, script_kind, decoder_type, hash_type,
            code_cell_tx_hash, code_cell_output_index
         FROM known_scripts FINAL
         WHERE hex(code_hash) IN ({}) AND network = 'mainnet'",
        hex_list.join(", ")
    );

    #[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
    struct LookupRow {
        code_hash: [u8; 32],
        name: String,
        script_kind: String,
        decoder_type: String,
        hash_type: String,
        code_cell_tx_hash: [u8; 32],
        code_cell_output_index: i16,
    }

    let rows: Vec<LookupRow> = state
        .pool
        .query_all(&query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to lookup scripts: {}", e)))?;

    let empty_hash = [0u8; 32];
    let mut result = HashMap::new();

    for row in rows {
        let code_hash = format!("0x{}", hex::encode(row.code_hash));
        result.insert(
            code_hash.clone(),
            ScriptLookupInfo {
                code_hash,
                name: row.name,
                script_kind: if row.script_kind.is_empty() {
                    None
                } else {
                    Some(row.script_kind)
                },
                decoder_type: if row.decoder_type.is_empty() {
                    None
                } else {
                    Some(row.decoder_type)
                },
                hash_type: if row.hash_type.is_empty() {
                    None
                } else {
                    Some(row.hash_type)
                },
                code_cell_tx_hash: if row.code_cell_tx_hash == empty_hash {
                    None
                } else {
                    Some(format!("0x{}", hex::encode(row.code_cell_tx_hash)))
                },
                code_cell_output_index: if row.code_cell_output_index < 0 {
                    None
                } else {
                    Some(row.code_cell_output_index)
                },
                live_cells_count: 0,
                live_capacity_sum: "0".to_string(),
            },
        );
    }

    ok(result)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct GetCodeCellParams {
    code_hash: String,
    hash_type: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeCellResponse {
    pub tx_hash: Option<String>,
    pub output_index: Option<i16>,
}

async fn get_code_cell(
    State(state): State<Arc<AppState>>,
    Query(params): Query<GetCodeCellParams>,
) -> ApiResult<CodeCellResponse> {
    let code_hash_hex = params.code_hash.trim_start_matches("0x").to_uppercase();
    let hash_type_escaped = params.hash_type.replace('\'', "''");

    let query = format!(
        "SELECT code_cell_tx_hash, code_cell_output_index
         FROM known_scripts FINAL
         WHERE hex(code_hash) = '{}' AND hash_type = '{}' AND network = 'mainnet'
         LIMIT 1",
        code_hash_hex, hash_type_escaped
    );

    #[derive(Debug, Clone, clickhouse::Row, serde::Deserialize)]
    struct CodeCellRow {
        code_cell_tx_hash: [u8; 32],
        code_cell_output_index: i16,
    }

    let row: Option<CodeCellRow> = state
        .pool
        .query_one(&query)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to query code cell: {}", e)))?;

    let empty_hash = [0u8; 32];

    match row {
        Some(r) => ok(CodeCellResponse {
            tx_hash: if r.code_cell_tx_hash == empty_hash {
                None
            } else {
                Some(format!("0x{}", hex::encode(r.code_cell_tx_hash)))
            },
            output_index: if r.code_cell_output_index < 0 {
                None
            } else {
                Some(r.code_cell_output_index)
            },
        }),
        None => ok(CodeCellResponse {
            tx_hash: None,
            output_index: None,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_script_serialization() {
        let script = KnownScript {
            code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8"
                .to_string(),
            name: "Default Lock".to_string(),
            description: Some("SECP256K1/blake160 lock script".to_string()),
            script_kind: Some("lock".to_string()),
            rfc: Some("https://...".to_string()),
            website: None,
            source_url: Some("https://github.com/...".to_string()),
            decoder_type: None,
            network: "mainnet".to_string(),
            hash_type: Some("type".to_string()),
            data_hash: None,
            type_hash: None,
            tag: None,
            deprecated: false,
            is_system: true,
            code_cell_tx_hash: None,
            code_cell_output_index: None,
        };
        let json = serde_json::to_string(&script).unwrap();
        assert!(json.contains("\"codeHash\""));
        assert!(json.contains("\"scriptKind\":\"lock\""));
        assert!(json.contains("\"isSystem\":true"));
    }

    #[test]
    fn test_script_usage_serialization() {
        let usage = ScriptUsage {
            name: "Default Lock".to_string(),
            cells_count: 1000,
            live_cells_count: 500,
            capacity_sum: "100000000000".to_string(),
            live_capacity_sum: "50000000000".to_string(),
            by_deployment: vec![DeploymentUsage {
                code_hash: "0x123...".to_string(),
                script_kind: Some("lock".to_string()),
                cells_count: 1000,
                live_cells_count: 500,
                capacity_sum: "100000000000".to_string(),
                live_capacity_sum: "50000000000".to_string(),
            }],
        };
        let json = serde_json::to_string(&usage).unwrap();
        assert!(json.contains("\"cellsCount\":1000"));
        assert!(json.contains("\"byDeployment\""));
    }

    #[test]
    fn test_known_script_deserialization() {
        let json = r#"{
            "codeHash": "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            "name": "Default Lock",
            "description": null,
            "scriptKind": "lock",
            "rfc": null,
            "website": null,
            "sourceUrl": null,
            "decoderType": null,
            "network": "mainnet",
            "hashType": "type",
            "dataHash": null,
            "typeHash": null,
            "tag": null,
            "deprecated": false,
            "isSystem": true,
            "codeCellTxHash": null,
            "codeCellOutputIndex": null
        }"#;
        let script: KnownScript = serde_json::from_str(json).unwrap();
        assert_eq!(script.name, "Default Lock");
        assert_eq!(script.script_kind, Some("lock".to_string()));
        assert!(script.is_system);
        assert!(!script.deprecated);
    }

    #[test]
    fn test_script_lookup_info_serialization() {
        let info = ScriptLookupInfo {
            code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8"
                .to_string(),
            name: "Default Lock".to_string(),
            script_kind: Some("lock".to_string()),
            decoder_type: None,
            hash_type: Some("type".to_string()),
            code_cell_tx_hash: None,
            code_cell_output_index: None,
            live_cells_count: 1000000,
            live_capacity_sum: "500000000000000".to_string(),
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"codeHash\":"));
        assert!(json.contains("\"liveCellsCount\":1000000"));
        assert!(json.contains("\"liveCapacitySum\":\"500000000000000\""));
    }

    #[test]
    fn test_code_cell_response_serialization() {
        let response = CodeCellResponse {
            tx_hash: Some(
                "0x71a7ba8fc96349fea0ed3a5c47992e3b4084b031a42264a018e0072e8172e46c".to_string(),
            ),
            output_index: Some(0),
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains(
            "\"txHash\":\"0x71a7ba8fc96349fea0ed3a5c47992e3b4084b031a42264a018e0072e8172e46c\""
        ));
        assert!(json.contains("\"outputIndex\":0"));
    }

    #[test]
    fn test_code_cell_response_null_values() {
        let response = CodeCellResponse {
            tx_hash: None,
            output_index: None,
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"txHash\":null"));
        assert!(json.contains("\"outputIndex\":null"));
    }

    #[test]
    fn test_deployment_usage_serialization() {
        let usage = DeploymentUsage {
            code_hash: "0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e"
                .to_string(),
            script_kind: Some("type".to_string()),
            cells_count: 50000,
            live_cells_count: 25000,
            capacity_sum: "10200000000000".to_string(),
            live_capacity_sum: "5100000000000".to_string(),
        };
        let json = serde_json::to_string(&usage).unwrap();
        assert!(json.contains("\"scriptKind\":\"type\""));
        assert!(json.contains("\"cellsCount\":50000"));
    }

    #[test]
    fn test_known_script_row_conversion_with_empty_values() {
        let row = KnownScriptRow {
            code_hash: [0u8; 32],
            name: "Test Script".to_string(),
            description: "".to_string(),
            script_kind: "".to_string(),
            rfc: "".to_string(),
            website: "".to_string(),
            source_url: "".to_string(),
            decoder_type: "".to_string(),
            network: "mainnet".to_string(),
            hash_type: "".to_string(),
            data_hash: [0u8; 32],
            type_hash: [0u8; 32],
            tag: "".to_string(),
            deprecated: 0,
            is_system: 0,
            code_cell_tx_hash: [0u8; 32],
            code_cell_output_index: -1,
        };
        let script: KnownScript = row.into();
        assert_eq!(script.name, "Test Script");
        assert!(script.description.is_none());
        assert!(script.script_kind.is_none());
        assert!(script.rfc.is_none());
        assert!(script.data_hash.is_none());
        assert!(script.code_cell_tx_hash.is_none());
        assert!(script.code_cell_output_index.is_none());
    }

    #[test]
    fn test_known_script_row_conversion_with_values() {
        let mut code_hash = [0u8; 32];
        code_hash[0] = 0x9b;
        code_hash[31] = 0xe8;
        let row = KnownScriptRow {
            code_hash,
            name: "Default Lock".to_string(),
            description: "A lock script".to_string(),
            script_kind: "lock".to_string(),
            rfc: "https://rfc.example.com".to_string(),
            website: "https://example.com".to_string(),
            source_url: "https://github.com/example".to_string(),
            decoder_type: "".to_string(),
            network: "mainnet".to_string(),
            hash_type: "type".to_string(),
            data_hash: [1u8; 32],
            type_hash: [2u8; 32],
            tag: "v1".to_string(),
            deprecated: 1,
            is_system: 1,
            code_cell_tx_hash: [3u8; 32],
            code_cell_output_index: 0,
        };
        let script: KnownScript = row.into();
        assert_eq!(script.name, "Default Lock");
        assert_eq!(script.description, Some("A lock script".to_string()));
        assert_eq!(script.script_kind, Some("lock".to_string()));
        assert!(script.deprecated);
        assert!(script.is_system);
        assert!(script.data_hash.is_some());
        assert!(script.code_cell_tx_hash.is_some());
        assert_eq!(script.code_cell_output_index, Some(0));
    }
}
