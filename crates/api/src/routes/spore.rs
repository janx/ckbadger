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
    let sync_status = state.cache.get_sync_status(&state.read_pool).await;

    if sync_status.spore_deferred {
        return ok(CursorPaginatedResponse::without_total(
            Vec::new(),
            params.limit.clamp(1, 100),
            None,
        ));
    }

    let limit = params.limit.clamp(1, 100);
    let cursor_block = params.cursor.unwrap_or(i64::MAX);

    type ClusterRow = (
        Vec<u8>,
        Option<String>,
        Option<String>,
        Vec<u8>,
        Option<Vec<u8>>,
        Option<i16>,
        Option<Vec<u8>>,
        i32,
        i64,
    );

    let rows = sqlx::query_as::<_, ClusterRow>(
        r#"
        SELECT sc.cluster_id, sc.name, sc.description, sc.owner_lock_hash,
               c.lock_code_hash, c.lock_hash_type, c.lock_args,
               sc.spores_count, sc.created_at_block
        FROM spore_clusters sc
        LEFT JOIN cells c ON sc.owner_lock_hash = c.lock_script_hash AND c.status = 0
        WHERE sc.created_at_block < $1
        GROUP BY sc.cluster_id, sc.name, sc.description, sc.owner_lock_hash, 
                 c.lock_code_hash, c.lock_hash_type, c.lock_args, sc.spores_count, sc.created_at_block
        ORDER BY sc.created_at_block DESC
        LIMIT $2
        "#,
    )
    .bind(cursor_block)
    .bind(limit + 1)
    .fetch_all(&state.read_pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let has_more = rows.len() as i64 > limit;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

    let next_cursor = if has_more {
        rows.last()
            .map(|(_, _, _, _, _, _, _, _, created_at_block)| created_at_block.to_string())
    } else {
        None
    };

    let network = &state.ckb_network;
    let clusters: Vec<ClusterResponse> = rows
        .into_iter()
        .map(
            |(
                cluster_id,
                name,
                description,
                owner_lock_hash,
                lock_code_hash,
                lock_hash_type,
                lock_args,
                spores_count,
                created_at_block,
            )| {
                let owner_address = lock_code_hash.as_ref().and_then(|code_hash| {
                    let hash_type = lock_hash_type.unwrap_or(0);
                    let args = lock_args.as_deref().unwrap_or(&[]);
                    script_to_address(code_hash, hash_type, args, network).ok()
                });
                ClusterResponse {
                    cluster_id: format!("0x{}", hex::encode(&cluster_id)),
                    name,
                    description,
                    owner_lock_hash: format!("0x{}", hex::encode(&owner_lock_hash)),
                    owner_address,
                    spores_count,
                    created_at_block,
                }
            },
        )
        .collect();

    ok(CursorPaginatedResponse::without_total(
        clusters,
        limit,
        next_cursor,
    ))
}

