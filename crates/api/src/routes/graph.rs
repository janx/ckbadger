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
        .route("/graph/cell/{tx_hash}/{output_index}", get(get_cell_graph))
        .route("/graph/transaction/{hash}", get(get_tx_graph))
        .route("/graph/proposals/{block_number}", get(get_proposal_graph))
}

async fn get_cell_graph(
    State(_state): State<Arc<AppState>>,
    Path((_tx_hash, _output_index)): Path<(String, i16)>,
    Query(_params): Query<HashMap<String, String>>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_tx_graph(
    State(_state): State<Arc<AppState>>,
    Path(_hash): Path<String>,
    Query(_params): Query<HashMap<String, String>>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}

async fn get_proposal_graph(
    State(_state): State<Arc<AppState>>,
    Path(_block_number): Path<String>,
    Query(_params): Query<HashMap<String, String>>,
) -> ApiResult<()> {
    Err(ApiError::internal("ClickHouse implementation pending"))
}
