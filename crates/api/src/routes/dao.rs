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
        .route("/dao/deposits", get(list_deposits))
        .route("/dao/deposits/{lock_hash}", get(get_deposits_by_address))
        .route("/dao/summary/{lock_hash}", get(get_address_dao_summary))
        .route("/dao/statistics", get(get_statistics))
        .route("/dao/calculator", get(calculate_compensation))
        .route("/dao/charts/total-deposit", get(get_total_deposit_chart))
        .route("/dao/charts/daily-deposit", get(get_daily_deposit_chart))
        .route(
            "/dao/charts/circulation-ratio",
            get(get_circulation_ratio_chart),
        )
}

async fn list_deposits(
    State(_state): State<Arc<AppState>>,
    Query(_params): Query<HashMap<String, String>>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_deposits_by_address(
    State(_state): State<Arc<AppState>>,
    Path(_lock_hash): Path<String>,
    Query(_params): Query<HashMap<String, String>>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_address_dao_summary(
    State(_state): State<Arc<AppState>>,
    Path(_lock_hash): Path<String>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_statistics(State(_state): State<Arc<AppState>>) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn calculate_compensation(
    State(_state): State<Arc<AppState>>,
    Query(_params): Query<HashMap<String, String>>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_total_deposit_chart(State(_state): State<Arc<AppState>>) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_daily_deposit_chart(State(_state): State<Arc<AppState>>) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_circulation_ratio_chart(State(_state): State<Arc<AppState>>) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}
