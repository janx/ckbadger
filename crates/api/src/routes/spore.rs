use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::response::{ok, ApiError, ApiResult, CursorPaginatedResponse};
use crate::utils::script_to_address;
use crate::AppState;

// Row structs for ClickHouse queries
#[derive(clickhouse::Row, serde::Deserialize)]
struct CountRow {
    count: i64,
}

#[derive(clickhouse::Row, serde::Deserialize)]
struct ClusterRow {
    cluster_id: Vec<u8>,
    name: Option<String>,
    description: Option<String>,
    owner_lock_hash: Vec<u8>,
    lock_code_hash: Option<Vec<u8>>,
    lock_hash_type: Option<i16>,
    lock_args: Option<Vec<u8>>,
    spores_count: i32,
    created_at_block: i64,
}

#[derive(clickhouse::Row, serde::Deserialize)]
struct SporeRow {
    spore_id: Vec<u8>,
    tx_hash: Vec<u8>,
    output_index: i16,
    cluster_id: Option<Vec<u8>>,
    content_type: String,
    content_size: i32,
    owner_lock_hash: Vec<u8>,
    lock_code_hash: Option<Vec<u8>>,
    lock_hash_type: Option<i16>,
    lock_args: Option<Vec<u8>>,
    is_live: bool,
    created_at_block: i64,
}

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