async fn get_spores_by_cluster(
    State(state): State<Arc<AppState>>,
    Path(cluster_id): Path<String>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<SporeResponse>> {
    let sync_status = state.cache.get_sync_status(&state.read_pool).await;

    if sync_status.spore_deferred {
        return ok(CursorPaginatedResponse::without_total(
            Vec::new(),
            params.limit.clamp(1, 100),
            None,
        ));
    }

    type SporeRow = (
        Vec<u8>,
        Vec<u8>,
        i16,
        Option<Vec<u8>>,
        String,
        i32,
        Vec<u8>,
        Option<Vec<u8>>,
        Option<i16>,
        Option<Vec<u8>>,
        bool,
        i64,
    );

    let id = hex::decode(cluster_id.strip_prefix("0x").unwrap_or(&cluster_id))
        .map_err(|_| ApiError::bad_request("Invalid cluster ID"))?;

    let limit = params.limit.clamp(1, 100);
    let cursor_block = params.cursor.unwrap_or(i64::MAX);

    let rows = sqlx::query_as::<_, SporeRow>(
        r#"
        SELECT s.spore_id, s.tx_hash, s.output_index, s.cluster_id, s.content_type, s.content_size, s.owner_lock_hash,
               c.lock_code_hash, c.lock_hash_type, c.lock_args,
               s.is_live, s.created_at_block
        FROM spore_cells s
        LEFT JOIN cells c ON s.tx_hash = c.tx_hash AND s.output_index = c.output_index
        WHERE s.cluster_id = $1 AND s.is_live = TRUE AND s.created_at_block < $2
        ORDER BY s.created_at_block DESC
        LIMIT $3
        "#,
    )
    .bind(&id)
    .bind(cursor_block)
    .bind(limit + 1)
    .fetch_all(&state.read_pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let has_more = rows.len() as i64 > limit;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

    let next_cursor = if has_more {
        rows.last()
            .map(|(_, _, _, _, _, _, _, _, _, _, _, created_at_block)| created_at_block.to_string())
    } else {
        None
    };

    let network = &state.ckb_network;
    let spores: Vec<SporeResponse> = rows
        .into_iter()
        .map(
            |(
                spore_id,
                tx_hash,
                output_index,
                cluster_id,
                content_type,
                content_size,
                owner_lock_hash,
                lock_code_hash,
                lock_hash_type,
                lock_args,
                is_live,
                created_at_block,
            )| {
                let owner_address = lock_code_hash.as_ref().and_then(|code_hash| {
                    let hash_type = lock_hash_type.unwrap_or(0);
                    let args = lock_args.as_deref().unwrap_or(&[]);
                    script_to_address(code_hash, hash_type, args, network).ok()
                });
                SporeResponse {
                    spore_id: format!("0x{}", hex::encode(&spore_id)),
                    tx_hash: format!("0x{}", hex::encode(&tx_hash)),
                    output_index: output_index as i32,
                    cluster_id: cluster_id.map(|c| format!("0x{}", hex::encode(&c))),
                    content_type,
                    content_size,
                    owner_lock_hash: format!("0x{}", hex::encode(&owner_lock_hash)),
                    owner_address,
                    is_live,
                    created_at_block,
                }
            },
        )
        .collect();

    ok(CursorPaginatedResponse::without_total(
        spores,
        limit,
        next_cursor,
    ))
}

async fn get_cluster(
    State(state): State<Arc<AppState>>,
    Path(cluster_id): Path<String>,
) -> ApiResult<ClusterResponse> {
    let sync_status = state.cache.get_sync_status(&state.read_pool).await;

    if sync_status.spore_deferred {
        return Err(ApiError::not_found(
            "Cluster data not yet available (rebuilding)",
        ));
    }

    type ClusterRow = (
        Vec<u8>,
        Option<String>,
        Option<String>,
        Vec<u8>,
        Option<Vec<u8>>,
        Option<i16>,
        Option<Vec<u8>>,
        i32,
        i64,
    );

    let id = hex::decode(cluster_id.strip_prefix("0x").unwrap_or(&cluster_id))
        .map_err(|_| ApiError::bad_request("Invalid cluster ID"))?;

    let row = sqlx::query_as::<_, ClusterRow>(
        r#"
        SELECT sc.cluster_id, sc.name, sc.description, sc.owner_lock_hash,
               c.lock_code_hash, c.lock_hash_type, c.lock_args,
               sc.spores_count, sc.created_at_block
        FROM spore_clusters sc
        LEFT JOIN cells c ON sc.owner_lock_hash = c.lock_script_hash AND c.status = 0
        WHERE sc.cluster_id = $1
        LIMIT 1
        "#,
    )
    .bind(&id)
    .fetch_optional(&state.read_pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    match row {
        Some((
            cluster_id,
            name,
            description,
            owner_lock_hash,
            lock_code_hash,
            lock_hash_type,
            lock_args,
            spores_count,
            created_at_block,
        )) => {
            let owner_address = lock_code_hash.as_ref().and_then(|code_hash| {
                let hash_type = lock_hash_type.unwrap_or(0);
                let args = lock_args.as_deref().unwrap_or(&[]);
                script_to_address(code_hash, hash_type, args, &state.ckb_network).ok()
            });
            ok(ClusterResponse {
                cluster_id: format!("0x{}", hex::encode(&cluster_id)),
                name,
                description,
                owner_lock_hash: format!("0x{}", hex::encode(&owner_lock_hash)),
                owner_address,
                spores_count,
                created_at_block,
            })
        }
        None => Err(ApiError::not_found("Cluster not found")),
    }
}

async fn list_spores(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> ApiResult<CursorPaginatedResponse<SporeResponse>> {
    let sync_status = state.cache.get_sync_status(&state.read_pool).await;

    if sync_status.spore_deferred {
        return ok(CursorPaginatedResponse::without_total(
            Vec::new(),
            params.limit.clamp(1, 100),
            None,
        ));
    }

    let limit = params.limit.clamp(1, 100);
    let cursor_block = params.cursor.unwrap_or(i64::MAX);

    type SporeRow = (
        Vec<u8>,
        Vec<u8>,
        i16,
        Option<Vec<u8>>,
        String,
        i32,
        Vec<u8>,
        Option<Vec<u8>>,
        Option<i16>,
        Option<Vec<u8>>,
        bool,
        i64,
    );

    let rows = sqlx::query_as::<_, SporeRow>(
        r#"
        SELECT s.spore_id, s.tx_hash, s.output_index, s.cluster_id, s.content_type, s.content_size, s.owner_lock_hash,
               c.lock_code_hash, c.lock_hash_type, c.lock_args,
               s.is_live, s.created_at_block
        FROM spore_cells s
        LEFT JOIN cells c ON s.tx_hash = c.tx_hash AND s.output_index = c.output_index
        WHERE s.is_live = TRUE AND s.created_at_block < $1
        ORDER BY s.created_at_block DESC
        LIMIT $2
        "#,
    )
    .bind(cursor_block)
    .bind(limit + 1)
    .fetch_all(&state.read_pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let has_more = rows.len() as i64 > limit;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

    let next_cursor = if has_more {
        rows.last()
            .map(|(_, _, _, _, _, _, _, _, _, _, _, created_at_block)| created_at_block.to_string())
    } else {
        None
    };

    let network = &state.ckb_network;
    let spores: Vec<SporeResponse> = rows
        .into_iter()
        .map(
            |(
                spore_id,
                tx_hash,
                output_index,
                cluster_id,
                content_type,
                content_size,
                owner_lock_hash,
                lock_code_hash,
                lock_hash_type,
                lock_args,
                is_live,
                created_at_block,
            )| {
                let owner_address = lock_code_hash.as_ref().and_then(|code_hash| {
                    let hash_type = lock_hash_type.unwrap_or(0);
                    let args = lock_args.as_deref().unwrap_or(&[]);
                    script_to_address(code_hash, hash_type, args, network).ok()
                });
                SporeResponse {
                    spore_id: format!("0x{}", hex::encode(&spore_id)),
                    tx_hash: format!("0x{}", hex::encode(&tx_hash)),
                    output_index: output_index as i32,
                    cluster_id: cluster_id.map(|c| format!("0x{}", hex::encode(&c))),
                    content_type,
                    content_size,
                    owner_lock_hash: format!("0x{}", hex::encode(&owner_lock_hash)),
                    owner_address,
                    is_live,
                    created_at_block,
                }
            },
        )
        .collect();

    ok(CursorPaginatedResponse::without_total(
        spores,
        limit,
        next_cursor,
    ))
}

async fn get_spore(
    State(state): State<Arc<AppState>>,
    Path(spore_id): Path<String>,
) -> ApiResult<SporeResponse> {
    let sync_status = state.cache.get_sync_status(&state.read_pool).await;

    if sync_status.spore_deferred {
        return Err(ApiError::not_found(
            "Spore data not yet available (rebuilding)",
        ));
    }

    type SporeRow = (
        Vec<u8>,
        Vec<u8>,
        i16,
        Option<Vec<u8>>,
        String,
        i32,
        Vec<u8>,
        Option<Vec<u8>>,
        Option<i16>,
        Option<Vec<u8>>,
        bool,
        i64,
    );

    let id = hex::decode(spore_id.strip_prefix("0x").unwrap_or(&spore_id))
        .map_err(|_| ApiError::bad_request("Invalid spore ID"))?;

    let row = sqlx::query_as::<_, SporeRow>(
        r#"
        SELECT s.spore_id, s.tx_hash, s.output_index, s.cluster_id, s.content_type, s.content_size, s.owner_lock_hash,
               c.lock_code_hash, c.lock_hash_type, c.lock_args,
               s.is_live, s.created_at_block
        FROM spore_cells s
        LEFT JOIN cells c ON s.tx_hash = c.tx_hash AND s.output_index = c.output_index
        WHERE s.spore_id = $1
        "#,
    )
    .bind(&id)
    .fetch_optional(&state.read_pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    match row {
        Some((
            spore_id,
            tx_hash,
            output_index,
            cluster_id,
            content_type,
            content_size,
            owner_lock_hash,
            lock_code_hash,
            lock_hash_type,
            lock_args,
            is_live,
            created_at_block,
        )) => {
            let owner_address = lock_code_hash.as_ref().and_then(|code_hash| {
                let hash_type = lock_hash_type.unwrap_or(0);
                let args = lock_args.as_deref().unwrap_or(&[]);
                script_to_address(code_hash, hash_type, args, &state.ckb_network).ok()
            });
            ok(SporeResponse {
                spore_id: format!("0x{}", hex::encode(&spore_id)),
                tx_hash: format!("0x{}", hex::encode(&tx_hash)),
                output_index: output_index as i32,
                cluster_id: cluster_id.map(|c| format!("0x{}", hex::encode(&c))),
                content_type,
                content_size,
                owner_lock_hash: format!("0x{}", hex::encode(&owner_lock_hash)),
                owner_address,
                is_live,
                created_at_block,
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
    let sync_status = state.cache.get_sync_status(&state.read_pool).await;

    if sync_status.spore_deferred {
        return ok(CursorPaginatedResponse::without_total(
            Vec::new(),
            params.limit.clamp(1, 100),
            None,
        ));
    }

    type SporeRow = (
        Vec<u8>,
        Vec<u8>,
        i16,
        Option<Vec<u8>>,
        String,
        i32,
        Vec<u8>,
        Option<Vec<u8>>,
        Option<i16>,
        Option<Vec<u8>>,
        bool,
        i64,
    );

    let hash = hex::decode(lock_hash.strip_prefix("0x").unwrap_or(&lock_hash))
        .map_err(|_| ApiError::bad_request("Invalid lock script hash"))?;

    let limit = params.limit.clamp(1, 100);
    let cursor_block = params.cursor.unwrap_or(i64::MAX);

    let rows = sqlx::query_as::<_, SporeRow>(
        r#"
        SELECT s.spore_id, s.tx_hash, s.output_index, s.cluster_id, s.content_type, s.content_size, s.owner_lock_hash,
               c.lock_code_hash, c.lock_hash_type, c.lock_args,
               s.is_live, s.created_at_block
        FROM spore_cells s
        LEFT JOIN cells c ON s.tx_hash = c.tx_hash AND s.output_index = c.output_index
        WHERE s.owner_lock_hash = $1 AND s.is_live = TRUE AND s.created_at_block < $2
        ORDER BY s.created_at_block DESC
        LIMIT $3
        "#,
    )
    .bind(&hash)
    .bind(cursor_block)
    .bind(limit + 1)
    .fetch_all(&state.read_pool)
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?;

    let has_more = rows.len() as i64 > limit;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

    let next_cursor = if has_more {
        rows.last()
            .map(|(_, _, _, _, _, _, _, _, _, _, _, created_at_block)| created_at_block.to_string())
    } else {
        None
    };

    let network = &state.ckb_network;
    let spores: Vec<SporeResponse> = rows
        .into_iter()
        .map(
            |(
                spore_id,
                tx_hash,
                output_index,
                cluster_id,
                content_type,
                content_size,
                owner_lock_hash,
                lock_code_hash,
                lock_hash_type,
                lock_args,
                is_live,
                created_at_block,
            )| {
                let owner_address = lock_code_hash.as_ref().and_then(|code_hash| {
                    let hash_type = lock_hash_type.unwrap_or(0);
                    let args = lock_args.as_deref().unwrap_or(&[]);
                    script_to_address(code_hash, hash_type, args, network).ok()
                });
                SporeResponse {
                    spore_id: format!("0x{}", hex::encode(&spore_id)),
                    tx_hash: format!("0x{}", hex::encode(&tx_hash)),
                    output_index: output_index as i32,
                    cluster_id: cluster_id.map(|c| format!("0x{}", hex::encode(&c))),
                    content_type,
                    content_size,
                    owner_lock_hash: format!("0x{}", hex::encode(&owner_lock_hash)),
                    owner_address,
                    is_live,
                    created_at_block,
                }
            },
        )
        .collect();

    ok(CursorPaginatedResponse::without_total(
        spores,
        limit,
        next_cursor,
    ))
}
