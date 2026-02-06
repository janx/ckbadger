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
        .route("/activities", get(list_activities))
        .route("/activities/address/{addr}", get(get_address_activities))
        .route(
            "/activities/transaction/{hash}",
            get(get_transaction_activities),
        )
}

async fn list_activities(
    State(_state): State<Arc<AppState>>,
    Query(_params): Query<HashMap<String, String>>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_address_activities(
    State(_state): State<Arc<AppState>>,
    Path(_addr): Path<String>,
    Query(_params): Query<HashMap<String, String>>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_transaction_activities(
    State(_state): State<Arc<AppState>>,
    Path(_hash): Path<String>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}