async fn list_clusters(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<ClusterResponse>> {
    let limit = params.limit.clamp(1, 100);
    let cursor_block = params.cursor.unwrap_or(i64::MAX);

    let total_row = state
        .clickhouse
        .client()
        .query("SELECT COUNT(*) as count FROM spore_clusters")
        .fetch_one::<CountRow>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let total = (total_row.count,);

    let query = format!(
        r#"
        SELECT sc.cluster_id, sc.name, sc.description, sc.owner_lock_hash,
               c.lock_code_hash, c.lock_hash_type, c.lock_args,
               sc.spores_count, sc.created_at_block
        FROM spore_clusters sc
        LEFT JOIN cells c ON sc.owner_lock_hash = c.lock_script_hash AND c.status = 0
        WHERE sc.created_at_block < {}
        ORDER BY sc.created_at_block DESC
        LIMIT {}
        "#,
        cursor_block,
        limit + 1
    );

    let rows = state
        .clickhouse
        .client()
        .query(&query)
        .fetch_all::<ClusterRow>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let has_more = rows.len() as i64 > limit;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

    let next_cursor = if has_more {
        rows.last().map(|row| row.created_at_block.to_string())
    } else {
        None
    };

    let network = &state.ckb_network;
    let clusters: Vec<ClusterResponse> = rows
        .into_iter()
        .map(|row| {
            let owner_address = row.lock_code_hash.as_ref().and_then(|code_hash| {
                let hash_type = row.lock_hash_type.unwrap_or(0);
                let args = row.lock_args.as_deref().unwrap_or(&[]);
                script_to_address(code_hash, hash_type, args, network).ok()
            });
            ClusterResponse {
                cluster_id: format!("0x{}", hex::encode(&row.cluster_id)),
                name: row.name,
                description: row.description,
                owner_lock_hash: format!("0x{}", hex::encode(&row.owner_lock_hash)),
                owner_address,
                spores_count: row.spores_count,
                created_at_block: row.created_at_block,
            }
        })
        .collect();

    ok(CursorPaginatedResponse::new(
        clusters,
        total.0,
        limit,
        next_cursor,
    ))
}

async fn get_spores_by_cluster(
    State(state): State<Arc<AppState>>,
    Path(cluster_id): Path<String>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<SporeResponse>> {
    let id = hex::decode(cluster_id.strip_prefix("0x").unwrap_or(&cluster_id))
        .map_err(|_| ApiError::bad_request("Invalid cluster ID"))?;

    let limit = params.limit.clamp(1, 100);
    let cursor_block = params.cursor.unwrap_or(i64::MAX);

    let id_hex = hex::encode(&id);
    let total_row = state
        .clickhouse
        .client()
        .query(&format!(
            "SELECT COUNT(*) as count FROM spore_cells WHERE cluster_id = unhex('{}') AND is_live = TRUE",
            id_hex
        ))
        .fetch_one::<CountRow>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let total = (total_row.count,);

    let query = format!(
        r#"
        SELECT s.spore_id, s.tx_hash, s.output_index, s.cluster_id, s.content_type, s.content_size, s.owner_lock_hash,
               c.lock_code_hash, c.lock_hash_type, c.lock_args,
               s.is_live, s.created_at_block
        FROM spore_cells s
        LEFT JOIN cells c ON s.tx_hash = c.tx_hash AND s.output_index = c.output_index
        WHERE s.cluster_id = unhex('{}') AND s.is_live = TRUE AND s.created_at_block < {}
        ORDER BY s.created_at_block DESC
        LIMIT {}
        "#,
        id_hex,
        cursor_block,
        limit + 1
    );

    let rows = state
        .clickhouse
        .client()
        .query(&query)
        .fetch_all::<SporeRow>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let has_more = rows.len() as i64 > limit;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

    let next_cursor = if has_more {
        rows.last().map(|row| row.created_at_block.to_string())
    } else {
        None
    };

    let network = &state.ckb_network;
    let spores: Vec<SporeResponse> = rows
        .into_iter()
        .map(|row| {
            let owner_address = row.lock_code_hash.as_ref().and_then(|code_hash| {
                let hash_type = row.lock_hash_type.unwrap_or(0);
                let args = row.lock_args.as_deref().unwrap_or(&[]);
                script_to_address(code_hash, hash_type, args, network).ok()
            });
            SporeResponse {
                spore_id: format!("0x{}", hex::encode(&row.spore_id)),
                tx_hash: format!("0x{}", hex::encode(&row.tx_hash)),
                output_index: row.output_index as i32,
                cluster_id: row
                    .cluster_id
                    .as_ref()
                    .map(|c| format!("0x{}", hex::encode(c))),
                content_type: row.content_type,
                content_size: row.content_size,
                owner_lock_hash: format!("0x{}", hex::encode(&row.owner_lock_hash)),
                owner_address,
                is_live: row.is_live,
                created_at_block: row.created_at_block,
            }
        })
        .collect();

    ok(CursorPaginatedResponse::new(
        spores,
        total.0,
        limit,
        next_cursor,
    ))
}

async fn get_cluster(
    State(state): State<Arc<AppState>>,
    Path(cluster_id): Path<String>,
) -> ApiResult<ClusterResponse> {
    let id = hex::decode(cluster_id.strip_prefix("0x").unwrap_or(&cluster_id))
        .map_err(|_| ApiError::bad_request("Invalid cluster ID"))?;

    let id_hex = hex::encode(&id);
    let query = format!(
        r#"
        SELECT sc.cluster_id, sc.name, sc.description, sc.owner_lock_hash,
               c.lock_code_hash, c.lock_hash_type, c.lock_args,
               sc.spores_count, sc.created_at_block
        FROM spore_clusters sc
        LEFT JOIN cells c ON sc.owner_lock_hash = c.lock_script_hash AND c.status = 0
        WHERE sc.cluster_id = unhex('{}')
        LIMIT 1
        "#,
        id_hex
    );

    let row = state
        .clickhouse
        .client()
        .query(&query)
        .fetch_optional::<ClusterRow>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    match row {
        Some(row) => {
            let owner_address = row.lock_code_hash.as_ref().and_then(|code_hash| {
                let hash_type = row.lock_hash_type.unwrap_or(0);
                let args = row.lock_args.as_deref().unwrap_or(&[]);
                script_to_address(code_hash, hash_type, args, &state.ckb_network).ok()
            });
            ok(ClusterResponse {
                cluster_id: format!("0x{}", hex::encode(&row.cluster_id)),
                name: row.name,
                description: row.description,
                owner_lock_hash: format!("0x{}", hex::encode(&row.owner_lock_hash)),
                owner_address,
                spores_count: row.spores_count,
                created_at_block: row.created_at_block,
            })
        }
        None => Err(ApiError::not_found("Cluster not found")),
    }
}

async fn list_spores(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<SporeResponse>> {
    let limit = params.limit.clamp(1, 100);
    let cursor_block = params.cursor.unwrap_or(i64::MAX);

    let total_row = state
        .clickhouse
        .client()
        .query("SELECT COUNT(*) as count FROM spore_cells WHERE is_live = TRUE")
        .fetch_one::<CountRow>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let total = (total_row.count,);

    let query = format!(
        r#"
        SELECT s.spore_id, s.tx_hash, s.output_index, s.cluster_id, s.content_type, s.content_size, s.owner_lock_hash,
               c.lock_code_hash, c.lock_hash_type, c.lock_args,
               s.is_live, s.created_at_block
        FROM spore_cells s
        LEFT JOIN cells c ON s.tx_hash = c.tx_hash AND s.output_index = c.output_index
        WHERE s.is_live = TRUE AND s.created_at_block < {}
        ORDER BY s.created_at_block DESC
        LIMIT {}
        "#,
        cursor_block,
        limit + 1
    );

    let rows = state
        .clickhouse
        .client()
        .query(&query)
        .fetch_all::<SporeRow>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let has_more = rows.len() as i64 > limit;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

    let next_cursor = if has_more {
        rows.last().map(|row| row.created_at_block.to_string())
    } else {
        None
    };

    let network = &state.ckb_network;
    let spores: Vec<SporeResponse> = rows
        .into_iter()
        .map(|row| {
            let owner_address = row.lock_code_hash.as_ref().and_then(|code_hash| {
                let hash_type = row.lock_hash_type.unwrap_or(0);
                let args = row.lock_args.as_deref().unwrap_or(&[]);
                script_to_address(code_hash, hash_type, args, network).ok()
            });
            SporeResponse {
                spore_id: format!("0x{}", hex::encode(&row.spore_id)),
                tx_hash: format!("0x{}", hex::encode(&row.tx_hash)),
                output_index: row.output_index as i32,
                cluster_id: row
                    .cluster_id
                    .as_ref()
                    .map(|c| format!("0x{}", hex::encode(c))),
                content_type: row.content_type,
                content_size: row.content_size,
                owner_lock_hash: format!("0x{}", hex::encode(&row.owner_lock_hash)),
                owner_address,
                is_live: row.is_live,
                created_at_block: row.created_at_block,
            }
        })
        .collect();

    ok(CursorPaginatedResponse::new(
        spores,
        total.0,
        limit,
        next_cursor,
    ))
}

async fn get_spore(
    State(state): State<Arc<AppState>>,
    Path(spore_id): Path<String>,
) -> ApiResult<SporeResponse> {
    let id = hex::decode(spore_id.strip_prefix("0x").unwrap_or(&spore_id))
        .map_err(|_| ApiError::bad_request("Invalid spore ID"))?;

    let id_hex = hex::encode(&id);
    let query = format!(
        r#"
        SELECT s.spore_id, s.tx_hash, s.output_index, s.cluster_id, s.content_type, s.content_size, s.owner_lock_hash,
               c.lock_code_hash, c.lock_hash_type, c.lock_args,
               s.is_live, s.created_at_block
        FROM spore_cells s
        LEFT JOIN cells c ON s.tx_hash = c.tx_hash AND s.output_index = c.output_index
        WHERE s.spore_id = unhex('{}')
        "#,
        id_hex
    );

    let row = state
        .clickhouse
        .client()
        .query(&query)
        .fetch_optional::<SporeRow>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    match row {
        Some(row) => {
            let owner_address = row.lock_code_hash.as_ref().and_then(|code_hash| {
                let hash_type = row.lock_hash_type.unwrap_or(0);
                let args = row.lock_args.as_deref().unwrap_or(&[]);
                script_to_address(code_hash, hash_type, args, &state.ckb_network).ok()
            });
            ok(SporeResponse {
                spore_id: format!("0x{}", hex::encode(&row.spore_id)),
                tx_hash: format!("0x{}", hex::encode(&row.tx_hash)),
                output_index: row.output_index as i32,
                cluster_id: row
                    .cluster_id
                    .as_ref()
                    .map(|c| format!("0x{}", hex::encode(c))),
                content_type: row.content_type,
                content_size: row.content_size,
                owner_lock_hash: format!("0x{}", hex::encode(&row.owner_lock_hash)),
                owner_address,
                is_live: row.is_live,
                created_at_block: row.created_at_block,
            })
        }
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

    let limit = params.limit.clamp(1, 100);
    let cursor_block = params.cursor.unwrap_or(i64::MAX);

    let hash_hex = hex::encode(&hash);
    let total_row = state
        .clickhouse
        .client()
        .query(&format!(
            "SELECT COUNT(*) as count FROM spore_cells WHERE owner_lock_hash = unhex('{}') AND is_live = TRUE",
            hash_hex
        ))
        .fetch_one::<CountRow>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let total = (total_row.count,);

    let query = format!(
        r#"
        SELECT s.spore_id, s.tx_hash, s.output_index, s.cluster_id, s.content_type, s.content_size, s.owner_lock_hash,
               c.lock_code_hash, c.lock_hash_type, c.lock_args,
               s.is_live, s.created_at_block
        FROM spore_cells s
        LEFT JOIN cells c ON s.tx_hash = c.tx_hash AND s.output_index = c.output_index
        WHERE s.owner_lock_hash = unhex('{}') AND s.is_live = TRUE AND s.created_at_block < {}
        ORDER BY s.created_at_block DESC
        LIMIT {}
        "#,
        hash_hex,
        cursor_block,
        limit + 1
    );

    let rows = state
        .clickhouse
        .client()
        .query(&query)
        .fetch_all::<SporeRow>()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let has_more = rows.len() as i64 > limit;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

    let next_cursor = if has_more {
        rows.last().map(|row| row.created_at_block.to_string())
    } else {
        None
    };

    let network = &state.ckb_network;
    let spores: Vec<SporeResponse> = rows
        .into_iter()
        .map(|row| {
            let owner_address = row.lock_code_hash.as_ref().and_then(|code_hash| {
                let hash_type = row.lock_hash_type.unwrap_or(0);
                let args = row.lock_args.as_deref().unwrap_or(&[]);
                script_to_address(code_hash, hash_type, args, network).ok()
            });
            SporeResponse {
                spore_id: format!("0x{}", hex::encode(&row.spore_id)),
                tx_hash: format!("0x{}", hex::encode(&row.tx_hash)),
                output_index: row.output_index as i32,
                cluster_id: row
                    .cluster_id
                    .as_ref()
                    .map(|c| format!("0x{}", hex::encode(c))),
                content_type: row.content_type,
                content_size: row.content_size,
                owner_lock_hash: format!("0x{}", hex::encode(&row.owner_lock_hash)),
                owner_address,
                is_live: row.is_live,
                created_at_block: row.created_at_block,
            }
        })
        .collect();

    ok(CursorPaginatedResponse::new(
        spores,
        total.0,
        limit,
        next_cursor,
    ))
}
