use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Router,
};
use std::collections::HashMap;
use std::sync::Arc;

use crate::response::{ApiError, ApiResult};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/forks", get(list_forks))
        .route("/forks/recent", get(get_recent_reorg))
        .route("/forks/{id}", get(get_fork_detail))
        .route("/admin/resolve-deep-fork", post(resolve_deep_fork))
}

async fn list_forks(
    State(_state): State<Arc<AppState>>,
    Query(_params): Query<HashMap<String, String>>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_recent_reorg(State(_state): State<Arc<AppState>>) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_fork_detail(
    State(_state): State<Arc<AppState>>,
    Path(_id): Path<String>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn resolve_deep_fork(
    State(_state): State<Arc<AppState>>,
    Query(_params): Query<HashMap<String, String>>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}
