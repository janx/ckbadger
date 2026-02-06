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
        .route("/tokens", get(list_tokens))
        .route("/tokens/{type_hash}", get(get_token))
        .route("/tokens/{type_hash}/holders", get(get_token_holders))
        .route("/tokens/{type_hash}/transfers", get(get_token_transfers))
}

async fn list_tokens(
    State(_state): State<Arc<AppState>>,
    Query(_params): Query<HashMap<String, String>>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_token(
    State(_state): State<Arc<AppState>>,
    Path(_type_hash): Path<String>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_token_holders(
    State(_state): State<Arc<AppState>>,
    Path(_type_hash): Path<String>,
    Query(_params): Query<HashMap<String, String>>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_token_transfers(
    State(_state): State<Arc<AppState>>,
    Path(_type_hash): Path<String>,
    Query(_params): Query<HashMap<String, String>>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}
