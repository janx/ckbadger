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
        .route("/transactions", get(list_transactions))
        .route("/transactions/{hash}", get(get_transaction))
        .route("/transactions/{hash}/detail", get(get_transaction_detail))
        .route("/transactions/{hash}/cell-deps", get(get_cell_deps))
        .route("/transactions/{hash}/cycles", get(get_cycles_status))
        .route(
            "/transactions/{hash}/lifecycle",
            get(get_transaction_lifecycle),
        )
        .route(
            "/transactions/{hash}/calculate-cycles",
            post(trigger_cycles_calculation),
        )
        .route(
            "/transactions/{hash}/asset-transfers",
            get(get_transaction_asset_transfers),
        )
        .route("/transactions/{hash}/activities", get(get_tx_activities))
}

async fn list_transactions(
    State(_state): State<Arc<AppState>>,
    Query(_params): Query<HashMap<String, String>>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_transaction(
    State(_state): State<Arc<AppState>>,
    Path(_hash): Path<String>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_transaction_detail(
    State(_state): State<Arc<AppState>>,
    Path(_hash): Path<String>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_cell_deps(
    State(_state): State<Arc<AppState>>,
    Path(_hash): Path<String>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_cycles_status(
    State(_state): State<Arc<AppState>>,
    Path(_hash): Path<String>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_transaction_lifecycle(
    State(_state): State<Arc<AppState>>,
    Path(_hash): Path<String>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn trigger_cycles_calculation(
    State(_state): State<Arc<AppState>>,
    Path(_hash): Path<String>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_transaction_asset_transfers(
    State(_state): State<Arc<AppState>>,
    Path(_hash): Path<String>,
    Query(_params): Query<HashMap<String, String>>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_tx_activities(
    State(_state): State<Arc<AppState>>,
    Path(_hash): Path<String>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}
