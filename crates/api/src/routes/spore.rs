use axum::{
    extract::{Path, Query, State},
    routing::get,
    Router,
};
use std::collections::HashMap;
use std::sync::Arc;

use crate::response::{ApiError, ApiResult};
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

async fn list_clusters(
    State(_state): State<Arc<AppState>>,
    Query(_params): Query<HashMap<String, String>>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_cluster(
    State(_state): State<Arc<AppState>>,
    Path(_cluster_id): Path<String>,
    Query(_params): Query<HashMap<String, String>>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_spores_by_cluster(
    State(_state): State<Arc<AppState>>,
    Path(_cluster_id): Path<String>,
    Query(_params): Query<HashMap<String, String>>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn list_spores(
    State(_state): State<Arc<AppState>>,
    Query(_params): Query<HashMap<String, String>>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_spore(
    State(_state): State<Arc<AppState>>,
    Path(_spore_id): Path<String>,
    Query(_params): Query<HashMap<String, String>>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_spores_by_owner(
    State(_state): State<Arc<AppState>>,
    Path(_lock_hash): Path<String>,
    Query(_params): Query<HashMap<String, String>>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}
