use axum::{extract::State, routing::get, Router};
use std::sync::Arc;

use crate::response::{ApiError, ApiResult};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/mempool/info", get(get_mempool_info))
        .route("/mempool/transactions", get(get_mempool_transactions))
        .route("/mempool/blocks", get(get_mempool_blocks))
        .route("/mempool/fees", get(get_recommended_fees))
        .route("/mempool/pending-proposals", get(get_pending_proposals))
}

async fn get_mempool_info(State(_state): State<Arc<AppState>>) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_mempool_transactions(State(_state): State<Arc<AppState>>) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_mempool_blocks(State(_state): State<Arc<AppState>>) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_recommended_fees(State(_state): State<Arc<AppState>>) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_pending_proposals(State(_state): State<Arc<AppState>>) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}
